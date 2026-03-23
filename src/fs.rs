//! Filesystem sync — bidirectional mapping between Nomen memories and local markdown files.
//!
//! Each memory is a markdown file with YAML frontmatter:
//! ```markdown
//! ---
//! d_tag: group:telegram:-1003821690204:room/8485
//! topic: room/8485
//! visibility: group
//! scope: telegram:-1003821690204
//! version: 3
//! created_at: 2026-03-19T16:00:00Z
//! updated_at: 2026-03-19T18:00:00Z
//! ---
//! Memory content here (markdown).
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::db::MemoryRecord;
use crate::memory;
use std::future::Future;
use std::pin::Pin;

/// A dispatch function that sends an action + params to Nomen (via HTTP or direct DB).
/// Returns the `result` value from the response.
pub type DispatchFn = Box<
    dyn Fn(String, serde_json::Value) -> Pin<Box<dyn Future<Output = Result<serde_json::Value>> + Send>>
        + Send
        + Sync,
>;

// ── Constants ────────────────────────────────────────────────────────

const SYNC_META_DIR: &str = ".nomen-fs";
const STATE_FILE: &str = "state.json";

// ── Sync state ───────────────────────────────────────────────────────

/// Per-file sync state entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncEntry {
    /// The d-tag this file maps to.
    pub d_tag: String,
    /// File content hash (blake2 or simple) at last sync.
    pub content_hash: String,
    /// Memory version at last sync.
    pub version: i64,
    /// Timestamp of last sync.
    pub synced_at: String,
}

/// Full sync state.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SyncState {
    /// Map from relative file path → sync entry.
    pub files: HashMap<String, SyncEntry>,
}

impl SyncState {
    fn load(dir: &Path) -> Result<Self> {
        let path = dir.join(SYNC_META_DIR).join(STATE_FILE);
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        Ok(serde_json::from_str(&text)?)
    }

    fn save(&self, dir: &Path) -> Result<()> {
        let path = dir.join(SYNC_META_DIR).join(STATE_FILE);
        let text = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, text)
            .with_context(|| format!("Failed to write {}", path.display()))?;
        Ok(())
    }
}

// ── YAML frontmatter ─────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct MemoryFrontmatter {
    pub d_tag: String,
    pub topic: String,
    pub visibility: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub scope: String,
    #[serde(default)]
    pub version: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// A parsed markdown memory file.
pub struct ParsedMemoryFile {
    pub frontmatter: MemoryFrontmatter,
    pub content: String,
}

// ── D-tag ↔ file path mapping ────────────────────────────────────────

/// Convert a d-tag to a relative file path.
///
/// Slash-format d-tags map directly:
/// - `public/my-topic` → `public/my-topic.md`
/// - `group/telegram:-1003821690204/room/8485` → `group/telegram:-1003821690204/room/8485.md`
/// - `personal/d29fe7c1.../projects/nomen` → `personal/d29fe7c1.../projects/nomen.md`
pub fn dtag_to_path(d_tag: &str) -> PathBuf {
    let (visibility, scope) = memory::extract_visibility_scope(d_tag);
    let topic = memory::dtag_topic(d_tag).unwrap_or(d_tag);

    let mut path = PathBuf::new();
    path.push(&visibility);

    if !scope.is_empty() {
        path.push(&scope);
    }

    // Topic may contain slashes → nested directories
    let topic_path = format!("{topic}.md");
    path.push(&topic_path);

    path
}

/// Convert a relative file path back to a d-tag.
///
/// Reverses `dtag_to_path`: strips `.md`, uses directory components directly.
pub fn path_to_dtag(rel_path: &Path) -> Option<String> {
    let path_str = rel_path.to_str()?;

    // Must end in .md
    let without_ext = path_str.strip_suffix(".md")?;

    // First component is visibility
    let mut components = without_ext.splitn(2, '/');
    let visibility = components.next()?;

    if !matches!(
        visibility,
        "public" | "group" | "circle" | "personal" | "private" | "internal"
    ) {
        return None;
    }

    let rest = components.next().unwrap_or("");

    if visibility == "public" || visibility == "private" || visibility == "internal" {
        // Unscoped tiers: {tier}/{topic}
        if rest.is_empty() {
            return None;
        }
        let vis = memory::normalize_tier_name(visibility);
        return Some(memory::build_dtag(&vis, "", rest));
    }

    // Scoped tiers (personal, group, circle): {tier}/{scope}/{topic}
    let mut parts = rest.splitn(2, '/');
    let scope = parts.next().unwrap_or("");
    let topic = parts.next();

    if scope.is_empty() {
        return None;
    }

    match topic {
        Some(t) if !t.is_empty() => Some(memory::build_dtag(visibility, scope, t)),
        _ => None,
    }
}

