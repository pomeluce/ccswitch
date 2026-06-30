use rusqlite::params;
use serde::Serialize;
use super::connection::Db;

#[derive(Debug, Clone, Serialize)]
pub struct UsageSummary {
    pub provider_id: String,
    pub profile_id: String,
    pub total_prompt: i64,
    pub total_completion: i64,
    pub request_count: i64,
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
}
