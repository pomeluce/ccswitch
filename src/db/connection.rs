use rusqlite::Connection;
use std::path::{Path, PathBuf};

use super::migrations;

pub struct Db {
    conn: Connection,
}

impl Db {
    /// Open (or create) the database at `path`. Migrates old 3-file layout
    /// if detected, then applies schema migrations.
    pub fn open(path: &Path) -> Result<Self, anyhow::Error> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }

        let new_db_exists = path.exists();

        // Detect and migrate old 3-file layout
        if !new_db_exists {
            let dir = path.parent().unwrap_or_else(|| Path::new("."));
            let old_model = dir.join("model.db");
            let old_usage = dir.join("usage.db");
            let old_session = dir.join("session.db");
            if old_model.exists() || old_usage.exists() || old_session.exists() {
                migrate_old_dbs(dir, path)?;
            }
        }

        // Clean orphaned WAL/SHM (belt-and-suspenders — SQLite recreates as needed)
        {
            let wal = PathBuf::from(format!("{}-wal", path.display()));
            let shm = PathBuf::from(format!("{}-shm", path.display()));
            if !path.exists() {
                std::fs::remove_file(&wal).ok();
                std::fs::remove_file(&shm).ok();
            }
        }

        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;

        {
            let version: i32 = conn
                .pragma_query_value(None, "user_version", |r| r.get(0))
                .unwrap_or(0);
            if version < migrations::CURRENT_USER_VERSION {
                tracing::info!(
                    "Applying DB migrations v{} → v{}",
                    version,
                    migrations::CURRENT_USER_VERSION
                );
                migrations::apply_migrations(&conn)?;
            }
        }

        Ok(Db { conn })
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }
}

// ── Old DB Migration ──────────────────────────────────────────────

