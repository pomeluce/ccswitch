use rusqlite::params;
use serde::Serialize;
use super::connection::Db;

#[derive(Debug, Clone, Serialize)]
pub struct SessionRecord {
    pub id: String,
    pub project_path: String,
    pub profile_id: Option<String>,
    pub mode: String,
    pub start_time: String,
    pub end_time: Option<String>,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub message_count: i64,
    pub title: Option<String>,
}

impl Db {
    pub fn insert_session(&self, s: &SessionRecord) -> Result<(), rusqlite::Error> {
        self.conn().execute(
            "INSERT OR REPLACE INTO session_history (id, project_path, profile_id, mode, start_time, end_time, prompt_tokens, completion_tokens, message_count, title)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![s.id, s.project_path, s.profile_id, s.mode, s.start_time, s.end_time, s.prompt_tokens, s.completion_tokens, s.message_count, s.title],
        )?;
        Ok(())
    }

    pub fn query_sessions(
        &self,
        project: Option<&str>,
        search: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SessionRecord>, rusqlite::Error> {
        let mut sql = String::from(
            "SELECT id, project_path, profile_id, mode, start_time, end_time, prompt_tokens, completion_tokens, message_count, title
             FROM session_history WHERE 1=1"
        );
        let mut param_values: Vec<String> = vec![];

        if let Some(p) = project {
            param_values.push(p.to_string());
            sql.push_str(&format!(" AND project_path LIKE '%{}%'", p));
        }
        if let Some(s) = search {
            param_values.push(format!("%{}%", s));
            sql.push_str(&format!(" AND (title LIKE '%{}%' OR id LIKE '%{}%')", s, s));
        }
        sql.push_str(" ORDER BY start_time DESC LIMIT ?");
        let limit_str = limit.to_string();

        let mut stmt = self.conn().prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(
            param_values.iter().map(|s| s.as_str()).chain(std::iter::once(limit_str.as_str()))
        ), |row| {
            Ok(SessionRecord {
                id: row.get(0)?,
                project_path: row.get(1)?,
                profile_id: row.get(2)?,
                mode: row.get(3)?,
                start_time: row.get(4)?,
                end_time: row.get(5)?,
                prompt_tokens: row.get(6)?,
                completion_tokens: row.get(7)?,
                message_count: row.get(8)?,
                title: row.get(9)?,
            })
        })?;
        rows.collect()
    }
}