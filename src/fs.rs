//! Filesystem sync — bidirectional mapping between Nomen memories and local markdown files.
//!
//! Each memory is a markdown file with YAML frontmatter:
//! ```markdown
//! ---
//! d_tag: group/telegram:-1003821690204/room/8485
//! topic: room/8485
//! visibility: group
//! scope: telegram:-1003821690204
//! version: 3
//! pinned: true
//! created_at: 2026-03-19T16:00:00Z
//! updated_at: 2026-03-19T18:00:00Z
//! ---
//! Detail content here (markdown).
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
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub importance: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

/// A parsed markdown memory file.
pub struct ParsedMemoryFile {
    pub frontmatter: MemoryFrontmatter,
    pub detail: String,
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
    let topic = memory::v2_dtag_topic(d_tag).unwrap_or(d_tag);

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
        "public" | "group" | "circle" | "personal" | "internal"
    ) {
        return None;
    }

    let rest = components.next().unwrap_or("");

    if visibility == "public" {
        // public/{topic}
        if rest.is_empty() {
            return None;
        }
        return Some(memory::build_v2_dtag("public", "", rest));
    }

    // For other visibilities: {scope}/{topic}
    // Scope is the first component of rest
    let mut parts = rest.splitn(2, '/');
    let scope = parts.next().unwrap_or("");
    let topic = parts.next();

    if scope.is_empty() {
        return None;
    }

    match topic {
        Some(t) if !t.is_empty() => Some(memory::build_v2_dtag(visibility, scope, t)),
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
        pinned: mem.pinned,
        importance: mem.importance,
        created_at: mem.created_at.clone(),
        updated_at: mem.updated_at.clone(),
    };

    let detail = mem.detail.as_deref().unwrap_or("");
    frontmatter_to_markdown(&fm, detail)
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
        pinned: val["pinned"].as_bool().unwrap_or(false),
        importance: val["importance"].as_i64(),
        created_at: val["created_at"].as_str().unwrap_or("").to_string(),
        updated_at: val["updated_at"].as_str().unwrap_or("").to_string(),
    };

    let detail = val["detail"].as_str().unwrap_or("");

    frontmatter_to_markdown(&fm, detail)
}

fn frontmatter_to_markdown(fm: &MemoryFrontmatter, detail: &str) -> String {
    let yaml = serde_yaml_ng::to_string(fm).unwrap_or_default();
    // serde_yaml_ng adds a trailing newline; trim it so the --- is clean
    let yaml = yaml.trim_end();

    let mut out = String::new();
    out.push_str("---\n");
    out.push_str(yaml);
    out.push_str("\n---\n");
    out.push_str(detail);
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

    // The entire body after frontmatter is the detail
    let detail = body.trim().to_string();

    Ok(ParsedMemoryFile {
        frontmatter,
        detail,
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
    for tier in &["public", "group", "personal", "internal"] {
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

        // Check if file already exists with same content
        if let Some(entry) = state.files.get(rel_path.to_str().unwrap_or("")) {
            if entry.content_hash == hash && entry.version == version {
                debug!(d_tag = %d_tag, "Skipping (unchanged)");
                continue;
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

        // Parse the markdown file
        let parsed = match parse_markdown(&content) {
            Ok(p) => p,
            Err(e) => {
                warn!(path = %abs_path.display(), "Skipping invalid file: {e}");
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

        // Extract clean topic from d-tag (handles both old colon and new slash formats)
        let topic = if !d_tag.is_empty() {
            memory::v2_dtag_topic(&d_tag)
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

        let mut params = serde_json::json!({
            "topic": topic,
            "detail": parsed.detail,
            "visibility": visibility,
            "scope": scope,
            "source": "fs-push",
        });

        if parsed.frontmatter.pinned {
            params["pinned"] = serde_json::json!(true);
        }

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
