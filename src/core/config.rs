use crate::core::models::{Profile, Provider, Source};
use crate::db::Db;
use anyhow::Context;
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// Config directory for ccswitch: XDG_CONFIG_HOME on Linux, AppData on Windows,
/// Library/Application Support on macOS.
pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ccswitch")
}

/// Data directory for ccswitch (logs, runtime data).
#[allow(dead_code)]
pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ccswitch")
}

/// Config DB path.
pub fn db_path() -> PathBuf {
    config_dir().join("ccswitch.db")
}

/// System defaults path (for TOML overrides). Uses XDG config + /etc fallback.
pub fn defaults_path() -> Option<PathBuf> {
    let user = config_dir().join("defaults.toml");
    if user.exists() {
        return Some(user);
    }
    let system = PathBuf::from("/etc/ccswitch/defaults.toml");
    if system.exists() {
        return Some(system);
    }
    None
}

fn default_config_path() -> PathBuf {
    let user = config_dir().join("defaults.toml");
    if user.exists() { return user; }
    PathBuf::from("/etc/ccswitch/defaults.toml")
}

#[derive(Debug, Deserialize)]
struct DefaultsFile {
    #[serde(default)]
    #[allow(dead_code)]
    version: u32,
    #[serde(default)]
    providers: Vec<ProviderToml>,
}

#[derive(Debug, Deserialize)]
struct ProviderToml {
    id: String,
    name: String,
    api_url: String,
    api_key: String,
    #[serde(default)]
    profiles: Vec<ProfileToml>,
}

#[derive(Debug, Deserialize)]
struct ProfileToml {
    id: String,
    name: String,
    #[serde(alias = "opus")]
    reasoning_model: String,
    #[serde(default, alias = "haiku")]
    task_model: String,
    #[serde(default)]
    default: bool,
}

pub struct ConfigManager {
    db: Db,
    system_providers: Vec<Provider>,
}

impl ConfigManager {
    pub fn new(db_path: &Path, defaults_path: Option<&Path>) -> Result<Self, anyhow::Error> {
        let dir = db_path.parent().unwrap_or_else(|| Path::new("."));
        let db = Db::open(&dir.join("ccswitch.db")).context("Failed to open ccswitch.db")?;

        let default_path = default_config_path();
        let defaults_path = defaults_path.unwrap_or_else(|| &default_path);
        let system_providers = if defaults_path.exists() {
            let content = std::fs::read_to_string(defaults_path)?;
            let defaults: DefaultsFile = toml::from_str(&content)?;
            defaults.providers.into_iter().map(|p| Provider {
                id: p.id, name: p.name, api_url: p.api_url, api_key: p.api_key,
                profiles: p.profiles.into_iter().map(|pr| Profile {
                    id: pr.id, name: pr.name,
                    reasoning_model: pr.reasoning_model, task_model: pr.task_model,
                    default: pr.default, source: Source::System,
                }).collect(),
                source: Source::System,
            }).collect()
        } else { vec![] };

        // Sync TOML providers/profiles to DB (source='system').
        // Always call — even when empty, to demote stale system providers.
        if let Err(e) = db.sync_system_providers("claude", &system_providers) {
            tracing::warn!("Failed to sync system providers to DB: {}", e);
        }

        Ok(ConfigManager { db, system_providers })
    }

    pub(crate) fn db(&self) -> &Db { &self.db }
    pub fn get_setting(&self, key: &str) -> Option<String> { self.db.get_setting(key) }
    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), rusqlite::Error> { self.db.set_setting(key, value) }

    pub fn list_providers(&self) -> Result<Vec<Provider>, anyhow::Error> {
        const APP: &str = "claude";
        let db_providers = self.db.get_providers(APP)?;
        let mut result = self.system_providers.clone();

        for dp in &db_providers {
            if let Some(existing) = result.iter_mut().find(|p| p.id == dp.id) {
                existing.name = dp.name.clone();
                existing.api_url = dp.api_url.clone();
                existing.api_key = dp.api_key.clone();
                existing.source = dp.source; // Use DB source (system/user)
            } else {
                result.push(dp.clone());
            }
        }

        for provider in &mut result {
            let db_profiles = self.db.get_profiles(&provider.id)?;
            for dp in &db_profiles {
                if let Some(existing) = provider.profiles.iter_mut().find(|p| p.id == dp.id) {
                    *existing = dp.clone();
                } else {
                    provider.profiles.push(dp.clone());
                }
            }
        }
        Ok(result)
    }

    pub fn find_profile(&self, provider_id: &str, profile_id: &str) -> Result<Option<(Provider, Profile)>, anyhow::Error> {
        for p in self.list_providers()? {
            if p.id == provider_id {
                for pr in &p.profiles {
                    if pr.id == profile_id { return Ok(Some((p.clone(), pr.clone()))); }
                }
            }
        }
        Ok(None)
    }
}
