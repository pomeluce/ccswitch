//! Session and usage data import from Claude Code / Codex CLI JSONL files.
//!
//! These functions read JSONL files from the filesystem, parse them, and write
//! results to the database. Separated from the `db` module to keep the DB
//! layer focused on CRUD operations.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserialize;

use crate::db::connection::Db;
use crate::db::sessions::SessionRecord;
use crate::db::usage::{ScanContext, ScanEvent, UsageRecord};

// ── Session Import ───────────────────────────────────────────────

/// A line from a Claude Code session JSONL file
#[derive(Debug, Deserialize)]
struct JsonlLine {
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    cwd: Option<String>,
    timestamp: Option<serde_json::Value>,
    #[serde(rename = "type")]
    #[allow(dead_code)]
    msg_type: Option<String>,
    #[allow(dead_code)]
    message: Option<MessageContent>,
    #[serde(rename = "customTitle")]
    custom_title: Option<String>,
    #[serde(rename = "aiTitle")]
    ai_title: Option<String>,
    #[serde(rename = "lastPrompt")]
    last_prompt: Option<String>,
    #[allow(dead_code)]
    #[serde(rename = "isMeta")]
    is_meta: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct MessageContent {
    content: Option<serde_json::Value>,
    #[allow(dead_code)]
    role: Option<String>,
    /// Proxy mode marker injected by CCSwitch
    #[serde(default)]
    ccs_proxy: Option<bool>,
}

fn claude_projects_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".claude").join("projects")
}

enum TitleField {
    Custom(String),
    Ai(String),
    LastPrompt(String),
}

fn parse_title_only(line: &str) -> Option<TitleField> {
    if let Some(v) = extract_json_str(line, "\"customTitle\"") {
        return Some(TitleField::Custom(v));
    }
    if let Some(v) = extract_json_str(line, "\"aiTitle\"") {
        return Some(TitleField::Ai(v));
    }
    if let Some(v) = extract_json_str(line, "\"lastPrompt\"") {
        return Some(TitleField::LastPrompt(v));
    }
    None
}

fn extract_json_str(line: &str, key: &str) -> Option<String> {
    let start = line.find(key)?;
    let after_key = &line[start + key.len()..];
    let colon = after_key.find(':')?;
    let after_colon = after_key[colon + 1..].trim();
    if !after_colon.starts_with('"') { return None; }
    let content = &after_colon[1..];
    let mut result = String::new();
    let mut chars = content.chars();
    while let Some(c) = chars.next() {
        if c == '\\' { chars.next(); continue; }
        if c == '"' { break; }
        result.push(c);
    }
    if result.is_empty() { None } else { Some(result) }
}

fn truncate_title(s: &str) -> String {
    let s = s.trim();
    if s.chars().count() > 40 {
        format!("{}...", s.chars().take(37).collect::<String>())
    } else {
        s.to_string()
    }
}

fn parse_timestamp(val: &serde_json::Value) -> Option<i64> {
    match val {
        serde_json::Value::Number(n) => {
            let ts = n.as_f64()? as i64;
            Some(if ts > 1_000_000_000_000 { ts } else { ts * 1000 })
        }
        serde_json::Value::String(s) => {
            chrono::DateTime::parse_from_rfc3339(s)
                .ok()
                .map(|dt| dt.timestamp_millis())
        }
        _ => None,
    }
}

fn extract_command(text: &str) -> Option<String> {
    let name = text
        .split("<command-name>").nth(1)?
        .split("</command-name>").next()?
        .trim().to_string();
    let args = text
        .split("<command-args>").nth(1)
        .and_then(|s| s.split("</command-args>").next())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());
    match args {
        Some(a) => Some(format!("{} {}", name, a)),
        None => Some(name),
    }
}

fn ts_to_iso(ts_ms: i64) -> String {
    let secs = ts_ms / 1000;
    let nanos = ((ts_ms % 1000) * 1_000_000) as u32;
    match chrono::TimeZone::timestamp_opt(&chrono::Utc, secs, nanos) {
        chrono::offset::LocalResult::Single(dt) => dt.format("%Y-%m-%d %H:%M:%S").to_string(),
        _ => String::new(),
    }
}

