use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use colored::Colorize;
use dialoguer::{Confirm, Input, Select, Password};
use nostr_sdk::prelude::*;
use tracing::debug;

use nomen::config::{
    Config, EmbeddingConfig, MemoryConsolidationConfig, MemorySection, ServerConfig,
};
use nomen::consolidate;
use nomen::contextvm;
use nomen::db;
use nomen::display::{display_memories, format_timestamp};
use nomen::entities;
use nomen::groups;
use nomen::ingest;
use nomen::mcp;
use nomen::kinds::{MEMORY_KIND, LESSON_KIND, LEGACY_LESSON_KIND};
use nomen::memory::{get_tag_value, parse_event};
use nomen::relay::{RelayConfig, RelayManager};
use nomen::search;
use nomen::send;
use nomen::signer::{NomenSigner, KeysSigner};

// ── CLI ─────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "nomen", about = "Nostr-native memory system CLI")]
struct Cli {
    /// Relay URL (overrides config file)
    #[arg(long)]
    relay: Option<String>,

    /// Nostr secret key (nsec1...), can be specified multiple times
    #[arg(long = "nsec")]
    nsecs: Vec<String>,

    /// Path to config file
    #[arg(long)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List all memory events (fetches directly from relay)
    List {
        /// Show only named memories (skip ephemeral)
        #[arg(long)]
        named: bool,
        /// Show only ephemeral memories (pending consolidation)
        #[arg(long)]
        ephemeral: bool,
        /// Show consolidation statistics
        #[arg(long)]
        stats: bool,
    },
    /// Show config file path and status
    Config,
    /// Sync memory events from relay to local SurrealDB
    Sync,
    /// Store a new memory
    Store {
        /// Topic/namespace for the memory
        topic: String,
        /// Short summary
        #[arg(long)]
        summary: String,
        /// Full detail text
        #[arg(long, default_value = "")]
        detail: String,
        /// Visibility tier
        #[arg(long, default_value = "public")]
        tier: String,
        /// Confidence score (0.0 to 1.0)
        #[arg(long, default_value = "0.8")]
        confidence: f64,
    },
    /// Delete a memory by topic or event ID
    Delete {
        /// Topic to delete
        topic: Option<String>,
        /// Event ID to delete
        #[arg(long)]
        id: Option<String>,
        /// Delete ephemeral (raw) messages instead of memories
        #[arg(long)]
        ephemeral: bool,
        /// Delete items older than this duration (e.g. 7d, 24h). Requires --ephemeral
        #[arg(long)]
        older_than: Option<String>,
    },
    /// Search memories (hybrid vector + full-text when embeddings are configured)
    Search {
        /// Search query
        query: String,
        /// Filter by tier
        #[arg(long)]
        tier: Option<String>,
        /// Max results
        #[arg(long, default_value = "10")]
        limit: usize,
        /// Vector similarity weight (0.0–1.0)
        #[arg(long, default_value = "0.7")]
        vector_weight: f32,
        /// Full-text BM25 weight (0.0–1.0)
        #[arg(long, default_value = "0.3")]
        text_weight: f32,
        /// Aggregate similar results (>0.85 embedding similarity) into single entries
        #[arg(long)]
        aggregate: bool,
    },
    /// Generate embeddings for memories that lack them
    Embed {
        /// Max memories to embed in one run
        #[arg(long, default_value = "100")]
        limit: usize,
    },
    /// Manage groups (create, list, members, add/remove members)
    Group {
        #[command(subcommand)]
        action: GroupAction,
    },
    /// Ingest a raw message
    Ingest {
        /// Message content
        content: String,
        /// Source system (e.g. telegram, nostr, webhook)
        #[arg(long, default_value = "cli")]
        source: String,
        /// Sender identifier
        #[arg(long, default_value = "local")]
        sender: String,
        /// Channel/room name
        #[arg(long)]
        channel: Option<String>,
    },
    /// List raw messages
    Messages {
        /// Filter by source
        #[arg(long)]
        source: Option<String>,
        /// Filter by channel
        #[arg(long)]
        channel: Option<String>,
        /// Filter by sender
        #[arg(long)]
        sender: Option<String>,
        /// Show messages since (RFC3339 timestamp)
        #[arg(long)]
        since: Option<String>,
        /// Max results
        #[arg(long, default_value = "50")]
        limit: usize,
        /// Show N messages around a specific source_id
        #[arg(long)]
        around: Option<String>,
        /// Number of context messages before/after --around target
        #[arg(long, default_value = "5")]
        context: usize,
    },
    /// Consolidate raw messages into memories
    Consolidate {
        /// Min messages required to trigger consolidation
        #[arg(long, default_value = "3")]
        min_messages: usize,
        /// Max messages to process per run
        #[arg(long, default_value = "50")]
        batch_size: usize,
        /// Preview what would be consolidated without publishing
        #[arg(long)]
        dry_run: bool,
        /// Only consolidate messages older than this duration (e.g. 30m, 1h, 7d)
        #[arg(long)]
        older_than: Option<String>,
        /// Only consolidate messages matching this tier
        #[arg(long)]
        tier: Option<String>,
    },
    /// List extracted entities
    Entities {
        /// Filter by kind (person, project, concept, place, organization)
        #[arg(long)]
        kind: Option<String>,
    },
    /// Prune unused/low-confidence memories and old raw messages
    Prune {
        /// Delete items older than N days
        #[arg(long, default_value = "90")]
        days: u64,
        /// Preview what would be pruned without deleting
        #[arg(long)]
        dry_run: bool,
    },
    /// Send a message to a recipient (npub, group, or public)
    Send {
        /// Message content
        content: String,
        /// Recipient: npub1... for DM, group:<id> for group, "public" for broadcast
        #[arg(long)]
        to: String,
        /// Delivery channel (default: nostr)
        #[arg(long)]
        channel: Option<String>,
    },
    /// Interactive first-time setup wizard
    Init {
        /// Overwrite existing config without prompting
        #[arg(long)]
        force: bool,
        /// Use defaults without interactive prompts (requires NOMEN_NSEC env var)
        #[arg(long)]
        non_interactive: bool,
    },
    /// Validate config and check connectivity
    Doctor,
    /// Start MCP server (JSON-RPC over stdio) or HTTP server
    Serve {
        /// Use stdio transport (MCP mode)
        #[arg(long)]
        stdio: bool,
        /// Start HTTP server on address (e.g. ":3000" or "127.0.0.1:3000")
        #[arg(long)]
        http: Option<String>,
        /// Directory for static web UI files (default: web/dist relative to binary)
        #[arg(long)]
        static_dir: Option<PathBuf>,
        /// Directory for landing page files (default: web/dist-landing relative to binary)
        #[arg(long)]
        landing_dir: Option<PathBuf>,
        /// Also start Context-VM (Nostr-native request/response listener)
        #[arg(long)]
        context_vm: bool,
        /// Allowed npubs for Context-VM requests (comma-separated hex or bech32)
        #[arg(long, value_delimiter = ',')]
        allowed_npubs: Vec<String>,
    },
}

