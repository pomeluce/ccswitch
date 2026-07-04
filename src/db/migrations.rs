use anyhow::Context;
use rusqlite::Connection;

/// Current schema version. Increment each time we add a migration step.
pub(crate) const CURRENT_USER_VERSION: i32 = 2;

/// Apply schema migrations on the given connection.
/// - user_version == 0  → fresh install: run v1 DDL, set version = 1
/// - user_version < CURRENT → run incremental steps
/// - user_version > CURRENT → error (DB from newer app version)
pub(crate) fn apply_migrations(conn: &Connection) -> Result<(), anyhow::Error> {
    let version: i32 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .context("read user_version")?;

    if version > CURRENT_USER_VERSION {
        anyhow::bail!(
            "Database version {} is newer than this app (max {}). \
             Please upgrade CCSwitch.",
            version,
            CURRENT_USER_VERSION
        );
    }

    if version < 1 {
        migrate_v1(conn).context("migrate v1")?;
    }
    if version < 2 {
        migrate_v2(conn).context("migrate v2")?;
    }

    Ok(())
}

fn migrate_v1(conn: &Connection) -> Result<(), anyhow::Error> {
    conn.execute_batch(
        "BEGIN;
         -- ── 配置层 ──
         CREATE TABLE IF NOT EXISTS providers (
             id TEXT NOT NULL,
             app_type TEXT NOT NULL CHECK(app_type IN ('claude','codex')),
             name TEXT NOT NULL,
             api_url TEXT NOT NULL,
             api_key TEXT NOT NULL DEFAULT '',
             env_key TEXT NOT NULL DEFAULT '',
             wire_api TEXT NOT NULL DEFAULT 'responses',
             provider_type TEXT,
             website_url TEXT,
             icon TEXT,
             sort_index INTEGER,
             is_current BOOLEAN NOT NULL DEFAULT 0,
             created_at TEXT NOT NULL DEFAULT (datetime('now')),
             PRIMARY KEY (id, app_type)
         );

         CREATE TABLE IF NOT EXISTS claude_profiles (
             id TEXT PRIMARY KEY,
             provider_id TEXT NOT NULL,
             name TEXT NOT NULL,
             opus_model TEXT NOT NULL,
             sonnet_model TEXT NOT NULL,
             haiku_model TEXT NOT NULL,
             subagent_model TEXT NOT NULL,
             is_default BOOLEAN NOT NULL DEFAULT 0,
             created_at TEXT NOT NULL DEFAULT (datetime('now'))
         );

         CREATE TABLE IF NOT EXISTS codex_profiles (
             id TEXT PRIMARY KEY,
             provider_id TEXT NOT NULL,
             name TEXT NOT NULL,
             model TEXT NOT NULL,
             reasoning_effort TEXT NOT NULL DEFAULT 'medium',
             reasoning_summary TEXT NOT NULL DEFAULT 'auto',
             verbosity TEXT NOT NULL DEFAULT 'medium',
             review_model TEXT NOT NULL DEFAULT '',
             plan_reasoning_effort TEXT NOT NULL DEFAULT '',
             is_default BOOLEAN NOT NULL DEFAULT 0,
             created_at TEXT NOT NULL DEFAULT (datetime('now'))
         );

         CREATE TABLE IF NOT EXISTS settings (
             key TEXT PRIMARY KEY,
             value TEXT NOT NULL
         );

         -- ── 数据层 ──
         CREATE TABLE IF NOT EXISTS usage_logs (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             app_type TEXT NOT NULL CHECK(app_type IN ('claude','codex')),
             provider_id TEXT NOT NULL DEFAULT '',
             profile_id TEXT NOT NULL DEFAULT '',
             session_id TEXT,
             model TEXT NOT NULL,
             prompt_tokens INTEGER NOT NULL DEFAULT 0,
             completion_tokens INTEGER NOT NULL DEFAULT 0,
             cache_read_tokens INTEGER NOT NULL DEFAULT 0,
             cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
             total_tokens INTEGER NOT NULL DEFAULT 0,
             timestamp TEXT NOT NULL DEFAULT (datetime('now')),
             data_source TEXT NOT NULL DEFAULT 'import',
             message_id TEXT
         );

         CREATE INDEX IF NOT EXISTS idx_usage_app_model ON usage_logs(app_type, model, timestamp);
         CREATE INDEX IF NOT EXISTS idx_usage_session ON usage_logs(session_id);
         CREATE UNIQUE INDEX IF NOT EXISTS idx_usage_msg_id ON usage_logs(message_id) WHERE message_id IS NOT NULL;

         CREATE TABLE IF NOT EXISTS session_history (
             id TEXT PRIMARY KEY,
             app_type TEXT NOT NULL CHECK(app_type IN ('claude','codex')),
             project_path TEXT NOT NULL,
             profile_id TEXT,
             mode TEXT NOT NULL CHECK(mode IN ('local','proxy')),
             start_time TEXT NOT NULL,
             end_time TEXT,
             prompt_tokens INTEGER NOT NULL DEFAULT 0,
             completion_tokens INTEGER NOT NULL DEFAULT 0,
             message_count INTEGER NOT NULL DEFAULT 0,
             title TEXT,
             size_bytes INTEGER NOT NULL DEFAULT 0,
             file_mtime TEXT NOT NULL DEFAULT ''
         );

         CREATE INDEX IF NOT EXISTS idx_session_app_project ON session_history(app_type, project_path, start_time DESC);
         CREATE INDEX IF NOT EXISTS idx_session_mtime ON session_history(file_mtime DESC);

         CREATE TABLE IF NOT EXISTS proxy_request_logs (
             request_id TEXT PRIMARY KEY,
             app_type TEXT NOT NULL CHECK(app_type IN ('claude','codex')),
             provider_id TEXT NOT NULL,
             model TEXT NOT NULL,
             request_model TEXT,
             pricing_model TEXT,
             input_tokens INTEGER NOT NULL DEFAULT 0,
             output_tokens INTEGER NOT NULL DEFAULT 0,
             cache_read_tokens INTEGER NOT NULL DEFAULT 0,
             cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
             input_cost_usd TEXT NOT NULL DEFAULT '0',
             output_cost_usd TEXT NOT NULL DEFAULT '0',
             cache_read_cost_usd TEXT NOT NULL DEFAULT '0',
             cache_creation_cost_usd TEXT NOT NULL DEFAULT '0',
             total_cost_usd TEXT NOT NULL DEFAULT '0',
             latency_ms INTEGER NOT NULL,
             first_token_ms INTEGER,
             duration_ms INTEGER,
             status_code INTEGER NOT NULL,
             error_message TEXT,
             session_id TEXT,
             is_streaming INTEGER NOT NULL DEFAULT 0,
             cost_multiplier TEXT NOT NULL DEFAULT '1.0',
             created_at INTEGER NOT NULL
         );

         -- ── 追踪层 ──
         CREATE TABLE IF NOT EXISTS session_log_sync (
             file_path TEXT PRIMARY KEY,
             file_mtime INTEGER NOT NULL,
             scan_type TEXT NOT NULL DEFAULT '',
             last_synced_at TEXT NOT NULL DEFAULT (datetime('now'))
         );

         -- ── 支撑层 ──
         CREATE TABLE IF NOT EXISTS model_pricing (
             model_id TEXT PRIMARY KEY,
             display_name TEXT NOT NULL,
             input_cost_per_million REAL NOT NULL DEFAULT 0,
             output_cost_per_million REAL NOT NULL DEFAULT 0,
             cache_read_cost_per_million REAL NOT NULL DEFAULT 0,
             cache_creation_cost_per_million REAL NOT NULL DEFAULT 0
         );

         CREATE TABLE IF NOT EXISTS provider_health (
             provider_id TEXT NOT NULL,
             app_type TEXT NOT NULL,
             is_healthy BOOLEAN NOT NULL DEFAULT 1,
             consecutive_failures INTEGER NOT NULL DEFAULT 0,
             last_failure_at TEXT,
             last_error TEXT,
             PRIMARY KEY (provider_id, app_type),
             FOREIGN KEY (provider_id, app_type) REFERENCES providers(id, app_type) ON DELETE CASCADE
         );

         PRAGMA user_version = 1;
         COMMIT;",
    )?;

    tracing::info!("Migration v1 complete: 10 tables created");
    Ok(())
}

