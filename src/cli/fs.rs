//! Filesystem sync CLI commands.

use std::path::PathBuf;

use anyhow::Result;
use colored::Colorize;

use nomen::config::Config;
use nomen::fs;

use super::helpers::{dispatch_http, Backend};
use super::FsAction;

/// Build a DispatchFn from the current backend.
fn build_dispatch(backend: &Backend) -> fs::DispatchFn {
    match backend {
        Backend::Http(base_url, nsec) => {
            let base_url = base_url.clone();
            let nsec = nsec.clone();
            Box::new(move |action, params| {
                let base_url = base_url.clone();
                let nsec = nsec.clone();
                Box::pin(async move {
                    dispatch_http(&base_url, &action, &params, nsec.as_deref()).await
                })
            })
        }
        Backend::Direct => {
            Box::new(move |_action, _params| {
                Box::pin(async move {
                    anyhow::bail!(
                        "Filesystem sync requires a running nomen HTTP server.\n\
                         Start one with: nomen serve --http :3000"
                    )
                })
            })
        }
    }
}

/// Resolve the sync directory: explicit --dir flag > config fs.sync_dir > current directory.
fn resolve_dir(explicit: Option<PathBuf>, config: &Config) -> PathBuf {
    if let Some(dir) = explicit {
        return dir;
    }
    if let Some(ref fs_cfg) = config.fs {
        if let Some(ref sync_dir) = fs_cfg.sync_dir {
            return PathBuf::from(sync_dir);
        }
    }
    PathBuf::from(".")
}

pub async fn cmd_fs(backend: &Backend, config: &Config, action: FsAction) -> Result<()> {
    match action {
        FsAction::Init { dir } => {
            let dir = dir.unwrap_or_else(|| PathBuf::from("."));
            fs::init_sync_dir(&dir)?;

            // Canonicalize and persist to config
            let abs_dir = dir
                .canonicalize()
                .unwrap_or_else(|_| std::path::absolute(&dir).unwrap_or(dir.clone()));
            Config::set_value("fs.sync_dir", &abs_dir.to_string_lossy())?;

            println!(
                "{} Initialized sync directory: {}",
                "Done".green().bold(),
                abs_dir.display()
            );
            println!("Saved to config: {}", Config::path().display());
        }
        FsAction::Pull { dir } => {
            let dir = resolve_dir(dir, config);
            let dispatch = build_dispatch(backend);
            let count = fs::pull(&dispatch, &dir).await?;
            println!(
                "{}: {} files written",
                "Pull complete".green().bold(),
                count
            );
        }
        FsAction::Push { dir } => {
            let dir = resolve_dir(dir, config);
            let dispatch = build_dispatch(backend);
            let count = fs::push(&dispatch, &dir).await?;
            println!(
                "{}: {} memories updated",
                "Push complete".green().bold(),
                count
            );
        }
        FsAction::Status { dir } => {
            let dir = resolve_dir(dir, config);
            let dispatch = build_dispatch(backend);
            fs::status(&dispatch, &dir).await?;
        }
        FsAction::Start {
            dir,
            poll_secs,
            verbose,
            debug,
            clean,
        } => {
            let dir = resolve_dir(dir, config);
            if debug {
                if let Backend::Http(base_url, _) = backend {
                    println!("[debug] HTTP backend: {base_url}");
                }
            }
            let dispatch = build_dispatch(backend);
            fs::start(&dispatch, &dir, poll_secs, verbose, debug, clean).await?;
        }
        FsAction::Stop { dir } => {
            let dir = resolve_dir(dir, config);
            fs::stop(&dir)?;
        }
    }
    Ok(())
}
