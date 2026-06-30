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
    let size_bytes = std::fs::metadata(path).map(|m| m.len() as i64).unwrap_or(0);
    // Read head lines for session metadata + title
    let content = std::fs::read_to_string(path)?;
    let lines: Vec<&str> = content.lines().collect();

    if lines.is_empty() {
        return Ok(None);
    }

    let mut session_id: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut created_at: Option<i64> = None;
    let mut custom_title: Option<String> = None;
    let mut ai_title: Option<String> = None;
    let mut last_prompt: Option<String> = None;
    let mut message_count: i64 = 0;

    // Parse all lines for metadata, titles, and message count
    for line in lines.iter() {
        let parsed: JsonlLine = match serde_json::from_str(line) {
            Ok(l) => l,
            Err(_) => continue,
        };

        // Collect metadata (first one wins)
        if let Some(ref sid) = parsed.session_id {
            if session_id.is_none() { session_id = Some(sid.clone()); }
        }
        if let Some(ref c) = parsed.cwd {
            if cwd.is_none() { cwd = Some(c.clone()); }
        }
        if let Some(ref ts) = parsed.timestamp {
            if created_at.is_none() {
                created_at = parse_timestamp(ts);
            }
        }

        // Collect titles: custom-title/ai-title take first, last-prompt takes last
        if let Some(ref ct) = parsed.custom_title {
            if custom_title.is_none() && !ct.is_empty() { custom_title = Some(ct.clone()); }
        }
        if let Some(ref at) = parsed.ai_title {
            if ai_title.is_none() && !at.is_empty() { ai_title = Some(at.clone()); }
        }
        if let Some(ref lp) = parsed.last_prompt {
            if !lp.is_empty() { last_prompt = Some(truncate_title(lp)); } // always take latest
        }

        // Count non-meta messages
        if parsed.is_meta != Some(true) && parsed.message.is_some() {
            message_count += 1;
        }
    }

    // Fallback: extract last real user message (skip system commands)
    let mut fallback_title: Option<String> = None;
    if custom_title.is_none() && ai_title.is_none() && last_prompt.is_none() {
        for line in lines.iter().rev() {
            let parsed: JsonlLine = match serde_json::from_str(line) { Ok(l) => l, Err(_) => continue };
            if parsed.msg_type.as_deref() == Some("user") {
                if let Some(ref msg) = parsed.message {
                    if let Some(ref content) = msg.content {
                        if let Some(text) = content.as_str() {
                            let t = text.trim();
                            if t.is_empty() { continue; }
                            // Skip slash commands and system messages
                            if t.starts_with('/') || t.starts_with('<') { continue; }
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
    }))
}
