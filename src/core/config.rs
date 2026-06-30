use std::path::{Path, PathBuf};
use serde::Deserialize;
use crate::core::models::{Provider, Profile, Source};
use crate::db::Db;

/// Read from ~/.ccswitch/defaults.toml (user-level config, may be managed by NixOS/home-manager)
fn default_config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".ccswitch").join("defaults.toml")
}

#[derive(Debug, Deserialize)]
struct DefaultsFile {
    #[serde(default)]
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
    opus: String,
    sonnet: String,
    haiku: String,
    subagent: String,
    #[serde(default)]
    default: bool,
}

pub struct ConfigManager {
    db: Db,
    system_providers: Vec<Provider>,
}

impl ConfigManager {
    pub fn new(db_path: &Path, defaults_path: Option<&Path>) -> Result<Self, anyhow::Error> {
        let db = Db::open(db_path)?;

        let default_path = default_config_path();
        let defaults_path = defaults_path.unwrap_or_else(|| &default_path);
        let system_providers = if defaults_path.exists() {
            let content = std::fs::read_to_string(defaults_path)?;
            let defaults: DefaultsFile = toml::from_str(&content)?;
            defaults.providers.into_iter().map(|p| {
                Provider {
                    id: p.id,
                    name: p.name,
                    api_url: p.api_url,
                    api_key: p.api_key,
                    profiles: p.profiles.into_iter().map(|pr| Profile {
                        id: pr.id,
                        name: pr.name,
                        opus: pr.opus,
                        sonnet: pr.sonnet,
                        haiku: pr.haiku,
                        subagent: pr.subagent,
                        default: pr.default,
                        source: Source::System,
                    }).collect(),
                    source: Source::System,
                }
            }).collect()
        } else {
            vec![]
        };

        Ok(ConfigManager { db, system_providers })
    }

    /// Return merged list: user providers override system by id,
    /// user profiles merge into their parent provider
    pub fn list_providers(&self) -> Result<Vec<Provider>, anyhow::Error> {
        let user_providers = self.db.get_user_providers()?;
        let mut result = self.system_providers.clone();

        for up in &user_providers {
            if let Some(existing) = result.iter_mut().find(|p| p.id == up.id) {
                // Override system provider fields
                existing.name = up.name.clone();
                existing.api_url = up.api_url.clone();
                existing.api_key = up.api_key.clone();
                existing.source = Source::User;
            } else {
                result.push(up.clone());
            }

            // Add user profiles to the matching provider
            let user_profiles = self.db.get_user_profiles(&up.id)?;
            if let Some(provider) = result.iter_mut().find(|p| p.id == up.id) {
                for uprof in &user_profiles {
                    if let Some(existing_prof) = provider.profiles.iter_mut().find(|p| p.id == uprof.id) {
                        *existing_prof = uprof.clone();
                        existing_prof.source = Source::User;
                    } else {
                        provider.profiles.push(uprof.clone());
                    }
                }
            }
        }

        Ok(result)
    }

    /// Find a specific profile by provider_id and profile_id
    pub fn find_profile(&self, provider_id: &str, profile_id: &str) -> Result<Option<(Provider, Profile)>, anyhow::Error> {
        let providers = self.list_providers()?;
        for p in providers {
            if p.id == provider_id {
                for pr in &p.profiles {
                    if pr.id == profile_id {
                        return Ok(Some((p.clone(), pr.clone())));
                    }
                }
            }
        }
        Ok(None)
    }

    pub fn db(&self) -> &Db {
        &self.db
    }
}