// ── Markdown serialization ───────────────────────────────────────────

/// Serialize a memory record to markdown with YAML frontmatter.
pub fn memory_to_markdown(mem: &MemoryRecord) -> String {
    let d_tag = mem.d_tag.clone().unwrap_or_default();
    let (visibility, scope) = memory::extract_visibility_scope(&d_tag);
    let topic = mem.topic.clone();

    let fm = MemoryFrontmatter {
        d_tag,
        topic,
        visibility,
        scope,
        version: mem.version,
        created_at: mem.created_at.clone(),
        updated_at: mem.updated_at.clone(),
    };

    let content = &mem.content;
    frontmatter_to_markdown(&fm, content)
}

/// Serialize a JSON memory value (from API response) to markdown.
pub fn json_memory_to_markdown(val: &serde_json::Value) -> String {
    let d_tag = val["d_tag"].as_str().unwrap_or("").to_string();
    let (visibility, scope) = memory::extract_visibility_scope(&d_tag);

    let fm = MemoryFrontmatter {
        d_tag,
        topic: val["topic"].as_str().unwrap_or("").to_string(),
        visibility,
        scope,
        version: val["version"].as_i64().unwrap_or(0),
        created_at: val["created_at"].as_str().unwrap_or("").to_string(),
        updated_at: val["updated_at"].as_str().unwrap_or("").to_string(),
    };

    let content = val["content"].as_str().unwrap_or("");

    frontmatter_to_markdown(&fm, content)
}

fn frontmatter_to_markdown(fm: &MemoryFrontmatter, content: &str) -> String {
    let yaml = serde_yaml_ng::to_string(fm).unwrap_or_default();
    // serde_yaml_ng adds a trailing newline; trim it so the --- is clean
    let yaml = yaml.trim_end();

    let mut out = String::new();
    out.push_str("---\n");
    out.push_str(yaml);
    out.push_str("\n---\n");
    out.push_str(content);
    out.push('\n');
    out
}

/// Parse a markdown file into frontmatter + detail.
pub fn parse_markdown(content: &str) -> Result<ParsedMemoryFile> {
    // Split on YAML frontmatter delimiters
    let content = content.trim_start_matches('\u{feff}'); // BOM
    if !content.starts_with("---") {
        bail!("Missing YAML frontmatter (must start with ---)");
    }

    let after_first = &content[3..];
    let end_idx = after_first
        .find("\n---")
        .context("Missing closing --- for frontmatter")?;

    let yaml_str = after_first[..end_idx].trim();
    let body = after_first[end_idx + 4..].trim_start_matches('\n');

    let frontmatter: MemoryFrontmatter =
        serde_yaml_ng::from_str(yaml_str).context("Invalid YAML frontmatter")?;

    // The entire body after frontmatter is the content
    let content = body.trim().to_string();

    Ok(ParsedMemoryFile {
        frontmatter,
        content,
    })
}

// ── Content hashing ──────────────────────────────────────────────────

fn content_hash(content: &str) -> String {
    // Simple hash: use the string length + first/last chars + a checksum
    // (Avoiding extra crypto deps; this is sufficient for change detection)
    let bytes = content.as_bytes();
    let mut hash: u64 = bytes.len() as u64;
    for (i, &b) in bytes.iter().enumerate() {
        hash = hash.wrapping_mul(31).wrapping_add(b as u64).wrapping_add(i as u64);
    }
    format!("{hash:016x}")
}

// ── Init ─────────────────────────────────────────────────────────────

/// Initialize a filesystem sync directory.
pub fn init_sync_dir(dir: &Path) -> Result<()> {
    let meta = dir.join(SYNC_META_DIR);
    std::fs::create_dir_all(&meta)
        .with_context(|| format!("Failed to create {}", meta.display()))?;

    // Create tier directories
    for tier in &["public", "group", "personal", "private"] {
        let tier_dir = dir.join(tier);
        std::fs::create_dir_all(&tier_dir)?;
    }

    // Initialize state
    let state = SyncState::default();
    state.save(dir)?;

    // Create conflicts directory
    std::fs::create_dir_all(meta.join("conflicts"))?;

    info!(dir = %dir.display(), "Initialized sync directory");
    Ok(())
}

