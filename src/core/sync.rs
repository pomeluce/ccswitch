use crate::core::config::ConfigManager;

/// Sync active provider/profile from Claude Code's settings.json (last_switch.source).
/// Called on app startup to align CCSwitch's active selection with the last switch.
pub fn sync_active_from_settings(mgr: &ConfigManager) {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let settings_path = std::path::PathBuf::from(&home).join(".claude/settings.json");
    if !settings_path.exists() {
        return;
    }
    let content = match std::fs::read_to_string(&settings_path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("Failed to read settings.json for sync: {}", e);
            return;
        }
    };
    let parsed: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("Failed to parse settings.json for sync: {}", e);
            return;
        }
    };
    let source = parsed
        .get("last_switch")
        .and_then(|v| v.get("source"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if let Some((pid, pfid)) = source.split_once('/') {
        let providers = match mgr.list_providers() {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("Failed to list providers for sync: {}", e);
                return;
            }
        };
        if providers
            .iter()
            .any(|p| p.id == pid && p.profiles.iter().any(|pr| pr.id == pfid))
        {
            if let Err(e) = mgr.set_setting("active_provider", pid) {
                tracing::warn!("sync: failed to save active_provider: {}", e);
            }
            if let Err(e) = mgr.set_setting("active_profile", pfid) {
                tracing::warn!("sync: failed to save active_profile: {}", e);
            }
        }

        // Restore proxy_mode from last switch
        let mode = parsed
            .get("last_switch")
            .and_then(|v| v.get("mode"))
            .and_then(|v| v.as_str())
            .unwrap_or("local");
        let is_proxy = mode == "proxy";
        if let Err(e) = mgr.set_setting("proxy_mode", &is_proxy.to_string()) {
            tracing::warn!("sync: failed to save proxy_mode: {}", e);
        }
    }
}
