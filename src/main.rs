use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use chrono::{TimeZone, Utc};
use clap::{Parser, Subcommand};
use colored::Colorize;
use nostr_sdk::prelude::*;
use nostr_sdk::prelude::nip44;
use serde::Deserialize;
use tracing::debug;

// ── Config ──────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
struct Config {
    #[serde(default)]
    relay: Option<String>,
    #[serde(default)]
    nsecs: Vec<String>,
    /// Single nsec shorthand
    #[serde(default)]
    nsec: Option<String>,
}

impl Config {
    fn load() -> Result<Self> {
        let path = Self::path();
        if path.exists() {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("Failed to read config: {}", path.display()))?;
            let cfg: Config = toml::from_str(&text)
                .with_context(|| format!("Failed to parse config: {}", path.display()))?;
            Ok(cfg)
        } else {
            Ok(Config::default())
        }
    }

    fn path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("nomen")
            .join("config.toml")
    }

    /// Merge nsec + nsecs into a single list
    fn all_nsecs(&self) -> Vec<String> {
        let mut out = self.nsecs.clone();
        if let Some(ref single) = self.nsec {
            if !out.contains(single) {
                out.insert(0, single.clone());
            }
        }
        out
    }
}

// ── CLI ─────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "nomen", about = "Nostr-native memory system CLI")]
struct Cli {
    /// Relay URL (overrides config file)
    #[arg(long)]
    relay: Option<String>,

    /// Nostr secret key (nsec1...), can be specified multiple times (overrides config file)
    #[arg(long = "nsec")]
    nsecs: Vec<String>,

    /// Path to config file (default: ~/.config/nomen/config.toml)
    #[arg(long)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List all memory events
    List,
    /// Show config file path and status
    Config,
}

// ── Memory parsing ──────────────────────────────────────────────────

#[derive(Deserialize)]
struct MemoryContent {
    summary: String,
    #[allow(dead_code)]
    detail: String,
    #[allow(dead_code)]
    context: Option<String>,
}

struct ParsedMemory {
    tier: String,
    topic: String,
    version: String,
    confidence: String,
    model: String,
    summary: String,
    created_at: Timestamp,
}

fn parse_d_tag(d_tag: &str) -> String {
    if let Some(topic) = d_tag.strip_prefix("snow:memory:") {
        topic.to_string()
    } else if let Some(rest) = d_tag.strip_prefix("snowclaw:memory:npub:") {
        format!("user:{}", &rest[..12.min(rest.len())])
    } else if let Some(group) = d_tag.strip_prefix("snowclaw:memory:group:") {
        format!("group:{group}")
    } else if d_tag.starts_with("snowclaw:config:") {
        format!("config:{}", d_tag.strip_prefix("snowclaw:config:").unwrap())
    } else {
        d_tag.to_string()
    }
}

fn parse_tier(tags: &Tags) -> String {
    let tier_val = get_tag_value(tags, "snow:tier").unwrap_or("unknown".to_string());
    if tier_val == "group" {
        if let Some(h) = get_tag_value(tags, "h") {
            return format!("group:{h}");
        }
    }
    tier_val
}

fn get_tag_value(tags: &Tags, name: &str) -> Option<String> {
    for tag in tags.iter() {
        let vec = tag.as_slice();
        if vec.len() >= 2 && vec[0] == name {
            return Some(vec[1].to_string());
        }
    }
    None
}

/// Try to decrypt NIP-44 encrypted content using the provided keys.
/// For private-tier events, the agent encrypts to itself (same pubkey).
/// Also tries decrypting with the `p` tag recipient if present.
fn try_decrypt_content(event: &Event, keys: &Keys) -> Option<String> {
    let content = event.content.as_str();

    // Quick check: if it parses as JSON already, it's not encrypted
    if content.starts_with('{') || content.starts_with('[') || content.starts_with('"') {
        return None;
    }

    // First try: self-encrypted (secret_key + own public_key)
    if let Ok(decrypted) = nip44::decrypt(keys.secret_key(), &keys.public_key(), content) {
        return Some(decrypted);
    }

    // Second try: encrypted to a `p` tag recipient
    for tag in event.tags.iter() {
        let vec = tag.as_slice();
        if vec.len() >= 2 && vec[0] == "p" {
            if let Ok(recipient_pk) = PublicKey::from_hex(&vec[1]) {
                if let Ok(decrypted) = nip44::decrypt(keys.secret_key(), &recipient_pk, content) {
                    return Some(decrypted);
                }
            }
        }
    }

    None
}

fn parse_event(event: &Event, keys: &Keys) -> ParsedMemory {
    let tags = &event.tags;
    let d_tag = get_tag_value(tags, "d").unwrap_or_default();
    let topic = parse_d_tag(&d_tag);
    let tier = parse_tier(tags);
    let version = get_tag_value(tags, "snow:version").unwrap_or("?".to_string());
    let confidence = get_tag_value(tags, "snow:confidence").unwrap_or("?".to_string());
    let model = get_tag_value(tags, "snow:model").unwrap_or("unknown".to_string());

    // Try decryption for private-tier events, fall back to raw content
    let content_str = if tier == "private" {
        match try_decrypt_content(event, keys) {
            Some(decrypted) => decrypted,
            None => event.content.to_string(),
        }
    } else {
        event.content.to_string()
    };

    let summary = match serde_json::from_str::<MemoryContent>(&content_str) {
        Ok(content) => content.summary,
        Err(_) => {
            if content_str.len() > 80 {
                format!("{}...", &content_str[..80])
            } else {
                content_str.to_string()
            }
        }
    };

    ParsedMemory {
        tier,
        topic,
        version,
        confidence,
        model,
        summary,
        created_at: event.created_at,
    }
}

