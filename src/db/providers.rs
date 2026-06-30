use rusqlite::params;
use crate::core::models::{Provider, Profile, Source};

use super::connection::Db;

// -- Providers --

impl Db {
    pub fn insert_user_provider(&self, p: &Provider) -> Result<(), rusqlite::Error> {
        self.conn().execute(
            "INSERT OR REPLACE INTO user_providers (id, name, api_url, api_key) VALUES (?1, ?2, ?3, ?4)",
            params![p.id, p.name, p.api_url, p.api_key],
        )?;
        Ok(())
    }

    pub fn get_user_providers(&self) -> Result<Vec<Provider>, rusqlite::Error> {
        let mut stmt = self.conn().prepare(
            "SELECT id, name, api_url, api_key FROM user_providers ORDER BY name"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Provider {
                id: row.get(0)?,
                name: row.get(1)?,
                api_url: row.get(2)?,
                api_key: row.get(3)?,
                profiles: vec![],
                source: Source::User,
            })
        })?;
        rows.collect()
    }

    pub fn delete_user_provider(&self, id: &str) -> Result<(), rusqlite::Error> {
        // Profiles cascade-delete via FK
        self.conn().execute("DELETE FROM user_providers WHERE id = ?1", params![id])?;
        Ok(())
    }
}

// -- Profiles --

impl Db {
    pub fn insert_user_profile(&self, provider_id: &str, p: &Profile) -> Result<(), rusqlite::Error> {
        self.conn().execute(
            "INSERT OR REPLACE INTO user_profiles (id, provider_id, name, opus_model, sonnet_model, haiku_model, subagent_model, is_default)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![p.id, provider_id, p.name, p.opus, p.sonnet, p.haiku, p.subagent, p.default as i32],
        )?;
        Ok(())
    }

    pub fn get_user_profiles(&self, provider_id: &str) -> Result<Vec<Profile>, rusqlite::Error> {
        let mut stmt = self.conn().prepare(
            "SELECT id, name, opus_model, sonnet_model, haiku_model, subagent_model, is_default
             FROM user_profiles WHERE provider_id = ?1 ORDER BY name"
        )?;
        let rows = stmt.query_map(params![provider_id], |row| {
            Ok(Profile {
                id: row.get(0)?,
                name: row.get(1)?,
                opus: row.get(2)?,
                sonnet: row.get(3)?,
                haiku: row.get(4)?,
                subagent: row.get(5)?,
                default: row.get::<_, i32>(6)? != 0,
                source: Source::User,
            })
        })?;
        rows.collect()
    }

    pub fn delete_user_profile(&self, id: &str) -> Result<(), rusqlite::Error> {
        self.conn().execute("DELETE FROM user_profiles WHERE id = ?1", params![id])?;
        Ok(())
    }
}