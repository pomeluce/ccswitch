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
    msg_type: Option<String>,
    message: Option<MessageContent>,
    #[serde(rename = "customTitle")]
    custom_title: Option<String>,
    #[serde(rename = "isMeta")]
    is_meta: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct MessageContent {
    content: Option<serde_json::Value>, // string or array of blocks
    role: Option<String>,
}

fn claude_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".claude")
}

fn projects_dir() -> PathBuf {
    claude_dir().join("projects")
}

/// Extract readable text from content (string or structured array)
fn extract_text(content: &serde_json::Value) -> Option<String> {
    match content {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Array(arr) => {
            let mut parts: Vec<String> = Vec::new();
            for item in arr {
                if let Some(t) = item.get("text").and_then(|v| v.as_str()) {
                    parts.push(t.to_string());
                } else if let Some(t) = item.get("input_text").and_then(|v| v.as_str()) {
                    parts.push(t.to_string());
                } else if let Some(t) = item.get("output_text").and_then(|v| v.as_str()) {
                    parts.push(t.to_string());
                }
            }
            if parts.is_empty() { None } else { Some(parts.join(" ")) }
        }
        _ => None,
    }
}

/// Check if a message is a system-injected command (should be skipped for titles)
fn is_system_message(text: &str) -> bool {
    text.contains("<local-command-caveat>")
        || text.starts_with("<command-name>")
        || text.starts_with("/clear")
        || text.starts_with("/compact")
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

fn ts_to_iso(ts_ms: i64) -> String {
    let secs = ts_ms / 1000;
    let nanos = ((ts_ms % 1000) * 1_000_000) as u32;
    match chrono::TimeZone::timestamp_opt(&chrono::Utc, secs, nanos) {
        chrono::offset::LocalResult::Single(dt) => dt.format("%Y-%m-%d %H:%M:%S").to_string(),
        _ => String::new(),
    }
}

impl Db {
    /// Scan ~/.claude/projects/ recursively for Claude Code session JSONL files
    /// and import them into session_history.
    pub fn import_claude_sessions(&self) -> Result<usize, anyhow::Error> {
        let projects_dir = projects_dir();
        if !projects_dir.exists() {
            return Ok(0);
        }

        let jsonl_files = collect_jsonl_files(&projects_dir);
        let mut imported = 0usize;

        for path in &jsonl_files {
            // Skip sub-agent sessions
            if let Some(name) = path.file_stem().and_then(|n| n.to_str()) {
                if name.starts_with("agent-") {
                    continue;
                }
            }

            match parse_session_file(path) {
                Ok(Some(record)) => {
                    self.insert_session(&record)?;
                    imported += 1;
                }
                Ok(None) => {} // Empty or unparseable
                Err(_) => {}    // Skip corrupt files
            }
        }

        Ok(imported)
    }
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
    // Read head lines for session metadata + title
    let content = std::fs::read_to_string(path)?;
    let lines: Vec<&str> = content.lines().collect();

    if lines.is_empty() {
        return Ok(None);
    }

    let head_count = 30.min(lines.len());
    let tail_count = 10.min(lines.len());

    let mut session_id: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut created_at: Option<i64> = None;
    let mut last_active_at: Option<i64> = None;
    let mut first_user_message: Option<String> = None;
    let mut custom_title: Option<String> = None;
    let mut message_count: i64 = 0;

    // Parse head lines for metadata + first user message
    for line in &lines[..head_count] {
        let parsed: JsonlLine = match serde_json::from_str(line) {
            Ok(l) => l,
            Err(_) => continue,
        };

        // Collect metadata from any line that has it
        if let Some(ref sid) = parsed.session_id {
            if session_id.is_none() {
                session_id = Some(sid.clone());
            }
        }
        if let Some(ref c) = parsed.cwd {
            if cwd.is_none() {
                cwd = Some(c.clone());
            }
        }
        if let Some(ref ts) = parsed.timestamp {
            let ts_ms = parse_timestamp(ts);
            if created_at.is_none() {
                created_at = ts_ms;
            }
        }

        // Count non-meta messages
        if parsed.is_meta != Some(true) && parsed.message.is_some() {
            message_count += 1;
        }

        // Extract title from first non-system user message
        if first_user_message.is_none()
            && parsed.msg_type.as_deref() == Some("user")
            || parsed.message.as_ref().map(|m| m.role.as_deref() == Some("user")).unwrap_or(false)
        {
            if let Some(ref msg) = parsed.message {
                if let Some(ref content_val) = msg.content {
                    if let Some(text) = extract_text(content_val) {
                        if !is_system_message(&text) {
                            let raw = text.lines().next().unwrap_or("").trim();
                            // Truncate at word boundary (60 chars max)
                            let title = if raw.chars().count() > 60 {
                                let truncated: String = raw.chars().take(57).collect();
                                format!("{}...", truncated)
                            } else {
                                raw.to_string()
                            };
                            if !title.is_empty() {
                                first_user_message = Some(title);
                            }
                        }
                    }
                }
            }
        }
    }

    // Parse tail lines for custom-title + last_active_at
    for line in lines.iter().rev().take(tail_count) {
        let parsed: JsonlLine = match serde_json::from_str(line) {
            Ok(l) => l,
            Err(_) => continue,
        };

        if let Some(ref ts) = parsed.timestamp {
            if last_active_at.is_none() {
                last_active_at = parse_timestamp(ts);
            }
        }

        if custom_title.is_none() {
            if let Some(ref ct) = parsed.custom_title {
                if !ct.is_empty() {
                    custom_title = Some(ct.clone());
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
    let title = custom_title
        .or(first_user_message)
        .unwrap_or(project_name);

    Ok(Some(SessionRecord {
        id: session_id,
        project_path: cwd,
        profile_id: None,
        mode: "local".to_string(),
        start_time,
        end_time: None, // We don't have accurate end time from JSONL
        prompt_tokens: 0,
        completion_tokens: 0,
        message_count,
        title: Some(title),
    }))
}