// ── Pull ─────────────────────────────────────────────────────────────

/// Pull all memories from DB to filesystem via API dispatch. Returns count of files written.
pub async fn pull(dispatch: &DispatchFn, dir: &Path) -> Result<usize> {
    let result = dispatch(
        "memory.list".to_string(),
        serde_json::json!({"limit": 10000}),
    )
    .await?;

    let memories = result["memories"]
        .as_array()
        .context("memory.list did not return memories array")?;

    let mut state = SyncState::load(dir)?;
    let mut count = 0;

    for mem in memories {
        let d_tag = match mem["d_tag"].as_str() {
            Some(d) if !d.is_empty() => d.to_string(),
            _ => continue,
        };

        let rel_path = dtag_to_path(&d_tag);
        let abs_path = dir.join(&rel_path);

        // Create parent directories
        if let Some(parent) = abs_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let markdown = json_memory_to_markdown(mem);
        let hash = content_hash(&markdown);
        let version = mem["version"].as_i64().unwrap_or(0);

        // Skip if file exists on disk and matches last sync
        if abs_path.exists() {
            if let Some(entry) = state.files.get(rel_path.to_str().unwrap_or("")) {
                if entry.content_hash == hash && entry.version == version {
                    debug!(d_tag = %d_tag, "Skipping (unchanged)");
                    continue;
                }
            }
        }

        std::fs::write(&abs_path, &markdown)
            .with_context(|| format!("Failed to write {}", abs_path.display()))?;

        let rel_key = rel_path.to_str().unwrap_or("").to_string();
        state.files.insert(
            rel_key,
            SyncEntry {
                d_tag,
                content_hash: hash,
                version,
                synced_at: chrono::Utc::now().to_rfc3339(),
            },
        );
        count += 1;
    }

    // Delete local files for memories removed from DB
    let remote_dtags: std::collections::HashSet<String> = memories
        .iter()
        .filter_map(|m| m["d_tag"].as_str())
        .filter(|d| !d.is_empty())
        .map(|d| d.to_string())
        .collect();

    let stale: Vec<String> = state
        .files
        .iter()
        .filter(|(_, entry)| !remote_dtags.contains(&entry.d_tag))
        .map(|(key, _)| key.clone())
        .collect();

    for rel_key in &stale {
        let abs_path = dir.join(rel_key);
        if abs_path.exists() {
            std::fs::remove_file(&abs_path)?;
            info!(path = %rel_key, "Deleted (memory removed from DB)");
        }
        state.files.remove(rel_key);
        count += 1;
    }

    state.save(dir)?;
    info!(count, "Pulled memories to filesystem");
    Ok(count)
}

// ── Push ─────────────────────────────────────────────────────────────

/// Push changed files back to Nomen via API dispatch. Returns count of memories updated.
pub async fn push(dispatch: &DispatchFn, dir: &Path) -> Result<usize> {
    let mut state = SyncState::load(dir)?;
    let mut count = 0;

    // Walk all .md files in the sync directory
    let md_files = walk_md_files(dir)?;

    for abs_path in md_files {
        let rel_path = abs_path
            .strip_prefix(dir)
            .unwrap_or(&abs_path)
            .to_path_buf();
        let rel_key = rel_path.to_str().unwrap_or("").to_string();

        let content = std::fs::read_to_string(&abs_path)?;
        let hash = content_hash(&content);

        // Check if unchanged since last sync
        if let Some(entry) = state.files.get(&rel_key) {
            if entry.content_hash == hash {
                continue;
            }
        }

        // Parse the markdown file — files without valid frontmatter are silently ignored
        let parsed = match parse_markdown(&content) {
            Ok(p) => p,
            Err(e) => {
                debug!(path = %abs_path.display(), "Skipping file without valid frontmatter: {e}");
                continue;
            }
        };

        // Resolve the d-tag: prefer frontmatter, fall back to path
        let d_tag = if !parsed.frontmatter.d_tag.is_empty() {
            parsed.frontmatter.d_tag.clone()
        } else {
            match path_to_dtag(&rel_path) {
                Some(dt) => dt,
                None => {
                    warn!(path = %abs_path.display(), "Cannot resolve d-tag, skipping");
                    continue;
                }
            }
        };

        // Extract clean topic from d-tag (handles both v0.2 colon and v0.3 slash formats)
        let topic = if !d_tag.is_empty() {
            memory::dtag_topic(&d_tag)
                .unwrap_or(&parsed.frontmatter.topic)
                .to_string()
        } else {
            parsed.frontmatter.topic.clone()
        };

        // Extract visibility and scope from d-tag for correctness
        let (visibility, scope) = if !d_tag.is_empty() {
            memory::extract_visibility_scope(&d_tag)
        } else {
            (parsed.frontmatter.visibility.clone(), parsed.frontmatter.scope.clone())
        };

        let params = serde_json::json!({
            "topic": topic,
            "content": parsed.content,
            "visibility": visibility,
            "scope": scope,
            "source": "fs-push",
        });

        match dispatch("memory.put".to_string(), params).await {
            Ok(result) => {
                let stored_dtag = result["d_tag"]
                    .as_str()
                    .unwrap_or(&d_tag)
                    .to_string();
                info!(d_tag = %stored_dtag, "Pushed from file");

                state.files.insert(
                    rel_key,
                    SyncEntry {
                        d_tag: stored_dtag,
                        content_hash: hash,
                        version: parsed.frontmatter.version + 1,
                        synced_at: chrono::Utc::now().to_rfc3339(),
                    },
                );
                count += 1;
            }
            Err(e) => {
                warn!(path = %abs_path.display(), "Push failed: {e}");
            }
        }
    }

    state.save(dir)?;
    info!(count, "Pushed files to Nomen");
    Ok(count)
}