#[derive(Subcommand)]
enum GroupAction {
    /// Create a new group
    Create {
        /// Group id (dot-separated hierarchy, e.g. "atlantislabs.engineering")
        id: String,
        /// Human-readable name
        #[arg(long)]
        name: String,
        /// Initial members (comma-separated npubs)
        #[arg(long, value_delimiter = ',')]
        members: Vec<String>,
        /// NIP-29 group id mapping
        #[arg(long)]
        nostr_group: Option<String>,
        /// Relay URL for this group
        #[arg(long)]
        relay: Option<String>,
    },
    /// List all groups
    List,
    /// Show members of a group
    Members {
        /// Group id
        id: String,
    },
    /// Add a member to a group
    AddMember {
        /// Group id
        id: String,
        /// Member npub to add
        npub: String,
    },
    /// Remove a member from a group
    RemoveMember {
        /// Group id
        id: String,
        /// Member npub to remove
        npub: String,
    },
}

// ── Resolve keys + relay from CLI + config ──────────────────────────

struct ResolvedConfig {
    nsecs: Vec<String>,
    relay: String,
}

fn load_config(cli: &Cli) -> Result<Config> {
    if let Some(ref path) = cli.config {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config: {}", path.display()))?;
        Ok(toml::from_str(&text)?)
    } else {
        Config::load()
    }
}

fn resolve_config(cli: &Cli) -> Result<ResolvedConfig> {
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
        .unwrap_or_else(|| "wss://zooid.atlantislabs.space".to_string());

    Ok(ResolvedConfig { nsecs, relay })
}

fn parse_keys(nsecs: &[String]) -> Result<(Vec<Keys>, Vec<PublicKey>)> {
    let mut all_keys = Vec::new();
    let mut pubkeys = Vec::new();
    for nsec in nsecs {
        let keys = Keys::parse(nsec).context("Failed to parse nsec key")?;
        pubkeys.push(keys.public_key());
        all_keys.push(keys);
    }
    Ok((all_keys, pubkeys))
}

fn build_signer(keys: &Keys) -> Arc<dyn NomenSigner> {
    Arc::new(KeysSigner::new(keys.clone()))
}

fn build_relay_manager(relay_url: &str, keys: &Keys) -> RelayManager {
    RelayManager::new(
        build_signer(keys),
        RelayConfig {
            relay_url: relay_url.to_string(),
            ..Default::default()
        },
    )
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

    // Handle init and doctor before resolve_config (config may not exist yet)
    match &cli.command {
        Command::Init { force, non_interactive } => {
            return cmd_init(*force, *non_interactive).await;
        }
        Command::Doctor => {
            return cmd_doctor().await;
        }
        _ => {}
    }

    let resolved = resolve_config(&cli)?;

    match cli.command {
        Command::List { named, ephemeral, stats } => {
            if resolved.nsecs.is_empty() {
                bail!(
                    "No nsec provided. Set it in {} or pass --nsec",
                    Config::path().display()
                );
            }
            cmd_list(&resolved.relay, &resolved.nsecs, named, ephemeral, stats).await?;
        }
        Command::Config => {
            cmd_config(&resolved.relay, &resolved.nsecs);
        }
        Command::Sync => {
            if resolved.nsecs.is_empty() {
                bail!("No nsec provided. Set it in {} or pass --nsec", Config::path().display());
            }
            cmd_sync(&resolved.relay, &resolved.nsecs).await?;
        }
        Command::Store { topic, summary, detail, tier, confidence } => {
            if resolved.nsecs.is_empty() {
                bail!("No nsec provided. Set it in {} or pass --nsec", Config::path().display());
            }
            cmd_store(&resolved.relay, &resolved.nsecs, &topic, &summary, &detail, &tier, confidence).await?;
        }
        Command::Delete { topic, id, ephemeral, older_than } => {
            if ephemeral {
                cmd_delete_ephemeral(older_than.as_deref()).await?;
            } else {
                if resolved.nsecs.is_empty() {
                    bail!("No nsec provided. Set it in {} or pass --nsec", Config::path().display());
                }
                cmd_delete(&resolved.relay, &resolved.nsecs, topic.as_deref(), id.as_deref()).await?;
            }
        }
        Command::Search { ref query, ref tier, limit, vector_weight, text_weight, aggregate } => {
            cmd_search(&cli, query, tier.as_deref(), limit, vector_weight, text_weight, aggregate).await?;
        }
        Command::Embed { limit } => {
            cmd_embed(&cli, limit).await?;
        }
        Command::Group { action } => {
            cmd_group(action).await?;
        }
        Command::Ingest { content, source, sender, channel } => {
            cmd_ingest(&content, &source, &sender, channel.as_deref()).await?;
        }
        Command::Messages { source, channel, sender, since, limit, around, context } => {
            cmd_messages(source.as_deref(), channel.as_deref(), sender.as_deref(), since.as_deref(), limit, around.as_deref(), context).await?;
        }
        Command::Consolidate { min_messages, batch_size, dry_run, ref older_than, ref tier } => {
            cmd_consolidate(&cli, min_messages, batch_size, dry_run, older_than.clone(), tier.clone()).await?;
        }
        Command::Entities { kind } => {
            cmd_entities(kind.as_deref()).await?;
        }
        Command::Prune { days, dry_run } => {
            cmd_prune(days, dry_run).await?;
        }
        Command::Send { ref content, ref to, ref channel } => {
            if resolved.nsecs.is_empty() {
                bail!("No nsec provided. Set it in {} or pass --nsec", Config::path().display());
            }
            cmd_send(&resolved.relay, &resolved.nsecs, &to, &content, channel.as_deref(), &cli).await?;
        }
        Command::Serve { stdio, http: ref http_addr, ref static_dir, ref landing_dir, context_vm, ref allowed_npubs } => {
            cmd_serve(&cli, stdio, http_addr.clone(), static_dir.clone(), landing_dir.clone(), context_vm, allowed_npubs.clone()).await?;
        }
        Command::Init { .. } | Command::Doctor => unreachable!("handled above"),
    }

    Ok(())
}

// ── Command: list ───────────────────────────────────────────────────

