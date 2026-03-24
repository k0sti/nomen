mod cli;

use anyhow::{bail, Result};
use clap::Parser;

use nomen::config::Config;

use cli::helpers::{
    build_nomen, build_nomen_with_relay, detect_backend, load_config, resolve_config, Backend,
};
use cli::{Cli, Command};

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
        .init();

    // Handle init and doctor before resolve_config (config may not exist yet)
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
        Command::Config => {
            cli::admin::cmd_config(&resolved.relay, &resolved.nsecs);
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
                cli::message::cmd_delete_old_messages(&backend, nomen.as_ref(), older_than.as_deref()).await?;
            } else {
                let nomen = if matches!(backend, Backend::Direct) {
                    Some(build_nomen_with_relay(&config, &resolved).await?)
                } else {
                    None
                };
                cli::memory::cmd_delete(&backend, nomen.as_ref(), topic.as_deref(), id.as_deref()).await?;
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
            channel,
        } => {
            let nomen = if matches!(backend, Backend::Direct) {
                Some(build_nomen(&config).await?)
            } else {
                None
            };
            cli::message::cmd_ingest(&backend, nomen.as_ref(), &content, &source, &sender, channel.as_deref()).await?;
        }
        Command::Messages {
            source,
            channel,
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
                source.as_deref(),
                channel.as_deref(),
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
            cli::sync::cmd_cluster(&backend, nomen.as_ref(), dry_run, prefix, min_members, namespace_depth).await?;
        }
        Command::Prune { days, dry_run } => {
            let nomen = if matches!(backend, Backend::Direct) {
                Some(build_nomen(&config).await?)
            } else {
                None
            };
            cli::admin::cmd_prune(&backend, nomen.as_ref(), days, dry_run).await?;
        }
        Command::Migrate { action } => {
            let nomen = build_nomen_with_relay(&config, &resolved).await?;
            match action {
                cli::MigrateAction::Status => {
                    cli::migrate::cmd_migrate_status(&nomen).await?;
                }
                cli::MigrateAction::Run { dry_run } => {
                    cli::migrate::cmd_migrate_run(&nomen, dry_run).await?;
                }
            }
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
            cli::message::cmd_send(&backend, nomen.as_ref(), &to, &content, channel.as_deref()).await?;
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
            cli::fs::cmd_fs(&backend, action).await?;
        }
        Command::Init { .. } | Command::Doctor => unreachable!("handled above"),
    }

    Ok(())
}
