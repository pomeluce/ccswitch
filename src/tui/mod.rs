pub mod app;
pub mod theme;
pub mod tabs;
pub mod widgets;

use std::path::PathBuf;

pub fn run_tui() -> anyhow::Result<()> {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let db_path = PathBuf::from(&home).join(".ccswitch").join("ccswitch.db");
    // XDG config path, fallback to legacy
    let defaults_path = {
        let xdg = PathBuf::from(&home).join(".config/ccswitch/defaults.toml");
        if xdg.exists() { xdg } else { PathBuf::from(&home).join(".ccswitch/defaults.toml") }
    };

    let mut app = app::App::new(db_path, defaults_path)?;
    app.run()
}
