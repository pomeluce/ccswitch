use rusqlite::Connection;
use std::path::{Path, PathBuf};

use super::migrations::MIGRATIONS;

pub struct Db {
    conn: Connection,
}

impl Db {
    /// Open (or create) the database with WAL journal mode.
    /// Cleans up any leftover WAL/SHM files first to prevent WSL2 I/O errors.
    pub fn open(path: &Path) -> Result<Self, rusqlite::Error> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        // Remove orphaned WAL/SHM files — on WSL2, leftover files from a
        // partially-deleted DB (e.g. rm usage.db but not usage.db-wal) cause
        // "disk I/O error" on first write. Safe: SQLite recreates them as needed.
        let wal_path = PathBuf::from(format!("{}-wal", path.display()));
        let shm_path = PathBuf::from(format!("{}-shm", path.display()));
        if !path.exists() {
            std::fs::remove_file(&wal_path).ok();
            std::fs::remove_file(&shm_path).ok();
        }

        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let db = Db { conn };
        db.run_migrations()?;
        Ok(db)
    }

    fn run_migrations(&self) -> Result<(), rusqlite::Error> {
        for sql in MIGRATIONS {
            if let Err(e) = self.conn.execute(sql, []) {
                let msg = format!("{}", e);
                // Ignore expected errors for ALTER TABLE duplicates
                if msg.contains("duplicate column") || msg.contains("already exists") {
                    continue;
                }
                // Log unexpected failures at warn level so they're visible
                tracing::warn!("Migration failed: {} — {}", &sql[..sql.len().min(80)], e);
            }
        }
        Ok(())
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }
}
