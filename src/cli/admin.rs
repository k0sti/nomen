//! Admin CLI commands: config, prune, init, doctor.

use anyhow::{bail, Context, Result};
use colored::Colorize;
use dialoguer::{Confirm, Input, Password};
use nostr_sdk::{Keys, ToBech32};
use serde_json::json;

use nomen::config::{
    Config, EmbeddingConfig, MemoryConsolidationConfig, MemorySection, ServerConfig,
};
use nomen::db;
use nomen::Nomen;

use super::helpers::{
    check_service, cli_dispatch, resolve_http_url, test_relay_connection, Backend,
};

pub fn cmd_config(
    config: &Config,
    relay: &str,
    nsecs: &[String],
    action: Option<super::ConfigAction>,
) {
    match action {
        None => {
            // Show overview (original behavior)
            let path = Config::path();
            println!("{}: {}", "Config path".bold(), path.display());
            println!(
                "{}: {}",
                "Exists".bold(),
                if path.exists() {
                    "yes".green()
                } else {
                    "no".red()
                }
            );
            println!("{}: {}", "Relay".bold(), relay);
            println!("{}: {}", "Keys configured".bold(), nsecs.len());
            if let Some(ref fs_cfg) = config.fs {
                if let Some(ref dir) = fs_cfg.sync_dir {
                    println!("{}: {}", "Sync dir".bold(), dir);
                }
            }
        }
        Some(super::ConfigAction::Get { key }) => {
            let value = config_get(config, &key);
            match value {
                Some(v) => println!("{v}"),
                None => {
                    eprintln!("{}: key '{}' not set", "Error".red().bold(), key);
                    std::process::exit(1);
                }
            }
        }
        Some(super::ConfigAction::Set { key, value }) => {
            if let Err(e) = Config::set_value(&key, &value) {
                eprintln!("{}: {e}", "Error".red().bold());
                std::process::exit(1);
            }
            println!("{} {key} = {value}", "Set".green().bold());
        }
    }
}

/// Read a config value by dotted key.
fn config_get(config: &Config, key: &str) -> Option<String> {
    match key {
        "relay" => config.relay.clone(),
        "nsec" => config.nsec.clone(),
        "owner" => config.owner.clone(),
        "fs.sync_dir" => config.fs.as_ref().and_then(|f| f.sync_dir.clone()),
        "server.listen" => config.server.as_ref().map(|s| s.listen.clone()),
        "server.enabled" => config.server.as_ref().map(|s| s.enabled.to_string()),
        _ => {
            // Fall back to reading raw TOML for arbitrary keys
            let path = Config::path();
            let text = std::fs::read_to_string(path).ok()?;
            let doc: toml_edit::DocumentMut = text.parse().ok()?;
            let parts: Vec<&str> = key.split('.').collect();
            let item = match parts.len() {
                1 => doc.get(parts[0]),
                2 => doc.get(parts[0]).and_then(|t| t.get(parts[1])),
                3 => doc
                    .get(parts[0])
                    .and_then(|t| t.get(parts[1]))
                    .and_then(|t| t.get(parts[2])),
                _ => None,
            };
            item.and_then(|i| i.as_value())
                .map(|v| v.to_string().trim_matches('"').to_string())
        }
    }
}

pub async fn cmd_prune(
    backend: &Backend,
    nomen: Option<&Nomen>,
    days: u64,
    dry_run: bool,
) -> Result<()> {
    if dry_run {
        println!(
            "{} Pruning memories (older than {} days)...",
            "[DRY RUN]".yellow().bold(),
            days
        );
    } else {
        println!("Pruning memories (older than {} days)...", days);
    }

    let result = cli_dispatch(
        backend,
        nomen,
        "memory.prune",
        &json!({"days": days, "dry_run": dry_run}),
    )
    .await?;

    let memories_pruned = result["memories_pruned"].as_u64().unwrap_or(0);

    if let Some(pruned) = result["pruned"].as_array() {
        if pruned.is_empty() {
            println!("No memories eligible for pruning.");
        } else {
            println!("\n{} memories eligible for pruning:", pruned.len());
            for mem in pruned {
                let topic = mem["topic"].as_str().unwrap_or("");
                let access_count = mem["access_count"].as_u64().unwrap_or(0);
                let created_at = mem["created_at"].as_str().unwrap_or("");
                let date = if created_at.len() >= 10 {
                    &created_at[..10]
                } else {
                    created_at
                };
                println!(
                    "  {} (accesses: {}, created: {})",
                    topic.bold(),
                    access_count,
                    date
                );
            }

            if dry_run {
                println!(
                    "\n{}: Would prune {} memories",
                    "[DRY RUN]".yellow().bold(),
                    memories_pruned
                );
            } else {
                println!(
                    "\n{}: {} memories pruned",
                    "Pruned".green().bold(),
                    memories_pruned
                );
            }
        }
    } else {
        println!("No memories eligible for pruning.");
    }

    Ok(())
}