/// Migrate data from old model.db + usage.db + session.db into the new ccswitch.db.
/// Each old DB is read entirely into memory, dropped, then written to the new DB.
/// Old files are preserved so users can verify before manual cleanup.
fn migrate_old_dbs(dir: &Path, new_path: &Path) -> Result<(), anyhow::Error> {
    tracing::info!("Migrating old 3-file DB layout to single ccswitch.db...");

    // Create the new DB first (with schema)
    {
        let conn = Connection::open(new_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        migrations::apply_migrations(&conn)?;
    }

    let new_conn = Connection::open(new_path)?;
    new_conn.execute_batch("PRAGMA foreign_keys=ON;")?;

    // ── model.db ──
    {
        let path = dir.join("model.db");
        if path.exists() {
            if let Ok(old) = Connection::open(&path) {
                // user_providers → providers
                if let Ok(mut stmt) =
                    old.prepare("SELECT id, name, api_url, api_key FROM user_providers")
                {
                    let rows: Vec<_> = stmt
                        .query_map([], |r| {
                            Ok((
                                r.get::<_, String>(0)?,
                                r.get::<_, String>(1)?,
                                r.get::<_, String>(2)?,
                                r.get::<_, String>(3)?,
                            ))
                        })
                        .map_err(|e| tracing::warn!("Migration query failed: {}", e))
                        .ok()
                        .into_iter()
                        .flat_map(|rows| rows.filter_map(|r| r.ok()))
                        .collect();
                    tracing::info!("Migrated {} providers from model.db", rows.len());
                    for (id, name, api_url, api_key) in &rows {
                        new_conn
                            .execute(
                                "INSERT OR IGNORE INTO providers (id, app_type, name, api_url, api_key, provider_type)
                                 VALUES (?1, 'claude', ?2, ?3, ?4, 'anthropic')",
                                rusqlite::params![id, name, api_url, api_key],
                            )
                            .map_err(|e| tracing::warn!("Migration insert failed: {}", e)).ok();
                    }
                }

                // user_profiles → claude_profiles
                if let Ok(mut stmt) = old.prepare(
                    "SELECT id, provider_id, name, opus_model, sonnet_model, haiku_model, subagent_model, is_default FROM user_profiles",
                ) {
                    let rows: Vec<_> = stmt
                        .query_map([], |r| {
                            Ok((
                                r.get::<_, String>(0)?,
                                r.get::<_, String>(1)?,
                                r.get::<_, String>(2)?,
                                r.get::<_, String>(3)?,
                                r.get::<_, String>(4)?,
                                r.get::<_, String>(5)?,
                                r.get::<_, String>(6)?,
                                r.get::<_, i32>(7)?,
                            ))
                        })
                        .map_err(|e| tracing::warn!("Migration query failed: {}", e))
                        .ok()
                        .into_iter()
                        .flat_map(|rows| rows.filter_map(|r| r.ok()))
                        .collect();
                    tracing::info!("Migrated {} profiles from model.db", rows.len());
                    for (id, provider_id, name, opus, sonnet, haiku, subagent, is_default) in &rows {
                        new_conn
                            .execute(
                                "INSERT OR IGNORE INTO claude_profiles (id, provider_id, name, opus_model, sonnet_model, haiku_model, subagent_model, is_default)
                                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                                rusqlite::params![id, provider_id, name, opus, sonnet, haiku, subagent, is_default],
                            )
                            .map_err(|e| tracing::warn!("Migration insert failed: {}", e)).ok();
                    }
                }

                // settings
                if let Ok(mut stmt) = old.prepare("SELECT key, value FROM settings") {
                    let rows: Vec<_> = stmt
                        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
                        .map_err(|e| tracing::warn!("Migration query failed: {}", e))
                        .ok()
                        .into_iter()
                        .flat_map(|rows| rows.filter_map(|r| r.ok()))
                        .collect();
                    tracing::info!("Migrated {} settings from model.db", rows.len());
                    for (key, value) in &rows {
                        new_conn
                            .execute(
                                "INSERT OR IGNORE INTO settings (key, value) VALUES (?1, ?2)",
                                rusqlite::params![key, value],
                            )
                            .map_err(|e| tracing::warn!("Migration insert failed: {}", e)).ok();
                    }
                }
            }
        }
    }

    // ── usage.db ──
    {
        let path = dir.join("usage.db");
        if path.exists() {
            if let Ok(old) = Connection::open(&path) {
                let has_app_type = column_exists(&old, "usage_logs", "app_type");
                let has_data_source = column_exists(&old, "usage_logs", "data_source");
                let _has_message_id = column_exists(&old, "usage_logs", "message_id");

                if let Ok(mut stmt) = old.prepare(
                    "SELECT provider_id, profile_id, session_id, model,
                            prompt_tokens, completion_tokens,
                            COALESCE(cache_read_tokens,0), COALESCE(cache_creation_tokens,0),
                            COALESCE(total_tokens,0), timestamp
                     FROM usage_logs",
                ) {
                    let mut count = 0usize;
                    if let Ok(rows) = stmt.query_map([], |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, Option<String>>(2)?,
                            r.get::<_, String>(3)?,
                            r.get::<_, i64>(4)?,
                            r.get::<_, i64>(5)?,
                            r.get::<_, i64>(6)?,
                            r.get::<_, i64>(7)?,
                            r.get::<_, i64>(8)?,
                            r.get::<_, String>(9)?,
                        ))
                    }) {
                        for row in rows {
                            if let Ok((pid, pfid, sid, model, pt, ct, cr, cc, total, ts)) = row {
                                let at = if has_app_type { "claude" } else { "claude" };
                                let ds = if has_data_source { "import" } else { "import" };
                                // Read message_id separately if column exists
                                new_conn
                                    .execute(
                                        "INSERT OR IGNORE INTO usage_logs (app_type, provider_id, profile_id, session_id, model,
                                         prompt_tokens, completion_tokens, cache_read_tokens, cache_creation_tokens,
                                         total_tokens, timestamp, data_source)
                                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                                        rusqlite::params![at, pid, pfid, sid, model, pt, ct, cr, cc, total, ts, ds],
                                    )
                                    .map_err(|e| tracing::warn!("Migration insert failed: {}", e)).ok();
                                count += 1;
                            }
                        }
                    }
                    tracing::info!("Migrated {} usage_logs from usage.db", count);
                }

                // session_usage_track → session_log_sync
                if let Ok(mut stmt) =
                    old.prepare("SELECT session_id, file_mtime FROM session_usage_track")
                {
                    let rows: Vec<_> = stmt
                        .query_map([], |r| {
                            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
                        })
                        .map_err(|e| tracing::warn!("Migration query failed: {}", e))
                        .ok()
                        .into_iter()
                        .flat_map(|rows| rows.filter_map(|r| r.ok()))
                        .collect();
                    tracing::info!(
                        "Migrated {} session_usage_track entries",
                        rows.len()
                    );
                    for (sid, mtime) in &rows {
                        new_conn
                            .execute(
                                "INSERT OR IGNORE INTO session_log_sync (file_path, file_mtime, scan_type)
                                 VALUES (?1, ?2, 'usage')",
                                rusqlite::params![format!("usage:{}", sid), mtime],
                            )
                            .map_err(|e| tracing::warn!("Migration insert failed: {}", e)).ok();
                    }
                }
            }
        }
    }

    // ── session.db ──
    {
        let path = dir.join("session.db");
        if path.exists() {
            if let Ok(old) = Connection::open(&path) {
                if let Ok(mut stmt) = old.prepare(
                    "SELECT id, project_path, profile_id, mode, start_time, end_time,
                            prompt_tokens, completion_tokens, message_count, title,
                            COALESCE(size_bytes,0), COALESCE(file_mtime,'')
                     FROM session_history",
                ) {
                    let mut count = 0usize;
                    if let Ok(rows) = stmt.query_map([], |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, Option<String>>(2)?,
                            r.get::<_, String>(3)?,
                            r.get::<_, String>(4)?,
                            r.get::<_, Option<String>>(5)?,
                            r.get::<_, i64>(6)?,
                            r.get::<_, i64>(7)?,
                            r.get::<_, i64>(8)?,
                            r.get::<_, Option<String>>(9)?,
                            r.get::<_, i64>(10)?,
                            r.get::<_, String>(11)?,
                        ))
                    }) {
                        for row in rows {
                            if let Ok((id, pp, pf, mode, st, et, pt, ct, mc, title, sz, fm)) = row {
                                new_conn
                                    .execute(
                                        "INSERT OR IGNORE INTO session_history (id, app_type, project_path, profile_id, mode,
                                         start_time, end_time, prompt_tokens, completion_tokens, message_count, title,
                                         size_bytes, file_mtime)
                                         VALUES (?1, 'claude', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                                        rusqlite::params![id, pp, pf, mode, st, et, pt, ct, mc, title, sz, fm],
                                    )
                                    .map_err(|e| tracing::warn!("Migration insert failed: {}", e)).ok();
                                count += 1;
                            }
                        }
                    }
                    tracing::info!("Migrated {} sessions from session.db", count);
                }

                // session_file_track → session_log_sync
                if let Ok(mut stmt) =
                    old.prepare("SELECT session_id, file_mtime FROM session_file_track")
                {
                    let rows: Vec<_> = stmt
                        .query_map([], |r| {
                            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
                        })
                        .map_err(|e| tracing::warn!("Migration query failed: {}", e))
                        .ok()
                        .into_iter()
                        .flat_map(|rows| rows.filter_map(|r| r.ok()))
                        .collect();
                    tracing::info!(
                        "Migrated {} session_file_track entries",
                        rows.len()
                    );
                    for (sid, mtime) in &rows {
                        new_conn
                            .execute(
                                "INSERT OR IGNORE INTO session_log_sync (file_path, file_mtime, scan_type)
                                 VALUES (?1, ?2, 'session')",
                                rusqlite::params![format!("session:{}", sid), mtime],
                            )
                            .map_err(|e| tracing::warn!("Migration insert failed: {}", e)).ok();
                    }
                }
            }
        }
    }

    tracing::info!("Old DB migration → {} complete", new_path.display());
    Ok(())
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> bool {
    let sql = format!("PRAGMA table_info(\"{table}\")");
    if let Ok(mut stmt) = conn.prepare(&sql) {
        stmt.query_map([], |r| r.get::<_, String>(1))
            .map(|rows| rows.filter_map(|r| r.ok()).any(|name| name == column))
            .unwrap_or(false)
    } else {
        false
    }
}
