use serde::{Deserialize, Serialize};

/// Represents whether a config came from system defaults or user DB
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Source {
    #[default]
    #[serde(rename = "system")]
    System,
    #[serde(rename = "user")]
    User,
}

impl Source {
    pub fn can_delete(&self) -> bool {
        matches!(self, Source::User)
    }
}

/// An API provider (vendor)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub api_url: String,
    pub api_key: String,
    #[serde(default)]
    pub profiles: Vec<Profile>,
    #[serde(skip)]
    pub source: Source,
}

/// A model configuration profile under a provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub opus: String,
    pub sonnet: String,
    pub haiku: String,
    pub subagent: String,
    #[serde(default)]
    pub default: bool,
    #[serde(skip)]
    pub source: Source,
}

/// The resolved active config (what gets applied)
#[derive(Debug, Clone)]
pub struct ActiveConfig {
    pub provider_id: String,
    pub profile_id: String,
    pub provider_name: String,
    pub profile_name: String,
    pub base_url: String,
    pub auth_token: String,
    pub opus_model: String,
    pub sonnet_model: String,
    pub haiku_model: String,
    pub subagent_model: String,
}

/// How the switch should be applied
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchMode {
    Local,
    Proxy,
}