pub async fn cmd_init(force: bool, non_interactive: bool) -> Result<()> {
    println!("\n  {} — Interactive Setup\n", "Nomen".bold());

    let config_path = Config::path();
    println!("  Config will be written to: {}\n", config_path.display());

    // Check existing config
    if config_path.exists() && !force {
        if non_interactive {
            bail!(
                "Config already exists at {}. Use --force to overwrite.",
                config_path.display()
            );
        }
        let overwrite = Confirm::new()
            .with_prompt("Config already exists. Overwrite?")
            .default(false)
            .interact()?;
        if !overwrite {
            println!("Aborted.");
            return Ok(());
        }
    }

    if non_interactive {
        return cmd_init_non_interactive().await;
    }

    // 1. Relay
    println!("  {}", "1. Relay".bold());
    let relay: String = Input::new()
        .with_prompt("     Nostr relay URL")
        .default("wss://nomen.atlantislabs.space".to_string())
        .interact_text()?;

    // 2. Identity
    println!("\n  {}", "2. Identity".bold());
    println!("     Nomen needs its own Nostr keypair to sign and encrypt memories.");
    let nomen_nsec: String = Password::new().with_prompt("     Nomen nsec").interact()?;
    let nomen_keys = Keys::parse(&nomen_nsec).context("Invalid nsec")?;
    let nomen_npub = nomen_keys.public_key().to_bech32()?;
    println!("     {} {}", "✓".green(), nomen_npub);

    // 3. Embedding
    println!("\n  {}", "3. Embedding".bold());
    let emb_provider: String = Input::new()
        .with_prompt("     Provider")
        .default("openai".to_string())
        .interact_text()?;
    let emb_model: String = Input::new()
        .with_prompt("     Model")
        .default("text-embedding-3-small".to_string())
        .interact_text()?;
    let emb_api_key: String = Password::new()
        .with_prompt("     API key (leave empty to use env var)")
        .allow_empty_password(true)
        .interact()?;
    let emb_api_key_env: String = if emb_api_key.is_empty() {
        Input::new()
            .with_prompt("     API key env var")
            .default("OPENAI_API_KEY".to_string())
            .interact_text()?
    } else {
        String::new()
    };
    let emb_base_url: String = Input::new()
        .with_prompt("     Base URL (leave default for provider default)")
        .default(String::new())
        .interact_text()?;
    let emb_dimensions: usize = Input::new()
        .with_prompt("     Dimensions")
        .default(1536)
        .interact_text()?;

    // 4. Consolidation
    println!("\n  {}", "4. Consolidation".bold());
    let consolidation_enabled = Confirm::new()
        .with_prompt("     Enable auto-consolidation?")
        .default(true)
        .interact()?;

    let memory_section = if consolidation_enabled {
        let cons_provider: String = Input::new()
            .with_prompt("     LLM provider")
            .default("openrouter".to_string())
            .interact_text()?;
        let cons_model: String = Input::new()
            .with_prompt("     Model")
            .default("anthropic/claude-sonnet-4-6".to_string())
            .interact_text()?;
        let cons_api_key: String = Password::new()
            .with_prompt("     API key (leave empty to use env var)")
            .allow_empty_password(true)
            .interact()?;
        let cons_api_key_env: String = if cons_api_key.is_empty() {
            Input::new()
                .with_prompt("     API key env var")
                .default("OPENROUTER_API_KEY".to_string())
                .interact_text()?
        } else {
            String::new()
        };
        let cons_interval: u32 = Input::new()
            .with_prompt("     Interval hours")
            .default(4)
            .interact_text()?;
        let cons_ttl: u32 = Input::new()
            .with_prompt("     Message age before consolidation (minutes)")
            .default(60)
            .interact_text()?;

        Some(MemorySection {
            cluster: None,
            consolidation: Some(MemoryConsolidationConfig {
                enabled: true,
                mode: "internal".to_string(),
                interval_hours: cons_interval,
                ephemeral_ttl_minutes: cons_ttl,
                max_ephemeral_count: 200,
                dry_run: false,
                callback: None,
                callback_url: None,
                callback_npub: None,
                session_ttl_minutes: 60,
                provider: Some(cons_provider),
                model: Some(cons_model),
                api_key_env: Some(cons_api_key_env),
                base_url: None,
            }),
        })
    } else {
        None
    };

    // 5. HTTP Server
    println!("\n  {}", "5. HTTP Server".bold());
    let server_enabled = Confirm::new()
        .with_prompt("     Enable HTTP server?")
        .default(true)
        .interact()?;

    let server_config = if server_enabled {
        let listen: String = Input::new()
            .with_prompt("     Listen address")
            .default("127.0.0.1:3000".to_string())
            .interact_text()?;
        Some(ServerConfig {
            enabled: true,
            listen,
        })
    } else {
        None
    };

    // Build config
    let config = Config {
        relay: Some(relay.clone()),
        nsec: Some(nomen_nsec.clone()),
        agent_nsecs: Vec::new(),
        owner: None,
        default_writer: None,
        embedding: Some(EmbeddingConfig {
            provider: emb_provider,
            model: emb_model,
            api_key_env: emb_api_key_env,
            api_key: if emb_api_key.is_empty() {
                None
            } else {
                Some(emb_api_key)
            },
            base_url: if emb_base_url.is_empty() {
                None
            } else {
                Some(emb_base_url)
            },
            dimensions: emb_dimensions,
            batch_size: 100,
        }),
        groups: Vec::new(),
        consolidation: None,
        memory: memory_section,
        messaging: None,
        server: server_config,
        entities: None,
        contextvm: None,
        socket: None,
        fs: None,
    };

    // Write config
    let toml_str = toml::to_string_pretty(&config).context("Failed to serialize config")?;

    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create config directory: {}", parent.display()))?;
    }

    std::fs::write(&config_path, &toml_str)
        .with_context(|| format!("Failed to write config: {}", config_path.display()))?;
    println!(
        "\n  {} Config written to {}",
        "✓".green(),
        config_path.display()
    );

    // Test relay connection
    print!("  {} Testing relay connection... ", "✓".green());
    let all_nsecs = config.all_nsecs();
    match test_relay_connection(&relay, &all_nsecs).await {
        Ok(count) => {
            println!("connected");
            println!(
                "  {} Found {} existing memories for configured identities",
                "✓".green(),
                count
            );
        }
        Err(e) => {
            println!("{}", "failed".red());
            println!("    Warning: {e}");
            println!("    Config was still written — fix relay settings and retry.");
        }
    }

    println!("\n  Run `nomen serve` to start the dashboard.\n");
    Ok(())
}

