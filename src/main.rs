mod access;
mod config;
mod consolidate;
mod db;
mod display;
mod embed;
mod entities;
mod groups;
mod ingest;
mod memory;
mod relay;
mod search;

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use colored::Colorize;
use nostr_sdk::prelude::*;
use tracing::debug;

use crate::config::Config;
use crate::display::{display_memories, format_timestamp};
use crate::memory::{get_tag_value, parse_event};

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
    List,
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
    },
    /// Consolidate raw messages into memories
    Consolidate {
        /// Min messages required to trigger consolidation
        #[arg(long, default_value = "3")]
        min_messages: usize,
        /// Max messages to process per run
        #[arg(long, default_value = "50")]
        batch_size: usize,
    },
    /// List extracted entities
    Entities {
        /// Filter by kind (person, project, concept, place, organization)
        #[arg(long)]
        kind: Option<String>,
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
    let resolved = resolve_config(&cli)?;

    match cli.command {
        Command::List => {
            if resolved.nsecs.is_empty() {
                bail!(
                    "No nsec provided. Set it in {} or pass --nsec",
                    Config::path().display()
                );
            }
            cmd_list(&resolved.relay, &resolved.nsecs).await?;
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
        Command::Delete { topic, id } => {
            if resolved.nsecs.is_empty() {
                bail!("No nsec provided. Set it in {} or pass --nsec", Config::path().display());
            }
            cmd_delete(&resolved.relay, &resolved.nsecs, topic.as_deref(), id.as_deref()).await?;
        }
        Command::Search { ref query, ref tier, limit, vector_weight, text_weight } => {
            cmd_search(&cli, &query, tier.as_deref(), limit, vector_weight, text_weight).await?;
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
        Command::Messages { source, channel, sender, since, limit } => {
            cmd_messages(source.as_deref(), channel.as_deref(), sender.as_deref(), since.as_deref(), limit).await?;
        }
        Command::Consolidate { min_messages, batch_size } => {
            cmd_consolidate(&cli, min_messages, batch_size).await?;
        }
        Command::Entities { kind } => {
            cmd_entities(kind.as_deref()).await?;
        }
    }

    Ok(())
}

// ── Command: list ───────────────────────────────────────────────────

async fn cmd_list(relay_url: &str, nsecs: &[String]) -> Result<()> {
    let (all_keys, pubkeys) = parse_keys(nsecs)?;
    debug!("Parsed {} keys", all_keys.len());

    let client = relay::connect(relay_url, &all_keys[0]).await?;
    let events = relay::fetch_memory_events(&client, &pubkeys).await?;

    let mut memories = Vec::new();
    let mut lesson_count = 0usize;

    for event in events.into_iter() {
        if event.kind == Kind::Custom(4129) {
            lesson_count += 1;
            continue;
        }
        let d_tag = get_tag_value(&event.tags, "d").unwrap_or_default();
        if d_tag.starts_with("snowclaw:config:") {
            continue;
        }
        memories.push(parse_event(&event, &all_keys[0]));
    }

    let npubs: Vec<String> = all_keys
        .iter()
        .filter_map(|k| k.public_key().to_bech32().ok())
        .collect();

    display_memories(&npubs, &memories, lesson_count);
    client.disconnect().await;
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
    let client = relay::connect(relay_url, &all_keys[0]).await?;
    let events = relay::fetch_memory_events(&client, &pubkeys).await?;

    let db = db::init_db().await?;

    let mut stored = 0usize;
    let mut skipped = 0usize;

    for event in events.into_iter() {
        if event.kind == Kind::Custom(4129) {
            continue;
        }
        let d_tag = get_tag_value(&event.tags, "d").unwrap_or_default();
        if d_tag.starts_with("snowclaw:config:") {
            continue;
        }

        let parsed = parse_event(&event, &all_keys[0]);
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

    client.disconnect().await;
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

    // Build content JSON
    let content = serde_json::json!({
        "summary": summary,
        "detail": if detail.is_empty() { summary } else { detail },
        "context": null
    });

    // Build d-tag
    let d_tag = format!("snow:memory:{topic}");

    // Build tags
    let tags = vec![
        Tag::custom(TagKind::Custom("d".into()), vec![d_tag.clone()]),
        Tag::custom(TagKind::Custom("snow:tier".into()), vec![tier.to_string()]),
        Tag::custom(TagKind::Custom("snow:model".into()), vec!["human/manual".to_string()]),
        Tag::custom(TagKind::Custom("snow:confidence".into()), vec![format!("{confidence:.2}")]),
        Tag::custom(TagKind::Custom("snow:source".into()), vec![keys.public_key().to_hex()]),
        Tag::custom(TagKind::Custom("snow:version".into()), vec!["1".to_string()]),
    ];

    let builder = EventBuilder::new(Kind::Custom(30078), content.to_string()).tags(tags);

    // Publish to relay
    println!("Publishing to relay...");
    let client = relay::connect(relay_url, keys).await?;
    let event_id = relay::publish_event(&client, builder).await?;

    // Store locally in SurrealDB
    let db = db::init_db().await?;
    let parsed = crate::memory::ParsedMemory {
        tier: tier.to_string(),
        topic: topic.to_string(),
        version: "1".to_string(),
        confidence: format!("{confidence:.2}"),
        model: "human/manual".to_string(),
        summary: summary.to_string(),
        created_at: Timestamp::now(),
        d_tag,
        source: keys.public_key().to_hex(),
        content_raw: content.to_string(),
        detail: if detail.is_empty() { summary.to_string() } else { detail.to_string() },
    };
    let _ = db::store_memory_direct(&db, &parsed, &event_id.to_hex()).await;

    println!(
        "{} stored: {} [{}]",
        "Memory".green().bold(),
        topic.bold(),
        tier
    );
    println!("  Event ID: {event_id}");

    client.disconnect().await;
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

    let client = relay::connect(relay_url, keys).await?;

    // If deleting by topic, we need to find the event first
    let target_event_id = if let Some(eid) = event_id {
        EventId::from_hex(eid).context("Invalid event ID")?
    } else {
        let topic = topic.unwrap();
        let d_tag = format!("snow:memory:{topic}");

        // Fetch to find the event with this d-tag
        let events = relay::fetch_memory_events(&client, &pubkeys).await?;
        let found = events.into_iter().find(|e| {
            get_tag_value(&e.tags, "d").as_deref() == Some(&d_tag)
        });

        match found {
            Some(event) => event.id,
            None => bail!("No memory found with topic: {topic}"),
        }
    };

    // Publish NIP-09 deletion event (kind 5)
    let delete_builder = EventBuilder::new(Kind::Custom(5), "")
        .tags(vec![Tag::event(target_event_id)]);

    relay::publish_event(&client, delete_builder).await?;

    // Remove from local SurrealDB
    let db = db::init_db().await?;
    if let Some(topic) = topic {
        let d_tag = format!("snow:memory:{topic}");
        db::delete_memory_by_dtag(&db, &d_tag).await?;
    } else if let Some(eid) = event_id {
        db::delete_memory_by_nostr_id(&db, eid).await?;
    }

    println!("{} Event {} marked for deletion", "Deleted.".red().bold(), target_event_id);

    client.disconnect().await;
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
) -> Result<()> {
    let config = load_config(cli)?;
    let embedder = config.build_embedder();
    let db = db::init_db().await?;

    let opts = search::SearchOptions {
        query: query.to_string(),
        tier: tier.map(|t| t.to_string()),
        allowed_scopes: None,
        limit,
        vector_weight,
        text_weight,
        min_confidence: None,
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
            "private" => tier_display.red(),
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

    let db = db::init_db().await?;
    let missing = db::get_memories_without_embeddings(&db, limit).await?;

    if missing.is_empty() {
        println!("All memories already have embeddings.");
        return Ok(());
    }

    println!("Generating embeddings for {} memories...", missing.len());

    // Collect texts to embed (use summary if available, otherwise content)
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

            // Also load config groups for display
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
                let parent_display = group
                    .parent
                    .as_deref()
                    .map(|p| format!(" (parent: {p})"))
                    .unwrap_or_default();
                let nostr_display = group
                    .nostr_group
                    .as_deref()
                    .map(|n| format!(" [NIP-29: {n}]"))
                    .unwrap_or_default();

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
            // Suppress unused variable warning
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
) -> Result<()> {
    let db = db::init_db().await?;

    let opts = ingest::MessageQuery {
        source: source.map(|s| s.to_string()),
        channel: channel.map(|c| c.to_string()),
        sender: sender.map(|s| s.to_string()),
        since: since.map(|s| s.to_string()),
        limit: Some(limit),
        consolidated_only: false,
    };

    let messages = ingest::get_messages(&db, &opts).await?;

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
        let channel_display = msg
            .channel
            .as_deref()
            .map(|c| format!(" #{c}"))
            .unwrap_or_default();
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

async fn cmd_consolidate(cli: &Cli, min_messages: usize, batch_size: usize) -> Result<()> {
    let config = load_config(cli)?;
    let embedder = config.build_embedder();
    let db = db::init_db().await?;

    let consolidation_config = consolidate::ConsolidationConfig {
        batch_size,
        min_messages,
        llm_provider: Box::new(consolidate::NoopLlmProvider),
    };

    println!("Running consolidation pipeline...");
    let report = consolidate::consolidate(&db, embedder.as_ref(), &consolidation_config).await?;

    if report.memories_created == 0 {
        println!("Nothing to consolidate (need at least {min_messages} unconsolidated messages).");
    } else {
        println!(
            "{}: {} messages → {} memories",
            "Consolidated".green().bold(),
            report.messages_processed,
            report.memories_created
        );
        if !report.channels.is_empty() {
            println!("  Channels: {}", report.channels.join(", "));
        }
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
