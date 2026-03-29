//! Service management: install/uninstall systemd user units, start/stop/restart/status/logs.

use std::path::PathBuf;

use anyhow::{bail, Result};

use nomen::config::Config;

const SERVICE_NAME: &str = "nomen";

/// Returns true if the unit file appears to be managed by Nix (symlink into /nix/store).
fn is_nix_managed(service_file: &std::path::Path) -> bool {
    std::fs::read_link(service_file)
        .map(|target| target.to_string_lossy().contains("/nix/store/"))
        .unwrap_or(false)
}

fn service_dir() -> Result<PathBuf> {
    Ok(dirs::config_dir()
        .ok_or_else(|| anyhow::anyhow!("Cannot determine config directory"))?
        .join("systemd/user"))
}

fn run_systemctl(args: &[&str]) -> Result<()> {
    let status = std::process::Command::new("systemctl")
        .arg("--user")
        .args(args)
        .status()?;
    if !status.success() {
        bail!("systemctl --user {} failed", args.join(" "));
    }
    Ok(())
}

fn resolve_service_config_path(config_path: &Option<PathBuf>) -> Result<PathBuf> {
    match config_path {
        Some(path) if path.is_absolute() => Ok(path.clone()),
        Some(path) => Ok(std::env::current_dir()?.join(path)),
        None => Ok(Config::path()),
    }
}

pub fn cmd_service(action: &super::ServiceAction, config_path: &Option<PathBuf>) -> Result<()> {
    let svc_dir = service_dir()?;
    let service_file = svc_dir.join(format!("{SERVICE_NAME}.service"));

    match action {
        super::ServiceAction::Install { force } => {
            // Warn if nix-managed
            if is_nix_managed(&service_file) {
                if !force {
                    println!(
                        "Warning: {} is a Nix-managed symlink.",
                        service_file.display()
                    );
                    println!("  Use --force to remove the symlink and install a regular unit file.");
                    println!("  Re-enable in nix-config with `nixos-rebuild` later.");
                    return Ok(());
                }
                // Remove the nix symlink so we can write a regular file
                std::fs::remove_file(&service_file)?;
                println!("Removed Nix-managed symlink: {}", service_file.display());
            }

            let exe = std::env::current_exe()?;
            let exe_path = exe.display();

            let config_path = resolve_service_config_path(config_path)?;

            let unit = format!(
                r#"[Unit]
Description=Nomen Memory Service
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart={exe_path} --config {config_path} serve
Restart=always
RestartSec=5
Environment=HOME={home}
Environment=XDG_CONFIG_HOME={home}/.config
Environment=RUST_LOG=nomen=info

[Install]
WantedBy=default.target
"#,
                config_path = config_path.display(),
                home = dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("/home"))
                    .display(),
            );

            std::fs::create_dir_all(&svc_dir)?;
            std::fs::write(&service_file, unit)?;
            run_systemctl(&["daemon-reload"])?;

            println!("Service installed: {}", service_file.display());
            println!();
            println!("Set environment variables if needed:");
            println!("  systemctl --user edit {SERVICE_NAME}");
            println!("  # Add under [Service]:");
            println!("  # Environment=OPENAI_API_KEY=sk-...");
            println!("  # Environment=OPENROUTER_API_KEY=sk-...");
            println!();
            println!("Then: nomen service start");
        }

        super::ServiceAction::Start => {
            run_systemctl(&["enable", "--now", SERVICE_NAME])?;
            println!("Service started");
        }

        super::ServiceAction::Stop => {
            run_systemctl(&["stop", SERVICE_NAME])?;
            println!("Service stopped");
        }

        super::ServiceAction::Restart => {
            run_systemctl(&["restart", SERVICE_NAME])?;
            println!("Service restarted");
        }

        super::ServiceAction::Status => {
            let _ = run_systemctl(&["status", SERVICE_NAME]);
        }

        super::ServiceAction::Logs { .. } | super::ServiceAction::Follow => {
            let follow = matches!(action, super::ServiceAction::Follow | super::ServiceAction::Logs { follow: true });
            let mut args = vec!["-o", "cat", "--user", "-u", SERVICE_NAME, "--no-pager"];
            if follow {
                args.push("-f");
            }
            let status = std::process::Command::new("journalctl")
                .args(&args)
                .status()?;
            if !status.success() {
                bail!("journalctl failed");
            }
        }

        super::ServiceAction::Uninstall => {
            if is_nix_managed(&service_file) {
                println!("Warning: {} is managed by Nix.", service_file.display());
                println!("  Remove the service from your nix-config instead.");
                return Ok(());
            }
            let _ = run_systemctl(&["disable", "--now", SERVICE_NAME]);
            if service_file.exists() {
                std::fs::remove_file(&service_file)?;
                run_systemctl(&["daemon-reload"])?;
                println!("Service uninstalled");
            } else {
                println!("Service not installed");
            }
        }
    }

    Ok(())
}
