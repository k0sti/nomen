use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use colored::Colorize;
use dialoguer::{Confirm, Input, Password, Select};
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
use nomen::kinds::{LEGACY_LESSON_KIND, LESSON_KIND};
use nomen::mcp;
use nomen::memory::{get_tag_value, parse_event};
use nomen::relay::{RelayConfig, RelayManager};
use nomen::search;
use nomen::send;
use nomen::signer::{KeysSigner, NomenSigner};
use nomen::{Nomen, NewMemory};

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
        /// Expand results by traversing graph edges (mentions, references, contradicts, consolidated_from)
        #[arg(long)]
        graph: bool,
        /// Max hops for graph traversal (default 1, requires --graph)
        #[arg(long, default_value = "1")]
        hops: usize,
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
        /// Filter by kind (person, project, concept, place, organization, technology)
        #[arg(long)]
        kind: Option<String>,
        /// Show relationships between entities
        #[arg(long)]
        relations: bool,
    },
    /// Run cluster fusion — synthesize related memories by namespace
    Cluster {
        /// Preview what clusters would be formed without storing
        #[arg(long)]
        dry_run: bool,
        /// Only fuse memories under this prefix (e.g. "user/")
        #[arg(long)]
        prefix: Option<String>,
        /// Minimum memories per cluster
        #[arg(long, default_value = "3")]
        min_members: usize,
        /// Namespace depth for grouping (e.g. 2 for "user/k0")
        #[arg(long, default_value = "2")]
        namespace_depth: usize,
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

/// Build a Nomen instance with relay connected.
async fn build_nomen_with_relay(config: &Config, resolved: &ResolvedConfig) -> Result<Nomen> {
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
async fn build_nomen(config: &Config) -> Result<Nomen> {
    Nomen::open(config).await
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
        Command::Init {
            force,
            non_interactive,
        } => {
            return cmd_init(*force, *non_interactive).await;
        }
        Command::Doctor => {
            return cmd_doctor().await;
        }
        _ => {}
    }

    // Load config and resolve once before match (avoids borrow-after-move issues)
    let config = load_config(&cli)?;
    let resolved = resolve_config(&cli)?;

    match cli.command {
        Command::List {
            named,
            ephemeral,
            stats,
        } => {
            if stats || ephemeral {
                let nomen = build_nomen(&config).await?;
                cmd_list_local(&nomen, ephemeral, stats).await?;
            } else {
                if resolved.nsecs.is_empty() {
                    bail!(
                        "No nsec provided. Set it in {} or pass --nsec",
                        Config::path().display()
                    );
                }
                cmd_list_relay(&resolved.relay, &resolved.nsecs, named).await?;
            }
        }
        Command::Config => {
            cmd_config(&resolved.relay, &resolved.nsecs);
        }
        Command::Sync => {
            let nomen = build_nomen_with_relay(&config, &resolved).await?;
            cmd_sync(&nomen).await?;
        }
        Command::Store {
            topic,
            summary,
            detail,
            tier,
            confidence,
        } => {
            let nomen = build_nomen_with_relay(&config, &resolved).await?;
            cmd_store(&nomen, &topic, &summary, &detail, &tier, confidence).await?;
        }
        Command::Delete {
            topic,
            id,
            ephemeral,
            older_than,
        } => {
            if ephemeral {
                let nomen = build_nomen(&config).await?;
                cmd_delete_ephemeral(&nomen, older_than.as_deref()).await?;
            } else {
                let nomen = build_nomen_with_relay(&config, &resolved).await?;
                cmd_delete(&nomen, topic.as_deref(), id.as_deref()).await?;
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
            let nomen = build_nomen(&config).await?;
            cmd_search(&nomen, &query, tier.as_deref(), limit, vector_weight, text_weight, aggregate, graph, hops).await?;
        }
        Command::Embed { limit } => {
            let nomen = build_nomen(&config).await?;
            cmd_embed(&nomen, limit).await?;
        }
        Command::Group { action } => {
            let nomen = build_nomen(&config).await?;
            cmd_group(&nomen, action).await?;
        }
        Command::Ingest {
            content,
            source,
            sender,
            channel,
        } => {
            let nomen = build_nomen(&config).await?;
            cmd_ingest(&nomen, &content, &source, &sender, channel.as_deref()).await?;
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
            cmd_messages(
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
            cmd_consolidate(
                &config,
                &resolved,
                min_messages,
                batch_size,
                dry_run,
                older_than,
                tier,
            )
            .await?;
        }
        Command::Entities { kind, relations } => {
            let nomen = build_nomen(&config).await?;
            cmd_entities(&nomen, kind.as_deref(), relations).await?;
        }
        Command::Cluster {
            dry_run,
            prefix,
            min_members,
            namespace_depth,
        } => {
            cmd_cluster(&config, &resolved, dry_run, prefix, min_members, namespace_depth).await?;
        }
        Command::Prune { days, dry_run } => {
            let nomen = build_nomen(&config).await?;
            cmd_prune(&nomen, days, dry_run).await?;
        }
        Command::Send {
            content,
            to,
            channel,
        } => {
            let nomen = build_nomen_with_relay(&config, &resolved).await?;
            cmd_send(&nomen, &to, &content, channel.as_deref()).await?;
        }
        Command::Serve {
            stdio,
            http: http_addr,
            static_dir,
            landing_dir,
            context_vm,
            allowed_npubs,
        } => {
            cmd_serve(
                &config,
                &resolved,
                stdio,
                http_addr,
                static_dir,
                landing_dir,
                context_vm,
                allowed_npubs,
            )
            .await?;
        }
        Command::Init { .. } | Command::Doctor => unreachable!("handled above"),
    }

    Ok(())
}

// ── Command: list (relay-based) ─────────────────────────────────────

async fn cmd_list_relay(
    relay_url: &str,
    nsecs: &[String],
    named: bool,
) -> Result<()> {
    let (all_keys, pubkeys) = parse_keys(nsecs)?;
    debug!("Parsed {} keys", all_keys.len());

    let signer = build_signer(&all_keys[0]);
    let mgr = build_relay_manager(relay_url, &all_keys[0]);
    mgr.connect().await?;
    let events = mgr.fetch_memories(&pubkeys).await?;

    let mut memories = Vec::new();
    let mut lesson_count = 0usize;

    for event in events.into_iter() {
        if event.kind == Kind::Custom(LESSON_KIND) || event.kind == Kind::Custom(LEGACY_LESSON_KIND)
        {
            lesson_count += 1;
            continue;
        }
        let d_tag = get_tag_value(&event.tags, "d").unwrap_or_default();
        if d_tag.starts_with("snowclaw:config:") {
            continue;
        }

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

// ── Command: list (local DB) ────────────────────────────────────────

async fn cmd_list_local(nomen: &Nomen, ephemeral: bool, stats: bool) -> Result<()> {
    if stats {
        let (total, _named_count, pending) = nomen.count_memories().await?;
        println!("\n{}\n{}", "Memory Statistics".bold(), "═".repeat(40));
        println!("  Named memories: {}", total);
        println!("  Ephemeral (pending): {}", pending.to_string().yellow());
        println!();
        return Ok(());
    }

    if ephemeral {
        // Ephemeral listing still uses raw DB query (no Nomen method for this specific view)
        let db_handle = db::init_db().await?;
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
                if msg.content.len() > 80 {
                    format!("{}...", &msg.content[..80])
                } else {
                    msg.content.clone()
                }
            );
            println!("    {}", msg.created_at.dimmed());
        }
        println!("\n{}: {} messages\n", "Total".bold(), messages.len());
    }

    Ok(())
}

// ── Command: config ─────────────────────────────────────────────────

fn cmd_config(relay: &str, nsecs: &[String]) {
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
}

// ── Command: sync ───────────────────────────────────────────────────

async fn cmd_sync(nomen: &Nomen) -> Result<()> {
    println!("Connecting to relay...");
    let report = nomen.sync().await?;
    println!(
        "Sync complete: {} stored, {} skipped (already up to date)",
        report.stored.to_string().green(),
        report.skipped
    );
    if report.errors > 0 {
        println!("  {} errors during sync", report.errors.to_string().red());
    }
    Ok(())
}

// ── Command: store ──────────────────────────────────────────────────

async fn cmd_store(
    nomen: &Nomen,
    topic: &str,
    summary: &str,
    detail: &str,
    tier: &str,
    confidence: f64,
) -> Result<()> {
    println!("Publishing to relay...");

    let mem = NewMemory {
        topic: topic.to_string(),
        summary: summary.to_string(),
        detail: detail.to_string(),
        tier: tier.to_string(),
        confidence,
        source: Some("cli".to_string()),
        model: Some("human/manual".to_string()),
    };

    let d_tag = nomen.store(mem).await?;

    println!(
        "{} stored: {} [{}]",
        "Memory".green().bold(),
        topic.bold(),
        tier
    );
    println!("  d_tag: {}", d_tag);

    Ok(())
}

// ── Command: delete ─────────────────────────────────────────────────

async fn cmd_delete(
    nomen: &Nomen,
    topic: Option<&str>,
    event_id: Option<&str>,
) -> Result<()> {
    if topic.is_none() && event_id.is_none() {
        bail!("Provide either a topic or --id <event-id>");
    }

    nomen.delete(topic, event_id).await?;

    if let Some(topic) = topic {
        println!(
            "{} Memory with topic: {}",
            "Deleted.".red().bold(),
            topic
        );
    } else if let Some(id) = event_id {
        println!(
            "{} Memory with event ID: {}",
            "Deleted.".red().bold(),
            id
        );
    }

    Ok(())
}

// ── Command: delete ephemeral ───────────────────────────────────────

async fn cmd_delete_ephemeral(nomen: &Nomen, older_than: Option<&str>) -> Result<()> {
    let older_than = older_than.ok_or_else(|| {
        anyhow::anyhow!("--older-than is required with --ephemeral (e.g. --older-than 7d)")
    })?;

    let count = nomen.delete_ephemeral(older_than).await?;

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
    nomen: &Nomen,
    query: &str,
    tier: Option<&str>,
    limit: usize,
    vector_weight: f32,
    text_weight: f32,
    aggregate: bool,
    graph_expand: bool,
    max_hops: usize,
) -> Result<()> {
    let opts = search::SearchOptions {
        query: query.to_string(),
        tier: tier.map(|t| t.to_string()),
        allowed_scopes: None,
        limit,
        vector_weight,
        text_weight,
        min_confidence: None,
        aggregate,
        graph_expand,
        max_hops,
    };

    let results = nomen.search(opts).await?;

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
            search::MatchType::Graph => {
                if let Some(ref edge) = result.graph_edge {
                    // We'll format this below
                    match edge.as_str() {
                        "contradicts" => " [graph:contradicts]",
                        "mentions" => " [graph:mentions]",
                        "references" => " [graph:references]",
                        "consolidated_from" => " [graph:consolidated]",
                        _ => " [graph]",
                    }
                } else {
                    " [graph]"
                }
            }
        };

        let contradicts_prefix = if result.contradicts {
            format!("{} ", "[CONTRADICTS]".red().bold())
        } else {
            String::new()
        };

        println!(
            "\n{}. {} {}{} (confidence: {}){}",
            i + 1,
            tier_colored,
            contradicts_prefix,
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

async fn cmd_embed(nomen: &Nomen, limit: usize) -> Result<()> {
    let report = nomen.embed(limit).await?;

    if report.total == 0 {
        println!("All memories already have embeddings.");
    } else {
        println!("{}: {} memories embedded", "Done".green().bold(), report.embedded);
    }

    Ok(())
}

// ── Command: group ─────────────────────────────────────────────────

async fn cmd_group(nomen: &Nomen, action: GroupAction) -> Result<()> {
    match action {
        GroupAction::Create {
            id,
            name,
            members,
            nostr_group,
            relay,
        } => {
            nomen.group_create(&id, &name, &members, nostr_group.as_deref(), relay.as_deref()).await?;
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
            let all = nomen.groups().list();

            if all.is_empty() {
                println!("No groups configured.");
                return Ok(());
            }

            println!("\n{}\n{}", "Groups".bold(), "═".repeat(60));

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
            println!();
        }
        GroupAction::Members { id } => {
            let members = nomen.group_members(&id).await?;
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
            nomen.group_add_member(&id, &npub).await?;
            println!("{} {} to group {}", "Added".green().bold(), npub, id.bold());
        }
        GroupAction::RemoveMember { id, npub } => {
            nomen.group_remove_member(&id, &npub).await?;
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
    nomen: &Nomen,
    content: &str,
    source: &str,
    sender: &str,
    channel: Option<&str>,
) -> Result<()> {
    let msg = ingest::RawMessage {
        source: source.to_string(),
        source_id: None,
        sender: sender.to_string(),
        channel: channel.map(|c| c.to_string()),
        content: content.to_string(),
        metadata: None,
        created_at: None,
    };

    let id = nomen.ingest_message(msg).await?;
    println!(
        "{} message from {} [{}]{}",
        "Ingested".green().bold(),
        sender.bold(),
        source,
        channel.map(|c| format!(" #{c}")).unwrap_or_default()
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
    let db_handle = db::init_db().await?;

    let messages = if let Some(source_id) = around {
        db::query_messages_around(&db_handle, source_id, context_count).await?
    } else {
        let opts = ingest::MessageQuery {
            source: source.map(|s| s.to_string()),
            channel: channel.map(|c| c.to_string()),
            sender: sender.map(|s| s.to_string()),
            since: since.map(|s| s.to_string()),
            limit: Some(limit),
            consolidated_only: false,
        };
        ingest::get_messages(&db_handle, &opts).await?
    };

    if messages.is_empty() {
        println!("No messages found.");
        return Ok(());
    }

    println!("\n{}\n{}", "Raw Messages".bold(), "═".repeat(60));

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
    config: &Config,
    resolved: &ResolvedConfig,
    min_messages: usize,
    batch_size: usize,
    dry_run: bool,
    older_than: Option<String>,
    tier: Option<String>,
) -> Result<()> {
    let embedder = config.build_embedder();
    let db_handle = db::init_db_with_dimensions(config.embedding_dimensions()).await?;

    // Build relay manager for NIP-09 deletion events
    let relay_manager = if !resolved.nsecs.is_empty() && !dry_run {
        let (all_keys, _) = parse_keys(&resolved.nsecs)?;
        let mgr = build_relay_manager(&resolved.relay, &all_keys[0]);
        mgr.connect().await.ok();
        Some(mgr)
    } else {
        None
    };

    // Build LLM provider from config
    let llm_provider: Box<dyn consolidate::LlmProvider> = config
        .consolidation_llm_config()
        .and_then(|c| consolidate::OpenAiLlmProvider::from_config(&c))
        .map(|p| Box::new(p) as Box<dyn consolidate::LlmProvider>)
        .unwrap_or_else(|| Box::new(consolidate::NoopLlmProvider));

    let author_pubkey = relay_manager
        .as_ref()
        .map(|mgr| mgr.public_key().to_hex())
        .or_else(|| {
            config
                .all_nsecs()
                .first()
                .and_then(|nsec| nostr_sdk::SecretKey::from_bech32(nsec).ok())
                .map(|sk| nostr_sdk::Keys::new(sk).public_key().to_hex())
        });
    let entity_extractor = entities::build_entity_extractor(config);

    let consolidation_config = consolidate::ConsolidationConfig {
        batch_size,
        min_messages,
        llm_provider,
        entity_extractor,
        dry_run,
        older_than,
        tier,
        author_pubkey,
    };

    if dry_run {
        println!(
            "{} Running consolidation pipeline...",
            "[DRY RUN]".yellow().bold()
        );
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
            prefix, report.messages_processed, report.memories_created
        );
        if report.events_published > 0 {
            println!(
                "  Published {} memories to relay (kind 31234)",
                report.events_published
            );
        }
        if report.events_deleted > 0 {
            println!(
                "  Deleted {} ephemeral events from relay (NIP-09)",
                report.events_deleted
            );
        }
        if !report.channels.is_empty() {
            println!("  Channels: {}", report.channels.join(", "));
        }
        for group in &report.groups {
            println!(
                "  {} → {} ({} messages)",
                group.key.dimmed(),
                group.topic.bold(),
                group.message_count
            );
        }
    }

    if let Some(ref mgr) = relay_manager {
        mgr.disconnect().await;
    }

    Ok(())
}

// ── Command: entities ───────────────────────────────────────────────

async fn cmd_entities(nomen: &Nomen, kind_filter: Option<&str>, show_relations: bool) -> Result<()> {
    if kind_filter.is_some() && entities::EntityKind::from_str(kind_filter.unwrap()).is_none() {
        bail!(
            "Unknown entity kind: {}. Valid kinds: person, project, concept, place, organization, technology",
            kind_filter.unwrap()
        );
    }

    let entity_list = nomen.entities(kind_filter).await?;

    if entity_list.is_empty() {
        println!("No entities found.");
        return Ok(());
    }

    println!("\n{}\n{}", "Entities".bold(), "═".repeat(60));

    for entity in &entity_list {
        println!("\n  {} [{}]", entity.name.bold(), entity.kind.yellow());
        println!("    Created: {}", entity.created_at.dimmed());
    }

    println!("\n{}: {} entities", "Total".bold(), entity_list.len());

    if show_relations {
        let relationships = nomen.entity_relationships(None).await?;
        if relationships.is_empty() {
            println!("\nNo relationships found.");
        } else {
            println!("\n{}\n{}", "Relationships".bold(), "═".repeat(60));
            for rel in &relationships {
                let detail_str = if rel.detail.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", rel.detail.dimmed())
                };
                println!(
                    "  {} {} {} {}{}",
                    rel.from_name.bold(),
                    "→".dimmed(),
                    rel.relation.cyan(),
                    rel.to_name.bold(),
                    detail_str,
                );
            }
            println!("\n{}: {} relationships", "Total".bold(), relationships.len());
        }
    }

    println!();
    Ok(())
}

// ── Command: cluster ────────────────────────────────────────────────

async fn cmd_cluster(
    config: &Config,
    _resolved: &ResolvedConfig,
    dry_run: bool,
    prefix: Option<String>,
    min_members: usize,
    namespace_depth: usize,
) -> Result<()> {
    let embedder = config.build_embedder();
    let db_handle = db::init_db_with_dimensions(config.embedding_dimensions()).await?;

    // Build LLM provider — reuse consolidation LLM config
    let llm_provider: Box<dyn nomen::cluster::ClusterLlmProvider> = config
        .consolidation_llm_config()
        .and_then(|c| nomen::cluster::OpenAiClusterLlmProvider::from_config(&c))
        .map(|p| Box::new(p) as Box<dyn nomen::cluster::ClusterLlmProvider>)
        .unwrap_or_else(|| Box::new(nomen::cluster::NoopClusterLlmProvider));

    // Author pubkey for d-tag construction
    let author_pubkey = config
        .all_nsecs()
        .first()
        .and_then(|nsec| nostr_sdk::SecretKey::from_bech32(nsec).ok())
        .map(|sk| nostr_sdk::Keys::new(sk).public_key().to_hex());

    let cluster_config = nomen::cluster::ClusterConfig {
        min_members,
        namespace_depth,
        llm_provider,
        dry_run,
        prefix_filter: prefix.clone(),
        author_pubkey,
    };

    if dry_run {
        println!(
            "{} Running cluster fusion...",
            "[DRY RUN]".yellow().bold()
        );
    } else {
        println!("Running cluster fusion...");
    }

    let report =
        nomen::cluster::run_cluster_fusion(&db_handle, embedder.as_ref(), &cluster_config, None)
            .await?;

    if report.clusters_found == 0 {
        println!(
            "No clusters found (need at least {min_members} memories per namespace prefix)."
        );
        if report.memories_scanned == 0 {
            println!("  No named memories in the database. Run `nomen consolidate` first.");
        } else {
            println!(
                "  Scanned {} memories at namespace depth {}.",
                report.memories_scanned, namespace_depth
            );
        }
    } else {
        let prefix_display = if dry_run {
            format!("{}", "[DRY RUN] Would synthesize".yellow())
        } else {
            format!("{}", "Synthesized".green().bold())
        };

        println!(
            "{}: {} clusters from {} memories",
            prefix_display,
            if dry_run {
                report.clusters_found
            } else {
                report.clusters_synthesized
            },
            report.memories_scanned
        );

        if !dry_run && report.edges_created > 0 {
            println!(
                "  Created {} 'summarizes' edges",
                report.edges_created
            );
        }

        for detail in &report.cluster_details {
            println!(
                "\n  {} ({} members)",
                detail.prefix.bold(),
                detail.member_count
            );
            for topic in &detail.member_topics {
                println!("    - {}", topic.dimmed());
            }
        }
    }

    println!();
    Ok(())
}

// ── Command: prune ──────────────────────────────────────────────────

async fn cmd_prune(nomen: &Nomen, days: u64, dry_run: bool) -> Result<()> {
    if dry_run {
        println!(
            "{} Pruning memories (older than {} days)...",
            "[DRY RUN]".yellow().bold(),
            days
        );
    } else {
        println!("Pruning memories (older than {} days)...", days);
    }

    let report = nomen.prune(days, dry_run).await?;

    if report.pruned.is_empty() {
        println!("No memories eligible for pruning.");
    } else {
        println!("\n{} memories eligible for pruning:", report.pruned.len());
        for mem in &report.pruned {
            let confidence_str = mem
                .confidence
                .map(|c| format!("{c:.2}"))
                .unwrap_or("?".to_string());
            let access_str = mem
                .access_count
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
            println!(
                "\n{}: Would prune {} memories",
                "[DRY RUN]".yellow().bold(),
                report.memories_pruned
            );
        } else {
            println!(
                "\n{}: {} memories pruned",
                "Pruned".green().bold(),
                report.memories_pruned
            );
        }
    }

    if report.raw_messages_pruned > 0 {
        if dry_run {
            println!(
                "{}: Would also prune {} consolidated raw messages",
                "[DRY RUN]".yellow().bold(),
                report.raw_messages_pruned
            );
        } else {
            println!(
                "{}: {} consolidated raw messages pruned",
                "Pruned".green().bold(),
                report.raw_messages_pruned
            );
        }
    }

    Ok(())
}

// ── Command: send ───────────────────────────────────────────────────

async fn cmd_send(
    nomen: &Nomen,
    recipient: &str,
    content: &str,
    channel: Option<&str>,
) -> Result<()> {
    let target = send::parse_recipient(recipient)?;
    let opts = send::SendOptions {
        target,
        content: content.to_string(),
        channel: channel.map(String::from),
        metadata: None,
    };

    let result = nomen.send(opts).await?;

    println!(
        "{} to {}: event_id={}",
        "Sent".green().bold(),
        recipient.bold(),
        result.event_id
    );
    println!("  {}", result.summary());

    Ok(())
}

// ── Command: serve (MCP server) ─────────────────────────────────────

async fn cmd_serve(
    config: &Config,
    resolved: &ResolvedConfig,
    stdio: bool,
    http_addr: Option<String>,
    static_dir: Option<PathBuf>,
    landing_dir: Option<PathBuf>,
    context_vm: bool,
    allowed_npubs: Vec<String>,
) -> Result<()> {
    let db = db::init_db_with_dimensions(config.embedding_dimensions()).await?;

    let default_channel = config
        .messaging
        .as_ref()
        .map(|m| m.default_channel.clone())
        .unwrap_or_else(|| "nostr".to_string());

    let group_store = groups::GroupStore::load(&config.groups, &db).await?;

    // Optionally build relay manager if nsecs are available
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
        let bind_addr = if addr.starts_with(':') {
            format!("0.0.0.0{addr}")
        } else {
            addr.clone()
        };

        let resolved_static = static_dir.or_else(|| {
            if let Ok(exe) = std::env::current_exe() {
                let dir = exe.parent()?.join("web/dist");
                if dir.is_dir() {
                    return Some(dir);
                }
            }
            let cwd = PathBuf::from("web/dist");
            if cwd.is_dir() {
                Some(cwd)
            } else {
                None
            }
        });

        let http_state = nomen::http::AppState {
            db,
            embedder: config.build_embedder(),
            relay: relay_manager,
            groups: group_store,
            default_channel,
            config: std::sync::Arc::new(tokio::sync::RwLock::new(config.clone())),
        };

        let resolved_landing = landing_dir.or_else(|| {
            if let Ok(exe) = std::env::current_exe() {
                let dir = exe.parent()?.join("web/dist-landing");
                if dir.is_dir() {
                    return Some(dir);
                }
            }
            let cwd = PathBuf::from("web/dist-landing");
            if cwd.is_dir() {
                Some(cwd)
            } else {
                None
            }
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

        let vm_nomen = nomen::Nomen::open_with_relay(config, vm_relay).await?;

        let vm_server = contextvm::ContextVmServer::new(
            vm_nomen,
            allowed_npubs,
            default_channel.clone(),
        );

        // Build MCP Nomen instance
        let mcp_nomen = if let Some(relay) = relay_manager {
            nomen::Nomen::open_with_relay(config, relay).await?
        } else {
            nomen::Nomen::open(config).await?
        };

        let mcp_future = mcp::serve_stdio(mcp_nomen, default_channel);
        let vm_future = vm_server.run();

        tokio::select! {
            result = mcp_future => result,
            result = vm_future => result,
        }
    } else {
        let nomen_instance = if let Some(relay) = relay_manager {
            nomen::Nomen::open_with_relay(config, relay).await?
        } else {
            nomen::Nomen::open(config).await?
        };
        mcp::serve_stdio(nomen_instance, default_channel).await
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
        .default("wss://zooid.atlantislabs.space".to_string())
        .interact_text()?;

    // 2. Identities
    println!("\n  {}", "2. Identities".bold());
    let guardian_nsec: String = Password::new()
        .with_prompt("     User nsec (your key)")
        .interact()?;
    let guardian_keys = Keys::parse(&guardian_nsec).context("Invalid user nsec")?;
    let guardian_npub = guardian_keys.public_key().to_bech32()?;
    println!("     {} {}", "✓".green(), guardian_npub);

    let mut agent_nsecs: Vec<String> = Vec::new();
    let add_agents = Confirm::new()
        .with_prompt("     Add agent identities?")
        .default(false)
        .interact()?;

    if add_agents {
        loop {
            let agent_nsec: String = Password::new().with_prompt("     Agent nsec").interact()?;
            let agent_keys = Keys::parse(&agent_nsec).context("Invalid agent nsec")?;
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
            cluster: None,
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
        entities: None,
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
    println!("  Guardian: {npub}");

    let relay = std::env::var("NOMEN_RELAY").unwrap_or_else(|_| "wss://relay.damus.io".to_string());

    let config = Config {
        relay: Some(relay.clone()),
        nsec: Some(nsec),
        nsecs: Vec::new(),
        default_writer: Some("guardian".to_string()),
        embedding: Some(EmbeddingConfig::default()),
        groups: Vec::new(),
        consolidation: None,
        memory: Some(MemorySection {
            cluster: None,
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
        entities: None,
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
        .unwrap_or("wss://zooid.atlantislabs.space");
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