// ── Status ───────────────────────────────────────────────────────────

/// Show sync status for a directory.
pub async fn status(dispatch: &DispatchFn, dir: &Path) -> Result<()> {
    if !dir.join(SYNC_META_DIR).exists() {
        println!("Not a nomen-fs directory: {}", dir.display());
        println!("Run: nomen fs init --dir {}", dir.display());
        return Ok(());
    }

    let state = SyncState::load(dir)?;

    let result = dispatch(
        "memory.list".to_string(),
        serde_json::json!({"limit": 10000}),
    )
    .await?;

    let memories = result["memories"]
        .as_array()
        .context("memory.list did not return memories array")?;

    let md_files = walk_md_files(dir)?;
    let mut local_changed = 0;
    let mut remote_new = 0;

    // Check local changes
    for abs_path in &md_files {
        let rel_path = abs_path.strip_prefix(dir).unwrap_or(abs_path);
        let rel_key = rel_path.to_str().unwrap_or("");
        let content = std::fs::read_to_string(abs_path).unwrap_or_default();
        let hash = content_hash(&content);

        if let Some(entry) = state.files.get(rel_key) {
            if entry.content_hash != hash {
                local_changed += 1;
            }
        } else {
            local_changed += 1; // new file
        }
    }

    // Check for memories not yet synced
    for mem in memories {
        let d_tag = match mem["d_tag"].as_str() {
            Some(d) if !d.is_empty() => d.to_string(),
            _ => continue,
        };
        let rel_path = dtag_to_path(&d_tag);
        let rel_key = rel_path.to_str().unwrap_or("");
        if !state.files.contains_key(rel_key) {
            remote_new += 1;
        }
    }

    println!("Sync directory: {}", dir.display());
    println!("Files tracked:  {}", state.files.len());
    println!("Local files:    {}", md_files.len());
    println!("DB memories:    {}", memories.len());
    println!("Local changes:  {local_changed}");
    println!("Remote new:     {remote_new}");

    Ok(())
}

// ── Daemon: real-time bidirectional sync ─────────────────────────────

const PID_FILE: &str = "daemon.pid";
const DEBOUNCE_MS: u64 = 500;
const WRITE_SUPPRESS_MS: u64 = 1500;