/// Import with progress callback. Incremental: only processes files whose
/// mtime differs from the stored index in session_log_sync.
pub fn import_claude_sessions_with_progress(
    db: &Db,
    on_progress: impl Fn(usize, usize, usize),
) -> Result<usize, anyhow::Error> {
    let projects_dir = claude_projects_dir();
    if !projects_dir.exists() {
        return Ok(0);
    }

    let file_index: HashMap<String, i64> = {
        let mut stmt = db.conn().prepare(
            "SELECT file_path, file_mtime FROM session_log_sync WHERE scan_type IN ('session','both')",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?;
        rows.filter_map(|r| r.ok()).collect()
    };

    let jsonl_files = collect_jsonl_files(&projects_dir);
    let total = jsonl_files.len();
    let mut imported = 0usize;
    let mut updated = 0usize;
    let mut last_report = 0usize;
    let now_iso = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    const APP_TYPE: &str = "claude";

    for (idx, path) in jsonl_files.iter().enumerate() {
        let sid = path.file_stem().and_then(|n| n.to_str()).unwrap_or("");
        if sid.is_empty() || sid.starts_with("agent-") {
            continue;
        }

        let current_mtime = file_mtime_secs(path);
        let file_path_str = path.to_string_lossy().to_string();
        if let Some(&stored_mtime) = file_index.get(&file_path_str) {
            if stored_mtime == current_mtime {
                continue;
            }
        }

        match parse_session_file(path) {
            Ok(Some(record)) => {
                db.insert_session(&record, APP_TYPE)?;
                if file_index.contains_key(&file_path_str) {
                    updated += 1;
                } else {
                    imported += 1;
                }
                db.conn().execute(
                    "INSERT OR REPLACE INTO session_log_sync (file_path, file_mtime, scan_type, last_synced_at)
                     VALUES (?1, ?2, 'session', ?3)",
                    rusqlite::params![file_path_str, current_mtime, now_iso],
                )?;
            }
            Ok(None) => {}
            Err(e) => { tracing::warn!("Failed to parse session file {:?}: {}", path, e); }
        }

        let files_done = idx + 1;
        if files_done - last_report >= 5 || files_done == total {
            on_progress(files_done, total, imported + updated);
            last_report = files_done;
        }
    }

    Ok(imported + updated)
}

pub fn import_claude_sessions(db: &Db) -> Result<usize, anyhow::Error> {
    import_claude_sessions_with_progress(db, |_, _, _| {})
}

fn file_mtime_secs(path: &PathBuf) -> i64 {
    file_mtime(path).unwrap_or(0)
}

fn collect_jsonl_files(dir: &PathBuf) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(collect_jsonl_files(&path));
            } else if path.extension().map_or(false, |e| e == "jsonl") {
                files.push(path);
            }
        }
    }
    files
}

/// Scan the last 30 lines of a JSONL file for an assistant message with `ccs_proxy` marker.
fn detect_mode(lines: &[&str]) -> String {
    for line in lines.iter().rev().take(30) {
        let parsed: JsonlLine = match serde_json::from_str(line) {
            Ok(l) => l,
            Err(_) => continue,
        };
        if parsed.msg_type.as_deref() == Some("assistant") {
            if let Some(ref msg) = parsed.message {
                if msg.ccs_proxy == Some(true) {
                    return "proxy".to_string();
                }
            }
            // First assistant message found without proxy marker → local
            return "local".to_string();
        }
    }
    "local".to_string()
}

