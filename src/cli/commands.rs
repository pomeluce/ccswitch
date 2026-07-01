use std::path::PathBuf;
use anyhow::{Context, Result};
use clap::CommandFactory;
use crate::cli::args::{CliArgs, Commands, ProxyAction};
use crate::core::config::ConfigManager;
use crate::core::models::SwitchMode;
use crate::core::switcher::switch_profile;

fn get_db_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".config/ccswitch").join("ccswitch.db")
}

fn get_defaults_path() -> Option<PathBuf> {
    None // let ConfigManager resolve via XDG/legacy path
}

pub fn run_cli(args: CliArgs) -> Result<()> {
    let command = args.command.unwrap_or_else(|| {
        // No args → launch TUI
        crate::tui::run_tui().expect("TUI failed");
        std::process::exit(0);
    });

    // Handle completions and man page generation before opening the database —
    // these are pure CLI introspection commands that don't need a DB connection.
    // This also ensures they work inside Nix build sandboxes where $HOME is
    // /homeless-shelter and the database directory cannot be created.
    match &command {
        Commands::Completions { shell } => return handle_completions(shell),
        Commands::Man => return handle_man(),
        _ => {}
    }

    let db_path = get_db_path();
    let defaults_path = get_defaults_path();
    let mgr = ConfigManager::new(&db_path, defaults_path.as_deref())?;

    match command {
        Commands::Switch { target, local: _, proxy } => {
            let mode = if proxy { SwitchMode::Proxy } else { SwitchMode::Local };
            handle_switch(&mgr, target, mode)?;
        }
        Commands::List { providers, profiles: _ } => {
            handle_list(&mgr, providers)?;
        }
        Commands::Add { what, provider } => {
            handle_add(&mgr, &what, provider.as_deref())?;
        }
        Commands::Edit { target } => {
            handle_edit(&mgr, &target)?;
        }
        Commands::Remove { target } => {
            handle_remove(&mgr, &target)?;
        }
        Commands::Proxy { action } => {
            handle_proxy(action)?;
        }
        Commands::Usage { range, profile } => {
            handle_usage(&mgr, &range, profile.as_deref())?;
        }
        Commands::History { project, search } => {
            handle_history(&mgr, project.as_deref(), search.as_deref())?;
        }
        // Completions and Man are handled before DB init — see run_cli()
        Commands::Completions { .. } | Commands::Man => unreachable!(),
    }
    Ok(())
}

fn handle_switch(mgr: &ConfigManager, target: Option<String>, mode: SwitchMode) -> Result<()> {
    let target = target.unwrap_or_else(|| {
        // TODO in Task 16: fuzzy-select via skim/fzf when TUI not available
        // For now: use default
        let providers = mgr.list_providers().unwrap_or_default();
        for p in &providers {
            for pr in &p.profiles {
                if pr.default {
                    return format!("{}/{}", p.id, pr.id);
                }
            }
        }
        String::new()
    });

    let (provider_id, profile_id) = target
        .split_once('/')
        .with_context(|| format!("Invalid target '{}'. Use provider_id/profile_id", target))?;

    let config = switch_profile(mgr, provider_id, profile_id, mode, None)?;
    println!("Switched to: {} / {}", config.provider_name, config.profile_name);
    println!("  Opus:   {}", config.opus_model);
    println!("  Sonnet: {}", config.sonnet_model);
    println!("  Haiku:  {}", config.haiku_model);
    println!("  Mode:   {:?}", mode);
    Ok(())
}

fn handle_list(mgr: &ConfigManager, providers_only: bool) -> Result<()> {
    let providers = mgr.list_providers()?;
    for p in &providers {
        let source_icon = if p.source.can_delete() { "👤" } else { "🔒" };
        let default_marker = if p.profiles.iter().any(|pr| pr.default) { " ★" } else { "" };
        println!("{} {} ({}) [{}]{}", source_icon, p.name, p.id, p.api_url, default_marker);
        if !providers_only {
            for pr in &p.profiles {
                let active = if pr.default { " (default)" } else { "" };
                println!("  ├─ {} ({}) [opus: {}]{}", pr.name, pr.id, pr.opus, active);
            }
        }
        println!();
    }
    Ok(())
}

