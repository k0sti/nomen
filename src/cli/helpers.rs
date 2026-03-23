//! Shared CLI helpers: config resolution, backend detection, dispatch.

use std::sync::Arc;

use anyhow::{bail, Context, Result};
use nostr_sdk::prelude::*;
use serde_json::json;
use tracing::debug;

use nomen::config::Config;
use nomen::relay::{RelayConfig, RelayManager};
use nomen::signer::{KeysSigner, NomenSigner};
use nomen::Nomen;

use super::Cli;

pub const CLI_CHANNEL: &str = "cli";

// ── Resolve keys + relay from CLI + config ──────────────────────────

pub struct ResolvedConfig {
    pub nsecs: Vec<String>,
    pub relay: String,
}

pub fn load_config(cli: &Cli) -> Result<Config> {
    if let Some(ref path) = cli.config {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config: {}", path.display()))?;
        Ok(toml::from_str(&text)?)
    } else {
        Config::load()
    }
}

pub fn resolve_config(cli: &Cli) -> Result<ResolvedConfig> {
    let config = if let Some(ref path) = cli.config {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config: {}", path.display()))?;
        toml::from_str(&text)?
    } else {
        Config::load()?
    };

    let nsecs = if !cli.nsecs.is_empty() {
        cli.nsecs.clone()
    } else {
        config.all_nsecs()
    };

    let relay = cli
        .relay
        .clone()
        .or(config.relay)
        .unwrap_or_else(|| "wss://nomen.atlantislabs.space".to_string());

    Ok(ResolvedConfig { nsecs, relay })
}

pub fn parse_keys(nsecs: &[String]) -> Result<(Vec<Keys>, Vec<PublicKey>)> {
    let mut all_keys = Vec::new();
    let mut pubkeys = Vec::new();
    for nsec in nsecs {
        let keys = Keys::parse(nsec).context("Failed to parse nsec key")?;
        pubkeys.push(keys.public_key());
        all_keys.push(keys);
    }
    Ok((all_keys, pubkeys))
}

pub fn build_signer(keys: &Keys) -> Arc<dyn NomenSigner> {
    Arc::new(KeysSigner::new(keys.clone()))
}

pub fn build_relay_manager(relay_url: &str, keys: &Keys) -> RelayManager {
    RelayManager::new(
        build_signer(keys),
        RelayConfig {
            relay_url: relay_url.to_string(),
            ..Default::default()
        },
    )
}

/// Build a Nomen instance with relay connected.
pub async fn build_nomen_with_relay(config: &Config, resolved: &ResolvedConfig) -> Result<Nomen> {
    if resolved.nsecs.is_empty() {
        bail!(
            "No nsec provided. Set it in {} or pass --nsec",
            Config::path().display()
        );
    }
    let (all_keys, _) = parse_keys(&resolved.nsecs)?;
    let mgr = build_relay_manager(&resolved.relay, &all_keys[0]);
    mgr.connect().await?;
    Nomen::open_with_relay(config, mgr).await
}

/// Build a Nomen instance without relay.
pub async fn build_nomen(config: &Config) -> Result<Nomen> {
    Nomen::open(config).await
}

// ── Backend detection ────────────────────────────────────────────────

pub enum Backend {
    Http(String, Option<String>), // dispatch base URL, optional nsec
    Direct,
}

/// Build the base URL from server config (e.g. "http://127.0.0.1:3849/memory/api").
pub fn resolve_http_url(config: &Config) -> Option<String> {
    let sc = config.server.as_ref()?;
    if !sc.enabled {
        return None;
    }
    let listen = &sc.listen;
    // Normalise: port-only ("3849"), colon-prefixed (":3849"), or full ("127.0.0.1:3849")
    let addr = if listen.starts_with(':') {
        format!("127.0.0.1{listen}")
    } else if !listen.contains(':') {
        format!("127.0.0.1:{listen}")
    } else {
        listen.clone()
    };
    Some(format!("http://{addr}/memory/api"))
}

/// Async health check — returns true if the service is reachable.
pub async fn check_service(base_url: &str) -> bool {
    let url = format!("{base_url}/health");
    reqwest::Client::new()
        .get(&url)
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

/// Detect whether to use the running HTTP service or open the DB directly.
pub async fn detect_backend(config: &Config) -> Backend {
    if let Some(base_url) = resolve_http_url(config) {
        if check_service(&base_url).await {
            debug!("Service detected at {base_url}, using HTTP backend");
            let nsec = config.all_nsecs().into_iter().next();
            return Backend::Http(base_url, nsec);
        }
    }
    Backend::Direct
}

/// POST an action to the HTTP dispatch endpoint.
pub async fn dispatch_http(
    base_url: &str,
    action: &str,
    params: &serde_json::Value,
    nsec: Option<&str>,
) -> Result<serde_json::Value> {
    let url = format!("{base_url}/dispatch");
    let body = json!({ "action": action, "params": params });
    let mut req = reqwest::Client::new().post(&url).json(&body);
    if let Some(nsec) = nsec {
        req = req.header("Authorization", format!("Nostr {nsec}"));
    }
    let resp = req
        .send()
        .await
        .context("HTTP dispatch request failed")?;
    let status = resp.status();
    let payload: serde_json::Value = resp.json().await.context("Failed to parse dispatch response")?;
    if !status.is_success() || payload.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        let err = payload["error"]
            .as_str()
            .unwrap_or("unknown error");
        bail!("{action}: {err}");
    }
    Ok(payload.get("result").cloned().unwrap_or(serde_json::Value::Null))
}

/// Call api::dispatch (direct) or HTTP dispatch, and extract the result or bail on error.
pub async fn cli_dispatch(
    backend: &Backend,
    nomen: Option<&Nomen>,
    action: &str,
    params: &serde_json::Value,
) -> Result<serde_json::Value> {
    match backend {
        Backend::Http(base_url, nsec) => dispatch_http(base_url, action, params, nsec.as_deref()).await,
        Backend::Direct => {
            let nomen = nomen.expect("Direct backend requires a Nomen instance");
            let resp = nomen::api::dispatch(nomen, CLI_CHANNEL, action, params).await;
            if resp.ok {
                Ok(resp.result.unwrap_or(serde_json::Value::Null))
            } else {
                let err = resp
                    .error
                    .map(|e| e.message)
                    .unwrap_or_else(|| "unknown error".to_string());
                bail!("{action}: {err}")
            }
        }
    }
}

/// Test relay connectivity and return count of existing memories.
pub async fn test_relay_connection(relay_url: &str, nsecs: &[String]) -> Result<usize> {
    if nsecs.is_empty() {
        bail!("No nsec keys configured");
    }
    let (all_keys, pubkeys) = parse_keys(nsecs)?;
    let mgr = build_relay_manager(relay_url, &all_keys[0]);
    mgr.connect().await?;
    let events = mgr.fetch_memories(&pubkeys).await?;
    let count = events.len();
    mgr.disconnect().await;
    Ok(count)
}
