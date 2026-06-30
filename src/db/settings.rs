use rusqlite::params;
use super::connection::Db;

impl Db {
    pub fn get_setting(&self, key: &str) -> Option<String> {
        self.conn()
            .query_row("SELECT value FROM settings WHERE key = ?1", params![key], |row| row.get(0))
            .ok()
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), rusqlite::Error> {
        self.conn().execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn remove_setting(&self, key: &str) -> Result<(), rusqlite::Error> {
        self.conn().execute("DELETE FROM settings WHERE key = ?1", params![key])?;
        Ok(())
    }
}