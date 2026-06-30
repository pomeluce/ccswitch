use rusqlite::Connection;
use std::path::Path;

use super::migrations::MIGRATIONS;

pub struct Db {
    conn: Connection,
}

impl Db {
    /// Open (or create) the database at the given path and run migrations
    pub fn open(path: &Path) -> Result<Self, rusqlite::Error> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let db = Db { conn };
        db.run_migrations()?;
        Ok(db)
    }

    fn run_migrations(&self) -> Result<(), rusqlite::Error> {
        for sql in MIGRATIONS {
            // Ignore errors (e.g. duplicate column in ALTER TABLE)
            self.conn.execute(sql, []).ok();
        }
        Ok(())
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }
}