/// Start the real-time bidirectional sync daemon.
///
/// Watches the filesystem for changes (inotify) and polls the DB for remote updates.
/// File changes are debounced (500ms) before pushing. Conflicts are saved to
/// `.nomen-fs/conflicts/`.
pub async fn start(dispatch: &DispatchFn, dir: &Path, poll_secs: u64, verbose: bool) -> Result<()> {
    use notify::{EventKind, RecursiveMode, Watcher};
    use std::time::{Duration, Instant};

    let pid_path = dir.join(SYNC_META_DIR).join(PID_FILE);

    // Check if already running
    if pid_path.exists() {
        let existing = std::fs::read_to_string(&pid_path)
            .unwrap_or_default()
            .trim()
            .to_string();
        let alive = std::process::Command::new("kill")
            .args(["-0", &existing])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if alive {
            bail!("Daemon already running (PID {existing}). Use `nomen fs stop` first.");
        }
        let _ = std::fs::remove_file(&pid_path);
    }

    // Write PID file
    std::fs::write(&pid_path, std::process::id().to_string())?;

    // Initial sync: push local changes first, then pull remote
    let pushed = push(dispatch, dir).await?;
    if pushed > 0 {
        println!("Initial push: {pushed} files");
    }
    let pulled = pull(dispatch, dir).await?;
    if pulled > 0 {
        println!("Initial pull: {pulled} files");
    }

    // Channel to receive filesystem events in the async runtime
    let (fs_tx, mut fs_rx) = tokio::sync::mpsc::channel::<(PathBuf, &'static str)>(256);

    // Set up filesystem watcher
    let dir_for_watcher = dir.to_path_buf();
    let mut watcher = notify::RecommendedWatcher::new(
        move |res: Result<notify::Event, notify::Error>| {
            if let Ok(event) = res {
                use notify::event::{AccessKind, AccessMode, DataChange, ModifyKind, RenameMode};
                let label = match event.kind {
                    EventKind::Create(_) => "created",
                    EventKind::Modify(ModifyKind::Data(DataChange::Content)) => "written",
                    EventKind::Modify(ModifyKind::Data(_)) => "written",
                    EventKind::Modify(ModifyKind::Name(RenameMode::From)) => "rename from",
                    EventKind::Modify(ModifyKind::Name(RenameMode::To)) => "rename to",
                    EventKind::Modify(ModifyKind::Name(_)) => "renamed",
                    EventKind::Modify(ModifyKind::Metadata(_)) => "metadata",
                    EventKind::Modify(_) => "modified",
                    EventKind::Access(AccessKind::Close(AccessMode::Write)) => "closed",
                    _ => return,
                };
                for path in event.paths {
                    if is_watched_path(&dir_for_watcher, &path) {
                        let _ = fs_tx.blocking_send((path, label));
                    }
                }
            }
        },
        notify::Config::default(),
    )
    .context("Failed to create filesystem watcher")?;
    watcher
        .watch(dir, RecursiveMode::Recursive)
        .context("Failed to watch directory")?;

    // Daemon state
    let mut pending: HashMap<PathBuf, Instant> = HashMap::new();
    let mut recently_written: HashMap<PathBuf, Instant> = HashMap::new();
    // Track pending "rename from" events to pair with "rename to"
    let mut rename_from: Option<(PathBuf, Instant)> = None;
    let mut poll_interval = tokio::time::interval(Duration::from_secs(poll_secs));
    let mut debounce_tick = tokio::time::interval(Duration::from_millis(100));

    println!(
        "Daemon started (PID {}), watching {}",
        std::process::id(),
        dir.display()
    );
    println!("Press Ctrl+C to stop.");

    loop {
        tokio::select! {
            // Filesystem event from watcher
            Some((path, label)) = fs_rx.recv() => {
                // Suppress echo from our own writes
                let suppressed = recently_written
                    .get(&path)
                    .map(|t| t.elapsed() < Duration::from_millis(WRITE_SUPPRESS_MS))
                    .unwrap_or(false);
                if !suppressed {
                    if verbose {
                        let rel = path.strip_prefix(dir).unwrap_or(&path);
                        println!("[watch] {} ({label})", rel.display());
                    }

                    match label {
                        "rename from" => {
                            rename_from = Some((path.clone(), Instant::now()));
                        }
                        "rename to" => {
                            if let Some((old_path, ts)) = rename_from.take() {
                                // Pair found within 500ms — handle as rename
                                if ts.elapsed() < Duration::from_millis(500) {
                                    match handle_rename(dispatch, dir, &old_path, &path, verbose).await {
                                        Ok(()) => {
                                            if verbose {
                                                let old_rel = old_path.strip_prefix(dir).unwrap_or(&old_path);
                                                let new_rel = path.strip_prefix(dir).unwrap_or(&path);
                                                println!("[rename] {} -> {}", old_rel.display(), new_rel.display());
                                            }
                                        }
                                        Err(e) => warn!("Rename handling failed: {e}"),
                                    }
                                } else {
                                    // Too old, treat rename-to as a new file change
                                    pending.insert(path.clone(), Instant::now());
                                }
                            } else {
                                // No matching rename-from, treat as new file
                                pending.insert(path.clone(), Instant::now());
                            }
                        }
                        _ => {
                            pending.insert(path.clone(), Instant::now());
                        }
                    }
                }
            }
            // Debounce tick: flush changes that have been quiet for DEBOUNCE_MS
            _ = debounce_tick.tick() => {
                let now = Instant::now();
                let ready: Vec<PathBuf> = pending
                    .iter()
                    .filter(|(_, t)| now.duration_since(**t) >= Duration::from_millis(DEBOUNCE_MS))
                    .map(|(p, _)| p.clone())
                    .collect();

                if !ready.is_empty() {
                    for p in &ready {
                        pending.remove(p);
                    }
                    match push_changed_files(dispatch, dir, &ready, verbose).await {
                        Ok(n) if n > 0 => {
                            if verbose {
                                println!("[push] {n} file(s) synced to DB");
                            }
                        }
                        Err(e) => warn!("Push error: {e}"),
                        _ => {}
                    }
                }

                // Expire old suppress entries
                recently_written.retain(|_, t| t.elapsed() < Duration::from_millis(WRITE_SUPPRESS_MS * 2));

                // Expire stale rename-from (no matching rename-to within 500ms = file deleted)
                if let Some((ref old_path, ts)) = rename_from {
                    if ts.elapsed() >= Duration::from_millis(500) {
                        if verbose {
                            let rel = old_path.strip_prefix(dir).unwrap_or(old_path);
                            println!("[watch] rename-from expired (file deleted?): {}", rel.display());
                        }
                        rename_from = None;
                    }
                }
            }
            // DB poll: check for remote changes
            _ = poll_interval.tick() => {
                match pull_incremental(dispatch, dir, &mut recently_written, verbose).await {
                    Ok(n) if n > 0 => {
                        if verbose {
                            println!("[pull] {n} file(s) updated from DB");
                        }
                    }
                    Err(e) => warn!("Pull error: {e}"),
                    _ => {}
                }
            }
            // Graceful shutdown
            _ = tokio::signal::ctrl_c() => {
                println!("\nShutting down...");
                break;
            }
        }
    }

    // Cleanup
    let _ = std::fs::remove_file(&pid_path);
    info!("Daemon stopped");
    Ok(())
}

/// Stop a running sync daemon by PID file.
pub fn stop(dir: &Path) -> Result<()> {
    let pid_path = dir.join(SYNC_META_DIR).join(PID_FILE);
    if !pid_path.exists() {
        bail!(
            "No daemon PID file found at {}. Is the daemon running?",
            pid_path.display()
        );
    }

    let pid_str = std::fs::read_to_string(&pid_path)?.trim().to_string();
    let _pid: u32 = pid_str.parse().context("Invalid PID in daemon.pid")?;

    let status = std::process::Command::new("kill")
        .arg(&pid_str)
        .status()
        .context("Failed to send signal")?;

    let _ = std::fs::remove_file(&pid_path);

    if status.success() {
        println!("Stopped daemon (PID {pid_str})");
    } else {
        println!("Daemon not running (cleaned up stale PID file)");
    }
    Ok(())
}

/// Pull changes from DB with conflict detection. Returns count of files updated.
///
/// Unlike `pull()`, this detects when both local and remote have changed since last
/// sync. In that case, the local version is saved to `.nomen-fs/conflicts/` and the
/// remote version overwrites the file (last-write-wins, matching Nostr semantics).
async fn pull_incremental(
    dispatch: &DispatchFn,
    dir: &Path,
    recently_written: &mut HashMap<PathBuf, std::time::Instant>,
    verbose: bool,
) -> Result<usize> {
    let result = dispatch(
        "memory.list".to_string(),
        serde_json::json!({"limit": 10000}),
    )
    .await?;

    let memories = result["memories"]
        .as_array()
        .context("memory.list did not return memories array")?;

    let mut state = SyncState::load(dir)?;
    let mut count = 0;

    for mem in memories {
        let d_tag = match mem["d_tag"].as_str() {
            Some(d) if !d.is_empty() => d.to_string(),
            _ => continue,
        };

        let rel_path = dtag_to_path(&d_tag);
        let abs_path = dir.join(&rel_path);
        let rel_key = rel_path.to_str().unwrap_or("").to_string();

        let markdown = json_memory_to_markdown(mem);
        let remote_hash = content_hash(&markdown);
        let version = mem["version"].as_i64().unwrap_or(0);

        // Check if unchanged since last sync (and file still exists)
        if abs_path.exists() {
        if let Some(entry) = state.files.get(&rel_key) {
            if entry.content_hash == remote_hash && entry.version == version {
                continue;
            }

            // Remote metadata changed (version/updated_at) but maybe not content.
            // Compare detail body to avoid echo-pulling after a push.
            if abs_path.exists() {
                let local_content = std::fs::read_to_string(&abs_path).unwrap_or_default();
                let local_hash = content_hash(&local_content);

                if local_hash != entry.content_hash {
                    // Local also changed → conflict
                    let conflict_path = dir
                        .join(SYNC_META_DIR)
                        .join("conflicts")
                        .join(&rel_key);
                    if let Some(parent) = conflict_path.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::write(&conflict_path, &local_content)?;
                    warn!(
                        path = %rel_key,
                        "Conflict: local and remote both changed. Local saved to conflicts/"
                    );
                    if verbose {
                        println!("[conflict] {} (local saved to conflicts/)", rel_key);
                    }
                } else {
                    // Local unchanged — check if remote content actually differs
                    let remote_content = mem["content"].as_str().unwrap_or("");
                    let local_body = parse_markdown(&local_content)
                        .map(|p| p.content)
                        .unwrap_or_default();
                    if local_body == remote_content {
                        // Only metadata changed (version/updated_at), not content.
                        // Update sync state silently, no file write needed.
                        state.files.insert(
                            rel_key,
                            SyncEntry {
                                d_tag,
                                content_hash: remote_hash,
                                version,
                                synced_at: chrono::Utc::now().to_rfc3339(),
                            },
                        );
                        continue;
                    }
                }
            }
        }
        } // abs_path.exists

        // Write remote version
        if let Some(parent) = abs_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&abs_path, &markdown)
            .with_context(|| format!("Failed to write {}", abs_path.display()))?;

        if verbose {
            println!("[pull] {}", rel_key);
        }

        // Suppress watcher echo for this write
        recently_written.insert(abs_path, std::time::Instant::now());

        state.files.insert(
            rel_key,
            SyncEntry {
                d_tag,
                content_hash: remote_hash,
                version,
                synced_at: chrono::Utc::now().to_rfc3339(),
            },
        );
        count += 1;
    }

    // Delete local files for memories removed from DB
    let remote_dtags: std::collections::HashSet<String> = memories
        .iter()
        .filter_map(|m| m["d_tag"].as_str())
        .filter(|d| !d.is_empty())
        .map(|d| d.to_string())
        .collect();

    let stale: Vec<String> = state
        .files
        .iter()
        .filter(|(_, entry)| !remote_dtags.contains(&entry.d_tag))
        .map(|(key, _)| key.clone())
        .collect();

    for rel_key in &stale {
        let abs_path = dir.join(rel_key);
        if abs_path.exists() {
            std::fs::remove_file(&abs_path)?;
            if verbose {
                println!("[delete] {} (memory removed from DB)", rel_key);
            }
        }
        state.files.remove(rel_key);
        count += 1;
    }

    if count > 0 {
        state.save(dir)?;
    }
    Ok(count)
}

