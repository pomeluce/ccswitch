use super::connection::Db;
use rayon::prelude::*;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
pub struct UsageSummary {
    pub model: String,
    pub total_prompt: i64,
    pub total_completion: i64,
    pub total_cache_read: i64,
    pub total_cache_create: i64,
    pub request_count: i64,
}

/// Progress event sent from the background scan thread to the TUI
#[derive(Debug, Clone)]
pub enum ScanEvent {
    /// A batch of parsed records from one file, ready for DB insert
    Batch { sid: String, file_path: PathBuf, records: Vec<UsageRecord> },
    /// Progress update (files done / files total, cumulative records)
    Progress { files_done: usize, files_total: usize, records: usize },
    /// All files parsed. total_imported = 0 (will be set by main thread after inserts)
    Done {},
}

/// Lightweight parser for assistant message usage data in JSONL files
#[derive(Debug, Deserialize)]
struct UsageLine {
    #[serde(rename = "type")]
    msg_type: Option<String>,
    message: Option<UsageMessage>,
    timestamp: Option<String>,
    #[allow(dead_code)]
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UsageMessage {
    id: Option<String>,
    #[allow(dead_code)]
    role: Option<String>,
    model: Option<String>,
    usage: Option<UsageData>,
}

#[derive(Debug, Deserialize)]
struct UsageData {
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    #[allow(dead_code)]
    cache_read_input_tokens: Option<i64>,
    #[allow(dead_code)]
    cache_creation_input_tokens: Option<i64>,
}

/// Parsed usage record awaiting DB insert — public so background thread can send via channel
#[derive(Debug, Clone)]
pub struct UsageRecord {
    pub msg_id: String,
    pub model: String,
    pub date: String,
    pub input: i64,
    pub output: i64,
    pub cr: i64,
    pub cc: i64,
}

/// Scan context prepared on main thread (DB queries only, fast).
/// Background thread uses this + does its own file-system walking.
#[derive(Debug, Clone)]
pub struct ScanContext {
    pub known_msg_ids: Vec<String>,
    pub file_index: std::collections::HashMap<String, i64>, // session_id → mtime
}

impl Db {
    pub fn insert_usage_log(
        &self,
        provider_id: &str,
        profile_id: &str,
        mode: &str,
        session_id: Option<&str>,
        prompt_tokens: i64,
        completion_tokens: i64,
        cache_read_tokens: i64,
        cache_create_tokens: i64,
    ) -> Result<(), rusqlite::Error> {
        let total = prompt_tokens + completion_tokens + cache_read_tokens + cache_create_tokens;
        self.conn().execute(
            "INSERT INTO usage_logs (model, provider_id, profile_id, mode, session_id, prompt_tokens, completion_tokens, cache_read_tokens, cache_create_tokens, total_tokens)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                provider_id,
                provider_id,
                profile_id,
                mode,
                session_id,
                prompt_tokens,
                completion_tokens,
                cache_read_tokens,
                cache_create_tokens,
                total
            ],
        )?;
        Ok(())
    }

    pub fn query_usage(&self, range: &str) -> Result<Vec<UsageSummary>, rusqlite::Error> {
        let date_filter = match range {
            "day" => "date(timestamp) = date('now')",
            "week" => "date(timestamp) >= date('now', '-7 days')",
            "month" => "date(timestamp) >= date('now', '-30 days')",
            _ => "1=1",
        };
        let sql = format!(
            "SELECT model, SUM(prompt_tokens), SUM(completion_tokens), SUM(cache_read_tokens), SUM(cache_create_tokens), COUNT(*)
             FROM usage_logs WHERE {} GROUP BY model ORDER BY MAX(timestamp) DESC",
            date_filter
        );
        let mut stmt = self.conn().prepare(&sql)?;
        let rows = stmt.query_map([], |row| {
            Ok(UsageSummary {
                model: row.get(0)?,

                total_prompt: row.get(1)?,
                total_completion: row.get(2)?,
                total_cache_read: row.get(3)?,
                total_cache_create: row.get(4)?,
                request_count: row.get(5)?,
            })
        })?;
        rows.collect()
    }

