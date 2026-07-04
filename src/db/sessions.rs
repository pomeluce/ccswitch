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
    pub size_bytes: i64,
    /// JSONL file modification time (ISO string) — used for relative-time display
    pub file_mtime: String,
    /// Pre-computed lowercase search text (title + project_path)
    #[serde(skip)]
    pub search_text: String,
}

impl Db {
    #[allow(dead_code)]
    pub fn insert_session(&self, s: &SessionRecord, app_type: &str) -> Result<(), rusqlite::Error> {
        self.conn().execute(
            "INSERT OR REPLACE INTO session_history (id, app_type, project_path, profile_id, mode, start_time, end_time, prompt_tokens, completion_tokens, message_count, title, size_bytes, file_mtime)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![s.id, app_type, s.project_path, s.profile_id, s.mode, s.start_time, s.end_time, s.prompt_tokens, s.completion_tokens, s.message_count, s.title, s.size_bytes, s.file_mtime],
        )?;
        Ok(())
    }

    /// Delete a session record by ID.
    pub fn delete_session(&self, id: &str) -> Result<(), rusqlite::Error> {
        self.conn().execute(
            "DELETE FROM session_history WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    pub fn query_sessions(
        &self,
        app_type: &str,
        project: Option<&str>,
        search: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SessionRecord>, rusqlite::Error> {
        let mut sql = String::from(
            "SELECT id, project_path, profile_id, mode, start_time, end_time, prompt_tokens, completion_tokens, message_count, title, size_bytes, file_mtime
             FROM session_history WHERE app_type = ?1",
        );
        let mut param_values: Vec<String> = vec![app_type.to_string()];

        if let Some(p) = project {
            param_values.push(format!("%{}%", p));
            let idx = param_values.len();
            sql.push_str(&format!(" AND project_path LIKE ?{}", idx));
        }
        if let Some(s) = search {
            let pattern = format!("%{}%", s);
            param_values.push(pattern.clone());
            param_values.push(pattern);
            let idx1 = param_values.len() - 1;
            let idx2 = param_values.len();
            sql.push_str(&format!(" AND (title LIKE ?{} OR id LIKE ?{})", idx1, idx2));
        }
        sql.push_str(" ORDER BY file_mtime DESC, start_time DESC LIMIT ?");
        let limit_str = limit.to_string();
        param_values.push(limit_str);

        let mut stmt = self.conn().prepare(&sql)?;
        let rows = stmt.query_map(
            rusqlite::params_from_iter(param_values.iter().map(|s| s.as_str())),
            |row| {
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
                    size_bytes: row.get::<_, i64>(10).unwrap_or(0),
                    file_mtime: row.get::<_, String>(11).unwrap_or_default(),
                    search_text: String::new(), // populated below
                })
            },
        )?;
        let mut rows: Vec<SessionRecord> = rows.collect::<Result<Vec<_>, _>>()?;
        for s in &mut rows {
            if s.search_text.is_empty() {
                s.search_text = format!(
                    "{} {}",
                    s.title.as_deref().unwrap_or(""),
                    s.project_path
                )
                .to_lowercase();
            }
        }
        Ok(rows)
    }
}