async fn cmd_init_non_interactive() -> Result<()> {
    let nsec = std::env::var("NOMEN_NSEC")
        .context("NOMEN_NSEC env var is required for --non-interactive mode")?;

    // Validate the nsec
    let keys = Keys::parse(&nsec).context("Invalid NOMEN_NSEC")?;
    let npub = keys.public_key().to_bech32()?;
    println!("  Nomen identity: {npub}");

    let relay = std::env::var("NOMEN_RELAY")
        .unwrap_or_else(|_| "wss://nomen.atlantislabs.space".to_string());

    let config = Config {
        relay: Some(relay.clone()),
        nsec: Some(nsec),
        agent_nsecs: Vec::new(),
        owner: None,
        default_writer: None,
        embedding: Some(EmbeddingConfig::default()),
        groups: Vec::new(),
        consolidation: None,
        memory: Some(MemorySection {
            cluster: None,
            consolidation: Some(MemoryConsolidationConfig {
                enabled: true,
                mode: "internal".to_string(),
                interval_hours: 4,
                ephemeral_ttl_minutes: 60,
                max_ephemeral_count: 200,
                dry_run: false,
                callback: None,
                callback_url: None,
                callback_npub: None,
                session_ttl_minutes: 60,
                provider: Some("openrouter".to_string()),
                model: Some("anthropic/claude-sonnet-4-6".to_string()),
                api_key_env: Some("OPENROUTER_API_KEY".to_string()),
                base_url: None,
            }),
        }),
        messaging: None,
        server: Some(ServerConfig {
            enabled: true,
            listen: "127.0.0.1:3000".to_string(),
        }),
        entities: None,
        contextvm: None,
        socket: None,
        fs: None,
    };

    let config_path = Config::path();
    let toml_str = toml::to_string_pretty(&config).context("Failed to serialize config")?;

    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    std::fs::write(&config_path, &toml_str)?;
    println!(
        "  {} Config written to {}",
        "✓".green(),
        config_path.display()
    );

    // Test relay
    let all_nsecs = config.all_nsecs();
    match test_relay_connection(&relay, &all_nsecs).await {
        Ok(count) => {
            println!(
                "  {} Relay connected, {} existing memories",
                "✓".green(),
                count
            );
        }
        Err(e) => {
            println!("  {} Relay test failed: {e}", "✗".red());
        }
    }

    Ok(())
}

