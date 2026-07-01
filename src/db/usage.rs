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
    pub total_cache_read: i64,
    pub total_cache_create: i64,
    pub request_count: i64,
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
            "INSERT INTO usage_logs (provider_id, profile_id, mode, session_id, prompt_tokens, completion_tokens, cache_read_tokens, cache_create_tokens, total_tokens)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![provider_id, profile_id, mode, session_id, prompt_tokens, completion_tokens, cache_read_tokens, cache_create_tokens, total],
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
            "SELECT provider_id, profile_id, SUM(prompt_tokens), SUM(completion_tokens), SUM(cache_read_tokens), SUM(cache_create_tokens), COUNT(*)
             FROM usage_logs WHERE {} GROUP BY provider_id, profile_id ORDER BY SUM(total_tokens) DESC",
            date_filter
        );
        let mut stmt = self.conn().prepare(&sql)?;
        let rows = stmt.query_map([], |row| {
            Ok(UsageSummary {
                provider_id: row.get(0)?,
                profile_id: row.get(1)?,
                total_prompt: row.get(2)?,
                total_completion: row.get(3)?,
                total_cache_read: row.get(4)?,
                total_cache_create: row.get(5)?,
                request_count: row.get(6)?,
            })
        })?;
        rows.collect()
    }

    /// Query per-day usage breakdown for a specific profile
    pub fn query_daily_usage(&self, provider: &str, profile: &str) -> Result<Vec<(String, i64, i64, i64, i64)>, rusqlite::Error> {
        let sql = "SELECT date(timestamp) as day,
                          SUM(prompt_tokens), SUM(completion_tokens),
                          SUM(cache_read_tokens), SUM(cache_create_tokens)
                   FROM usage_logs
                   WHERE provider_id = ?1 AND profile_id = ?2
                     AND date(timestamp) >= date('now', '-6 days')
                   GROUP BY day ORDER BY day";
        let mut stmt = self.conn().prepare(sql)?;
        let rows = stmt.query_map(params![provider, profile], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?, row.get::<_, i64>(4)?))
        })?;
        rows.collect()
    }

    /// Scan local JSONL files, store each assistant message as a row,
    /// dedup by message.id, track file mtime for incremental updates.
    pub fn scan_local_usage(&self) -> Result<usize, anyhow::Error> {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        let projects_dir = PathBuf::from(&home).join(".claude/projects");
        if !projects_dir.exists() { return Ok(0); }

        let jsonl_files = collect_jsonl_files(&projects_dir);
        let model_map = self.build_model_profile_map();
        let mut imported = 0usize;

        // Pre-load existing message IDs into a HashSet for fast dedup
        let mut known_msg_ids: std::collections::HashSet<String> = {
            let mut stmt = self.conn().prepare("SELECT message_id FROM usage_logs WHERE message_id IS NOT NULL")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            rows.filter_map(|r| r.ok()).collect()
        };

        for path in &jsonl_files {
            let sid = path.file_stem().and_then(|n| n.to_str()).unwrap_or("");
            let mtime = std::fs::metadata(path)
                .and_then(|m| m.modified())
                .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64)
                .unwrap_or(0);
            if mtime == 0 { continue; }

            // Check mtime from tracking table
            let last_mtime: i64 = self.conn().query_row(
                "SELECT file_mtime FROM session_usage_track WHERE session_id = ?1",
                params![sid], |r| r.get(0),
            ).unwrap_or(0);

            if last_mtime >= mtime { continue; } // unchanged

            let content = match std::fs::read_to_string(path) { Ok(c) => c, Err(_) => continue };
            let mut count = 0usize;

            for line in content.lines() {
                let parsed: UsageLine = match serde_json::from_str(line) { Ok(l) => l, Err(_) => continue };
                if parsed.msg_type.as_deref() != Some("assistant") { continue; }
                let msg = match &parsed.message { Some(m) => m, None => continue };
                let usage = match &msg.usage { Some(u) => u, None => continue };

                // Dedup by message.id
                let msg_id = msg.id.as_deref().unwrap_or("");
                if !msg_id.is_empty() && known_msg_ids.contains(msg_id) { continue; }

                let model = msg.model.as_deref().unwrap_or("unknown");
                let (pid, pfid) = model_map.get(model).cloned().unwrap_or(("Claude Code".into(), "local".into()));
                let date = parsed.timestamp.as_deref().unwrap_or("").get(0..10).unwrap_or("today").to_string();
                let input = usage.input_tokens.unwrap_or(0);
                let output = usage.output_tokens.unwrap_or(0);
                let cr = usage.cache_read_input_tokens.unwrap_or(0);
                let cc = usage.cache_creation_input_tokens.unwrap_or(0);

                self.conn().execute(
                    "INSERT INTO usage_logs (provider_id, profile_id, mode, session_id, message_id, prompt_tokens, completion_tokens, cache_read_tokens, cache_create_tokens, total_tokens, timestamp)
                     VALUES (?1, ?2, 'local', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    params![pid, pfid, sid, msg_id, input, output, cr, cc, input + output + cr + cc, format!("{} 00:00:00", date)],
                )?;
                if !msg_id.is_empty() { known_msg_ids.insert(msg_id.to_string()); }
                count += 1;
            }

            // Update mtime tracking
            self.conn().execute(
                "INSERT OR REPLACE INTO session_usage_track (session_id, file_mtime) VALUES (?1, ?2)",
                params![sid, mtime],
            )?;
            imported += count;
        }
        Ok(imported)
    }

    /// Build a mapping from model name to (provider_id, profile_id) by reading settings
    fn build_model_profile_map(&self) -> std::collections::HashMap<String, (String, String)> {
        let mut map = std::collections::HashMap::new();
        // Try to get from configuration
        if let Ok(providers) = self.conn().prepare("SELECT id, name FROM user_providers")
            .and_then(|mut s| s.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?.collect::<Result<Vec<_>, _>>())
        {
            for (pid, _) in &providers {
                if let Ok(profiles) = self.conn().prepare(
                    "SELECT id, opus_model, sonnet_model, haiku_model, subagent_model FROM user_profiles WHERE provider_id = ?1"
                ).and_then(|mut s| s.query_map(params![pid], |r| Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                )))?.collect::<Result<Vec<_>, _>>())
                {
                    for (pfid, opus, sonnet, haiku, subagent) in &profiles {
                        for m in [opus, sonnet, haiku, subagent] {
                            let clean = m.replace("[1m]", "");
                            map.insert(clean, (pid.clone(), pfid.clone()));
                            map.insert(m.clone(), (pid.clone(), pfid.clone())); // also keep original
                        }
                    }
                }
            }
        }
        map
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
