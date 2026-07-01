use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use super::connection::Db;

#[derive(Debug, Clone, Serialize)]
pub struct UsageSummary {
    pub provider_id: String,
    pub profile_id: String,
    pub total_prompt: i64,
    pub total_completion: i64,
    pub request_count: i64,
}

/// Lightweight parser for assistant message usage data in JSONL files
#[derive(Debug, Deserialize)]
struct UsageLine {
    #[serde(rename = "type")]
    msg_type: Option<String>,
    message: Option<UsageMessage>,
}

#[derive(Debug, Deserialize)]
struct UsageMessage {
    usage: Option<UsageData>,
}

#[derive(Debug, Deserialize)]
struct UsageData {
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
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
    ) -> Result<(), rusqlite::Error> {
        self.conn().execute(
            "INSERT INTO usage_logs (provider_id, profile_id, mode, session_id, prompt_tokens, completion_tokens)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![provider_id, profile_id, mode, session_id, prompt_tokens, completion_tokens],
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
            "SELECT provider_id, profile_id, SUM(prompt_tokens), SUM(completion_tokens), COUNT(*)
             FROM usage_logs WHERE {} GROUP BY provider_id, profile_id ORDER BY SUM(prompt_tokens + completion_tokens) DESC",
            date_filter
        );
        let mut stmt = self.conn().prepare(&sql)?;
        let rows = stmt.query_map([], |row| {
            Ok(UsageSummary {
                provider_id: row.get(0)?,
                profile_id: row.get(1)?,
                total_prompt: row.get(2)?,
                total_completion: row.get(3)?,
                request_count: row.get(4)?,
            })
        })?;
        rows.collect()
    }

    /// Query per-day usage for a specific profile
    pub fn query_daily_usage(&self, provider: &str, profile: &str) -> Result<Vec<(String, i64)>, rusqlite::Error> {
        let sql = "SELECT date(timestamp) as day, SUM(prompt_tokens + completion_tokens)
                   FROM usage_logs
                   WHERE provider_id = ?1 AND profile_id = ?2
                     AND date(timestamp) >= date('now', '-6 days')
                   GROUP BY day ORDER BY day";
        let mut stmt = self.conn().prepare(sql)?;
        let rows = stmt.query_map(params![provider, profile], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        rows.collect()
    }

    /// Scan Claude Code session JSONL files for assistant message usage data.
    /// Only imports sessions not already in usage_logs.
    pub fn scan_local_usage(&self) -> Result<usize, anyhow::Error> {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        let projects_dir = PathBuf::from(&home).join(".claude/projects");
        if !projects_dir.exists() { return Ok(0); }

        let jsonl_files = collect_jsonl_files(&projects_dir);
        // Track which sessions we've already imported
        let existing: std::collections::HashSet<String> = {
            let mut stmt = self.conn().prepare("SELECT DISTINCT session_id FROM usage_logs WHERE mode = 'local'")?;
            let rows = stmt.query_map([], |r| r.get::<_, Option<String>>(0))?;
            rows.filter_map(|r| r.ok().flatten()).collect()
        };

        let mut imported = 0usize;
        for path in &jsonl_files {
            let sid = path.file_stem().and_then(|n| n.to_str()).unwrap_or("");
            if existing.contains(sid) { continue; }
            let content = match std::fs::read_to_string(path) { Ok(c) => c, Err(_) => continue };
            let mut prompt = 0i64;
            let mut completion = 0i64;
            for line in content.lines().take(200) { // head only for performance
                let parsed: UsageLine = match serde_json::from_str(line) { Ok(l) => l, Err(_) => continue };
                if parsed.msg_type.as_deref() == Some("assistant") {
                    if let Some(ref msg) = parsed.message {
                        if let Some(ref u) = msg.usage {
                            prompt += u.input_tokens.unwrap_or(0);
                            completion += u.output_tokens.unwrap_or(0);
                        }
                    }
                }
            }
            if prompt > 0 || completion > 0 {
                self.insert_usage_log(
                    "Claude Code", "local", "local", Some(sid), prompt, completion,
                )?;
                imported += 1;
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
            if path.is_dir() { files.extend(collect_jsonl_files(&path)); }
            else if path.extension().map_or(false, |e| e == "jsonl") { files.push(path); }
        }
    }
    files
}