/// Push specific changed files to the DB. Returns count of memories updated.
/// Handle a rename/move: delete old memory, push file from new location.
async fn handle_rename(
    dispatch: &DispatchFn,
    dir: &Path,
    old_path: &Path,
    new_path: &Path,
    verbose: bool,
) -> Result<()> {
    let mut state = SyncState::load(dir)?;

    // Find the old d_tag from SyncState
    let old_rel = old_path
        .strip_prefix(dir)
        .unwrap_or(old_path)
        .to_str()
        .unwrap_or("")
        .to_string();

    if let Some(old_entry) = state.files.remove(&old_rel) {
        // Delete the old memory from DB
        let params = serde_json::json!({ "d_tag": old_entry.d_tag });
        match dispatch("memory.delete".to_string(), params).await {
            Ok(_) => {
                info!(d_tag = %old_entry.d_tag, "Deleted old memory after rename");
                if verbose {
                    println!("[rename] deleted old: {}", old_entry.d_tag);
                }
            }
            Err(e) => {
                warn!(d_tag = %old_entry.d_tag, "Failed to delete old memory: {e}");
            }
        }
        state.save(dir)?;
    }

    // Push the new file (will create memory with new path-derived d_tag)
    push_changed_files(dispatch, dir, &[new_path.to_path_buf()], verbose).await?;

    Ok(())
}

