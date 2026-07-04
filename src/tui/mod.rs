pub mod app;
pub mod file_watcher;
pub mod lang;
pub mod theme;
pub mod tabs;
pub mod widgets;

use std::path::PathBuf;

pub fn run_tui() -> anyhow::Result<()> {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let db_path = PathBuf::from(&home).join(".config/ccswitch").join("ccswitch.db");
    let defaults_path = PathBuf::from(&home).join(".config/ccswitch/defaults.toml");

    let mut app = app::App::new(db_path, defaults_path)?;
    app.run()
}
