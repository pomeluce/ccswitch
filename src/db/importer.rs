use std::path::PathBuf;
use serde::Deserialize;
use crate::db::sessions::SessionRecord;
use super::connection::Db;

/// Minimal session metadata from ~/.claude/sessions/*.json
#[derive(Debug, Deserialize)]
struct SessionMeta {
    #[serde(rename = "sessionId")]
    session_id: String,
    cwd: Option<String>,
    #[serde(rename = "startedAt")]
    started_at: Option<i64>,
    #[serde(rename = "updatedAt")]
    updated_at: Option<i64>,
    status: Option<String>,
}

/// Message entry from ~/.claude/projects/<hash>/<id>.jsonl
#[derive(Debug, Deserialize)]
struct SessionMessage {
    uuid: Option<String>,
    #[allow(dead_code)]
    #[serde(rename = "type")]
    msg_type: Option<String>,
    #[serde(rename = "userType")]
    user_type: Option<String>,
    #[allow(dead_code)]
    #[serde(rename = "parentUuid")]
    parent_uuid: Option<String>,
    attachment: Option<serde_json::Value>,
    #[allow(dead_code)]
    timestamp: Option<i64>,
}

fn claude_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".claude")
}

fn sessions_dir() -> PathBuf {
    claude_dir().join("sessions")
}

/// Convert a project path like "/home/user/my-project" to the hash used in ~/.claude/projects/
fn project_hash(cwd: &str) -> String {
    cwd.replace('/', "-")
}

impl Db {
    /// Scan Claude Code's local session files and import them into session_history.
    /// Skips sessions that already exist in the database.
    pub fn import_claude_sessions(&self) -> Result<usize, anyhow::Error> {
        let sessions_dir = sessions_dir();
        if !sessions_dir.exists() {
            return Ok(0);
        }

        let mut imported = 0usize;

        for entry in std::fs::read_dir(&sessions_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().map_or(true, |e| e != "json") {
                continue;
            }

            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let meta: SessionMeta = match serde_json::from_str(&content) {
                Ok(m) => m,
                Err(_) => continue,
            };

            let session_id = meta.session_id;
            if session_id.is_empty() {
                continue;
            }

            // Skip if already imported
            if self.session_exists(&session_id)? {
                continue;
            }

            let cwd = meta.cwd.unwrap_or_default();
            let start_time = meta
                .started_at
                .map(ts_to_iso)
                .unwrap_or_default();

            let end_time = if meta.status.as_deref() == Some("busy") {
                None // Still active
            } else {
                meta.updated_at.map(ts_to_iso)
            };

            // Try to extract title and message count from project session data
            let (title, message_count) = read_session_details(&cwd, &session_id);

            let mode = "local"; // Sessions are from local Claude Code

            let record = SessionRecord {
                id: session_id,
                project_path: cwd,
                profile_id: None,
                mode: mode.to_string(),
                start_time,
                end_time,
                prompt_tokens: 0,
                completion_tokens: 0,
                message_count,
                title,
            };

            self.insert_session(&record)?;
            imported += 1;
        }

        Ok(imported)
    }

    fn session_exists(&self, id: &str) -> Result<bool, rusqlite::Error> {
        let count: i64 = self.conn().query_row(
            "SELECT COUNT(*) FROM session_history WHERE id = ?1",
            rusqlite::params![id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }
}

/// Read session details from ~/.claude/projects/<hash>/<id>.jsonl
/// Returns (title, message_count) where title is the first user message text
fn read_session_details(cwd: &str, session_id: &str) -> (Option<String>, i64) {
    let jsonl_path = claude_dir()
        .join("projects")
        .join(project_hash(cwd))
        .join(format!("{}.jsonl", session_id));

    if !jsonl_path.exists() {
        return (None, 0);
    }

    let content = match std::fs::read_to_string(&jsonl_path) {
        Ok(c) => c,
        Err(_) => return (None, 0),
    };

    let mut message_count: i64 = 0;
    let mut first_user_message: Option<String> = None;

    for line in content.lines() {
        let msg: SessionMessage = match serde_json::from_str(line) {
            Ok(m) => m,
            Err(_) => continue,
        };

        // Only count actual message events (skip snapshot/attachment-only)
        if msg.uuid.is_some() {
            message_count += 1;
        }

        // Capture first user message as title
        if first_user_message.is_none() && msg.user_type.as_deref() == Some("user") {
            if let Some(attachment) = &msg.attachment {
                // Extract text content from attachment
                if let Some(content_array) = attachment.as_array() {
                    for block in content_array {
                        if block
                            .get("type")
                            .and_then(|t| t.as_str())
                            .filter(|t| *t == "text")
                            .is_some()
                        {
                            if let Some(text_content) = block.get("text").and_then(|t| t.as_str()) {
                                // Truncate to first line / reasonable length
                                let title = text_content
                                    .lines()
                                    .next()
                                    .unwrap_or("")
                                    .chars()
                                    .take(80)
                                    .collect::<String>();
                                if !title.is_empty() {
                                    first_user_message = Some(title);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    (first_user_message, message_count)
}

fn ts_to_iso(ts: i64) -> String {
    let secs = ts / 1000;
    let nanos = ((ts % 1000) * 1_000_000) as u32;
    match chrono::TimeZone::timestamp_opt(&chrono::Utc, secs, nanos) {
        chrono::offset::LocalResult::Single(dt) => dt.format("%Y-%m-%d %H:%M:%S").to_string(),
        _ => String::new(),
    }
}
