//! CLI types and subcommand definitions.

pub mod admin;
pub mod fs;
pub mod group;
pub mod helpers;
pub mod memory;
pub mod message;
pub mod server;
pub mod service;
pub mod sync;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

// ── CLI ─────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "nomen", about = "Nostr-native memory system CLI")]
pub struct Cli {
    /// Relay URL (overrides config file)
    #[arg(long)]
    pub relay: Option<String>,

    /// Nostr secret key (nsec1...), can be specified multiple times
    #[arg(long = "nsec")]
    pub nsecs: Vec<String>,

    /// Path to config file
    #[arg(long)]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
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
    /// Show or edit configuration
    Config {
        #[command(subcommand)]
        action: Option<ConfigAction>,
    },
    /// Sync memory events from relay to local SurrealDB
    Sync,
    /// Store a new memory
    Store {
        /// Topic/namespace for the memory
        topic: String,
        /// Memory content
        #[arg(long)]
        content: String,
        /// Visibility tier
        #[arg(long, default_value = "public")]
        tier: String,
    },
    /// Delete a memory by topic or event ID
    Delete {
        /// Topic to delete
        topic: Option<String>,
        /// Event ID to delete
        #[arg(long)]
        id: Option<String>,
        /// Delete ephemeral collected messages instead of memories
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
    /// Ingest a collected message
    Ingest {
        /// Message content
        content: String,
        /// Source system (e.g. telegram, nostr, webhook)
        #[arg(long, default_value = "cli")]
        source: String,
        /// Sender identifier
        #[arg(long, default_value = "local")]
        sender: String,
        /// Canonical platform namespace (defaults to source)
        #[arg(long)]
        platform: Option<String>,
        /// Canonical community/container id
        #[arg(long)]
        community: Option<String>,
        /// Canonical chat id
        #[arg(long)]
        chat: Option<String>,
        /// Canonical thread/topic id
        #[arg(long)]
        thread: Option<String>,
    },
    /// List collected messages
    Messages {
        /// Filter by platform
        #[arg(long)]
        platform: Option<String>,
        /// Filter by chat id
        #[arg(long)]
        chat: Option<String>,
        /// Filter by thread/topic id
        #[arg(long)]
        thread: Option<String>,
        /// Filter by sender
        #[arg(long)]
        sender: Option<String>,
        /// Show messages since (RFC3339 timestamp)
        #[arg(long)]
        since: Option<String>,
        /// Max results
        #[arg(long, default_value = "50")]
        limit: usize,
        /// Show recent context for a chat/thread selection (requires --chat)
        #[arg(long)]
        around: Option<String>,
        /// Number of context messages before/after --around target
        #[arg(long, default_value = "5")]
        context: usize,
    },
    /// Consolidate collected messages into memories
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
    /// Prune unused memories and old collected messages
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
    /// Bidirectional filesystem sync (markdown ↔ DB)
    Fs {
        #[command(subcommand)]
        action: FsAction,
    },
    /// Manage systemd user service
    Service {
        #[command(subcommand)]
        action: ServiceAction,
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
        /// Enable socket server
        #[arg(long)]
        socket: bool,
        /// Also start Context-VM (Nostr-native request/response listener)
        #[arg(long)]
        context_vm: bool,
        /// Allowed npubs for Context-VM requests (comma-separated hex or bech32)
        #[arg(long, value_delimiter = ',')]
        allowed_npubs: Vec<String>,
    },
}

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Get a config value by dotted key (e.g. relay, fs.sync_dir)
    Get {
        /// Config key (e.g. relay, fs.sync_dir)
        key: String,
    },
    /// Set a config value by dotted key
    Set {
        /// Config key (e.g. relay, fs.sync_dir)
        key: String,
        /// Value to set
        value: String,
    },
}

#[derive(Subcommand)]
pub enum FsAction {
    /// Initialize a sync directory
    Init {
        /// Directory to initialize (default: current directory)
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Pull memories from DB to filesystem
    Pull {
        /// Sync directory (default: current directory)
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Push changed files back to DB
    Push {
        /// Sync directory (default: current directory)
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Show sync status
    Status {
        /// Sync directory (default: current directory)
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Start real-time bidirectional sync daemon
    Start {
        /// Sync directory (default: current directory)
        #[arg(long)]
        dir: Option<PathBuf>,
        /// DB poll interval in seconds
        #[arg(long, default_value = "30")]
        poll_secs: u64,
        /// Print a line per file change pushed/pulled
        #[arg(long)]
        verbose: bool,
        /// Detailed diagnostic output (cwd, poll interval, watcher events, etc.)
        #[arg(long)]
        debug: bool,
        /// Remove local files not in memory DB
        #[arg(long)]
        clean: bool,
    },
    /// Stop the sync daemon
    Stop {
        /// Sync directory (default: current directory)
        #[arg(long)]
        dir: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
pub enum GroupAction {
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

#[derive(Subcommand)]
pub enum ServiceAction {
    /// Install systemd user service
    Install {
        /// Overwrite even if managed by Nix
        #[arg(long)]
        force: bool,
    },
    /// Start the service
    Start,
    /// Stop the service
    Stop,
    /// Restart the service
    Restart,
    /// Show service status
    Status,
    /// Follow service logs
    Logs {
        /// Follow log output
        #[arg(short, long)]
        follow: bool,
    },
    /// Uninstall the service
    Uninstall,
}