fn handle_add(mgr: &ConfigManager, what: &str, parent_provider: Option<&str>) -> Result<()> {
    match what {
        "provider" => {
            use dialoguer::Input;
            let id: String = Input::new().with_prompt("Provider ID").interact_text()?;
            let name: String = Input::new().with_prompt("Name").interact_text()?;
            let api_url: String = Input::new().with_prompt("API URL").interact_text()?;
            let api_key: String = Input::new().with_prompt("API Key (or env:VAR)").interact_text()?;
            let p = crate::core::models::Provider {
                id, name, api_url, api_key,
                profiles: vec![],
                source: crate::core::models::Source::User,
            };
            mgr.db().insert_user_provider(&p)?;
            println!("Provider added.");
        }
        "profile" => {
            let provider_id = parent_provider.context("Usage: ccs add profile <provider_id>")?;
            // Ensure provider exists
            let providers = mgr.list_providers()?;
            let provider = providers.iter().find(|p| p.id == provider_id)
                .with_context(|| format!("Provider '{}' not found. Create it first: ccs add provider", provider_id))?;
            // Auto-insert into user_providers if it's a system default (FK needs a row)
            if !provider.source.can_delete() {
                mgr.db().insert_user_provider(provider)?;
            }
            use dialoguer::Input;
            let id: String = Input::new().with_prompt("Profile ID").interact_text()?;
            let name: String = Input::new().with_prompt("Name").interact_text()?;
            let opus: String = Input::new().with_prompt("Opus model").interact_text()?;
            let sonnet: String = Input::new().with_prompt("Sonnet model").interact_text()?;
            let haiku: String = Input::new().with_prompt("Haiku model").interact_text()?;
            let subagent: String = Input::new().with_prompt("SubAgent model").interact_text()?;
            let pr = crate::core::models::Profile {
                id, name, opus, sonnet, haiku, subagent,
                default: false,
                source: crate::core::models::Source::User,
            };
            mgr.db().insert_user_profile(provider_id, &pr)?;
            println!("Profile added to provider '{}'.", provider.name);
        }
        _ => anyhow::bail!("Usage: ccs add <provider|profile> [parent_provider]"),
    }
    Ok(())
}

fn handle_edit(mgr: &ConfigManager, target: &str) -> Result<()> {
    println!("Editing {} (interactive edit — launch TUI for full edit, or use add/remove)", target);
    // For CLI: just print current state; TUI provides full edit
    let providers = mgr.list_providers()?;
    if let Some((provider_id, profile_id)) = target.split_once('/') {
        if let Some((p, pr)) = mgr.find_profile(provider_id, profile_id)? {
            println!("Provider: {} ({})", p.name, p.id);
            println!("Profile:  {} ({})", pr.name, pr.id);
            println!("  opus={} sonnet={} haiku={} subagent={}", pr.opus, pr.sonnet, pr.haiku, pr.subagent);
        }
    } else {
        for p in &providers {
            if p.id == target {
                println!("Provider: {} ({})", p.name, p.id);
                println!("  URL: {}  Key: {}", p.api_url, p.api_key);
            }
        }
    }
    Ok(())
}

fn handle_remove(mgr: &ConfigManager, target: &str) -> Result<()> {
    if let Some((provider_id, profile_id)) = target.split_once('/') {
        // Check if it's a system profile
        if let Some((_, pr)) = mgr.find_profile(provider_id, profile_id)? {
            if !pr.source.can_delete() {
                anyhow::bail!("Cannot delete system default profile '{}'", target);
            }
        }
        mgr.db().delete_user_profile(profile_id)?;
        println!("Removed profile: {}", target);
    } else {
        let providers = mgr.list_providers()?;
        for p in &providers {
            if p.id == target && !p.source.can_delete() {
                anyhow::bail!("Cannot delete system default provider '{}'", target);
            }
        }
        mgr.db().delete_user_provider(target)?;
        println!("Removed provider: {}", target);
    }
    Ok(())
}

