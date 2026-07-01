pub const MIGRATIONS: &[&str] = &[
    // v1: initial schema
    "CREATE TABLE IF NOT EXISTS user_providers (
        id TEXT PRIMARY KEY,
        system_id TEXT,
        name TEXT NOT NULL,
        api_url TEXT NOT NULL,
        api_key TEXT NOT NULL,
        created_at TEXT NOT NULL DEFAULT (datetime('now'))
    );",
    "CREATE TABLE IF NOT EXISTS user_profiles (
        id TEXT PRIMARY KEY,
        provider_id TEXT NOT NULL REFERENCES user_providers(id) ON DELETE CASCADE,
        system_profile_id TEXT,
        name TEXT NOT NULL,
        opus_model TEXT NOT NULL,
        sonnet_model TEXT NOT NULL,
        haiku_model TEXT NOT NULL,
        subagent_model TEXT NOT NULL,
        is_default INTEGER NOT NULL DEFAULT 0,
        created_at TEXT NOT NULL DEFAULT (datetime('now'))
    );",
    "CREATE TABLE IF NOT EXISTS usage_logs (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        provider_id TEXT NOT NULL,
        profile_id TEXT NOT NULL,
        mode TEXT NOT NULL CHECK(mode IN ('local', 'proxy')),
        session_id TEXT,
        prompt_tokens INTEGER NOT NULL DEFAULT 0,
        completion_tokens INTEGER NOT NULL DEFAULT 0,
        timestamp TEXT NOT NULL DEFAULT (datetime('now'))
    );",
    "CREATE TABLE IF NOT EXISTS proxy_sessions (
        id TEXT PRIMARY KEY,
        profile_id TEXT NOT NULL,
        started_at TEXT NOT NULL DEFAULT (datetime('now')),
        ended_at TEXT,
        total_prompt_tokens INTEGER NOT NULL DEFAULT 0,
        total_completion_tokens INTEGER NOT NULL DEFAULT 0
    );",
    "CREATE TABLE IF NOT EXISTS session_history (
        id TEXT PRIMARY KEY,
        project_path TEXT NOT NULL,
        profile_id TEXT,
        mode TEXT NOT NULL CHECK(mode IN ('local', 'proxy')),
        start_time TEXT NOT NULL,
        end_time TEXT,
        prompt_tokens INTEGER NOT NULL DEFAULT 0,
        completion_tokens INTEGER NOT NULL DEFAULT 0,
        message_count INTEGER NOT NULL DEFAULT 0,
        title TEXT
    );",
    "CREATE TABLE IF NOT EXISTS settings (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );",
    "CREATE INDEX IF NOT EXISTS idx_usage_profile ON usage_logs(profile_id, timestamp);",
    "CREATE INDEX IF NOT EXISTS idx_session_project ON session_history(project_path, start_time);",
    // v2: add size_bytes column
    "ALTER TABLE session_history ADD COLUMN size_bytes INTEGER NOT NULL DEFAULT 0;",
    // v3: add cache_tokens column
    "ALTER TABLE usage_logs ADD COLUMN cache_read_tokens INTEGER NOT NULL DEFAULT 0;",
    "ALTER TABLE usage_logs ADD COLUMN cache_create_tokens INTEGER NOT NULL DEFAULT 0;",
    "ALTER TABLE usage_logs ADD COLUMN total_tokens INTEGER NOT NULL DEFAULT 0;",
    "ALTER TABLE usage_logs ADD COLUMN message_id TEXT;",
    // v4: track session mtime + message IDs for incremental usage scanning
    "CREATE TABLE IF NOT EXISTS session_usage_track (
        session_id TEXT PRIMARY KEY,
        file_mtime INTEGER NOT NULL
    );",
    "CREATE UNIQUE INDEX IF NOT EXISTS idx_usage_msg_id ON usage_logs(message_id) WHERE message_id IS NOT NULL;",
];