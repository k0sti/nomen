//! Serve command: HTTP, MCP stdio, CVM, socket server startup.

use std::path::PathBuf;

use anyhow::{bail, Result};

use nomen::config::{Config, ContextVmConfigExt};
use nomen::cvm;
use nomen::mcp;

use super::helpers::{build_relay_manager, parse_keys, ResolvedConfig};

/// Resolve static/landing dir: CLI flag > config value.
fn resolve_dir(cli: Option<PathBuf>, config_val: Option<&str>) -> Option<PathBuf> {
    if let Some(p) = cli {
        return Some(p);
    }
    config_val.map(PathBuf::from)
}

pub async fn cmd_serve(
    config: &Config,
    resolved: &ResolvedConfig,
    stdio: bool,
    http_addr: Option<String>,
    static_dir: Option<PathBuf>,
    landing_dir: Option<PathBuf>,
    socket: bool,
    context_vm: bool,
    allowed_npubs: Vec<String>,
) -> Result<()> {
    // ── Resolve effective settings: config first, CLI overrides ──

    let server_cfg = config.server.as_ref();

    // HTTP: CLI --http overrides config; if neither, check config enabled
    let effective_http = if let Some(addr) = http_addr {
        Some(addr)
    } else if server_cfg.map(|s| s.enabled).unwrap_or(false) {
        Some(server_cfg.unwrap().listen.clone())
    } else {
        None
    };

    // Static/landing dirs: CLI > config
    let resolved_static = resolve_dir(static_dir, server_cfg.and_then(|s| s.static_dir.as_deref()));
    let resolved_landing =
        resolve_dir(landing_dir, server_cfg.and_then(|s| s.landing_dir.as_deref()));

    // Socket: CLI --socket flag overrides config
    let socket_config = config.socket.as_ref();
    let socket_enabled = socket || socket_config.map(|c| c.enabled).unwrap_or(false);

    // CVM: CLI --context-vm flag overrides config
    let cvm_config = config.contextvm.as_ref();
    let cvm_enabled = context_vm || cvm_config.map(|c| c.enabled).unwrap_or(false);

    // Allowed npubs: CLI overrides config
    let effective_allowed_npubs = if allowed_npubs.is_empty() {
        cvm_config
            .map(|c| c.allowed_npubs.clone())
            .unwrap_or_default()
    } else {
        allowed_npubs
    };

    // ── Build shared resources ──────────────────────────────────

    // Optionally build relay manager if nsecs are available
    let relay_manager = if !resolved.nsecs.is_empty() {
        let (all_keys, _) = parse_keys(&resolved.nsecs)?;
        let mgr = build_relay_manager(&resolved.relay, &all_keys[0]);
        mgr.connect().await.ok();
        Some(mgr)
    } else {
        None
    };

    // Open DB once and share across CVM, socket, and HTTP servers
    // SurrealDB 3.x uses exclusive file locks — cannot open the same path twice
    let shared_db = nomen::db::init_db_with_dimensions(config.embedding_dimensions()).await?;

    // Validate CVM requirements early
    if cvm_enabled && resolved.nsecs.is_empty() {
        bail!(
            "CVM requires nsec keys. Set in {} or pass --nsec",
            Config::path().display()
        );
    }

    // ── Build CVM server (if enabled) ────────────────────────────
    let cvm_server = if cvm_enabled {
        let (all_keys, _) = parse_keys(&resolved.nsecs)?;
        let cvm_keys = all_keys[0].clone();

        let cvm_relay = cvm_config
            .and_then(|c| c.relay.clone())
            .unwrap_or_else(|| resolved.relay.clone());
        let cvm_encryption = cvm_config
            .map(|c| c.encryption_mode())
            .unwrap_or(contextvm_sdk::EncryptionMode::Optional);
        let cvm_rate_limit = cvm_config.map(|c| c.rate_limit).unwrap_or(30);
        let cvm_announce = cvm_config.map(|c| c.announce).unwrap_or(true);

        let cvm_nomen = nomen::Nomen::open_with_db(config, shared_db.clone()).await?;

        Some(
            cvm::CvmServer::new(
                Box::new(cvm_nomen),
                cvm_keys,
                &cvm_relay,
                cvm_encryption,
                effective_allowed_npubs,
                cvm_rate_limit,
                cvm_announce,
            )
            .await?,
        )
    } else {
        None
    };

    // ── Build Socket server (if enabled) ────────────────────────
    if socket_enabled {
        let sock_config = socket_config
            .cloned()
            .unwrap_or_else(|| nomen::config::SocketConfig {
                enabled: true,
                path: nomen::config::default_socket_path(),
                max_connections: 32,
                max_frame_size: 16 * 1024 * 1024,
            });

        let (event_tx, _) = tokio::sync::broadcast::channel(1024);

        let mut socket_nomen = nomen::Nomen::open_with_db(config, shared_db.clone()).await?;
        socket_nomen.set_event_emitter(event_tx.clone());

        let server = nomen::socket::SocketServer::new(
            std::sync::Arc::new(socket_nomen) as std::sync::Arc<dyn nomen_api::NomenBackend>,
            &sock_config,
            Some(event_tx),
        );

        tokio::spawn(async move {
            if let Err(e) = server.run().await {
                tracing::error!("Socket server error: {e}");
            }
        });
    }

    // ── Run the selected combination ─────────────────────────────
    match (effective_http, cvm_server) {
        // HTTP (± CVM): build HTTP state, run concurrently if CVM enabled
        (Some(addr), cvm_opt) => {
            let bind_addr = if addr.starts_with(':') {
                format!("0.0.0.0{addr}")
            } else {
                addr
            };

            let nomen_instance = if let Some(relay) = relay_manager {
                nomen::Nomen::open_with_db_and_relay(config, shared_db.clone(), relay).await?
            } else {
                nomen::Nomen::open_with_db(config, shared_db.clone()).await?
            };

            let http_state = nomen::http::AppState {
                nomen: std::sync::Arc::new(nomen_instance)
                    as std::sync::Arc<dyn nomen_api::NomenBackend>,
                config: std::sync::Arc::new(tokio::sync::RwLock::new(config.clone())),
            };

            let http_fut =
                nomen::http::serve(&bind_addr, http_state, resolved_static, resolved_landing);

            if let Some(cvm) = cvm_opt {
                // HTTP + CVM: run both concurrently
                let cvm_fut = cvm.run();
                tokio::select! {
                    result = http_fut => result,
                    result = cvm_fut => result,
                }
            } else {
                // HTTP only
                http_fut.await
            }
        }
        // CVM (± stdio MCP)
        (None, Some(cvm)) => {
            if stdio {
                // CVM + stdio MCP: run both concurrently
                let mcp_nomen = if let Some(relay) = relay_manager {
                    nomen::Nomen::open_with_db_and_relay(config, shared_db.clone(), relay).await?
                } else {
                    nomen::Nomen::open_with_db(config, shared_db.clone()).await?
                };
                let mcp_nomen_arc: std::sync::Arc<dyn nomen_api::NomenBackend> =
                    std::sync::Arc::new(mcp_nomen);
                let mcp_fut = mcp::serve_stdio_arc(mcp_nomen_arc);
                let cvm_fut = cvm.run();
                tokio::select! {
                    result = mcp_fut => result,
                    result = cvm_fut => result,
                }
            } else {
                // CVM only
                cvm.run().await
            }
        }
        // stdio MCP only (default)
        (None, None) => {
            let _ = stdio;
            let nomen_instance = if let Some(relay) = relay_manager {
                nomen::Nomen::open_with_db_and_relay(config, shared_db.clone(), relay).await?
            } else {
                nomen::Nomen::open_with_db(config, shared_db.clone()).await?
            };
            let nomen_arc: std::sync::Arc<dyn nomen_api::NomenBackend> =
                std::sync::Arc::new(nomen_instance);
            mcp::serve_stdio_arc(nomen_arc).await
        }
    }
}
