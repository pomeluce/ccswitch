mod cli;
mod core;
mod db;
mod proxy;
mod tui;

use clap::Parser;
use cli::args::CliArgs;

fn main() {
    tracing_subscriber::fmt::init();

    let args = CliArgs::parse();
    if args.command.is_none() {
        // No subcommand → launch TUI
        if let Err(e) = tui::run_tui() {
            eprintln!("TUI error: {}", e);
            std::process::exit(1);
        }
        return;
    }

    if let Err(e) = cli::commands::run_cli(args) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
