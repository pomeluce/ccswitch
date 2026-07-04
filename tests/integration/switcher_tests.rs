use tempfile::tempdir;
use std::fs;
use ccswitch::core::config::ConfigManager;
use ccswitch::core::switcher::switch_profile;
use ccswitch::core::models::SwitchMode;

#[test]
fn test_switch_local_writes_settings_json() {
    let dir = tempdir().unwrap();
    let defaults_path = dir.path().join("defaults.toml");
    fs::write(&defaults_path, r#"
version = 1
[[providers]]
id = "p1"
name = "Test"
api_url = "https://api.test.com"
api_key = "sk-test-key"
[[providers.profiles]]
id = "prof1"
name = "Default"
opus = "opus-model"
sonnet = "sonnet-model"
haiku = "haiku-model"
subagent = "sub-model"
default = true
"#).unwrap();

    let db_path = dir.path().join("test.db");
    let settings_path = dir.path().join("settings.json");
    // Simulate HOME
    std::env::set_var("HOME", dir.path().to_str().unwrap());

    let mgr = ConfigManager::new(&db_path, Some(&defaults_path)).unwrap();
    let config = switch_profile(
        &mgr, "p1", "prof1", SwitchMode::Local,
        Some(&settings_path),
    ).unwrap();

    assert_eq!(config.opus_model, "opus-model");
    assert_eq!(config.auth_token, "sk-test-key");

    // Verify settings.json was written
    let content = fs::read_to_string(&settings_path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(parsed["env"]["ANTHROPIC_MODEL"], "opus-model");
    assert_eq!(parsed["env"]["ANTHROPIC_BASE_URL"], "https://api.test.com");
}

#[test]
fn test_switch_proxy_updates_sqlite() {
    let dir = tempdir().unwrap();
    let defaults_path = dir.path().join("defaults.toml");
    fs::write(&defaults_path, r#"
version = 1
[[providers]]
id = "p1"
name = "Test"
api_url = "https://api.test.com"
api_key = "env:TEST_KEY"
[[providers.profiles]]
id = "prof1"
name = "Default"
opus = "opus-model"
sonnet = "sonnet-model"
haiku = "haiku-model"
subagent = "sub-model"
"#).unwrap();

    std::env::set_var("TEST_KEY", "resolved-key");
    let db_path = dir.path().join("test.db");
    let settings_path = dir.path().join("settings.json");

    let mgr = ConfigManager::new(&db_path, Some(&defaults_path)).unwrap();
    let config = switch_profile(
        &mgr, "p1", "prof1", SwitchMode::Proxy,
        Some(&settings_path),
    ).unwrap();

    // In proxy mode, settings.json points to localhost
    assert_eq!(config.base_url, "http://127.0.0.1:15721");
    // SQLite should have active settings
    assert_eq!(mgr.get_setting("active_provider"), Some("p1".into()));
    assert_eq!(mgr.get_setting("active_profile"), Some("prof1".into()));
    assert_eq!(mgr.get_setting("proxy_mode"), Some("true".into()));
}
