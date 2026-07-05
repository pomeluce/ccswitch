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
reasoning_model = "r-model"
task_model = "t-model"
default = true
"#).unwrap();

    let db_path = dir.path().join("test.db");
    let settings_path = dir.path().join("settings.json");
    std::env::set_var("HOME", dir.path().to_str().unwrap());

    let mgr = ConfigManager::new(&db_path, Some(&defaults_path)).unwrap();
    let config = switch_profile(
        &mgr, "p1", "prof1", SwitchMode::Local,
        Some(&settings_path),
    ).unwrap();

    assert_eq!(config.reasoning_model, "r-model");
    assert_eq!(config.task_model, "t-model");
    assert_eq!(config.auth_token, "sk-test-key");

    let content = fs::read_to_string(&settings_path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(parsed["env"]["ANTHROPIC_MODEL"], "r-model");
    assert_eq!(parsed["env"]["ANTHROPIC_DEFAULT_HAIKU_MODEL"], "t-model");
    assert_eq!(parsed["env"]["CLAUDE_CODE_SUBAGENT_MODEL"], "t-model");
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
reasoning_model = "r-model"
task_model = "t-model"
"#).unwrap();

    std::env::set_var("TEST_KEY", "resolved-key");
    let db_path = dir.path().join("test.db");
    let settings_path = dir.path().join("settings.json");

    let mgr = ConfigManager::new(&db_path, Some(&defaults_path)).unwrap();
    let config = switch_profile(
        &mgr, "p1", "prof1", SwitchMode::Proxy,
        Some(&settings_path),
    ).unwrap();

    assert_eq!(config.base_url, "http://127.0.0.1:15721");
    assert_eq!(mgr.get_setting("active_provider"), Some("p1".into()));
    assert_eq!(mgr.get_setting("active_profile"), Some("prof1".into()));
    assert_eq!(mgr.get_setting("proxy_mode"), Some("true".into()));
}