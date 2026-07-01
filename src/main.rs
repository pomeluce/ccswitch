mod cli;
mod core;
mod db;
mod proxy;
mod tui;

use clap::Parser;
use cli::args::CliArgs;

fn main() {
    let args = CliArgs::parse();
    if args.command.is_none() {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());

        // First-launch check: import session data with progress before TUI starts.
        // Subsequent launches skip this — session_history already has data.
        pre_tui_import(&home);

        // Init tracing to file in TUI mode — stderr output corrupts the terminal.
        let log_path = std::path::PathBuf::from(&home).join(".config/ccswitch/ccs.log");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .unwrap_or_else(|_| std::fs::File::create("/dev/null").unwrap());
        tracing_subscriber::fmt().with_writer(std::sync::Mutex::new(file)).with_target(false).init();

        // Launch TUI
        if let Err(e) = tui::run_tui() {
            eprintln!("TUI error: {}", e);
            std::process::exit(1);
        }
        return;
    }

    tracing_subscriber::fmt::init();
    if let Err(e) = cli::commands::run_cli(args) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

/// If this is the first launch (no session data yet), run session import
/// with a terminal progress bar before the TUI starts.
fn pre_tui_import(home: &str) {
    let session_db_path = std::path::PathBuf::from(home).join(".config/ccswitch/session.db");

    let db = match db::Db::open(&session_db_path) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("Warning: cannot open session DB: {}", e);
            return;
        }
    };

    // Check if import is needed
    let has_sessions: bool = db
        .conn()
        .query_row("SELECT COUNT(*) FROM session_history", [], |r| r.get::<_, i64>(0))
        .map(|c| c > 0)
        .unwrap_or(false);

    if has_sessions {
        return; // Already imported — skip
    }

    // First launch — show progress bar
    eprintln!("\n🔄 First launch: importing Claude Code sessions...\n");

    match db.import_claude_sessions_with_progress(|files_done, files_total, imported| {
        let pct = if files_total > 0 {
            (files_done as f64 / files_total as f64 * 100.0) as usize
        } else {
            0
        };
        let bar_len = (pct / 4).min(25);
        let bar = format!("{}{}", "█".repeat(bar_len), "░".repeat(25usize.saturating_sub(bar_len)));
        // Clear line with \r and reprint
        eprint!("\r  [{}] {:>3}%  {}/{} files  {} sessions imported", bar, pct, files_done, files_total, imported);
        std::io::Write::flush(&mut std::io::stderr()).ok();
    }) {
        Ok(n) => eprintln!("\n\n✅ Imported {} sessions. Launching CCSwitch...\n", n),
        Err(e) => eprintln!("\n\n⚠️  Import finished with errors: {}\n", e),
    }
}
