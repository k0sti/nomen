use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Group configuration entry from config.toml [[groups]] section.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GroupConfig {
    /// Dot-separated hierarchical id, e.g. "atlantislabs.engineering"
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Member npubs
    #[serde(default)]
    pub members: Vec<String>,
    /// NIP-29 group id mapping (h-tag value on relay)
    #[serde(default)]
    pub nostr_group: Option<String>,
    /// Relay URL for this group (overrides default)
    #[serde(default)]
    pub relay: Option<String>,
}

#[derive(Clone, Deserialize, Serialize, Default)]
pub struct Config {
    #[serde(default)]
    pub relay: Option<String>,
    #[serde(default)]
    pub nsecs: Vec<String>,
    /// Single nsec shorthand
    #[serde(default)]
    pub nsec: Option<String>,
    /// Default writer identity: "guardian" or "agent:N"
    #[serde(default)]
    pub default_writer: Option<String>,
    /// Embedding provider configuration
    #[serde(default)]
    pub embedding: Option<EmbeddingConfig>,
    /// Group definitions
    #[serde(default)]
    pub groups: Vec<GroupConfig>,
    /// Consolidation LLM configuration (top-level [consolidation] — backward compat)
    #[serde(default)]
    pub consolidation: Option<ConsolidationLlmConfig>,
    /// Memory section with nested consolidation (spec-compliant [memory.consolidation])
    #[serde(default)]
    pub memory: Option<MemorySection>,
    /// Messaging configuration
    #[serde(default)]
    pub messaging: Option<MessagingConfig>,
    /// HTTP server configuration
    #[serde(default)]
    pub server: Option<ServerConfig>,
    /// Entity extraction LLM configuration
    #[serde(default)]
    pub entities: Option<EntityExtractionConfig>,
}

/// The [memory] config section, per spec.
#[derive(Deserialize, Serialize, Clone, Default)]
pub struct MemorySection {
    /// Consolidation settings per spec: [memory.consolidation]
    #[serde(default)]
    pub consolidation: Option<MemoryConsolidationConfig>,
    /// Cluster fusion settings: [memory.cluster]
    #[serde(default)]
    pub cluster: Option<MemoryClusterConfig>,
}

/// Cluster fusion config matching the spec [memory.cluster] section.
#[derive(Deserialize, Serialize, Clone)]
pub struct MemoryClusterConfig {
    /// Whether cluster fusion is enabled
    #[serde(default)]
    pub enabled: bool,
    /// Minimum memories per cluster to trigger synthesis
    #[serde(default = "default_cluster_min_members")]
    pub min_members: usize,
    /// How deep to group by namespace prefix (e.g. 2 for "user/k0")
    #[serde(default = "default_cluster_namespace_depth")]
    pub namespace_depth: usize,
    /// Run cluster fusion every N hours
    #[serde(default = "default_cluster_interval_hours")]
    pub interval_hours: u32,
}

fn default_cluster_min_members() -> usize {
    3
}

fn default_cluster_namespace_depth() -> usize {
    2
}

fn default_cluster_interval_hours() -> u32 {
    24
}

/// Full consolidation config matching the spec [memory.consolidation] section.
#[derive(Deserialize, Serialize, Clone)]
pub struct MemoryConsolidationConfig {
    /// Whether consolidation is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Run consolidation every N hours
    #[serde(default = "default_interval_hours")]
    pub interval_hours: u32,
    /// Consolidate messages older than N minutes
    #[serde(default = "default_ephemeral_ttl_minutes")]
    pub ephemeral_ttl_minutes: u32,
    /// Force consolidation above this count
    #[serde(default = "default_max_ephemeral_count")]
    pub max_ephemeral_count: usize,
    /// Dry run mode
    #[serde(default)]
    pub dry_run: bool,
    /// LLM provider (inlined, or fall back to top-level [consolidation])
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
}

fn default_true() -> bool {
    true
}
fn default_interval_hours() -> u32 {
    4
}
fn default_ephemeral_ttl_minutes() -> u32 {
    60
}
fn default_max_ephemeral_count() -> usize {
    200
}

#[derive(Deserialize, Serialize, Clone)]
pub struct MessagingConfig {
    /// Default delivery channel (e.g. "nostr", "telegram"). Defaults to "nostr".
    #[serde(default = "default_messaging_channel")]
    pub default_channel: String,
}

fn default_messaging_channel() -> String {
    "nostr".to_string()
}

/// LLM provider config for consolidation (top-level [consolidation] section).
#[derive(Deserialize, Serialize, Clone)]
pub struct ConsolidationLlmConfig {
    /// Provider: "openrouter", "openai", or "none"
    #[serde(default = "default_consolidation_provider")]
    pub provider: String,
    /// Model name (e.g. "anthropic/claude-sonnet-4-6")
    #[serde(default = "default_consolidation_model")]
    pub model: String,
    /// Environment variable name containing the API key
    #[serde(default = "default_consolidation_api_key_env")]
    pub api_key_env: String,
    /// Base URL override
    #[serde(default)]
    pub base_url: Option<String>,
}

fn default_consolidation_provider() -> String {
    "openrouter".to_string()
}

fn default_consolidation_model() -> String {
    "anthropic/claude-sonnet-4-6".to_string()
}

fn default_consolidation_api_key_env() -> String {
    "OPENROUTER_API_KEY".to_string()
}

