//! CVM Smoke Test — sends tools/list and one real tool call to a running Nomen CVM server.
//!
//! Usage:
//!   cargo run --example cvm_smoke_test -- \
//!     --server-pubkey <hex-or-npub> \
//!     --relay wss://zooid.atlantislabs.space \
//!     --nsec <client-nsec>
//!
//! The client nsec must be one the server allows (or server must have empty allowlist).
//! Exits 0 on success, 1 on timeout/failure.

use std::time::Duration;

use anyhow::{bail, Context, Result};
use contextvm_sdk::proxy::{NostrMCPProxy, ProxyConfig};
use contextvm_sdk::transport::client::NostrClientTransportConfig;
use contextvm_sdk::{EncryptionMode, JsonRpcMessage, JsonRpcRequest};
use nostr_sdk::prelude::*;
use serde_json::json;

#[derive(Debug)]
struct Args {
    server_pubkey: String,
    relay: String,
    nsec: String,
    timeout_secs: u64,
    encryption: EncryptionMode,
}

fn parse_args() -> Result<Args> {
    let mut args = std::env::args().skip(1);
    let mut server_pubkey = None;
    let mut relay = "wss://zooid.atlantislabs.space".to_string();
    let mut nsec = None;
    let mut timeout_secs = 30u64;
    let mut encryption = EncryptionMode::Optional;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--server-pubkey" => {
                server_pubkey = Some(args.next().context("--server-pubkey requires a value")?);
            }
            "--relay" => {
                relay = args.next().context("--relay requires a value")?;
            }
            "--nsec" => {
                nsec = Some(args.next().context("--nsec requires a value")?);
            }
            "--timeout" => {
                timeout_secs = args
                    .next()
                    .context("--timeout requires a value")?
                    .parse()
                    .context("--timeout must be a number")?;
            }
            "--encryption" => {
                encryption = match args
                    .next()
                    .context("--encryption requires a value")?
                    .as_str()
                {
                    "disabled" => EncryptionMode::Disabled,
                    "required" => EncryptionMode::Required,
                    _ => EncryptionMode::Optional,
                };
            }
            "--help" | "-h" => {
                eprintln!(
                    "Usage: cvm_smoke_test --server-pubkey <hex|npub> --nsec <nsec> [--relay <url>] [--timeout <secs>] [--encryption optional|disabled|required]"
                );
                std::process::exit(0);
            }
            other => bail!("Unknown argument: {other}"),
        }
    }

    let server_pubkey = server_pubkey.context("--server-pubkey is required")?;
    let nsec = nsec.context("--nsec is required")?;

    // Normalize server pubkey to hex
    let server_pubkey = if server_pubkey.starts_with("npub") {
        PublicKey::from_bech32(&server_pubkey)
            .context("Invalid npub")?
            .to_hex()
    } else {
        server_pubkey
    };

    Ok(Args {
        server_pubkey,
        relay,
        nsec,
        timeout_secs,
        encryption,
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("contextvm_sdk=debug".parse().unwrap())
                .add_directive("cvm_smoke_test=info".parse().unwrap()),
        )
        .init();

    let args = parse_args()?;
    let timeout = Duration::from_secs(args.timeout_secs);

    eprintln!("CVM Smoke Test");
    eprintln!("  Server pubkey: {}", &args.server_pubkey[..16]);
    eprintln!("  Relay: {}", args.relay);
    eprintln!("  Timeout: {}s", args.timeout_secs);
    eprintln!("  Encryption: {:?}", args.encryption);
    eprintln!();

    // Parse client keys
    let client_keys = Keys::parse(&args.nsec).context("Invalid nsec")?;
    eprintln!("  Client pubkey: {}", client_keys.public_key().to_bech32()?);

    // Build proxy
    let config = ProxyConfig {
        nostr_config: NostrClientTransportConfig {
            relay_urls: vec![args.relay.clone()],
            server_pubkey: args.server_pubkey.clone(),
            encryption_mode: args.encryption,
            is_stateless: true,
            timeout,
        },
    };

    let mut proxy = NostrMCPProxy::new(client_keys, config).await?;
    let mut rx = proxy.start().await?;

    // ── Test 1: tools/list ──────────────────────────────────────
    eprintln!("\n[1] Sending tools/list...");

    let tools_list_req = JsonRpcMessage::Request(JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: json!(1),
        method: "tools/list".to_string(),
        params: None,
    });

    proxy.send(&tools_list_req).await?;

    let resp = tokio::time::timeout(timeout, rx.recv())
        .await
        .context("Timeout waiting for tools/list response")?
        .context("Channel closed before receiving response")?;

    match &resp {
        JsonRpcMessage::Response(r) => {
            let tools = r.result["tools"].as_array().map(|a| a.len()).unwrap_or(0);
            eprintln!("  OK: received {} tools", tools);
            if tools == 0 {
                eprintln!("  WARNING: tools list is empty");
            }
        }
        JsonRpcMessage::ErrorResponse(e) => {
            eprintln!("  ERROR: {} (code {})", e.error.message, e.error.code);
            proxy.stop().await?;
            std::process::exit(1);
        }
        other => {
            eprintln!("  UNEXPECTED: {:?}", other);
            proxy.stop().await?;
            std::process::exit(1);
        }
    }

    // ── Test 2: memory.list via direct action ────────────────────
    eprintln!("\n[2] Sending memory.list (direct action)...");

    let list_req = JsonRpcMessage::Request(JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: json!(2),
        method: "memory.list".to_string(),
        params: Some(json!({})),
    });

    proxy.send(&list_req).await?;

    let resp2 = tokio::time::timeout(timeout, rx.recv())
        .await
        .context("Timeout waiting for memory.list response")?
        .context("Channel closed before receiving response")?;

    match &resp2 {
        JsonRpcMessage::Response(r) => {
            let ok = r.result["ok"].as_bool().unwrap_or(false);
            if ok {
                eprintln!("  OK: memory.list succeeded");
            } else {
                eprintln!(
                    "  ERROR: memory.list returned ok=false: {}",
                    serde_json::to_string_pretty(&r.result).unwrap_or_default()
                );
                proxy.stop().await?;
                std::process::exit(1);
            }
        }
        JsonRpcMessage::ErrorResponse(e) => {
            eprintln!("  ERROR: {} (code {})", e.error.message, e.error.code);
            proxy.stop().await?;
            std::process::exit(1);
        }
        other => {
            eprintln!("  UNEXPECTED: {:?}", other);
            proxy.stop().await?;
            std::process::exit(1);
        }
    }

    // ── Done ────────────────────────────────────────────────────
    proxy.stop().await?;
    eprintln!("\nAll smoke tests passed.");
    Ok(())
}
