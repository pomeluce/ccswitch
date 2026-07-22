use rusqlite::Connection;
use std::path::{Path, PathBuf};

use super::migrations;

pub struct Db {
    conn: Connection,
}

impl Db {
    /// Open (or create) the database at `path`, applying schema migrations as needed.
    pub fn open(path: &Path) -> Result<Self, anyhow::Error> {
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                tracing::warn!("Failed to create DB directory '{}': {}", parent.display(), e);
            }
        }

        // Clean orphaned WAL/SHM files (e.g. after manual DB deletion or crash)
        // SQLite auto-recovers in WAL mode, but stray files waste disk space.
        let wal = PathBuf::from(format!("{}-wal", path.display()));
        let shm = PathBuf::from(format!("{}-shm", path.display()));
        if !path.exists() {
            std::fs::remove_file(&wal).ok();
            std::fs::remove_file(&shm).ok();
        }

        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;

        let version: i32 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .inspect_err(|e| {
                tracing::warn!("Failed to read DB user_version (corrupt DB?): {}", e);
            })
            .unwrap_or(migrations::CURRENT_USER_VERSION); // only migrate if genuinely too old
        if version < migrations::CURRENT_USER_VERSION {
            tracing::info!(
                "Applying DB migrations v{} → v{}",
                version,
                migrations::CURRENT_USER_VERSION
            );
            migrations::apply_migrations(&conn)?;
        }

        Ok(Db { conn })
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }
}
