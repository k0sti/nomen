//! Filesystem sync CLI commands.

use std::path::PathBuf;

use anyhow::Result;
use colored::Colorize;

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
                Box::pin(async move { dispatch_http(&base_url, &action, &params, nsec.as_deref()).await })
            })
        }
        Backend::Direct => {
            // For direct backend, we still go through HTTP dispatch since fs sync
            // is designed around the dispatch API pattern. The user should have
            // `nomen serve` running, or use --http mode.
            // Fall back to an error message if no HTTP backend.
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

pub async fn cmd_fs(backend: &Backend, action: FsAction) -> Result<()> {
    match action {
        FsAction::Init { dir } => {
            let dir = dir.unwrap_or_else(|| PathBuf::from("."));
            fs::init_sync_dir(&dir)?;
            println!("{} Initialized sync directory: {}", "Done".green().bold(), dir.display());
        }
        FsAction::Pull { dir } => {
            let dir = dir.unwrap_or_else(|| PathBuf::from("."));
            let dispatch = build_dispatch(backend);
            let count = fs::pull(&dispatch, &dir).await?;
            println!("{}: {} files written", "Pull complete".green().bold(), count);
        }
        FsAction::Push { dir } => {
            let dir = dir.unwrap_or_else(|| PathBuf::from("."));
            let dispatch = build_dispatch(backend);
            let count = fs::push(&dispatch, &dir).await?;
            println!("{}: {} memories updated", "Push complete".green().bold(), count);
        }
        FsAction::Status { dir } => {
            let dir = dir.unwrap_or_else(|| PathBuf::from("."));
            let dispatch = build_dispatch(backend);
            fs::status(&dispatch, &dir).await?;
        }
        FsAction::Start {
            dir,
            poll_secs,
            verbose,
            clean,
        } => {
            let dir = dir.unwrap_or_else(|| PathBuf::from("."));
            let dispatch = build_dispatch(backend);
            fs::start(&dispatch, &dir, poll_secs, verbose, clean).await?;
        }
        FsAction::Stop { dir } => {
            let dir = dir.unwrap_or_else(|| PathBuf::from("."));
            fs::stop(&dir)?;
        }
    }
    Ok(())
}
