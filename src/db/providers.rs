use rusqlite::params;
use crate::core::models::{Profile, Provider, Source};
use super::connection::Db;

// ── Providers ──

impl Db {
    pub fn insert_provider(&self, p: &Provider, app_type: &str) -> Result<(), rusqlite::Error> {
        let source_str: &str = p.source.as_str();
        self.conn().execute(
            "INSERT OR REPLACE INTO providers (id, app_type, name, api_url, api_key, source)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![p.id, app_type, p.name, p.api_url, p.api_key, source_str],
        )?;
        Ok(())
    }

    pub fn get_providers(&self, app_type: &str) -> Result<Vec<Provider>, rusqlite::Error> {
        let mut stmt = self.conn().prepare(
            "SELECT id, name, api_url, api_key, source FROM providers WHERE app_type = ?1 ORDER BY name",
        )?;
        let rows = stmt.query_map(params![app_type], |row| {
            let source_str: String = row.get(4)?;
            Ok(Provider {
                id: row.get(0)?,
                name: row.get(1)?,
                api_url: row.get(2)?,
                api_key: row.get(3)?,
                profiles: vec![],
                source: source_str.parse().unwrap_or(Source::User),
            })
        })?;
        rows.collect()
    }

    pub fn delete_provider(&self, id: &str, app_type: &str) -> Result<(), rusqlite::Error> {
        // Cascade-delete profiles for this provider
        self.conn().execute(
            "DELETE FROM profiles WHERE provider_id = ?1",
            params![id],
        )?;
        self.conn().execute(
            "DELETE FROM providers WHERE id = ?1 AND app_type = ?2",
            params![id, app_type],
        )?;
        Ok(())
    }

    /// Sync system providers/profiles from defaults.toml into the DB.
    /// - New TOML providers → INSERT with source='system'
    /// - Existing system providers → UPDATE fields from TOML
    /// - User-added providers (source='user') → never touched
    /// - DB providers with source='system' not in TOML → demote to source='user'
    pub fn sync_system_providers(
        &self,
        app_type: &str,
        system_providers: &[Provider],
    ) -> Result<(), rusqlite::Error> {
        let mut toml_ids: Vec<&str> = Vec::new();
        for p in system_providers {
            toml_ids.push(&p.id);
            // INSERT only if not already present (user row takes priority)
            self.conn().execute(
                "INSERT OR IGNORE INTO providers (id, app_type, name, api_url, api_key, source)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'system')",
                params![p.id, app_type, p.name, p.api_url, p.api_key],
            )?;
            // UPDATE existing system providers with latest TOML values
            self.conn().execute(
                "UPDATE providers SET name=?1, api_url=?2, api_key=?3, source='system'
                 WHERE id=?4 AND app_type=?5 AND source='system'",
                params![p.name, p.api_url, p.api_key, p.id, app_type],
            )?;
            // Sync profiles: INSERT OR IGNORE for system ones
            for pr in &p.profiles {
                self.conn().execute(
                    "INSERT OR IGNORE INTO profiles (id, name, provider_id, reasoning_model, task_model, is_default, source)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'system')",
                    params![pr.id, pr.name, p.id, pr.reasoning_model, pr.task_model, pr.default as i32],
                )?;
            }
        }
        // Demote system providers that no longer exist in TOML
        if !toml_ids.is_empty() {
            let placeholders: Vec<String> = toml_ids.iter().enumerate()
                .map(|(i, _)| format!("?{}", i + 2))
                .collect();
            let sql = format!(
                "UPDATE providers SET source = 'user' WHERE app_type = ?1 AND source = 'system' AND id NOT IN ({})",
                placeholders.join(",")
            );
            let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
            param_values.push(Box::new(app_type.to_string()));
            for id in &toml_ids {
                param_values.push(Box::new(id.to_string()));
            }
            self.conn().execute(
                &sql,
                rusqlite::params_from_iter(param_values.iter().map(|p| p.as_ref())),
            )?;
        }
        Ok(())
    }
}

// ── Profiles (shared between claude/codex) ──

impl Db {
    pub fn insert_profile(&self, provider_id: &str, p: &Profile) -> Result<(), rusqlite::Error> {
        let source_str: &str = p.source.as_str();
        self.conn().execute(
            "INSERT OR REPLACE INTO profiles (id, name, provider_id, reasoning_model, task_model, is_default, source)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![p.id, p.name, provider_id, p.reasoning_model, p.task_model, p.default as i32, source_str],
        )?;
        Ok(())
    }

    pub fn get_profiles(&self, provider_id: &str) -> Result<Vec<Profile>, rusqlite::Error> {
        let mut stmt = self.conn().prepare(
            "SELECT id, name, reasoning_model, task_model, is_default, source
             FROM profiles WHERE provider_id = ?1 ORDER BY name",
        )?;
        let rows = stmt.query_map(params![provider_id], |row| {
            let source_str: String = row.get(5)?;
            Ok(Profile {
                id: row.get(0)?,
                name: row.get(1)?,
                reasoning_model: row.get(2)?,
                task_model: row.get(3)?,
                default: row.get::<_, i32>(4)? != 0,
                source: source_str.parse().unwrap_or(Source::User),
            })
        })?;
        rows.collect()
    }

    pub fn delete_profile(&self, id: &str) -> Result<(), rusqlite::Error> {
        self.conn().execute("DELETE FROM profiles WHERE id = ?1", params![id])?;
        Ok(())
    }
}