    /// Query per-day usage breakdown for a specific profile
    pub fn query_daily_usage(&self, model: &str) -> Result<Vec<(String, i64, i64, i64, i64)>, rusqlite::Error> {
        let sql = "SELECT date(timestamp) as day,
                          SUM(prompt_tokens), SUM(completion_tokens),
                          SUM(cache_read_tokens), SUM(cache_create_tokens)
                   FROM usage_logs
                   WHERE model = ?1
                     AND date(timestamp) >= date('now', '-6 days')
                   GROUP BY day ORDER BY day";
        let mut stmt = self.conn().prepare(sql)?;
        let rows = stmt.query_map(params![model], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?;
        rows.collect()
    }

    /// Query token usage for a specific session (by session ID)
    pub fn query_session_tokens(&self, session_id: &str) -> Result<(i64, i64), rusqlite::Error> {
        let mut stmt = self.conn().prepare(
            "SELECT COALESCE(SUM(prompt_tokens),0), COALESCE(SUM(completion_tokens),0)
             FROM usage_logs WHERE session_id = ?1",
        )?;
        stmt.query_row(params![session_id], |row| Ok((row.get(0)?, row.get(1)?)))
    }

    /// Prepare scan context: ensure tables exist + load known IDs + file index.
    /// Called from main thread. Fast — no filesystem walk.
    pub fn prepare_scan_context(&self) -> Result<ScanContext, anyhow::Error> {
        // Ensure core tables exist (belt-and-suspenders — migrations should have handled this)
        self.conn().execute_batch(
            "CREATE TABLE IF NOT EXISTS usage_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                provider_id TEXT NOT NULL,
                profile_id TEXT NOT NULL,
                mode TEXT NOT NULL CHECK(mode IN ('local', 'proxy')),
                session_id TEXT,
                prompt_tokens INTEGER NOT NULL DEFAULT 0,
                completion_tokens INTEGER NOT NULL DEFAULT 0,
                timestamp TEXT NOT NULL DEFAULT (datetime('now')),
                cache_read_tokens INTEGER NOT NULL DEFAULT 0,
                cache_create_tokens INTEGER NOT NULL DEFAULT 0,
                total_tokens INTEGER NOT NULL DEFAULT 0,
                message_id TEXT,
                model TEXT NOT NULL DEFAULT ''
            );
            CREATE TABLE IF NOT EXISTS session_usage_track (
                session_id TEXT PRIMARY KEY,
                file_mtime INTEGER NOT NULL,
                scanned_at TEXT NOT NULL DEFAULT ''
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_usage_msg_id ON usage_logs(message_id) WHERE message_id IS NOT NULL;",
        )?;

        // Cleanup synthetic entries
        self.conn().execute("DELETE FROM usage_logs WHERE model = '<synthetic>'", [])?;

        // Pre-load existing message IDs for dedup
        let known_msg_ids: Vec<String> = {
            let mut stmt = self.conn().prepare("SELECT message_id FROM usage_logs WHERE message_id IS NOT NULL AND message_id != ''")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            rows.filter_map(|r| r.ok()).collect()
        };

        // Load stored file index: session_id → mtime
        let file_index: std::collections::HashMap<String, i64> = {
            let mut stmt = self.conn().prepare("SELECT session_id, file_mtime FROM session_usage_track")?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
            rows.filter_map(|r| r.ok()).collect()
        };

        Ok(ScanContext { known_msg_ids, file_index })
    }

    /// Insert a batch of parsed records into usage_logs (single transaction for speed).
    /// Called from main thread after receiving ScanEvent::Batch.
    pub fn insert_usage_batch(&self, sid: &str, file_path: &PathBuf, records: &[UsageRecord]) -> Result<usize, anyhow::Error> {
        let mut imported = 0usize;
        self.conn().execute("BEGIN", [])?;
        for r in records {
            self.conn().execute(
                "INSERT OR REPLACE INTO usage_logs (model, provider_id, profile_id, mode, session_id, message_id, prompt_tokens, completion_tokens, cache_read_tokens, cache_create_tokens, total_tokens, timestamp)
                 VALUES (?1, ?2, ?3, 'local', ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![r.model, "", "", sid, r.msg_id, r.input, r.output, r.cr, r.cc, r.input + r.output + r.cr + r.cc, r.date],
            )?;
            imported += 1;
        }
        // Update file index
        if let Some(mtime) = file_mtime(file_path) {
            let now_iso = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
            self.conn().execute(
                "INSERT OR REPLACE INTO session_usage_track (session_id, file_mtime, scanned_at) VALUES (?1, ?2, ?3)",
                params![sid, mtime, now_iso],
            )?;
        }
        self.conn().execute("COMMIT", [])?;
        Ok(imported)
    }
}

/// Background-thread function: collect changed files, parse them, send batches via channel.
/// No database access — pure file I/O + JSON parsing. File-system walk is done here
/// (in the background) to keep the main thread responsive.
pub fn parse_files_in_background(ctx: ScanContext, batch_size: usize, tx: std::sync::mpsc::Sender<ScanEvent>) {
    // Collect changed files (in background — this is the slow part on first launch)
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let projects_dir = std::path::PathBuf::from(&home).join(".claude/projects");
    let mut changed_files: Vec<(std::path::PathBuf, String)> = Vec::new();
    if projects_dir.exists() {
        collect_changed_files(&projects_dir, &mut changed_files, &ctx.file_index);
    }

    let total = changed_files.len();
    if total == 0 {
        let _ = tx.send(ScanEvent::Done {});
        return;
    }

    let known_set: std::collections::HashSet<&str> = ctx.known_msg_ids.iter().map(|s| s.as_str()).collect();
    let mut total_records = 0usize;
    let mut last_report = 0usize;

    // Process files sequentially (sending Batches), but parse each file's lines in parallel via rayon
    for (idx, (path, sid)) in changed_files.iter().enumerate() {
        let records = parse_single_file(path, sid, &known_set);
        let n = records.len();
        total_records += n;

        let _ = tx.send(ScanEvent::Batch {
            sid: sid.clone(),
            file_path: path.clone(),
            records,
        });

        let files_done = idx + 1;
        if files_done - last_report >= batch_size || files_done == total {
            let _ = tx.send(ScanEvent::Progress {
                files_done,
                files_total: total,
                records: total_records,
            });
            last_report = files_done;
        }
    }

    let _ = tx.send(ScanEvent::Done {});
}

/// Parse a single JSONL file, returning all new (unseen) usage records
fn parse_single_file(path: &PathBuf, _sid: &str, known_msg_ids: &std::collections::HashSet<&str>) -> Vec<UsageRecord> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    // Use rayon to parse lines in parallel
    content
        .par_lines()
        .filter_map(|line| {
            let parsed: UsageLine = serde_json::from_str(line).ok()?;
            if parsed.msg_type.as_deref() != Some("assistant") {
                return None;
            }
            let msg = parsed.message.as_ref()?;
            let usage = msg.usage.as_ref()?;
            let msg_id = msg.id.as_deref().unwrap_or("").to_string();
            if !msg_id.is_empty() && known_msg_ids.contains(msg_id.as_str()) {
                return None;
            }
            let model = msg.model.as_deref().unwrap_or("unknown").replace("[1m]", "");
            if model == "<synthetic>" {
                return None;
            }
            let ts = parsed.timestamp.as_deref().unwrap_or("");
            // "2026-07-02T12:02:02.866Z" → "2026-07-02 12:02:02"
            let date = if ts.len() >= 19 {
                format!("{} {}", &ts[..10], &ts[11..19])
            } else if ts.len() >= 10 {
                format!("{} 00:00:00", &ts[..10])
            } else {
                "today".to_string()
            };
            Some(UsageRecord {
                msg_id,
                model: model.to_string(),
                date,
                input: usage.input_tokens.unwrap_or(0),
                output: usage.output_tokens.unwrap_or(0),
                cr: usage.cache_read_input_tokens.unwrap_or(0),
                cc: usage.cache_creation_input_tokens.unwrap_or(0),
            })
        })
        .collect()
}

/// Read file mtime as unix timestamp (seconds)
fn file_mtime(path: &PathBuf) -> Option<i64> {
    let meta = std::fs::metadata(path).ok()?;
    let dur = meta.modified().ok()?;
    let secs = dur.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(secs.as_secs() as i64)
}

/// Recursively collect changed .jsonl files under a directory
fn collect_changed_files(dir: &PathBuf, out: &mut Vec<(PathBuf, String)>, file_index: &std::collections::HashMap<String, i64>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_changed_files(&path, out, file_index);
            } else if path.extension().map_or(false, |e| e == "jsonl") {
                let sid = path.file_stem().and_then(|n| n.to_str()).unwrap_or("").to_string();
                if !sid.is_empty() {
                    let mtime = file_mtime(&path).unwrap_or(0);
                    let changed = file_index.get(&sid).map_or(true, |&old| old != mtime);
                    if changed {
                        out.push((path, sid));
                    }
                }
            }
        }
    }
}
