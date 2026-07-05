use crate::core::config::ConfigManager;
use crate::core::env::resolve_api_key;
use crate::core::models::{ActiveConfig, SwitchMode};
use anyhow::{Context, Result};
use serde_json::json;
use std::path::Path;

const DEFAULT_PROXY_PORT: u16 = 15721;

pub fn switch_profile(mgr: &ConfigManager, provider_id: &str, profile_id: &str, mode: SwitchMode, settings_path: Option<&Path>) -> Result<ActiveConfig> {
    let (provider, profile) = mgr.find_profile(provider_id, profile_id)?
        .with_context(|| format!("Profile not found: {}/{}", provider_id, profile_id))?;

    let auth_token = resolve_api_key(&provider.api_key);
    let base_url = match mode {
        SwitchMode::Proxy => format!("http://127.0.0.1:{}", DEFAULT_PROXY_PORT),
        SwitchMode::Local => provider.api_url.clone(),
    };

    let config = ActiveConfig {
        provider_id: provider.id.clone(), profile_id: profile.id.clone(),
        provider_name: provider.name.clone(), profile_name: profile.name.clone(),
        base_url: base_url.clone(), auth_token: auth_token.clone(),
        reasoning_model: profile.reasoning_model.clone(),
        task_model: profile.task_model.clone(),
    };

    match mode {
        SwitchMode::Local => { write_settings_json(&config, settings_path)?; }
        SwitchMode::Proxy => {
            write_settings_json(&config, settings_path)?;
            mgr.set_setting("active_provider", &config.provider_id)?;
            mgr.set_setting("active_profile", &config.profile_id)?;
            mgr.set_setting("proxy_mode", "true")?;
            mgr.set_setting("proxy_port", &DEFAULT_PROXY_PORT.to_string())?;
        }
    }
    Ok(config)
}

fn write_settings_json(config: &ActiveConfig, path: Option<&Path>) -> Result<()> {
    let settings_path = path.map(|p| p.to_path_buf()).unwrap_or_else(|| {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        Path::new(&home).join(".claude").join("settings.json")
    });

    let mut existing: serde_json::Value = if settings_path.exists() {
        let content = std::fs::read_to_string(&settings_path)?;
        serde_json::from_str(&content).unwrap_or(json!({}))
    } else { json!({}) };

    if let Some(parent) = settings_path.parent() { std::fs::create_dir_all(parent)?; }

    let task_model = config.task_model.replace("[1m]", "");
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    if existing["env"].is_null() || !existing["env"].is_object() { existing["env"] = json!({}); }
    let env = &mut existing["env"];
    env["ANTHROPIC_BASE_URL"] = json!(config.base_url);
    env["ANTHROPIC_AUTH_TOKEN"] = json!(config.auth_token);
    env["ANTHROPIC_MODEL"] = json!(config.reasoning_model);
    env["ANTHROPIC_DEFAULT_OPUS_MODEL"] = json!(config.reasoning_model);
    env["ANTHROPIC_DEFAULT_SONNET_MODEL"] = json!(config.reasoning_model);
    env["ANTHROPIC_DEFAULT_HAIKU_MODEL"] = json!(&task_model);
    env["CLAUDE_CODE_SUBAGENT_MODEL"] = json!(&task_model);
    existing["last_switch"] = json!({
        "source": format!("{}/{}", config.provider_id, config.profile_id), "at": now,
    });

    std::fs::write(&settings_path, serde_json::to_string_pretty(&existing)?)?;
    Ok(())
}
