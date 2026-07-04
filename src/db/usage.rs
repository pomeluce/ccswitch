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
    Batch {
        #[allow(dead_code)]
        app_type: String,
        sid: String,
        file_path: PathBuf,
        records: Vec<UsageRecord>,
    },
    /// Progress update (files done / files total, cumulative records)
    Progress {
        files_done: usize,
        files_total: usize,
        records: usize,
    },
    /// All files parsed.
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

/// Parsed usage record awaiting DB insert
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
#[derive(Debug, Clone)]
pub struct ScanContext {
    pub known_msg_ids: Vec<String>,
    pub file_index: std::collections::HashMap<String, i64>, // file_path → mtime
}

impl Db {
    pub fn insert_usage_log(
        &self,
        app_type: &str,
        provider_id: &str,
        profile_id: &str,
        session_id: Option<&str>,
        prompt_tokens: i64,
        completion_tokens: i64,
        cache_read_tokens: i64,
        cache_create_tokens: i64,
        data_source: &str,
    ) -> Result<(), rusqlite::Error> {
        let total = prompt_tokens + completion_tokens + cache_read_tokens + cache_create_tokens;
        self.conn().execute(
            "INSERT INTO usage_logs (app_type, model, provider_id, profile_id, session_id,
             prompt_tokens, completion_tokens, cache_read_tokens, cache_create_tokens,
             total_tokens, data_source)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                app_type,
                provider_id,
                provider_id,
                profile_id,
                session_id,
                prompt_tokens,
                completion_tokens,
                cache_read_tokens,
                cache_create_tokens,
                total,
                data_source
            ],
        )?;
        Ok(())
    }

    pub fn query_usage(&self, app_type: &str, range: &str) -> Result<Vec<UsageSummary>, rusqlite::Error> {
        let date_filter = match range {
            "day" => "date(timestamp) = date('now')",
            "week" => "date(timestamp) >= date('now', '-7 days')",
            "month" => "date(timestamp) >= date('now', '-30 days')",
            _ => "1=1",
        };
        let sql = format!(
            "SELECT model, SUM(prompt_tokens), SUM(completion_tokens), SUM(cache_read_tokens), SUM(cache_create_tokens), COUNT(*)
             FROM usage_logs WHERE app_type = ?1 AND {} GROUP BY model ORDER BY MAX(timestamp) DESC",
            date_filter
        );
        let mut stmt = self.conn().prepare(&sql)?;
        let rows = stmt.query_map(params![app_type], |row| {
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

    /// Query per-day usage breakdown for a specific model
    pub fn query_daily_usage(
        &self,
        app_type: &str,
        model: &str,
    ) -> Result<Vec<(String, i64, i64, i64, i64)>, rusqlite::Error> {
        let sql = "SELECT date(timestamp) as day,
                          SUM(prompt_tokens), SUM(completion_tokens),
                          SUM(cache_read_tokens), SUM(cache_create_tokens)
                   FROM usage_logs
                   WHERE app_type = ?1 AND model = ?2
                     AND date(timestamp) >= date('now', '-6 days')
                   GROUP BY day ORDER BY day";
        let mut stmt = self.conn().prepare(sql)?;
        let rows = stmt.query_map(params![app_type, model], |row| {
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

    /// Prepare scan context: load known message IDs and file sync index from session_log_sync.
    /// Called from main thread. Fast — no filesystem walk.
    pub fn prepare_scan_context(&self, app_type: &str) -> Result<ScanContext, anyhow::Error> {
        // Cleanup synthetic entries
        self.conn()
            .execute("DELETE FROM usage_logs WHERE model = '<synthetic>'", [])
            .ok();

        // Pre-load existing message IDs for dedup
        let known_msg_ids: Vec<String> = {
            let mut stmt = self.conn().prepare(
                "SELECT message_id FROM usage_logs WHERE message_id IS NOT NULL AND message_id != '' AND app_type = ?1",
            )?;
            let rows = stmt.query_map(params![app_type], |r| r.get::<_, String>(0))?;
            rows.filter_map(|r| r.ok()).collect()
        };

        // Load stored file index from session_log_sync
        let file_index: std::collections::HashMap<String, i64> = {
            let mut stmt = self.conn().prepare(
                "SELECT file_path, file_mtime FROM session_log_sync WHERE scan_type IN ('usage','both')",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            })?;
            rows.filter_map(|r| r.ok()).collect()
        };

        Ok(ScanContext {
            known_msg_ids,
            file_index,
        })
    }

    /// Insert a batch of parsed records into usage_logs (single transaction for speed).
    pub fn insert_usage_batch(
        &self,
        app_type: &str,
        sid: &str,
        file_path: &PathBuf,
        records: &[UsageRecord],
    ) -> Result<usize, anyhow::Error> {
        let mut imported = 0usize;
        self.conn().execute("BEGIN", [])?;
        for r in records {
            self.conn().execute(
                "INSERT OR IGNORE INTO usage_logs (app_type, model, provider_id, profile_id, session_id, message_id,
                 prompt_tokens, completion_tokens, cache_read_tokens, cache_creation_tokens, total_tokens, timestamp, data_source)
                 VALUES (?1, ?2, '', '', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'import')",
                params![
                    app_type,
                    r.model,
                    sid,
                    r.msg_id,
                    r.input,
                    r.output,
                    r.cr,
                    r.cc,
                    r.input + r.output + r.cr + r.cc,
                    r.date
                ],
            )?;
            imported += 1;
        }
        // Update session_log_sync
        if let Some(mtime) = file_mtime(file_path) {
            let now_iso = chrono::Local::now()
                .format("%Y-%m-%d %H:%M:%S")
                .to_string();
            let file_path_str = file_path.to_string_lossy().to_string();
            self.conn().execute(
                "INSERT OR REPLACE INTO session_log_sync (file_path, file_mtime, scan_type, last_synced_at)
                 VALUES (?1, ?2, 'usage', ?3)",
                params![file_path_str, mtime, now_iso],
            )?;
        }
        self.conn().execute("COMMIT", [])?;
        Ok(imported)
    }
}

/// Background-thread function: collect changed files, parse them, send batches via channel.
pub fn parse_files_in_background(
    app_type: String,
    ctx: ScanContext,
    batch_size: usize,
    tx: std::sync::mpsc::Sender<ScanEvent>,
) {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let projects_dir = std::path::PathBuf::from(&home).join(".claude/projects");
    let mut changed_files: Vec<(PathBuf, String)> = Vec::new();
    if projects_dir.exists() {
        collect_changed_files(&projects_dir, &mut changed_files, &ctx.file_index);
    }

    let total = changed_files.len();
    if total == 0 {
        let _ = tx.send(ScanEvent::Done {});
        return;
    }

    let known_set: std::collections::HashSet<&str> =
        ctx.known_msg_ids.iter().map(|s| s.as_str()).collect();
    let mut total_records = 0usize;
    let mut last_report = 0usize;

    for (idx, (path, sid)) in changed_files.iter().enumerate() {
        let records = parse_single_file(path, sid, &known_set);
        let n = records.len();
        total_records += n;

        let _ = tx.send(ScanEvent::Batch {
            app_type: app_type.clone(),
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
fn parse_single_file(
    path: &PathBuf,
    _sid: &str,
    known_msg_ids: &std::collections::HashSet<&str>,
) -> Vec<UsageRecord> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

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
            let model = msg
                .model
                .as_deref()
                .unwrap_or("unknown")
                .replace("[1m]", "");
            if model == "<synthetic>" {
                return None;
            }
            let ts = parsed.timestamp.as_deref().unwrap_or("");
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
fn collect_changed_files(
    dir: &PathBuf,
    out: &mut Vec<(PathBuf, String)>,
    file_index: &std::collections::HashMap<String, i64>,
) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_changed_files(&path, out, file_index);
            } else if path.extension().map_or(false, |e| e == "jsonl") {
                let sid = path
                    .file_stem()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();
                if !sid.is_empty() {
                    let mtime = file_mtime(&path).unwrap_or(0);
                    let file_path_str = path.to_string_lossy().to_string();
                    let changed = file_index
                        .get(&file_path_str)
                        .map_or(true, |&old| old != mtime);
                    if changed {
                        out.push((path, sid));
                    }
                }
            }
        }
    }
}