/// v2: Fix FK on claude_profiles / codex_profiles.
/// `providers(id)` is not unique (PK is `(id, app_type)`), so FOREIGN KEY to it
/// causes "foreign key mismatch" errors. Profiles are already scoped by table name,
/// so the FK is unnecessary — remove it.
fn migrate_v2(conn: &Connection) -> Result<(), anyhow::Error> {
    conn.execute_batch(
        "BEGIN;
         -- Recreate claude_profiles without FK
         CREATE TABLE claude_profiles_new (
             id TEXT PRIMARY KEY,
             provider_id TEXT NOT NULL,
             name TEXT NOT NULL,
             opus_model TEXT NOT NULL,
             sonnet_model TEXT NOT NULL,
             haiku_model TEXT NOT NULL,
             subagent_model TEXT NOT NULL,
             is_default BOOLEAN NOT NULL DEFAULT 0,
             created_at TEXT NOT NULL DEFAULT (datetime('now'))
         );
         INSERT INTO claude_profiles_new SELECT * FROM claude_profiles;
         DROP TABLE claude_profiles;
         ALTER TABLE claude_profiles_new RENAME TO claude_profiles;

         -- Recreate codex_profiles without FK
         CREATE TABLE codex_profiles_new (
             id TEXT PRIMARY KEY,
             provider_id TEXT NOT NULL,
             name TEXT NOT NULL,
             model TEXT NOT NULL,
             reasoning_effort TEXT NOT NULL DEFAULT 'medium',
             reasoning_summary TEXT NOT NULL DEFAULT 'auto',
             verbosity TEXT NOT NULL DEFAULT 'medium',
             review_model TEXT NOT NULL DEFAULT '',
             plan_reasoning_effort TEXT NOT NULL DEFAULT '',
             is_default BOOLEAN NOT NULL DEFAULT 0,
             created_at TEXT NOT NULL DEFAULT (datetime('now'))
         );
         INSERT INTO codex_profiles_new SELECT * FROM codex_profiles;
         DROP TABLE codex_profiles;
         ALTER TABLE codex_profiles_new RENAME TO codex_profiles;

         PRAGMA user_version = 2;
         COMMIT;",
    )?;

    tracing::info!("Migration v2 complete: removed FK on profiles");
    Ok(())
}