async fn cmd_list(relay_url: &str, nsecs: &[String], named: bool, ephemeral: bool, stats: bool) -> Result<()> {
    // If --stats or --ephemeral, use local DB
    if stats || ephemeral {
        let db_handle = db::init_db().await?;

        if stats {
            let (total, _named_count, pending) = db::count_memories_by_type(&db_handle).await?;
            println!(
                "\n{}\n{}",
                "Memory Statistics".bold(),
                "═".repeat(40)
            );
            println!("  Named memories: {}", total);
            println!("  Ephemeral (pending): {}", pending.to_string().yellow());
            println!();
            return Ok(());
        }

        if ephemeral {
            let messages = db::get_unconsolidated_messages(&db_handle, 200).await?;
            if messages.is_empty() {
                println!("No ephemeral messages pending consolidation.");
                return Ok(());
            }
            println!(
                "\n{} ({} pending)\n{}",
                "Ephemeral Messages".bold(),
                messages.len(),
                "═".repeat(60)
            );
            for msg in &messages {
                let channel_display = if msg.channel.is_empty() {
                    String::new()
                } else {
                    format!(" #{}", msg.channel)
                };
                println!(
                    "  [{}] {}{}: {}",
                    msg.source,
                    msg.sender.bold(),
                    channel_display,
                    if msg.content.len() > 80 { format!("{}...", &msg.content[..80]) } else { msg.content.clone() }
                );
                println!("    {}", msg.created_at.dimmed());
            }
            println!("\n{}: {} messages\n", "Total".bold(), messages.len());
            return Ok(());
        }
    }

    let (all_keys, pubkeys) = parse_keys(nsecs)?;
    debug!("Parsed {} keys", all_keys.len());

    let signer = build_signer(&all_keys[0]);
    let mgr = build_relay_manager(relay_url, &all_keys[0]);
    mgr.connect().await?;
    let events = mgr.fetch_memories(&pubkeys).await?;

    let mut memories = Vec::new();
    let mut lesson_count = 0usize;

    for event in events.into_iter() {
        if event.kind == Kind::Custom(LESSON_KIND) || event.kind == Kind::Custom(LEGACY_LESSON_KIND) {
            lesson_count += 1;
            continue;
        }
        let d_tag = get_tag_value(&event.tags, "d").unwrap_or_default();
        if d_tag.starts_with("snowclaw:config:") {
            continue;
        }

        // If --named, skip ephemeral-looking entries
        if named {
            let topic = nomen::memory::parse_d_tag(&d_tag);
            if topic.starts_with("conv:") || topic.starts_with("consolidated/") {
                continue;
            }
        }

        memories.push(parse_event(&event, signer.as_ref()));
    }

    let npubs: Vec<String> = all_keys
        .iter()
        .filter_map(|k| k.public_key().to_bech32().ok())
        .collect();

    display_memories(&npubs, &memories, lesson_count);
    mgr.disconnect().await;
    Ok(())
}

// ── Command: config ─────────────────────────────────────────────────

