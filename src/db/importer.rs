use std::path::PathBuf;
use serde::Deserialize;
use crate::db::sessions::SessionRecord;
use super::connection::Db;

/// A line from a Claude Code session JSONL file
#[derive(Debug, Deserialize)]
struct JsonlLine {
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    cwd: Option<String>,
    timestamp: Option<serde_json::Value>, // number (ms/s) or RFC3339 string
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
}

fn claude_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".claude")
}

fn projects_dir() -> PathBuf {
    claude_dir().join("projects")
}

enum TitleField {
    Custom(String),
    Ai(String),
    LastPrompt(String),
}

/// Lightweight string-based title extraction (avoids full JSON parse for middle lines)
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

/// Truncate title to 40 chars max
fn truncate_title(s: &str) -> String {
    let s = s.trim();
    if s.chars().count() > 40 {
        format!("{}...", s.chars().take(37).collect::<String>())
    } else {
        s.to_string()
    }
}

/// Parse timestamp that could be milliseconds or seconds
fn parse_timestamp(val: &serde_json::Value) -> Option<i64> {
    match val {
        serde_json::Value::Number(n) => {
            let ts = n.as_f64()? as i64;
            // > 1e12 = milliseconds, <= 1e12 = seconds → convert to ms
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

/// Extract a readable command string from XML content like
/// "<command-name>/clear</command-name> <command-message>clear</command-message> <command-args>foo</command-args>"
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

impl Db {
    /// Import with progress callback. Incremental: only processes files whose
    /// mtime differs from the stored index in session_file_track.
    /// Changed/new files get INSERT OR REPLACE (full field refresh).
    pub fn import_claude_sessions_with_progress(
        &self,
        on_progress: impl Fn(usize, usize, usize),
    ) -> Result<usize, anyhow::Error> {
        let projects_dir = projects_dir();
        if !projects_dir.exists() {
            return Ok(0);
        }

        // Load stored file index: session_id -> mtime
        let file_index: std::collections::HashMap<String, i64> = {
            let mut stmt = self.conn().prepare(
                "SELECT session_id, file_mtime FROM session_file_track"
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

        for (idx, path) in jsonl_files.iter().enumerate() {
            let sid = path.file_stem().and_then(|n| n.to_str()).unwrap_or("");
            if sid.is_empty() || sid.starts_with("agent-") {
                continue;
            }

            // Check if file mtime changed (incremental)
            let current_mtime = file_mtime_secs(path);
            if let Some(&stored_mtime) = file_index.get(sid) {
                if stored_mtime == current_mtime {
                    continue; // Unchanged — skip
                }
            }

            match parse_session_file(path) {
                Ok(Some(record)) => {
                    // INSERT OR REPLACE refreshes all fields when file changed
                    self.insert_session(&record)?;
                    if file_index.contains_key(sid) {
                        updated += 1;
                    } else {
                        imported += 1;
                    }
                    // Update file index
                    self.conn().execute(
                        "INSERT OR REPLACE INTO session_file_track (session_id, file_mtime, scanned_at) VALUES (?1, ?2, ?3)",
                        rusqlite::params![sid, current_mtime, now_iso],
                    )?;
                }
                Ok(None) => {}
                Err(_) => {}
            }

            let files_done = idx + 1;
            if files_done - last_report >= 5 || files_done == total {
                on_progress(files_done, total, imported + updated);
                last_report = files_done;
            }
        }

        Ok(imported + updated)
    }

    /// Scan ~/.claude/projects/ recursively for Claude Code session JSONL files
    /// and import them into session_history (no progress callback).
    pub fn import_claude_sessions(&self) -> Result<usize, anyhow::Error> {
        self.import_claude_sessions_with_progress(|_, _, _| {})
    }
}

/// Get file modification time as unix timestamp (seconds)
fn file_mtime_secs(path: &std::path::PathBuf) -> i64 {
    std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Recursively collect all .jsonl files under a directory
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

/// Parse a single Claude Code session JSONL file
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
    // Approximate message count from total lines (skip empty)
    let message_count = lines.iter().filter(|l| !l.trim().is_empty()).count() as i64;

    let mut session_id: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut created_at: Option<i64> = None;
    let mut custom_title: Option<String> = None;
    let mut ai_title: Option<String> = None;
    let mut last_prompt: Option<String> = None;

    // Single pass: parse head+tail for metadata, all lines for titles
    for (i, line) in lines.iter().enumerate() {
        let in_range = i < head_count || i >= lines.len().saturating_sub(tail_count);
        if !in_range {
            // Only parse titles from middle lines (skip full JSON parse)
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

    // Fallback: extract last user message from tail
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

    // Session ID fallback: use filename stem (UUID)
    let session_id = session_id.or_else(|| {
        path.file_stem().and_then(|n| n.to_str()).map(|s| s.to_string())
    }).unwrap_or_default();

    if session_id.is_empty() {
        return Ok(None);
    }

    let cwd = cwd.unwrap_or_default();
    let start_time = created_at.map(ts_to_iso).unwrap_or_default();

    // Title: custom-title > first user message > project basename
    let project_name = std::path::Path::new(&cwd)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    // Title priority: custom-title > ai-title > last-prompt > fallback user msg > project name
    let title = custom_title
        .or(ai_title)
        .or(last_prompt)
        .or(fallback_title)
        .unwrap_or(project_name);

    Ok(Some(SessionRecord {
        id: session_id,
        project_path: cwd,
        profile_id: None,
        mode: "local".to_string(),
        start_time,
        end_time: None,
        prompt_tokens: 0,
        completion_tokens: 0,
        message_count,
        title: Some(title),
        size_bytes,
        file_mtime,
    }))
}
