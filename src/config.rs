use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Deserialize;

/// Group configuration entry from config.toml [[groups]] section.
#[derive(Debug, Clone, Deserialize)]
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

#[derive(Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub relay: Option<String>,
    #[serde(default)]
    pub nsecs: Vec<String>,
    /// Single nsec shorthand
    #[serde(default)]
    pub nsec: Option<String>,
    /// Embedding provider configuration
    #[serde(default)]
    pub embedding: Option<EmbeddingConfig>,
    /// Group definitions
    #[serde(default)]
    pub groups: Vec<GroupConfig>,
    /// Consolidation LLM configuration
    #[serde(default)]
    pub consolidation: Option<ConsolidationLlmConfig>,
    /// Messaging configuration
    #[serde(default)]
    pub messaging: Option<MessagingConfig>,
}

#[derive(Deserialize, Clone)]
pub struct MessagingConfig {
    /// Default delivery channel (e.g. "nostr", "telegram"). Defaults to "nostr".
    #[serde(default = "default_messaging_channel")]
    pub default_channel: String,
}

fn default_messaging_channel() -> String {
    "nostr".to_string()
}

#[derive(Deserialize, Clone)]
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

#[derive(Deserialize, Clone)]
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

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            model: default_model(),
            api_key_env: default_api_key_env(),
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

    /// Build an embedder from config. Returns NoopEmbedder if no config or no API key.
    pub fn build_embedder(&self) -> Box<dyn crate::embed::Embedder> {
        let Some(ref emb) = self.embedding else {
            return Box::new(crate::embed::NoopEmbedder);
        };

        let api_key = std::env::var(&emb.api_key_env).unwrap_or_default();
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
}