async fn push_changed_files(
    dispatch: &DispatchFn,
    dir: &Path,
    paths: &[PathBuf],
    verbose: bool,
) -> Result<usize> {
    let mut state = SyncState::load(dir)?;
    let mut count = 0;

    for abs_path in paths {
        if !abs_path.exists() {
            debug!(path = %abs_path.display(), "File deleted, skipping");
            continue;
        }

        let rel_path = abs_path
            .strip_prefix(dir)
            .unwrap_or(abs_path)
            .to_path_buf();
        let rel_key = rel_path.to_str().unwrap_or("").to_string();

        let content = match std::fs::read_to_string(abs_path) {
            Ok(c) => c,
            Err(e) => {
                warn!(path = %abs_path.display(), "Cannot read: {e}");
                continue;
            }
        };
        let hash = content_hash(&content);

        // Skip if unchanged since last sync
        if let Some(entry) = state.files.get(&rel_key) {
            if entry.content_hash == hash {
                continue;
            }
        }

        let parsed = match parse_markdown(&content) {
            Ok(p) => p,
            Err(e) => {
                debug!(path = %abs_path.display(), "Skipping file without valid frontmatter: {e}");
                continue;
            }
        };

        let d_tag = if !parsed.frontmatter.d_tag.is_empty() {
            parsed.frontmatter.d_tag.clone()
        } else {
            match path_to_dtag(&rel_path) {
                Some(dt) => dt,
                None => {
                    warn!(path = %abs_path.display(), "Cannot resolve d-tag, skipping");
                    continue;
                }
            }
        };

        let topic = if !d_tag.is_empty() {
            memory::dtag_topic(&d_tag)
                .unwrap_or(&parsed.frontmatter.topic)
                .to_string()
        } else {
            parsed.frontmatter.topic.clone()
        };

        let (visibility, scope) = if !d_tag.is_empty() {
            memory::extract_visibility_scope(&d_tag)
        } else {
            (
                parsed.frontmatter.visibility.clone(),
                parsed.frontmatter.scope.clone(),
            )
        };

        let params = serde_json::json!({
            "topic": topic,
            "content": parsed.content,
            "visibility": visibility,
            "scope": scope,
            "source": "fs-daemon",
        });

        match dispatch("memory.put".to_string(), params).await {
            Ok(result) => {
                let stored_dtag = result["d_tag"]
                    .as_str()
                    .unwrap_or(&d_tag)
                    .to_string();
                info!(d_tag = %stored_dtag, path = %rel_key, "Pushed file change");
                if verbose {
                    println!("[push] {} -> {}", rel_key, stored_dtag);
                }

                state.files.insert(
                    rel_key,
                    SyncEntry {
                        d_tag: stored_dtag,
                        content_hash: hash,
                        version: parsed.frontmatter.version + 1,
                        synced_at: chrono::Utc::now().to_rfc3339(),
                    },
                );
                count += 1;
            }
            Err(e) => {
                warn!(path = %abs_path.display(), "Push failed: {e}");
            }
        }
    }

    if count > 0 {
        state.save(dir)?;
    }
    Ok(count)
}

