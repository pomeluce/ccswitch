use crate::core::config::ConfigManager;
use crate::core::env::resolve_api_key;
use crate::core::models::{ActiveConfig, SwitchMode};
use anyhow::{Context, Result};
use serde_json::json;
use std::path::Path;

const DEFAULT_PROXY_PORT: u16 = 15721;

pub fn switch_profile(mgr: &ConfigManager, provider_id: &str, profile_id: &str, mode: SwitchMode, settings_path: Option<&Path>) -> Result<ActiveConfig> {
    let (provider, profile) = mgr
        .find_profile(provider_id, profile_id)?
        .with_context(|| format!("Profile not found: {}/{}", provider_id, profile_id))?;

    let auth_token = resolve_api_key(&provider.api_key);
    let base_url = match mode {
        SwitchMode::Proxy => format!("http://127.0.0.1:{}", DEFAULT_PROXY_PORT),
        SwitchMode::Local => provider.api_url.clone(),
    };

    tracing::info!("switch_profile: mode={:?} provider={} profile={} base_url={}", mode, provider_id, profile_id, base_url);

    let config = ActiveConfig {
        provider_id: provider.id.clone(),
        profile_id: profile.id.clone(),
        provider_name: provider.name.clone(),
        profile_name: profile.name.clone(),
        base_url: base_url.clone(),
        auth_token: auth_token.clone(),
        reasoning_model: profile.reasoning_model.clone(),
        task_model: profile.task_model.clone(),
    };

    match mode {
        SwitchMode::Local => {
            write_settings_json(&config, mode, settings_path)?;
        }
        SwitchMode::Proxy => {
            write_settings_json(&config, mode, settings_path)?;
            mgr.set_setting("active_provider", &config.provider_id)?;
            mgr.set_setting("active_profile", &config.profile_id)?;
            mgr.set_setting("proxy_mode", "true")?;
            mgr.set_setting("proxy_port", &DEFAULT_PROXY_PORT.to_string())?;
        }
    }
    Ok(config)
}

fn write_settings_json(config: &ActiveConfig, mode: SwitchMode, path: Option<&Path>) -> Result<()> {
    let settings_path = path.map(|p| p.to_path_buf()).unwrap_or_else(|| {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        Path::new(&home).join(".claude").join("settings.json")
    });

    tracing::debug!(
        "write_settings_json: path={} base_url={} auth_token={} reasoning={} task={}",
        settings_path.display(),
        config.base_url,
        if config.auth_token.is_empty() { "(empty)" } else { "(set)" },
        config.reasoning_model,
        config.task_model,
    );

    let mut existing: serde_json::Value = if settings_path.exists() {
        let content = std::fs::read_to_string(&settings_path)?;
        serde_json::from_str(&content).unwrap_or(json!({}))
    } else {
        json!({})
    };

    if let Some(parent) = settings_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    if existing["env"].is_null() || !existing["env"].is_object() {
        existing["env"] = json!({});
    }
    let env = &mut existing["env"];

    // Always write the base URL (proxy address or upstream URL)
    env["ANTHROPIC_BASE_URL"] = json!(config.base_url);

    match mode {
        SwitchMode::Local => {
            // Local: write model vars + auth token for Claude Code
            env["ANTHROPIC_AUTH_TOKEN"] = json!(config.auth_token);
            env["ANTHROPIC_MODEL"] = json!(config.reasoning_model);
            env["ANTHROPIC_DEFAULT_OPUS_MODEL"] = json!(config.reasoning_model);
            env["ANTHROPIC_DEFAULT_SONNET_MODEL"] = json!(config.reasoning_model);
            let task_model = config.task_model.replace("[1m]", "");
            env["ANTHROPIC_DEFAULT_HAIKU_MODEL"] = json!(&task_model);
            env["CLAUDE_CODE_SUBAGENT_MODEL"] = json!(&task_model);
        }
        SwitchMode::Proxy => {
            // Proxy: set dummy auth token (Claude Code requires it to skip login),
            // remove model vars — proxy server handles model routing
            env["ANTHROPIC_AUTH_TOKEN"] = json!("ccswitch-proxy");
            let model_keys = [
                "ANTHROPIC_MODEL",
                "ANTHROPIC_DEFAULT_OPUS_MODEL",
                "ANTHROPIC_DEFAULT_SONNET_MODEL",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL",
                "CLAUDE_CODE_SUBAGENT_MODEL",
            ];
            for k in &model_keys {
                env.as_object_mut().and_then(|o| o.remove(*k));
            }
        }
    }

    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    existing["last_switch"] = json!({
        "source": format!("{}/{}", config.provider_id, config.profile_id),
        "mode": match mode { SwitchMode::Local => "local", SwitchMode::Proxy => "proxy" },
        "at": now,
    });

    std::fs::write(&settings_path, serde_json::to_string_pretty(&existing)?)?;
    tracing::debug!("write_settings_json: wrote to {}", settings_path.display());
    Ok(())
}
