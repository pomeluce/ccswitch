use crate::core::config::ConfigManager;
use crate::core::env::resolve_api_key;
use crate::core::models::{ActiveConfig, SwitchMode};
use anyhow::{Context, Result};
use serde_json::json;
use std::path::Path;

const DEFAULT_PROXY_PORT: u16 = 15721;

/// Apply a profile switch. In Local mode, writes to settings.json.
/// In Proxy mode, updates SQLite so the proxy picks it up.
pub fn switch_profile(mgr: &ConfigManager, provider_id: &str, profile_id: &str, mode: SwitchMode, settings_path: Option<&Path>) -> Result<ActiveConfig> {
    let (provider, profile) = mgr
        .find_profile(provider_id, profile_id)?
        .with_context(|| format!("Profile not found: {}/{}", provider_id, profile_id))?;

    let auth_token = resolve_api_key(&provider.api_key);
    let base_url = match mode {
        SwitchMode::Proxy => format!("http://127.0.0.1:{}", DEFAULT_PROXY_PORT),
        SwitchMode::Local => provider.api_url.clone(),
    };

    let config = ActiveConfig {
        provider_id: provider.id.clone(),
        profile_id: profile.id.clone(),
        provider_name: provider.name.clone(),
        profile_name: profile.name.clone(),
        base_url: base_url.clone(),
        auth_token: auth_token.clone(),
        api_key: provider.api_key.clone(),
        opus_model: profile.opus.clone(),
        sonnet_model: profile.sonnet.clone(),
        haiku_model: profile.haiku.clone(),
        subagent_model: profile.subagent.clone(),
    };

    match mode {
        SwitchMode::Local => {
            write_settings_json(&config, settings_path)?;
        }
        SwitchMode::Proxy => {
            // Settings.json should point to localhost proxy
            write_settings_json(&config, settings_path)?;
            // Persist active selection in SQLite for the proxy to read
            mgr.db().set_setting("active_provider", &config.provider_id)?;
            mgr.db().set_setting("active_profile", &config.profile_id)?;
            mgr.db().set_setting("proxy_mode", "true")?;
            mgr.db().set_setting("proxy_port", &DEFAULT_PROXY_PORT.to_string())?;
        }
    }

    Ok(config)
}

/// If the original api_key is an `env:XXX` reference or empty, return the reference
/// instead of the resolved plaintext value, so it is never written to disk in plaintext.
fn preserve_env_ref(original: &str, resolved: &str) -> String {
    if original.starts_with("env:") || original.is_empty() {
        if original.is_empty() {
            "env:CLAUDE_API_KEY".to_string()
        } else {
            original.to_string()
        }
    } else {
        // Literal key — user chose to store it
        resolved.to_string()
    }
}

fn write_settings_json(config: &ActiveConfig, path: Option<&Path>) -> Result<()> {
    let settings_path = path.map(|p| p.to_path_buf()).unwrap_or_else(|| {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        Path::new(&home).join(".claude").join("settings.json")
    });

    // Read existing or start fresh
    let mut existing: serde_json::Value = if settings_path.exists() {
        let content = std::fs::read_to_string(&settings_path)?;
        serde_json::from_str(&content).unwrap_or(json!({}))
    } else {
        json!({})
    };

    // Ensure parent dir
    if let Some(parent) = settings_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Build env block
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    existing["env"] = json!({
        "ANTHROPIC_BASE_URL": config.base_url,
        "ANTHROPIC_AUTH_TOKEN": preserve_env_ref(&config.api_key, &config.auth_token),
        "ANTHROPIC_MODEL": config.opus_model,
        "ANTHROPIC_DEFAULT_OPUS_MODEL": config.opus_model,
        "ANTHROPIC_DEFAULT_SONNET_MODEL": config.sonnet_model,
        "ANTHROPIC_DEFAULT_HAIKU_MODEL": config.haiku_model,
        "CLAUDE_CODE_SUBAGENT_MODEL": config.subagent_model,
    });
    existing["last_switch"] = json!({
        "source": format!("{}/{}", config.provider_id, config.profile_id),
        "at": now,
    });

    std::fs::write(&settings_path, serde_json::to_string_pretty(&existing)?)?;
    Ok(())
}