#[derive(Deserialize, Serialize, Clone)]
pub struct EmbeddingConfig {
    /// Provider: "openai", "openrouter", or "none"
    #[serde(default = "default_provider")]
    pub provider: String,
    /// Model name
    #[serde(default = "default_model")]
    pub model: String,
    /// Environment variable name containing the API key
    #[serde(default = "default_api_key_env")]
    pub api_key_env: String,
    /// Direct API key passthrough (avoids env var mutation in multi-threaded contexts).
    /// Takes precedence over `api_key_env` when set.
    #[serde(default, skip_serializing)]
    pub api_key: Option<String>,
    /// Base URL override (optional, provider default used if absent)
    #[serde(default)]
    pub base_url: Option<String>,
    /// Embedding dimensions
    #[serde(default = "default_dimensions")]
    pub dimensions: usize,
    /// Batch size for embedding requests
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
}

fn default_provider() -> String {
    "openai".to_string()
}

fn default_model() -> String {
    "text-embedding-3-small".to_string()
}

fn default_api_key_env() -> String {
    "OPENAI_API_KEY".to_string()
}

fn default_dimensions() -> usize {
    1536
}

fn default_batch_size() -> usize {
    100
}

/// HTTP server configuration [server] section.
#[derive(Deserialize, Serialize, Clone)]
pub struct ServerConfig {
    /// Whether the HTTP server is enabled
    #[serde(default)]
    pub enabled: bool,
    /// Listen address (e.g. "127.0.0.1:3000")
    #[serde(default = "default_listen")]
    pub listen: String,
}

fn default_listen() -> String {
    "127.0.0.1:3000".to_string()
}

/// Entity extraction LLM configuration [entities] section.
#[derive(Deserialize, Serialize, Clone)]
pub struct EntityExtractionConfig {
    /// Provider: "openrouter", "openai", "heuristic", or "none"
    #[serde(default = "default_entity_provider")]
    pub provider: String,
    /// Model name (e.g. "anthropic/claude-sonnet-4-6")
    #[serde(default = "default_entity_model")]
    pub model: String,
    /// Environment variable name containing the API key
    #[serde(default = "default_entity_api_key_env")]
    pub api_key_env: String,
    /// Base URL override
    #[serde(default)]
    pub base_url: Option<String>,
}

fn default_entity_provider() -> String {
    "openrouter".to_string()
}

fn default_entity_model() -> String {
    "anthropic/claude-sonnet-4-6".to_string()
}

fn default_entity_api_key_env() -> String {
    "OPENROUTER_API_KEY".to_string()
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            model: default_model(),
            api_key_env: default_api_key_env(),
            api_key: None,
            base_url: None,
            dimensions: default_dimensions(),
            batch_size: default_batch_size(),
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
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

    pub fn path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("nomen")
            .join("config.toml")
    }

    /// Merge nsec + nsecs into a single list
    pub fn all_nsecs(&self) -> Vec<String> {
        let mut out = self.nsecs.clone();
        if let Some(ref single) = self.nsec {
            if !out.contains(single) {
                out.insert(0, single.clone());
            }
        }
        out
    }

    /// Resolve the effective consolidation LLM config.
    ///
    /// Checks [memory.consolidation] first (spec-compliant), falls back to
    /// top-level [consolidation] (backward compat).
    pub fn consolidation_llm_config(&self) -> Option<ConsolidationLlmConfig> {
        // Try [memory.consolidation] first
        if let Some(ref mem) = self.memory {
            if let Some(ref mc) = mem.consolidation {
                if mc.provider.is_some() || mc.model.is_some() || mc.api_key_env.is_some() {
                    return Some(ConsolidationLlmConfig {
                        provider: mc
                            .provider
                            .clone()
                            .unwrap_or_else(default_consolidation_provider),
                        model: mc.model.clone().unwrap_or_else(default_consolidation_model),
                        api_key_env: mc
                            .api_key_env
                            .clone()
                            .unwrap_or_else(default_consolidation_api_key_env),
                        base_url: mc.base_url.clone(),
                    });
                }
            }
        }

        // Fall back to top-level [consolidation]
        self.consolidation.clone()
    }

    /// Get the embedding dimensions from config (defaults to 1536).
    pub fn embedding_dimensions(&self) -> usize {
        self.embedding
            .as_ref()
            .map(|e| e.dimensions)
            .unwrap_or(1536)
    }

    /// Build an embedder from config. Returns NoopEmbedder if no config or no API key.
    pub fn build_embedder(&self) -> Box<dyn crate::embed::Embedder> {
        let Some(ref emb) = self.embedding else {
            return Box::new(crate::embed::NoopEmbedder);
        };

        let api_key = emb
            .api_key
            .clone()
            .unwrap_or_else(|| std::env::var(&emb.api_key_env).unwrap_or_default());
        if api_key.is_empty() {
            tracing::warn!(
                "Embedding API key env {} not set, using NoopEmbedder",
                emb.api_key_env
            );
            return Box::new(crate::embed::NoopEmbedder);
        }

        crate::embed::create_embedder(
            &emb.provider,
            emb.base_url.as_deref(),
            &api_key,
            &emb.model,
            emb.dimensions,
            emb.batch_size,
        )
    }

    /// Build a signer from the first nsec in config.
    ///
    /// Returns `None` if no nsec is configured (library callers provide their own signer).
    pub fn build_signer(&self) -> Option<std::sync::Arc<dyn crate::signer::NomenSigner>> {
        let nsecs = self.all_nsecs();
        let nsec = nsecs.first()?;
        let keys = nostr_sdk::Keys::parse(nsec).ok()?;
        Some(std::sync::Arc::new(crate::signer::KeysSigner::new(keys)))
    }
}