fn parse_session_file(path: &PathBuf) -> Result<Option<SessionRecord>, anyhow::Error> {
    let meta = std::fs::metadata(path)?;
    let size_bytes = meta.len() as i64;
    let file_mtime = meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| {
            let secs = d.as_secs() as i64;
            ts_to_iso(secs * 1000)
        })
        .unwrap_or_default();
    let content = std::fs::read_to_string(path)?;
    let lines: Vec<&str> = content.lines().collect();

    if lines.is_empty() {
        return Ok(None);
    }

    let head_count = 50.min(lines.len());
    let tail_count = 30.min(lines.len());
    let message_count = lines.iter().filter(|l| !l.trim().is_empty()).count() as i64;

    let mut session_id: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut created_at: Option<i64> = None;
    let mut custom_title: Option<String> = None;
    let mut ai_title: Option<String> = None;
    let mut last_prompt: Option<String> = None;

    for (i, line) in lines.iter().enumerate() {
        let in_range = i < head_count || i >= lines.len().saturating_sub(tail_count);
        if !in_range {
            if let Some(title) = parse_title_only(line) {
                match title {
                    TitleField::Custom(t) => { custom_title = Some(t); }
                    TitleField::Ai(t) => { ai_title = Some(t); }
                    TitleField::LastPrompt(t) => { last_prompt = Some(truncate_title(&t)); }
                }
            }
            continue;
        }
        let parsed: JsonlLine = match serde_json::from_str(line) { Ok(l) => l, Err(_) => continue };
        if let Some(ref sid) = parsed.session_id { if session_id.is_none() { session_id = Some(sid.clone()); } }
        if let Some(ref c) = parsed.cwd { if cwd.is_none() { cwd = Some(c.clone()); } }
        if let Some(ref ts) = parsed.timestamp { if created_at.is_none() { created_at = parse_timestamp(ts); } }
        if let Some(ref ct) = parsed.custom_title { if !ct.is_empty() { custom_title = Some(ct.clone()); } }
        if let Some(ref at) = parsed.ai_title { if !at.is_empty() { ai_title = Some(at.clone()); } }
        if let Some(ref lp) = parsed.last_prompt { if !lp.is_empty() { last_prompt = Some(truncate_title(lp)); } }
    }

    let mut fallback_title: Option<String> = None;
    if custom_title.is_none() && ai_title.is_none() && last_prompt.is_none() {
        for line in lines.iter().rev().take(tail_count) {
            let parsed: JsonlLine = match serde_json::from_str(line) { Ok(l) => l, Err(_) => continue };
            if parsed.msg_type.as_deref() == Some("user") {
                if let Some(ref msg) = parsed.message {
                    if let Some(ref content) = msg.content {
                        if let Some(text) = content.as_str() {
                            let t = text.trim();
                            if t.is_empty() { continue; }
                            if t.starts_with('<') {
                                if let Some(cmd) = extract_command(t) {
                                    fallback_title = Some(cmd);
                                    break;
                                }
                                continue;
                            }
                            fallback_title = Some(truncate_title(t));
                            break;
                        }
                    }
                }
            }
        }
    }

    let session_id = session_id.or_else(|| {
        path.file_stem().and_then(|n| n.to_str()).map(|s| s.to_string())
    }).unwrap_or_default();

    if session_id.is_empty() {
        return Ok(None);
    }

    let cwd = cwd.unwrap_or_default();
    let start_time = created_at.map(ts_to_iso).unwrap_or_default();
    let project_name = std::path::Path::new(&cwd)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let title = custom_title
        .or(ai_title)
        .or(last_prompt)
        .or(fallback_title)
        .unwrap_or(project_name);

    // Detect proxy mode: scan last 30 lines for assistant message with ccs_proxy marker
    let mode = detect_mode(&lines);

    let search_text = format!("{} {}", title, cwd).to_lowercase();
    Ok(Some(SessionRecord {
        id: session_id,
        project_path: cwd,
        profile_id: None,
        mode,
        start_time,
        end_time: None,
        prompt_tokens: 0,
        completion_tokens: 0,
        message_count,
        search_text,
        title: Some(title),
        size_bytes,
        file_mtime,
    }))
}

// ── Usage Scan (background) ──────────────────────────────────────

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
    /// Actual upstream model name injected by CCSwitch proxy
    #[serde(default)]
    ccs_model: Option<String>,
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

/// Background-thread function: collect changed files, parse them, send batches via channel.
pub fn parse_files_in_background(
    app_type: String,
    ctx: ScanContext,
    batch_size: usize,
    tx: std::sync::mpsc::Sender<ScanEvent>,
) {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let projects_dir = PathBuf::from(&home).join(".claude/projects");
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
        let records = parse_single_file(path, &known_set);
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

fn parse_single_file(
    path: &PathBuf,
    known_msg_ids: &std::collections::HashSet<&str>,
) -> Vec<UsageRecord> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    content
        .lines()
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
            // Prefer ccs_model (actual upstream model from proxy) over message.model
            let model = msg
                .ccs_model
                .as_deref()
                .or(msg.model.as_deref())
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

// ── Helpers ──────────────────────────────────────────────────────

pub fn collect_changed_files(
    dir: &PathBuf,
    out: &mut Vec<(PathBuf, String)>,
    file_index: &HashMap<String, i64>,
) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_changed_files(&path, out, file_index);
            } else if path.extension().map_or(false, |e| e == "jsonl") {
                let sid = path.file_stem().and_then(|n| n.to_str()).unwrap_or("").to_string();
                if !sid.is_empty() {
                    let mtime = file_mtime(&path).unwrap_or(0);
                    let file_path_str = path.to_string_lossy().to_string();
                    let changed = file_index.get(&file_path_str).map_or(true, |&old| old != mtime);
                    if changed {
                        out.push((path, sid));
                    }
                }
            }
        }
    }
}

/// Read file mtime as unix timestamp (seconds) — public for db/usage.rs
pub fn file_mtime(path: &PathBuf) -> Option<i64> {
    let meta = std::fs::metadata(path).ok()?;
    let dur = meta.modified().ok()?;
    let secs = dur.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(secs.as_secs() as i64)
}
