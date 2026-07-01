use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use rayon::prelude::*;
use super::connection::Db;

#[derive(Debug, Clone, Serialize)]
pub struct UsageSummary {
    pub model: String,
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

/// Parsed usage record awaiting DB insert
struct UsageRecord {
    sid: String, msg_id: String, model: String,
    date: String,
    input: i64, output: i64, cr: i64, cc: i64,
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
            params![provider_id, provider_id, profile_id, mode, session_id, prompt_tokens, completion_tokens, cache_read_tokens, cache_create_tokens, total],
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
             FROM usage_logs WHERE {} GROUP BY model ORDER BY SUM(total_tokens) DESC",
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
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?, row.get::<_, i64>(4)?))
        })?;
        rows.collect()
    }

    /// Scan all local JSONL files, dedup by message.id only.
    /// Full scan on every startup — fast because known IDs are skipped.
    pub fn scan_local_usage(&self) -> Result<usize, anyhow::Error> {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        let projects_dir = PathBuf::from(&home).join(".claude/projects");
        if !projects_dir.exists() { return Ok(0); }

        let jsonl_files = collect_jsonl_files(&projects_dir);

        // Pre-load existing message IDs for dedup
        let known_msg_ids: std::collections::HashSet<String> = {
            let mut stmt = self.conn().prepare("SELECT message_id FROM usage_logs WHERE message_id IS NOT NULL AND message_id != ''")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            rows.filter_map(|r| r.ok()).collect()
        };

        if jsonl_files.is_empty() { return Ok(0); }

        // Parallel parse all files
        let all_records: Vec<Vec<UsageRecord>> = jsonl_files.par_iter().filter_map(|path| {
            let content = std::fs::read_to_string(path).ok()?;
            let sid = path.file_stem().and_then(|n| n.to_str()).unwrap_or("");
            let records: Vec<UsageRecord> = content.lines().filter_map(|line| {
                let parsed: UsageLine = serde_json::from_str(line).ok()?;
                if parsed.msg_type.as_deref() != Some("assistant") { return None; }
                let msg = parsed.message.as_ref()?;
                let usage = msg.usage.as_ref()?;
                let msg_id = msg.id.as_deref().unwrap_or("").to_string();
                if !msg_id.is_empty() && known_msg_ids.contains(&msg_id) { return None; }
                let model = msg.model.as_deref().unwrap_or("unknown").replace("[1m]", "");
                let date = parsed.timestamp.as_deref().unwrap_or("").get(0..10).unwrap_or("today").to_string();
                Some(UsageRecord {
                    sid: sid.to_string(), msg_id, model: model.to_string(), date,
                    input: usage.input_tokens.unwrap_or(0),
                    output: usage.output_tokens.unwrap_or(0),
                    cr: usage.cache_read_input_tokens.unwrap_or(0),
                    cc: usage.cache_creation_input_tokens.unwrap_or(0),
                })
            }).collect();
            if records.is_empty() { None } else { Some(records) }
        }).collect();

        // Serial DB insert
        let mut imported = 0usize;
        for records in &all_records {
            for r in records {
                self.conn().execute(
                    "INSERT OR IGNORE INTO usage_logs (model, provider_id, profile_id, mode, session_id, message_id, prompt_tokens, completion_tokens, cache_read_tokens, cache_create_tokens, total_tokens, timestamp)
                     VALUES (?1, ?2, ?3, 'local', ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                    params![r.model, "", "", r.sid, r.msg_id, r.input, r.output, r.cr, r.cc, r.input + r.output + r.cr + r.cc, format!("{} 00:00:00", r.date)],
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
