mod cli;

use anyhow::{bail, Result};
use clap::Parser;

use nomen::config::Config;

use cli::helpers::{
    build_nomen, build_nomen_with_relay, detect_backend, load_config, resolve_config, Backend,
};
use cli::{Cli, Command};

/// Compact timestamp: `MM-DD HH:MM:SS`
struct CompactTimer;
impl tracing_subscriber::fmt::time::FormatTime for CompactTimer {
    fn format_time(&self, w: &mut tracing_subscriber::fmt::format::Writer<'_>) -> std::fmt::Result {
        use std::time::SystemTime;
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        // Civil time from epoch (UTC)
        let secs_in_day = now % 86400;
        let hour = secs_in_day / 3600;
        let min = (secs_in_day % 3600) / 60;
        let sec = secs_in_day % 60;
        let days = (now / 86400) as i64 + 719468; // days since 0000-03-01
        let era = days.div_euclid(146097);
        let doe = days.rem_euclid(146097);
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let _year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
        write!(w, "{month:02}-{day:02} {hour:02}:{min:02}:{sec:02}")
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // SurrealDB 3.x requires a rustls CryptoProvider
    let _ = rustls::crypto::ring::default_provider().install_default();

    let cli_args = Cli::parse();

    // Use warn level for CLI commands (avoid noisy relay logs), info for serve
    let default_level = if matches!(cli_args.command, Command::Serve { .. }) {
        "nomen=info"
    } else {
        "nomen=warn"
    };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(default_level.parse().unwrap()),
        )
        .with_timer(CompactTimer)
        .init();

    // Handle init, doctor, and service before resolve_config (config may not exist yet)
    match &cli_args.command {
        Command::Init {
            force,
            non_interactive,
        } => {
            return cli::admin::cmd_init(*force, *non_interactive).await;
        }
        Command::Doctor => {
            return cli::admin::cmd_doctor().await;
        }
        Command::Service { action } => {
            return cli::service::cmd_service(action, &cli_args.config);
        }
        _ => {}
    }

    // Load config and resolve once before match
    let config = load_config(&cli_args)?;
    let resolved = resolve_config(&cli_args)?;
    let backend = detect_backend(&config).await;