pub async fn cmd_doctor() -> Result<()> {
    println!("\n  {} — System Check\n", "Nomen Doctor".bold());

    let config_path = Config::path();
    let mut all_ok = true;

    // 1. Config file
    print!("  Config: {} ", config_path.display());
    if config_path.exists() {
        println!("{}", "✓ exists".green());
    } else {
        println!("{}", "✗ not found".red());
        println!("    Run `nomen init` to create a config file.\n");
        return Ok(());
    }

    let config = match Config::load() {
        Ok(c) => {
            println!("  Config parse: {}", "✓ valid".green());
            c
        }
        Err(e) => {
            println!("  Config parse: {} {e}", "✗ invalid".red());
            return Ok(());
        }
    };

    // 2. Keys
    let nsecs = config.all_nsecs();
    if nsecs.is_empty() {
        println!("  Keys: {}", "✗ no nsec configured".red());
        all_ok = false;
    } else {
        for (i, nsec) in nsecs.iter().enumerate() {
            let label = if i == 0 {
                "Guardian".to_string()
            } else {
                format!("Agent #{i}")
            };
            match Keys::parse(nsec) {
                Ok(keys) => {
                    let npub = keys
                        .public_key()
                        .to_bech32()
                        .unwrap_or_else(|_| "?".to_string());
                    println!("  {} key: {} {}", label, "✓".green(), npub);
                }
                Err(e) => {
                    println!("  {} key: {} {e}", label, "✗".red());
                    all_ok = false;
                }
            }
        }
    }

    // 3. Relay connectivity
    let relay_url = config
        .relay
        .as_deref()
        .unwrap_or("wss://nomen.atlantislabs.space");
    print!("  Relay ({relay_url}): ");
    if !nsecs.is_empty() {
        match test_relay_connection(relay_url, &nsecs).await {
            Ok(count) => println!("{} ({count} memories)", "✓ connected".green()),
            Err(e) => {
                println!("{} {e}", "✗ failed".red());
                all_ok = false;
            }
        }
    } else {
        println!("{}", "⚠ skipped (no keys)".yellow());
    }

    // 4. Embedding API key
    if let Some(ref emb) = config.embedding {
        let key_set = std::env::var(&emb.api_key_env)
            .map(|v| !v.is_empty())
            .unwrap_or(false);
        if key_set {
            println!(
                "  Embedding ({}): {} ${} is set",
                emb.provider,
                "✓".green(),
                emb.api_key_env
            );
        } else {
            println!(
                "  Embedding ({}): {} ${} not set",
                emb.provider,
                "✗".red(),
                emb.api_key_env
            );
            all_ok = false;
        }
    } else {
        println!("  Embedding: {}", "⚠ not configured".yellow());
    }

    // 5. Local DB writable (or HTTP service running)
    print!("  Local DB: ");
    match db::init_db().await {
        Ok(_) => println!("{}", "✓ writable".green()),
        Err(_) => {
            // DB locked — check if the service is running instead
            if let Some(base_url) = resolve_http_url(&config) {
                if check_service(&base_url).await {
                    println!("{}", "✓ service running (HTTP mode)".green());
                } else {
                    println!("{}", "✗ DB locked and service not reachable".red());
                    all_ok = false;
                }
            } else {
                println!("{}", "✗ DB locked (no server configured)".red());
                all_ok = false;
            }
        }
    }

    // 6. Consolidation API key
    if let Some(llm_config) = config.consolidation_llm_config() {
        let key_set = std::env::var(&llm_config.api_key_env)
            .map(|v| !v.is_empty())
            .unwrap_or(false);
        if key_set {
            println!(
                "  Consolidation ({}): {} ${} is set",
                llm_config.provider,
                "✓".green(),
                llm_config.api_key_env
            );
        } else {
            println!(
                "  Consolidation ({}): {} ${} not set",
                llm_config.provider,
                "✗".red(),
                llm_config.api_key_env
            );
            all_ok = false;
        }
    } else {
        println!("  Consolidation: {}", "⚠ not configured".yellow());
    }

    // Summary
    println!();
    if all_ok {
        println!("  {}\n", "All checks passed!".green().bold());
    } else {
        println!("  {}\n", "Some checks failed. Review above.".red().bold());
    }

    Ok(())
}