fn format_timestamp(ts: Timestamp) -> String {
    let secs = ts.as_u64() as i64;
    match Utc.timestamp_opt(secs, 0) {
        chrono::LocalResult::Single(dt) => dt.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
        _ => format!("{secs}"),
    }
}

// ── Main ────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("nomen=info".parse().unwrap()),
        )
        .init();

    let cli = Cli::parse();

    // Load config file
    let config = if let Some(ref path) = cli.config {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config: {}", path.display()))?;
        toml::from_str(&text)?
    } else {
        Config::load()?
    };

    // CLI args override config file
    let nsecs = if !cli.nsecs.is_empty() {
        cli.nsecs
    } else {
        config.all_nsecs()
    };

    let relay = cli
        .relay
        .or(config.relay)
        .unwrap_or_else(|| "wss://zooid.atlantislabs.space".to_string());

    match cli.command {
        Command::List => {
            if nsecs.is_empty() {
                bail!(
                    "No nsec provided. Set it in {} or pass --nsec",
                    Config::path().display()
                );
            }
            list_memories(&relay, &nsecs).await?;
        }
        Command::Config => {
            let path = Config::path();
            println!("{}: {}", "Config path".bold(), path.display());
            println!(
                "{}: {}",
                "Exists".bold(),
                if path.exists() { "yes".green() } else { "no".red() }
            );
            println!("{}: {}", "Relay".bold(), relay);
            println!("{}: {}", "Keys configured".bold(), nsecs.len());
        }
    }

    Ok(())
}

async fn list_memories(relay_url: &str, nsecs: &[String]) -> Result<()> {
    // Parse all nsec keys
    let mut all_keys: Vec<Keys> = Vec::new();
    let mut pubkeys: Vec<PublicKey> = Vec::new();

    for nsec in nsecs {
        let keys = Keys::parse(nsec).context("Failed to parse nsec key")?;
        pubkeys.push(keys.public_key());
        all_keys.push(keys);
    }

    debug!("Parsed {} keys", all_keys.len());

    // Use the first key as the signer for AUTH
    let client = ClientBuilder::new()
        .signer(all_keys[0].clone())
        .build();

    client.add_relay(relay_url).await?;
    client.connect().await;

    debug!("Connected to {relay_url}");

    // Build filter for memory events (kind 30078) and agent lessons (kind 4129)
    let filter = Filter::new()
        .kinds(vec![Kind::Custom(30078), Kind::Custom(4129)])
        .authors(pubkeys.clone());

    debug!("Fetching events...");
    let events = client
        .fetch_events(filter, Duration::from_secs(10))
        .await
        .context("Failed to fetch events")?;

    // Separate kind 30078 (memories) and kind 4129 (lessons)
    let mut memories: Vec<ParsedMemory> = Vec::new();
    let mut lesson_count: usize = 0;

    for event in events.into_iter() {
        if event.kind == Kind::Custom(4129) {
            lesson_count += 1;
            continue;
        }

        // Skip config events
        let d_tag = get_tag_value(&event.tags, "d").unwrap_or_default();
        if d_tag.starts_with("snowclaw:config:") {
            continue;
        }

        memories.push(parse_event(&event, &all_keys[0]));
    }

    // Display results per pubkey
    for keys in &all_keys {
        let npub = keys.public_key().to_bech32()?;
        println!(
            "\n{}\n{}",
            format!("Memory Events for {npub}").bold(),
            "═".repeat(60)
        );
    }

    if memories.is_empty() && lesson_count == 0 {
        println!("\n  No memory events found.\n");
        client.disconnect().await;
        return Ok(());
    }

    // Count tiers
    let mut public_count = 0usize;
    let mut group_count = 0usize;
    let mut private_count = 0usize;

    for mem in &memories {
        match mem.tier.as_str() {
            "public" => public_count += 1,
            "private" => private_count += 1,
            t if t.starts_with("group") => group_count += 1,
            _ => public_count += 1,
        }
    }

    for mem in &memories {
        let tier_display = format!("[{}]", mem.tier);
        let tier_colored = match mem.tier.as_str() {
            "public" => tier_display.green(),
            "private" => tier_display.red(),
            _ => tier_display.yellow(),
        };

        println!(
            "\n{} {} (v{}, confidence: {})",
            tier_colored,
            mem.topic.bold(),
            mem.version,
            mem.confidence
        );
        println!("  Model: {}", mem.model);
        println!("  Summary: {}", mem.summary);
        println!("  Created: {}", format_timestamp(mem.created_at));
    }

    if lesson_count > 0 {
        println!(
            "\n  {} agent lessons (kind 4129) found",
            lesson_count.to_string().bold()
        );
    }

    println!(
        "\n{}: {} memories ({} public, {} group, {} private)\n",
        "Total".bold(),
        memories.len(),
        public_count,
        group_count,
        private_count
    );

    client.disconnect().await;
    Ok(())
}