    match cli_args.command {
        Command::List {
            named,
            ephemeral,
            stats,
        } => {
            if stats || ephemeral {
                let nomen = if matches!(backend, Backend::Direct) {
                    Some(build_nomen(&config).await?)
                } else {
                    None
                };
                cli::memory::cmd_list_local(&backend, nomen.as_ref(), ephemeral, stats).await?;
            } else {
                if resolved.nsecs.is_empty() {
                    bail!(
                        "No nsec provided. Set it in {} or pass --nsec",
                        Config::path().display()
                    );
                }
                cli::memory::cmd_list_relay(&resolved.relay, &resolved.nsecs, named).await?;
            }
        }
        Command::Config { action } => {
            cli::admin::cmd_config(&config, &resolved.relay, &resolved.nsecs, action);
        }
        Command::Sync => {
            let nomen = if matches!(backend, Backend::Direct) {
                Some(build_nomen_with_relay(&config, &resolved).await?)
            } else {
                None
            };
            cli::sync::cmd_sync(&backend, nomen.as_ref()).await?;
        }
        Command::Store {
            topic,
            content,
            tier,
        } => {
            let nomen = if matches!(backend, Backend::Direct) {
                Some(build_nomen_with_relay(&config, &resolved).await?)
            } else {
                None
            };
            cli::memory::cmd_store(&backend, nomen.as_ref(), &topic, &content, &tier).await?;
        }
        Command::Delete {
            topic,
            id,
            ephemeral,
            older_than,
        } => {
            if ephemeral {
                let nomen = if matches!(backend, Backend::Direct) {
                    Some(build_nomen(&config).await?)
                } else {
                    None
                };
                cli::message::cmd_delete_old_messages(
                    &backend,
                    nomen.as_ref(),
                    older_than.as_deref(),
                )
                .await?;
            } else {
                let nomen = if matches!(backend, Backend::Direct) {
                    Some(build_nomen_with_relay(&config, &resolved).await?)
                } else {
                    None
                };
                cli::memory::cmd_delete(&backend, nomen.as_ref(), topic.as_deref(), id.as_deref())
                    .await?;
            }
        }
        Command::Search {
            query,
            tier,
            limit,
            vector_weight,
            text_weight,
            aggregate,
            graph,
            hops,
        } => {
            let nomen = if matches!(backend, Backend::Direct) {
                Some(build_nomen(&config).await?)
            } else {
                None
            };
            cli::memory::cmd_search(
                &backend,
                nomen.as_ref(),
                &query,
                tier.as_deref(),
                limit,
                vector_weight,
                text_weight,
                aggregate,
                graph,
                hops,
            )
            .await?;
        }
        Command::Embed { limit } => {
            let nomen = if matches!(backend, Backend::Direct) {
                Some(build_nomen(&config).await?)
            } else {
                None
            };
            cli::sync::cmd_embed(&backend, nomen.as_ref(), limit).await?;
        }
        Command::Group { action } => {
            let nomen = if matches!(backend, Backend::Direct) {
                Some(build_nomen(&config).await?)
            } else {
                None
            };
            cli::group::cmd_group(&backend, nomen.as_ref(), action).await?;
        }
        Command::Ingest {
            content,
            source,
            sender,
            platform,
            community,
            chat,
            thread,
        } => {
            let nomen = if matches!(backend, Backend::Direct) {
                Some(build_nomen(&config).await?)
            } else {
                None
            };
            cli::message::cmd_ingest(
                &backend,
                nomen.as_ref(),
                &content,
                &source,
                &sender,
                platform.as_deref(),
                community.as_deref(),
                chat.as_deref(),
                thread.as_deref(),
            )
            .await?;
        }
        Command::Messages {
            platform,
            chat,
            thread,
            sender,
            since,
            limit,
            around,
            context,
        } => {
            let nomen = if matches!(backend, Backend::Direct) {
                Some(build_nomen(&config).await?)
            } else {
                None
            };
            cli::message::cmd_messages(
                &backend,
                nomen.as_ref(),
                platform.as_deref(),
                chat.as_deref(),
                thread.as_deref(),
                sender.as_deref(),
                since.as_deref(),
                limit,
                around.as_deref(),
                context,
            )
            .await?;
        }
        Command::Consolidate {
            min_messages,
            batch_size,
            dry_run,
            older_than,
            tier,
        } => {
            let nomen = if matches!(backend, Backend::Direct) {
                Some(build_nomen_with_relay(&config, &resolved).await?)
            } else {
                None
            };
            cli::sync::cmd_consolidate(
                &backend,
                nomen.as_ref(),
                min_messages,
                batch_size,
                dry_run,
                older_than,
                tier,
            )
            .await?;
        }
        Command::Entities { kind, relations } => {
            let nomen = if matches!(backend, Backend::Direct) {
                Some(build_nomen(&config).await?)
            } else {
                None
            };
            cli::sync::cmd_entities(&backend, nomen.as_ref(), kind.as_deref(), relations).await?;
        }
        Command::Cluster {
            dry_run,
            prefix,
            min_members,
            namespace_depth,
        } => {
            let nomen = if matches!(backend, Backend::Direct) {
                Some(build_nomen(&config).await?)
            } else {
                None
            };
            cli::sync::cmd_cluster(
                &backend,
                nomen.as_ref(),
                dry_run,
                prefix,
                min_members,
                namespace_depth,
            )
            .await?;
        }
        Command::Prune { days, dry_run } => {
            let nomen = if matches!(backend, Backend::Direct) {
                Some(build_nomen(&config).await?)
            } else {
                None
            };
            cli::admin::cmd_prune(&backend, nomen.as_ref(), days, dry_run).await?;
        }
        Command::Send {
            content,
            to,
            channel,
        } => {
            let nomen = if matches!(backend, Backend::Direct) {
                Some(build_nomen_with_relay(&config, &resolved).await?)
            } else {
                None
            };
            cli::message::cmd_send(&backend, nomen.as_ref(), &to, &content, channel.as_deref())
                .await?;
        }
        Command::Serve {
            stdio,
            http: http_addr,
            static_dir,
            landing_dir,
            socket,
            context_vm,
            allowed_npubs,
        } => {
            cli::server::cmd_serve(
                &config,
                &resolved,
                stdio,
                http_addr,
                static_dir,
                landing_dir,
                socket,
                context_vm,
                allowed_npubs,
            )
            .await?;
        }
        Command::Fs { action } => {
            cli::fs::cmd_fs(&backend, &config, action).await?;
        }
        Command::Init { .. } | Command::Doctor | Command::Service { .. } => {
            unreachable!("handled above")
        }
    }

    Ok(())
}