fn handle_proxy(action: ProxyAction) -> Result<()> {
    use crate::proxy::service;
    match action {
        ProxyAction::Start => service::start_proxy()?,
        ProxyAction::Stop => service::stop_proxy()?,
        ProxyAction::Status => service::proxy_status()?,
        ProxyAction::Serve => {
            let db_path = get_db_path();
            let defaults_path = get_defaults_path();
            let mgr = ConfigManager::new(&db_path, defaults_path.as_deref())?;
            let port: u16 = mgr
                .db()
                .get_setting("proxy_port")
                .and_then(|s| s.parse().ok())
                .unwrap_or(15721);
            let server = crate::proxy::server::ProxyServer::new(mgr);
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(server.serve(port))?;
        }
    }
    Ok(())
}

fn handle_usage(mgr: &ConfigManager, range: &str, profile: Option<&str>) -> Result<()> {
    let summaries = mgr.db().query_usage(range)?;
    let total_tokens: i64 = summaries.iter().map(|s| s.total_prompt + s.total_completion).sum();
    println!("Token Usage ({})", range);
    println!("{:<30} {:>10} {:>10} {:>8}", "Profile", "Prompt", "Completion", "Reqs");
    println!("{}", "-".repeat(60));
    for s in &summaries {
        if let Some(filter) = profile {
            let key = format!("{}/{}", s.provider_id, s.profile_id);
            if !key.contains(filter) { continue; }
        }
        let key = format!("{}/{}", s.provider_id, s.profile_id);
        println!("{:<30} {:>10} {:>10} {:>8}", key, s.total_prompt, s.total_completion, s.request_count);
    }
    println!("{}", "-".repeat(60));
    println!("Total: {} tokens across {} requests", total_tokens, summaries.len());
    Ok(())
}

fn project_name(s: &crate::db::sessions::SessionRecord) -> Option<String> {
    std::path::Path::new(&s.project_path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
}

fn handle_history(mgr: &ConfigManager, project: Option<&str>, search: Option<&str>) -> Result<()> {
    // Auto-import Claude Code sessions before listing
    match mgr.db().import_claude_sessions() {
        Ok(n) if n > 0 => eprintln!("Imported {} new session(s)", n),
        Err(e) => eprintln!("Warning: failed to import sessions: {}", e),
        _ => {}
    }
    let sessions = mgr.db().query_sessions(project, search, 200)?;
    println!("Session History");
    println!("{:<6} {:<40} {:<12} {:>8} {:>6} Profile", "Date", "Title", "Project", "Tokens", "Msgs");
    println!("{}", "-".repeat(100));
    for s in &sessions {
        let date = &s.start_time[5..16]; // "MM-DD HH:MM"
        let raw = s.title.as_deref().unwrap_or(&s.id);
        let is_uuid = raw.len() >= 32 && raw.chars().filter(|c| *c == '-').count() >= 4;
        let title: String = if is_uuid {
            project_name(s).unwrap_or_else(|| raw.to_string())
        } else {
            raw.to_string()
        };
        let title = title.chars().take(40).collect::<String>();
        let project_short = project_name(s).unwrap_or_default().chars().take(12).collect::<String>();
        let tokens = s.prompt_tokens + s.completion_tokens;
        let profile = s.profile_id.as_deref().unwrap_or("-");
        println!("{:<6} {:<40} {:<12} {:>8} {:>6} {}", date, title, project_short, tokens, s.message_count, profile);
    }
    Ok(())
}

fn handle_completions(shell: &str) -> Result<()> {
    use clap_complete::{generate, shells};
    use crate::cli::args::CliArgs;
    let mut cmd = CliArgs::command();
    match shell {
        "zsh" => generate(shells::Zsh, &mut cmd, "ccs", &mut std::io::stdout()),
        "bash" => generate(shells::Bash, &mut cmd, "ccs", &mut std::io::stdout()),
        "fish" => generate(shells::Fish, &mut cmd, "ccs", &mut std::io::stdout()),
        _ => anyhow::bail!("Unsupported shell: {}. Use zsh, bash, or fish.", shell),
    }
    Ok(())
}

fn handle_man() -> Result<()> {
    use clap_mangen::Man;
    use crate::cli::args::CliArgs;
    let cmd = CliArgs::command();
    let man = Man::new(cmd);
    man.render(&mut std::io::stdout())?;
    Ok(())
}