fn cmd_config(relay: &str, nsecs: &[String]) {
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

// ── Command: sync ───────────────────────────────────────────────────

async fn cmd_sync(relay_url: &str, nsecs: &[String]) -> Result<()> {
    let (all_keys, pubkeys) = parse_keys(nsecs)?;

    println!("Connecting to relay...");
    let signer = build_signer(&all_keys[0]);
    let mgr = build_relay_manager(relay_url, &all_keys[0]);
    mgr.connect().await?;
    let events = mgr.fetch_memories(&pubkeys).await?;

    let db = db::init_db().await?;

    let mut stored = 0usize;
    let mut skipped = 0usize;

    for event in events.into_iter() {
        if event.kind == Kind::Custom(LESSON_KIND) || event.kind == Kind::Custom(LEGACY_LESSON_KIND) {
            continue;
        }
        let d_tag = get_tag_value(&event.tags, "d").unwrap_or_default();
        if d_tag.starts_with("snowclaw:config:") {
            continue;
        }

        let parsed = parse_event(&event, signer.as_ref());
        match db::store_memory(&db, &parsed, &event).await {
            Ok(true) => stored += 1,
            Ok(false) => skipped += 1,
            Err(e) => {
                tracing::warn!("Failed to store memory {}: {e}", parsed.topic);
                skipped += 1;
            }
        }
    }

    println!(
        "Sync complete: {} stored, {} skipped (already up to date)",
        stored.to_string().green(),
        skipped
    );

    mgr.disconnect().await;
    Ok(())
}

// ── Command: store ──────────────────────────────────────────────────

async fn cmd_store(
    relay_url: &str,
    nsecs: &[String],
    topic: &str,
    summary: &str,
    detail: &str,
    tier: &str,
    confidence: f64,
) -> Result<()> {
    let (all_keys, _pubkeys) = parse_keys(nsecs)?;
    let keys = &all_keys[0];

    let mgr = build_relay_manager(relay_url, keys);
    mgr.connect().await?;

    // Build content JSON
    let content_str = serde_json::json!({
        "summary": summary,
        "detail": if detail.is_empty() { summary } else { detail },
        "context": null
    })
    .to_string();

    // Encrypt if personal/internal tier (or legacy "private")
    let final_content = if tier == "personal" || tier == "internal" || tier == "private" {
        mgr.signer().encrypt(&content_str)?
    } else {
        content_str.clone()
    };

    // Build v0.2 d-tag: {visibility}:{context}:{topic}
    let context = if tier == "personal" || tier == "internal" || tier == "private" {
        mgr.public_key().to_hex()
    } else if let Some(group_id) = tier.strip_prefix("group:") {
        group_id.to_string()
    } else if tier == "group" {
        String::new()
    } else {
        String::new() // public
    };
    let visibility = nomen::memory::base_tier(tier);
    let d_tag = nomen::memory::build_v2_dtag(visibility, &context, topic);

    // Check for existing event with this d-tag (for supersedes)
    // Also match old v0.1 d-tags that resolve to the same topic
    let previous_event_id = {
        let events = mgr.fetch_memories(&_pubkeys).await?;
        events.into_iter().find(|e| {
            let raw_dtag = get_tag_value(&e.tags, "d").unwrap_or_default();
            // Match against new v0.2 d-tag or parsed v0.1 topic
            raw_dtag == d_tag || nomen::memory::parse_d_tag(&raw_dtag) == topic
        }).map(|e| e.id)
    };

    // Build version based on previous event
    let version = if previous_event_id.is_some() { "2" } else { "1" };

    // Build tags (v0.2: no tier or source tags — visibility is in d-tag, source is event.pubkey)
    let mut tags = vec![
        Tag::custom(TagKind::Custom("d".into()), vec![d_tag.clone()]),
        Tag::custom(TagKind::Custom("model".into()), vec!["human/manual".to_string()]),
        Tag::custom(TagKind::Custom("confidence".into()), vec![format!("{confidence:.2}")]),
        Tag::custom(TagKind::Custom("version".into()), vec![version.to_string()]),
    ];

    // Add supersedes tag if updating an existing memory
    if let Some(prev_id) = previous_event_id {
        tags.push(Tag::custom(
            TagKind::Custom("supersedes".into()),
            vec![prev_id.to_hex()],
        ));
    }

    // Add topic tags for relay-side filtering (NIP-78 spec)
    for part in topic.split('/') {
        if !part.is_empty() {
            tags.push(Tag::custom(TagKind::Custom("t".into()), vec![part.to_string()]));
        }
    }

    // Add h tag for group-scoped memories (NIP-29)
    if tier.starts_with("group") {
        if let Some(group_id) = tier.strip_prefix("group:") {
            tags.push(Tag::custom(TagKind::Custom("h".into()), vec![group_id.to_string()]));
        }
    }

    let builder = EventBuilder::new(Kind::Custom(MEMORY_KIND), final_content).tags(tags);

    // Publish to relay
    println!("Publishing to relay...");
    let result = mgr.publish(builder).await?;

    // Store locally in SurrealDB
    let db = db::init_db().await?;
    let parsed = nomen::memory::ParsedMemory {
        tier: tier.to_string(),
        topic: topic.to_string(),
        version: version.to_string(),
        confidence: format!("{confidence:.2}"),
        model: "human/manual".to_string(),
        summary: summary.to_string(),
        created_at: Timestamp::now(),
        d_tag: d_tag.clone(),
        source: mgr.public_key().to_hex(),
        content_raw: content_str,
        detail: if detail.is_empty() { summary.to_string() } else { detail.to_string() },
    };
    let _ = db::store_memory_direct(&db, &parsed, &result.event_id.to_hex()).await;

    println!(
        "{} stored: {} [{}]",
        "Memory".green().bold(),
        topic.bold(),
        tier
    );
    println!("  Event ID: {}", result.event_id);
    println!("  Relay: {}", result.summary());

    mgr.disconnect().await;
    Ok(())
}

// ── Command: delete ─────────────────────────────────────────────────

async fn cmd_delete(
    relay_url: &str,
    nsecs: &[String],
    topic: Option<&str>,
    event_id: Option<&str>,
) -> Result<()> {
    if topic.is_none() && event_id.is_none() {
        bail!("Provide either a topic or --id <event-id>");
    }

    let (all_keys, pubkeys) = parse_keys(nsecs)?;
    let keys = &all_keys[0];

    let mgr = build_relay_manager(relay_url, keys);
    mgr.connect().await?;

    // If deleting by topic, we need to find the event first
    let target_event_id = if let Some(eid) = event_id {
        EventId::from_hex(eid).context("Invalid event ID")?
    } else {
        let topic = topic.unwrap();

        // Fetch to find the event with this topic (check both old and new d-tag formats)
        let events = mgr.fetch_memories(&pubkeys).await?;
        let found = events.into_iter().find(|e| {
            let raw_dtag = get_tag_value(&e.tags, "d").unwrap_or_default();
            let parsed_topic = nomen::memory::parse_d_tag(&raw_dtag);
            parsed_topic == topic
        });

        match found {
            Some(event) => event.id,
            None => bail!("No memory found with topic: {topic}"),
        }
    };

    // Publish NIP-09 deletion event (kind 5)
    let delete_builder = EventBuilder::new(Kind::Custom(5), "")
        .tags(vec![Tag::event(target_event_id)]);

    let result = mgr.publish(delete_builder).await?;

    // Remove from local SurrealDB (clean d_tag = topic)
    let db = db::init_db().await?;
    if let Some(topic) = topic {
        db::delete_memory_by_dtag(&db, topic).await?;
    } else if let Some(eid) = event_id {
        db::delete_memory_by_nostr_id(&db, eid).await?;
    }

    println!(
        "{} Event {} marked for deletion ({})",
        "Deleted.".red().bold(),
        target_event_id,
        result.summary()
    );

    mgr.disconnect().await;
    Ok(())
}

// ── Command: delete ephemeral ───────────────────────────────────────

async fn cmd_delete_ephemeral(older_than: Option<&str>) -> Result<()> {
    let older_than = older_than.ok_or_else(|| {
        anyhow::anyhow!("--older-than is required with --ephemeral (e.g. --older-than 7d)")
    })?;

    let secs = consolidate::parse_duration_str(older_than)?;
    let cutoff = chrono::Utc::now() - chrono::Duration::seconds(secs);
    let cutoff_str = cutoff.to_rfc3339();

    let db_handle = db::init_db().await?;
    let count = db::delete_ephemeral_before(&db_handle, &cutoff_str).await?;

    if count == 0 {
        println!("No ephemeral messages older than {older_than} to delete.");
    } else {
        println!(
            "{}: {} ephemeral messages deleted (older than {older_than})",
            "Deleted".red().bold(),
            count
        );
    }

    Ok(())
}

// ── Command: search ─────────────────────────────────────────────────

async fn cmd_search(
    cli: &Cli,
    query: &str,
    tier: Option<&str>,
    limit: usize,
    vector_weight: f32,
    text_weight: f32,
    aggregate: bool,
) -> Result<()> {
    let config = load_config(cli)?;
    let embedder = config.build_embedder();
    let db = db::init_db_with_dimensions(config.embedding_dimensions()).await?;

    let opts = search::SearchOptions {
        query: query.to_string(),
        tier: tier.map(|t| t.to_string()),
        allowed_scopes: None,
        limit,
        vector_weight,
        text_weight,
        min_confidence: None,
        aggregate,
    };

    let results = search::search(&db, embedder.as_ref(), &opts).await?;

    if results.is_empty() {
        println!("No results found for: {query}");
        return Ok(());
    }

    println!(
        "\n{} for \"{}\"\n{}",
        "Search Results".bold(),
        query,
        "═".repeat(60)
    );

    for (i, result) in results.iter().enumerate() {
        let tier_display = format!("[{}]", result.tier);
        let tier_colored = match result.tier.as_str() {
            "public" => tier_display.green(),
            "personal" | "internal" | "private" => tier_display.red(),
            _ => tier_display.yellow(),
        };

        let match_indicator = match result.match_type {
            search::MatchType::Hybrid => " [hybrid]",
            search::MatchType::Vector => " [vector]",
            search::MatchType::Text => " [text]",
        };

        println!(
            "\n{}. {} {} (confidence: {}){}",
            i + 1,
            tier_colored,
            result.topic.bold(),
            result.confidence,
            match_indicator.dimmed()
        );
        println!("   {}", result.summary);
        println!("   Created: {}", format_timestamp(result.created_at));
    }

    println!("\n{}: {} results\n", "Found".bold(), results.len());
    Ok(())
}

// ── Command: embed ─────────────────────────────────────────────────

async fn cmd_embed(cli: &Cli, limit: usize) -> Result<()> {
    let config = load_config(cli)?;
    let embedder = config.build_embedder();

    if embedder.dimensions() == 0 {
        bail!(
            "No embedding provider configured. Add [embedding] section to {}",
            Config::path().display()
        );
    }

    let db = db::init_db_with_dimensions(config.embedding_dimensions()).await?;
    let missing = db::get_memories_without_embeddings(&db, limit).await?;

    if missing.is_empty() {
        println!("All memories already have embeddings.");
        return Ok(());
    }

    println!("Generating embeddings for {} memories...", missing.len());

    let texts: Vec<String> = missing
        .iter()
        .map(|m| m.summary.clone().unwrap_or_else(|| m.content.clone()))
        .collect();

    let embeddings = embedder.embed(&texts).await?;

    let mut embedded = 0usize;
    for (row, embedding) in missing.iter().zip(embeddings.into_iter()) {
        if let Some(ref d_tag) = row.d_tag {
            db::store_embedding(&db, d_tag, embedding).await?;
            embedded += 1;
        }
    }

    println!(
        "{}: {} memories embedded",
        "Done".green().bold(),
        embedded
    );
    Ok(())
}

// ── Command: group ─────────────────────────────────────────────────

async fn cmd_group(action: GroupAction) -> Result<()> {
    let db = db::init_db().await?;

    match action {
        GroupAction::Create { id, name, members, nostr_group, relay } => {
            groups::create_group(
                &db,
                &id,
                &name,
                &members,
                nostr_group.as_deref(),
                relay.as_deref(),
            )
            .await?;
            println!(
                "{} group: {} ({})",
                "Created".green().bold(),
                id.bold(),
                name
            );
            if !members.is_empty() {
                println!("  Members: {}", members.join(", "));
            }
        }
        GroupAction::List => {
            let db_groups = groups::list_groups(&db).await?;

            let config = Config::load()?;
            let store = groups::GroupStore::load(&config.groups, &db).await?;
            let all = store.list();

            if all.is_empty() {
                println!("No groups configured.");
                return Ok(());
            }

            println!(
                "\n{}\n{}",
                "Groups".bold(),
                "═".repeat(60)
            );

            for group in all {
                let parent_display = if group.parent.is_empty() {
                    String::new()
                } else {
                    format!(" (parent: {})", group.parent)
                };
                let nostr_display = if group.nostr_group.is_empty() {
                    String::new()
                } else {
                    format!(" [NIP-29: {}]", group.nostr_group)
                };

                println!(
                    "\n  {} — {}{}{}",
                    group.id.bold(),
                    group.name,
                    parent_display,
                    nostr_display.dimmed()
                );
                println!(
                    "    Members: {}",
                    if group.members.is_empty() {
                        "(none)".to_string()
                    } else {
                        format!("{} member(s)", group.members.len())
                    }
                );
            }
            let _ = db_groups;
            println!();
        }
        GroupAction::Members { id } => {
            let members = groups::get_members(&db, &id).await?;
            println!("\n{} members of {}:\n", "Showing".bold(), id.bold());
            if members.is_empty() {
                println!("  (no members)");
            } else {
                for m in &members {
                    println!("  {m}");
                }
            }
            println!();
        }
        GroupAction::AddMember { id, npub } => {
            groups::add_member(&db, &id, &npub).await?;
            println!(
                "{} {} to group {}",
                "Added".green().bold(),
                npub,
                id.bold()
            );
        }
        GroupAction::RemoveMember { id, npub } => {
            groups::remove_member(&db, &id, &npub).await?;
            println!(
                "{} {} from group {}",
                "Removed".red().bold(),
                npub,
                id.bold()
            );
        }
    }

    Ok(())
}

// ── Command: ingest ─────────────────────────────────────────────────

async fn cmd_ingest(
    content: &str,
    source: &str,
    sender: &str,
    channel: Option<&str>,
) -> Result<()> {
    let db = db::init_db().await?;

    let msg = ingest::RawMessage {
        source: source.to_string(),
        source_id: None,
        sender: sender.to_string(),
        channel: channel.map(|c| c.to_string()),
        content: content.to_string(),
        metadata: None,
        created_at: None,
    };

    let id = ingest::ingest_message(&db, &msg).await?;
    println!(
        "{} message from {} [{}]{}",
        "Ingested".green().bold(),
        sender.bold(),
        source,
        channel
            .map(|c| format!(" #{c}"))
            .unwrap_or_default()
    );
    debug!("Record ID: {id}");
    Ok(())
}

// ── Command: messages ───────────────────────────────────────────────

async fn cmd_messages(
    source: Option<&str>,
    channel: Option<&str>,
    sender: Option<&str>,
    since: Option<&str>,
    limit: usize,
    around: Option<&str>,
    context_count: usize,
) -> Result<()> {
    let db = db::init_db().await?;

    let messages = if let Some(source_id) = around {
        db::query_messages_around(&db, source_id, context_count).await?
    } else {
        let opts = ingest::MessageQuery {
            source: source.map(|s| s.to_string()),
            channel: channel.map(|c| c.to_string()),
            sender: sender.map(|s| s.to_string()),
            since: since.map(|s| s.to_string()),
            limit: Some(limit),
            consolidated_only: false,
        };
        ingest::get_messages(&db, &opts).await?
    };

    if messages.is_empty() {
        println!("No messages found.");
        return Ok(());
    }

    println!(
        "\n{}\n{}",
        "Raw Messages".bold(),
        "═".repeat(60)
    );

    for msg in &messages {
        let channel_display = if msg.channel.is_empty() {
            String::new()
        } else {
            format!(" #{}", msg.channel)
        };
        let consolidated_marker = if msg.consolidated {
            " [consolidated]".dimmed().to_string()
        } else {
            String::new()
        };

        println!(
            "\n  [{}] {}{}{}\n    {}",
            msg.source,
            msg.sender.bold(),
            channel_display,
            consolidated_marker,
            msg.content
        );
        println!("    {}", msg.created_at.dimmed());
    }

    println!("\n{}: {} messages\n", "Total".bold(), messages.len());
    Ok(())
}

// ── Command: consolidate ────────────────────────────────────────────

async fn cmd_consolidate(
    cli: &Cli,
    min_messages: usize,
    batch_size: usize,
    dry_run: bool,
    older_than: Option<String>,
    tier: Option<String>,
) -> Result<()> {
    let config = load_config(cli)?;
    let embedder = config.build_embedder();
    let db_handle = db::init_db_with_dimensions(config.embedding_dimensions()).await?;
    let resolved = resolve_config(cli)?;

    // Build relay manager for NIP-09 deletion events
    let relay_manager = if !resolved.nsecs.is_empty() && !dry_run {
        let (all_keys, _) = parse_keys(&resolved.nsecs)?;
        let mgr = build_relay_manager(&resolved.relay, &all_keys[0]);
        mgr.connect().await.ok();
        Some(mgr)
    } else {
        None
    };

    // Build LLM provider from config (checks [memory.consolidation] then [consolidation])
    let llm_provider: Box<dyn consolidate::LlmProvider> = config
        .consolidation_llm_config()
        .and_then(|c| consolidate::OpenAiLlmProvider::from_config(&c))
        .map(|p| Box::new(p) as Box<dyn consolidate::LlmProvider>)
        .unwrap_or_else(|| Box::new(consolidate::NoopLlmProvider));

    let author_pubkey = relay_manager.as_ref()
        .map(|mgr| mgr.public_key().to_hex())
        .or_else(|| {
            config.all_nsecs().first()
                .and_then(|nsec| nostr_sdk::SecretKey::from_bech32(nsec).ok())
                .map(|sk| nostr_sdk::Keys::new(sk).public_key().to_hex())
        });
    let consolidation_config = consolidate::ConsolidationConfig {
        batch_size,
        min_messages,
        llm_provider,
        dry_run,
        older_than,
        tier,
        author_pubkey,
    };

    if dry_run {
        println!("{} Running consolidation pipeline...", "[DRY RUN]".yellow().bold());
    } else {
        println!("Running consolidation pipeline...");
    }

    let report = consolidate::consolidate(
        &db_handle,
        embedder.as_ref(),
        &consolidation_config,
        relay_manager.as_ref(),
    )
    .await?;

    if report.memories_created == 0 {
        println!("Nothing to consolidate (need at least {min_messages} unconsolidated messages).");
    } else {
        let prefix = if dry_run {
            format!("{}", "[DRY RUN] Would consolidate".yellow())
        } else {
            format!("{}", "Consolidated".green().bold())
        };
        println!(
            "{}: {} messages → {} memories",
            prefix,
            report.messages_processed,
            report.memories_created
        );
        if report.events_published > 0 {
            println!("  Published {} memories to relay (kind 31234)", report.events_published);
        }
        if report.events_deleted > 0 {
            println!("  Deleted {} ephemeral events from relay (NIP-09)", report.events_deleted);
        }
        if !report.channels.is_empty() {
            println!("  Channels: {}", report.channels.join(", "));
        }
        for group in &report.groups {
            println!("  {} → {} ({} messages)", group.key.dimmed(), group.topic.bold(), group.message_count);
        }
    }

    if let Some(ref mgr) = relay_manager {
        mgr.disconnect().await;
    }

    Ok(())
}

// ── Command: entities ───────────────────────────────────────────────

async fn cmd_entities(kind_filter: Option<&str>) -> Result<()> {
    let db = db::init_db().await?;

    let kind = kind_filter.and_then(entities::EntityKind::from_str);

    if kind_filter.is_some() && kind.is_none() {
        bail!(
            "Unknown entity kind: {}. Valid kinds: person, project, concept, place, organization",
            kind_filter.unwrap()
        );
    }

    let entity_list = db::list_entities(&db, kind.as_ref()).await?;

    if entity_list.is_empty() {
        println!("No entities found.");
        return Ok(());
    }

    println!(
        "\n{}\n{}",
        "Entities".bold(),
        "═".repeat(60)
    );

    for entity in &entity_list {
        println!(
            "\n  {} [{}]",
            entity.name.bold(),
            entity.kind.yellow()
        );
        println!("    Created: {}", entity.created_at.dimmed());
    }

    println!("\n{}: {} entities\n", "Total".bold(), entity_list.len());
    Ok(())
}

// ── Command: prune ──────────────────────────────────────────────────

async fn cmd_prune(days: u64, dry_run: bool) -> Result<()> {
    let db_handle = db::init_db().await?;

    if dry_run {
        println!("{} Pruning memories (older than {} days)...", "[DRY RUN]".yellow().bold(), days);
    } else {
        println!("Pruning memories (older than {} days)...", days);
    }

    let report = db::prune_memories(&db_handle, days, dry_run).await?;

    if report.pruned.is_empty() {
        println!("No memories eligible for pruning.");
    } else {
        println!("\n{} memories eligible for pruning:", report.pruned.len());
        for mem in &report.pruned {
            let confidence_str = mem.confidence
                .map(|c| format!("{c:.2}"))
                .unwrap_or("?".to_string());
            let access_str = mem.access_count
                .map(|c| c.to_string())
                .unwrap_or("0".to_string());
            println!(
                "  {} (confidence: {}, accesses: {}, created: {})",
                mem.topic.bold(),
                confidence_str,
                access_str,
                &mem.created_at[..10]
            );
        }

        if dry_run {
            println!("\n{}: Would prune {} memories", "[DRY RUN]".yellow().bold(), report.memories_pruned);
        } else {
            println!("\n{}: {} memories pruned", "Pruned".green().bold(), report.memories_pruned);
        }
    }

    if report.raw_messages_pruned > 0 {
        if dry_run {
            println!("{}: Would also prune {} consolidated raw messages", "[DRY RUN]".yellow().bold(), report.raw_messages_pruned);
        } else {
            println!("{}: {} consolidated raw messages pruned", "Pruned".green().bold(), report.raw_messages_pruned);
        }
    }

    Ok(())
}

// ── Command: send ───────────────────────────────────────────────────

async fn cmd_send(
    relay_url: &str,
    nsecs: &[String],
    recipient: &str,
    content: &str,
    channel: Option<&str>,
    cli: &Cli,
) -> Result<()> {
    let (all_keys, _) = parse_keys(nsecs)?;
    let config = load_config(cli)?;

    let mgr = build_relay_manager(relay_url, &all_keys[0]);
    mgr.connect().await?;

    let db = db::init_db().await?;
    let group_store = groups::GroupStore::load(&config.groups, &db).await?;

    let target = send::parse_recipient(recipient)?;
    let opts = send::SendOptions {
        target,
        content: content.to_string(),
        channel: channel.map(String::from),
        metadata: None,
    };

    let result = send::send_message(&mgr, &db, &group_store, opts).await?;

    println!(
        "{} to {}: event_id={}",
        "Sent".green().bold(),
        recipient.bold(),
        result.event_id
    );
    println!("  {}", result.summary());

    mgr.disconnect().await;
    Ok(())
}

// ── Command: serve (MCP server) ─────────────────────────────────────

async fn cmd_serve(
    cli: &Cli,
    stdio: bool,
    http_addr: Option<String>,
    static_dir: Option<PathBuf>,
    landing_dir: Option<PathBuf>,
    context_vm: bool,
    allowed_npubs: Vec<String>,
) -> Result<()> {
    let config = load_config(cli)?;
    let db = db::init_db_with_dimensions(config.embedding_dimensions()).await?;

    let default_channel = config
        .messaging
        .as_ref()
        .map(|m| m.default_channel.clone())
        .unwrap_or_else(|| "nostr".to_string());

    let group_store = groups::GroupStore::load(&config.groups, &db).await?;

    // Optionally build relay manager if nsecs are available
    let resolved = resolve_config(cli)?;
    let relay_manager = if !resolved.nsecs.is_empty() {
        let (all_keys, _) = parse_keys(&resolved.nsecs)?;
        let mgr = build_relay_manager(&resolved.relay, &all_keys[0]);
        mgr.connect().await.ok();
        Some(mgr)
    } else {
        None
    };

    // HTTP server mode
    if let Some(ref addr) = http_addr {
        // Normalize address: ":3000" → "0.0.0.0:3000"
        let bind_addr = if addr.starts_with(':') {
            format!("0.0.0.0{addr}")
        } else {
            addr.clone()
        };

        // Resolve static dir: explicit flag > web/dist relative to binary > web/dist relative to cwd
        let resolved_static = static_dir.or_else(|| {
            // Try relative to binary
            if let Ok(exe) = std::env::current_exe() {
                let dir = exe.parent()?.join("web/dist");
                if dir.is_dir() {
                    return Some(dir);
                }
            }
            // Try relative to cwd
            let cwd = PathBuf::from("web/dist");
            if cwd.is_dir() { Some(cwd) } else { None }
        });

        let http_state = nomen::http::AppState {
            db,
            embedder: config.build_embedder(),
            relay: relay_manager,
            groups: group_store,
            default_channel,
            config: std::sync::Arc::new(tokio::sync::RwLock::new(config)),
        };

        // Resolve landing dir: explicit flag > web/dist-landing relative to binary > cwd
        let resolved_landing = landing_dir.or_else(|| {
            if let Ok(exe) = std::env::current_exe() {
                let dir = exe.parent()?.join("web/dist-landing");
                if dir.is_dir() {
                    return Some(dir);
                }
            }
            let cwd = PathBuf::from("web/dist-landing");
            if cwd.is_dir() { Some(cwd) } else { None }
        });

        return nomen::http::serve(&bind_addr, http_state, resolved_static, resolved_landing).await;
    }

    // Default: stdio MCP mode (for backwards compat when neither --stdio nor --http given)
    let _ = stdio; // accept --stdio flag but it's the default

    if context_vm {
        // Need relay + keys for Context-VM
        if relay_manager.is_none() {
            bail!(
                "Context-VM requires nsec keys. Set in {} or pass --nsec",
                Config::path().display()
            );
        }

        // Build a second relay manager for Context-VM (it needs its own)
        let (all_keys, _) = parse_keys(&resolved.nsecs)?;
        let vm_relay = build_relay_manager(&resolved.relay, &all_keys[0]);
        vm_relay.connect().await?;

        let vm_embedder = config.build_embedder();
        let vm_db = db::init_db_with_dimensions(config.embedding_dimensions()).await?;
        let vm_groups = groups::GroupStore::load(&config.groups, &vm_db).await?;

        let vm_server = contextvm::ContextVmServer::new(
            vm_db,
            vm_embedder,
            vm_relay,
            allowed_npubs,
            vm_groups,
            default_channel.clone(),
        );

        // Run MCP on stdio and Context-VM on Nostr concurrently
        let mcp_embedder = config.build_embedder();
        let mcp_future = mcp::serve_stdio(db, mcp_embedder, relay_manager, group_store, default_channel);
        let vm_future = vm_server.run();

        tokio::select! {
            result = mcp_future => result,
            result = vm_future => result,
        }
    } else {
        let embedder = config.build_embedder();
        mcp::serve_stdio(db, embedder, relay_manager, group_store, default_channel).await
    }
}

// ── Command: init ───────────────────────────────────────────────────

async fn cmd_init(force: bool, non_interactive: bool) -> Result<()> {
    println!("\n  {} — Interactive Setup\n", "Nomen".bold());

    let config_path = Config::path();
    println!("  Config will be written to: {}\n", config_path.display());

    // Check existing config
    if config_path.exists() && !force {
        if non_interactive {
            bail!("Config already exists at {}. Use --force to overwrite.", config_path.display());
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
        .default("wss://relay.damus.io".to_string())
        .interact_text()?;

    // 2. Identities
    println!("\n  {}", "2. Identities".bold());
    let guardian_nsec: String = Password::new()
        .with_prompt("     Guardian nsec (your key)")
        .interact()?;
    let guardian_keys = Keys::parse(&guardian_nsec)
        .context("Invalid guardian nsec")?;
    let guardian_npub = guardian_keys.public_key().to_bech32()?;
    println!("     {} {}", "✓".green(), guardian_npub);

    let mut agent_nsecs: Vec<String> = Vec::new();
    let add_agents = Confirm::new()
        .with_prompt("     Add agent identities?")
        .default(false)
        .interact()?;

    if add_agents {
        loop {
            let agent_nsec: String = Password::new()
                .with_prompt("     Agent nsec")
                .interact()?;
            let agent_keys = Keys::parse(&agent_nsec)
                .context("Invalid agent nsec")?;
            let agent_npub = agent_keys.public_key().to_bech32()?;
            let idx = agent_nsecs.len() + 1;
            println!("     {} {} (agent #{})", "✓".green(), agent_npub, idx);
            agent_nsecs.push(agent_nsec);

            let add_another = Confirm::new()
                .with_prompt("     Add another?")
                .default(false)
                .interact()?;
            if !add_another {
                break;
            }
        }
    }

    // Default writer selection
    let mut writer_options = vec![format!("Guardian ({}...)", &guardian_npub[..16])];
    for (i, nsec) in agent_nsecs.iter().enumerate() {
        let k = Keys::parse(nsec)?;
        let npub = k.public_key().to_bech32()?;
        writer_options.push(format!("Agent #{} ({}...)", i + 1, &npub[..16]));
    }

    let default_writer = if writer_options.len() > 1 {
        println!("\n     {}", "Default writer identity:".bold());
        let selection = Select::new()
            .with_prompt("     Select")
            .items(&writer_options)
            .default(0)
            .interact()?;
        if selection == 0 {
            "guardian".to_string()
        } else {
            format!("agent:{}", selection - 1)
        }
    } else {
        "guardian".to_string()
    };

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
    let emb_api_key_env: String = Input::new()
        .with_prompt("     API key env var")
        .default("OPENAI_API_KEY".to_string())
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
        let cons_api_key_env: String = Input::new()
            .with_prompt("     API key env var")
            .default("OPENROUTER_API_KEY".to_string())
            .interact_text()?;
        let cons_interval: u32 = Input::new()
            .with_prompt("     Interval hours")
            .default(4)
            .interact_text()?;
        let cons_ttl: u32 = Input::new()
            .with_prompt("     Message age before consolidation (minutes)")
            .default(60)
            .interact_text()?;

        Some(MemorySection {
            consolidation: Some(MemoryConsolidationConfig {
                enabled: true,
                interval_hours: cons_interval,
                ephemeral_ttl_minutes: cons_ttl,
                max_ephemeral_count: 200,
                dry_run: false,
                provider: Some(cons_provider),
                model: Some(cons_model),
                api_key_env: Some(cons_api_key_env),
                base_url: None,
            }),
        })
    } else {
        None
    };

    // 5. Dashboard Server
    println!("\n  {}", "5. Dashboard Server".bold());
    let server_enabled = Confirm::new()
        .with_prompt("     Enable HTTP server?")
        .default(true)
        .interact()?;

    let server_config = if server_enabled {
        let listen: String = Input::new()
            .with_prompt("     Listen address")
            .default("127.0.0.1:3000".to_string())
            .interact_text()?;
        Some(ServerConfig { enabled: true, listen })
    } else {
        None
    };

    // Build config
    let config = Config {
        relay: Some(relay.clone()),
        nsec: Some(guardian_nsec.clone()),
        nsecs: agent_nsecs.clone(),
        default_writer: Some(default_writer),
        embedding: Some(EmbeddingConfig {
            provider: emb_provider,
            model: emb_model,
            api_key_env: emb_api_key_env,
            api_key: None,
            base_url: None,
            dimensions: emb_dimensions,
            batch_size: 100,
        }),
        groups: Vec::new(),
        consolidation: None,
        memory: memory_section,
        messaging: None,
        server: server_config,
    };

    // Write config
    let toml_str = toml::to_string_pretty(&config)
        .context("Failed to serialize config")?;

    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create config directory: {}", parent.display()))?;
    }

    std::fs::write(&config_path, &toml_str)
        .with_context(|| format!("Failed to write config: {}", config_path.display()))?;
    println!("\n  {} Config written to {}", "✓".green(), config_path.display());

    // Test relay connection
    print!("  {} Testing relay connection... ", "✓".green());
    let all_nsecs = config.all_nsecs();
    match test_relay_connection(&relay, &all_nsecs).await {
        Ok(count) => {
            println!("connected");
            println!("  {} Found {} existing memories for configured identities", "✓".green(), count);
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
    println!("  Guardian: {npub}");

    let relay = std::env::var("NOMEN_RELAY")
        .unwrap_or_else(|_| "wss://relay.damus.io".to_string());

    let config = Config {
        relay: Some(relay.clone()),
        nsec: Some(nsec),
        nsecs: Vec::new(),
        default_writer: Some("guardian".to_string()),
        embedding: Some(EmbeddingConfig::default()),
        groups: Vec::new(),
        consolidation: None,
        memory: Some(MemorySection {
            consolidation: Some(MemoryConsolidationConfig {
                enabled: true,
                interval_hours: 4,
                ephemeral_ttl_minutes: 60,
                max_ephemeral_count: 200,
                dry_run: false,
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
    };

    let config_path = Config::path();
    let toml_str = toml::to_string_pretty(&config)
        .context("Failed to serialize config")?;

    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    std::fs::write(&config_path, &toml_str)?;
    println!("  {} Config written to {}", "✓".green(), config_path.display());

    // Test relay
    let all_nsecs = config.all_nsecs();
    match test_relay_connection(&relay, &all_nsecs).await {
        Ok(count) => {
            println!("  {} Relay connected, {} existing memories", "✓".green(), count);
        }
        Err(e) => {
            println!("  {} Relay test failed: {e}", "✗".red());
        }
    }

    Ok(())
}

async fn test_relay_connection(relay_url: &str, nsecs: &[String]) -> Result<usize> {
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

// ── Command: doctor ─────────────────────────────────────────────────

async fn cmd_doctor() -> Result<()> {
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
            let label = if i == 0 { "Guardian".to_string() } else { format!("Agent #{i}") };
            match Keys::parse(nsec) {
                Ok(keys) => {
                    let npub = keys.public_key().to_bech32().unwrap_or_else(|_| "?".to_string());
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
    let relay_url = config.relay.as_deref().unwrap_or("wss://zooid.atlantislabs.space");
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
        let key_set = std::env::var(&emb.api_key_env).map(|v| !v.is_empty()).unwrap_or(false);
        if key_set {
            println!("  Embedding ({}): {} ${} is set", emb.provider, "✓".green(), emb.api_key_env);
        } else {
            println!("  Embedding ({}): {} ${} not set", emb.provider, "✗".red(), emb.api_key_env);
            all_ok = false;
        }
    } else {
        println!("  Embedding: {}", "⚠ not configured".yellow());
    }

    // 5. Local DB writable
    print!("  Local DB: ");
    match db::init_db().await {
        Ok(_) => println!("{}", "✓ writable".green()),
        Err(e) => {
            println!("{} {e}", "✗ failed".red());
            all_ok = false;
        }
    }

    // 6. Consolidation API key
    if let Some(llm_config) = config.consolidation_llm_config() {
        let key_set = std::env::var(&llm_config.api_key_env).map(|v| !v.is_empty()).unwrap_or(false);
        if key_set {
            println!("  Consolidation ({}): {} ${} is set", llm_config.provider, "✓".green(), llm_config.api_key_env);
        } else {
            println!("  Consolidation ({}): {} ${} not set", llm_config.provider, "✗".red(), llm_config.api_key_env);
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