/// Check if a path should be watched (is an .md file, not in meta/hidden dirs).
fn is_watched_path(base: &Path, path: &Path) -> bool {
    let rel = match path.strip_prefix(base) {
        Ok(r) => r,
        Err(_) => return false,
    };

    // Skip .nomen-fs/, .obsidian/, and other hidden dirs/files
    for component in rel.components() {
        let s = component.as_os_str().to_str().unwrap_or("");
        if s.starts_with('.') || s == SYNC_META_DIR {
            return false;
        }
    }

    path.extension().map(|e| e == "md").unwrap_or(false)
}

// ── Helpers ──────────────────────────────────────────────────────────

/// Walk all .md files in dir, excluding .nomen-fs/ and .obsidian/.
fn walk_md_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    walk_dir_recursive(dir, dir, &mut files)?;
    Ok(files)
}

fn walk_dir_recursive(base: &Path, current: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    let entries = std::fs::read_dir(current)
        .with_context(|| format!("Cannot read directory: {}", current.display()))?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_str().unwrap_or("");

        // Skip hidden/meta dirs
        if name_str.starts_with('.') || name_str == SYNC_META_DIR {
            continue;
        }

        if path.is_dir() {
            walk_dir_recursive(base, &path, files)?;
        } else if path.extension().map(|e| e == "md").unwrap_or(false) {
            files.push(path);
        }
    }

    Ok(())
}
