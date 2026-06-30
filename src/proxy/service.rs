use std::path::PathBuf;
use std::process::Command;

use anyhow::Context;

const SYSTEMD_UNIT_NAME: &str = "ccs-proxy.service";

fn systemd_user_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home)
        .join(".config")
        .join("systemd")
        .join("user")
}

fn write_systemd_unit() -> anyhow::Result<PathBuf> {
    let dir = systemd_user_dir();
    std::fs::create_dir_all(&dir)?;
    let unit_path = dir.join(SYSTEMD_UNIT_NAME);
    let exe = std::env::current_exe()?;
    let unit = format!(
        r#"[Unit]
Description=CCSwitch Proxy Server
After=network.target

[Service]
ExecStart={} proxy serve
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
"#,
        exe.display()
    );
    std::fs::write(&unit_path, unit)?;
    Ok(unit_path)
}

/// Detect whether systemd --user is available.
///
/// Returns `true` if either:
/// - `systemctl --user is-active --quiet` succeeds, **or**
/// - the `SYSTEMD_EXEC_PID` environment variable is set (indicating we are
///   running inside a systemd --user scope).
fn systemd_available() -> bool {
    Command::new("systemctl")
        .args(["--user", "is-active", "--quiet", "-"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
        || std::env::var("SYSTEMD_EXEC_PID").is_ok()
}

/// Start the proxy daemon.
///
/// If systemd --user is available, a user-level service unit is written and
/// started via `systemctl --user enable --now`.  Otherwise the current binary
/// is forked into the background with stdout/stderr discarded.
pub fn start_proxy() -> anyhow::Result<()> {
    if systemd_available() {
        write_systemd_unit()?;
        Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .status()
            .context("systemctl daemon-reload failed")?;
        Command::new("systemctl")
            .args(["--user", "enable", "--now", SYSTEMD_UNIT_NAME])
            .status()
            .context("Failed to start proxy via systemctl")?;
        println!("Proxy started via systemd (user service: {SYSTEMD_UNIT_NAME})");
    } else {
        let exe = std::env::current_exe()?;
        let child = Command::new(exe)
            .arg("proxy")
            .arg("serve")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .context("Failed to fork proxy process")?;
        println!("Proxy started in background (PID: {})", child.id());
    }
    Ok(())
}

/// Stop the proxy daemon.
///
/// Uses systemctl --user stop when available; otherwise kills matching
/// processes via `pkill -f "ccs proxy serve"`.
pub fn stop_proxy() -> anyhow::Result<()> {
    if systemd_available() {
        Command::new("systemctl")
            .args(["--user", "stop", SYSTEMD_UNIT_NAME])
            .status()
            .context("Failed to stop proxy via systemctl")?;
        println!("Proxy stopped (systemd)");
    } else {
        let output = Command::new("pkill").args(["-f", "ccs proxy serve"]).output();
        match output {
            Ok(o) if o.status.success() => println!("Proxy stopped"),
            _ => println!("No proxy process found"),
        }
    }
    Ok(())
}

/// Report the current proxy daemon status.
pub fn proxy_status() -> anyhow::Result<()> {
    if systemd_available() {
        let status = Command::new("systemctl")
            .args(["--user", "status", SYSTEMD_UNIT_NAME])
            .output()?;
        println!("{}", String::from_utf8_lossy(&status.stdout));
        if !status.status.success() {
            let stderr = String::from_utf8_lossy(&status.stderr);
            if !stderr.is_empty() {
                eprintln!("{}", stderr);
            }
        }
    } else {
        let output = Command::new("pgrep")
            .args(["-f", "ccs proxy serve"])
            .output()?;
        if output.status.success() {
            let pids = String::from_utf8_lossy(&output.stdout);
            println!("Proxy running, PIDs: {}", pids.trim());
        } else {
            println!("Proxy not running");
        }
    }
    Ok(())
}
