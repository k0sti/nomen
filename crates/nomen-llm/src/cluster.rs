use std::collections::HashMap;

use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use surrealdb::engine::local::Db;
use surrealdb::Surreal;
use tracing::{debug, info, warn};

use nomen_core::embed::Embedder;
use nomen_relay::RelayManager;

/// Configuration for the cluster fusion pipeline.
pub struct ClusterConfig {
    /// Minimum memories per cluster to trigger synthesis (default: 3).
    pub min_members: usize,
    /// How deep to group by namespace prefix (default: 2, e.g. "user/k0").
    pub namespace_depth: usize,
    /// LLM provider for cluster synthesis.
    pub llm_provider: Box<dyn ClusterLlmProvider>,
    /// If true, preview clusters without storing anything.
    pub dry_run: bool,
    /// Only process clusters matching this prefix (e.g. "user/").
    pub prefix_filter: Option<String>,
    /// Author pubkey hex for d-tag construction.
    pub author_pubkey: Option<String>,
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            min_members: 3,
            namespace_depth: 2,
            llm_provider: Box::new(NoopClusterLlmProvider),
            dry_run: false,
            prefix_filter: None,
            author_pubkey: None,
        }
    }
}

/// A synthesized cluster summary from the LLM.
#[derive(Debug, Clone)]
pub struct ClusterSynthesis {
    pub content: String,
}

/// Trait for LLM-powered cluster synthesis.
#[async_trait]
pub trait ClusterLlmProvider: Send + Sync {
    /// Synthesize a coherent summary from a set of related memory summaries.
    async fn synthesize_cluster(&self, prefix: &str, context: &str) -> Result<ClusterSynthesis>;
}

/// Noop implementation — concatenates member summaries without LLM call.
pub struct NoopClusterLlmProvider;

#[async_trait]
impl ClusterLlmProvider for NoopClusterLlmProvider {
    async fn synthesize_cluster(&self, prefix: &str, context: &str) -> Result<ClusterSynthesis> {
        Ok(ClusterSynthesis {
            content: format!("Cluster summary for {prefix}\n\n{context}"),
        })
    }
}

/// OpenAI/OpenRouter-compatible LLM provider for cluster synthesis.
pub struct OpenAiClusterLlmProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
}

impl OpenAiClusterLlmProvider {
    pub fn new(base_url: &str, api_key: &str, model: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
        }
    }

    /// Create from consolidation LLM config, returning None if API key is missing.
    pub fn from_config(config: &nomen_core::config::ConsolidationLlmConfig) -> Option<Self> {
        let api_key = std::env::var(&config.api_key_env).unwrap_or_default();
        if api_key.is_empty() {
            warn!(
                "Cluster API key env {} not set, will use NoopClusterLlmProvider",
                config.api_key_env
            );
            return None;
        }

        let base_url = config
            .base_url
            .clone()
            .unwrap_or_else(|| match config.provider.as_str() {
                "openai" => "https://api.openai.com/v1".to_string(),
                "openrouter" => "https://openrouter.ai/api/v1".to_string(),
                _ => "https://openrouter.ai/api/v1".to_string(),
            });

        Some(Self::new(&base_url, &api_key, &config.model))
    }
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Deserialize)]
struct ChatMessage {
    content: String,
}

#[derive(Deserialize)]
struct LlmClusterOutput {
    content: String,
}

#[async_trait]
impl ClusterLlmProvider for OpenAiClusterLlmProvider {
    async fn synthesize_cluster(&self, prefix: &str, context: &str) -> Result<ClusterSynthesis> {
        let system_prompt =
            "You are a memory synthesis agent. Given a collection of related memories \
grouped under the same topic namespace, produce a coherent, comprehensive summary that captures \
the key information across all of them. \
Return JSON with this exact structure: {\"content\": \"comprehensive synthesis as plain text\"}. \
The content should weave together all the information into a coherent narrative, \
resolving any contradictions and noting important relationships.";

        let user_prompt = format!(
            "Synthesize these related memories under the namespace \"{prefix}\":\n\n{context}\n\n\
             Produce a single coherent summary covering all the information."
        );

        let body = serde_json::json!({
            "model": self.model,
            "messages": [
                { "role": "system", "content": system_prompt },
                { "role": "user", "content": user_prompt }
            ],
            "temperature": 0.3,
            "response_format": { "type": "json_object" }
        });

        let url = format!("{}/chat/completions", self.base_url);
        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("LLM API error {status}: {text}");
        }

        let chat_resp: ChatResponse = resp.json().await?;
        let content = chat_resp
            .choices
            .first()
            .map(|c| c.message.content.as_str())
            .unwrap_or("{}");

        let output: LlmClusterOutput = serde_json::from_str(content).unwrap_or_else(|e| {
            warn!("Failed to parse LLM cluster response as JSON: {e}");
            LlmClusterOutput {
                content: format!("Cluster summary for {prefix}\n\n{context}"),
            }
        });

        Ok(ClusterSynthesis {
            content: output.content,
        })
    }
}

/// Report from a cluster fusion run.
#[derive(Debug, Default)]
pub struct ClusterReport {
    /// Total named memories scanned.
    pub memories_scanned: usize,
    /// Number of clusters identified (meeting min_members threshold).
    pub clusters_found: usize,
    /// Number of cluster summaries created or updated.
    pub clusters_synthesized: usize,
    /// Number of "summarizes" edges created.
    pub edges_created: usize,
    /// Whether this was a dry run.
    pub dry_run: bool,
    /// Details of each cluster for reporting.
    pub cluster_details: Vec<ClusterDetail>,
}

/// Detail about a single cluster for reporting.
#[derive(Debug)]
pub struct ClusterDetail {
    pub prefix: String,
    pub member_count: usize,
    pub member_topics: Vec<String>,
}

/// A memory record with the fields we need for clustering.
#[derive(Debug, Clone)]
struct ClusterableMemory {
    topic: String,
    d_tag: String,
    detail: String,
    tier: String,
}

/// Group memories by their topic namespace prefix at the given depth.
///
/// For depth=2, "user/k0/preferences" -> "user/k0"
/// For depth=1, "user/k0/preferences" -> "user"
fn group_by_namespace(
    memories: &[ClusterableMemory],
    depth: usize,
) -> HashMap<String, Vec<&ClusterableMemory>> {
    let mut groups: HashMap<String, Vec<&ClusterableMemory>> = HashMap::new();

    for mem in memories {
        let parts: Vec<&str> = mem.topic.split('/').collect();
        if parts.len() <= depth {
            // Topic is too shallow to cluster at this depth — skip it
            continue;
        }
        let prefix = parts[..depth].join("/");
        groups.entry(prefix).or_default().push(mem);
    }

    groups
}

/// Derive the cluster tier from its member memories.
///
/// Uses the most restrictive tier among members:
/// private > personal > group > public
fn derive_cluster_tier(members: &[&ClusterableMemory]) -> String {
    let has_private = members
        .iter()
        .any(|m| m.tier == "private" || m.tier == "internal");
    let has_personal = members.iter().any(|m| m.tier == "personal");
    let has_group = members.iter().any(|m| m.tier.starts_with("group"));

    if has_private {
        "private".to_string()
    } else if has_personal {
        "personal".to_string()
    } else if has_group {
        // Use the first group tier found
        members
            .iter()
            .find(|m| m.tier.starts_with("group"))
            .map(|m| m.tier.clone())
            .unwrap_or_else(|| "group".to_string())
    } else {
        "public".to_string()
    }
}

/// Run the cluster fusion pipeline.
///
/// 1. Query all named memories, group by namespace prefix
/// 2. For each cluster with >= min_members:
///    a. Build context from member summaries + details
///    b. LLM synthesis to produce a coherent cluster summary
///    c. Store as cluster memory with topic "cluster/<prefix>"
///    d. Create `references` edges (relation: "summarizes") from cluster -> source memories
/// 3. Cluster memories are replaceable (refresh on next run via d-tag)
pub async fn run_cluster_fusion(
    db: &Surreal<Db>,
    embedder: &dyn Embedder,
    config: &ClusterConfig,
    _relay: Option<&RelayManager>,
) -> Result<ClusterReport> {
    let mut report = ClusterReport {
        dry_run: config.dry_run,
        ..Default::default()
    };

    // 1. Query all named memories
    let all_memories = nomen_db::list_memories(db, None, 10000).await?;
    report.memories_scanned = all_memories.len();

    if all_memories.is_empty() {
        info!("No memories found for cluster fusion");
        return Ok(report);
    }

    // Convert to clusterable format, filtering out existing cluster memories
    let clusterable: Vec<ClusterableMemory> = all_memories
        .into_iter()
        .filter_map(|m| {
            // Skip memories that are already cluster summaries
            if m.topic.starts_with("cluster/") {
                return None;
            }
            // Skip memories with no meaningful topic path
            if m.topic.is_empty() || !m.topic.contains('/') {
                return None;
            }
            Some(ClusterableMemory {
                topic: m.topic.clone(),
                d_tag: m.d_tag.unwrap_or_default(),
                detail: m.content,
                tier: m.tier,
            })
        })
        .collect();

    debug!(count = clusterable.len(), "Clusterable memories found");

    // 2. Group by namespace prefix
    let clusters = group_by_namespace(&clusterable, config.namespace_depth);
    debug!(groups = clusters.len(), "Namespace groups found");

    // 3. Process each cluster
    for (prefix, members) in &clusters {
        // Apply prefix filter if set
        if let Some(ref filter) = config.prefix_filter {
            if !prefix.starts_with(filter.trim_end_matches('/')) {
                continue;
            }
        }

        if members.len() < config.min_members {
            debug!(
                prefix = %prefix,
                count = members.len(),
                min = config.min_members,
                "Skipping cluster below threshold"
            );
            continue;
        }

        report.clusters_found += 1;

        let detail = ClusterDetail {
            prefix: prefix.clone(),
            member_count: members.len(),
            member_topics: members.iter().map(|m| m.topic.clone()).collect(),
        };
        report.cluster_details.push(detail);

        if config.dry_run {
            continue;
        }

        // Build context from member summaries
        let context: String = members
            .iter()
            .map(|m| {
                let detail_preview = if m.detail.len() > 200 {
                    format!("{}...", &m.detail[..200])
                } else {
                    m.detail.clone()
                };
                format!("- [{}]\n  {}", m.topic, detail_preview)
            })
            .collect::<Vec<_>>()
            .join("\n");

        // LLM synthesis
        let synthesis = match config
            .llm_provider
            .synthesize_cluster(prefix, &context)
            .await
        {
            Ok(s) => s,
            Err(e) => {
                warn!(prefix = %prefix, "Cluster synthesis failed: {e}");
                continue;
            }
        };

        // Determine tier from members
        let tier = derive_cluster_tier(members);
        let cluster_topic = format!("cluster/{prefix}");
        let author_hex = config.author_pubkey.as_deref().unwrap_or("");

        // Store as cluster memory using store_direct
                let mem = nomen_core::NewMemory {
            memory_type: Some("cluster".to_string()),
            topic: cluster_topic.clone(),
            content: synthesis.content,
            tier,
            importance: None,
            source: Some("cluster_fusion".to_string()),
            model: Some("nomen/cluster".to_string()),
        };

        let d_tag =
            match crate::store::store_direct_with_author(db, embedder, mem, author_hex).await {
                Ok(dt) => dt,
                Err(e) => {
                    warn!(prefix = %prefix, "Failed to store cluster memory: {e}");
                    continue;
                }
            };

        report.clusters_synthesized += 1;

        // Create "summarizes" edges from cluster memory to each source memory
        for member in members {
            if member.d_tag.is_empty() {
                continue;
            }
            match nomen_db::create_references_edge(db, &d_tag, &member.d_tag, "summarizes").await {
                Ok(_) => {
                    report.edges_created += 1;
                }
                Err(e) => {
                    debug!(
                        from = %d_tag,
                        to = %member.d_tag,
                        "Failed to create summarizes edge: {e}"
                    );
                }
            }
        }

        info!(
            prefix = %prefix,
            members = members.len(),
            d_tag = %d_tag,
            "Cluster memory synthesized"
        );
    }

    if config.dry_run {
        info!(
            clusters = report.clusters_found,
            scanned = report.memories_scanned,
            "Dry run complete"
        );
    } else {
        info!(
            synthesized = report.clusters_synthesized,
            edges = report.edges_created,
            scanned = report.memories_scanned,
            "Cluster fusion complete"
        );
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_group_by_namespace_depth_2() {
        let memories = vec![
            ClusterableMemory {
                topic: "user/k0/preferences".to_string(),
                d_tag: "d1".to_string(),
                summary: "prefs".to_string(),
                detail: "detail".to_string(),
                tier: "public".to_string(),
            },
            ClusterableMemory {
                topic: "user/k0/timezone".to_string(),
                d_tag: "d2".to_string(),
                summary: "tz".to_string(),
                detail: "detail".to_string(),
                tier: "public".to_string(),
            },
            ClusterableMemory {
                topic: "user/k0/projects".to_string(),
                d_tag: "d3".to_string(),
                summary: "projects".to_string(),
                detail: "detail".to_string(),
                tier: "public".to_string(),
            },
            ClusterableMemory {
                topic: "project/nomen/architecture".to_string(),
                d_tag: "d4".to_string(),
                summary: "arch".to_string(),
                detail: "detail".to_string(),
                tier: "public".to_string(),
            },
            ClusterableMemory {
                topic: "shallow".to_string(),
                d_tag: "d5".to_string(),
                summary: "shallow".to_string(),
                detail: "detail".to_string(),
                tier: "public".to_string(),
            },
        ];

        let groups = group_by_namespace(&memories, 2);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups["user/k0"].len(), 3);
        assert_eq!(groups["project/nomen"].len(), 1);
        // "shallow" has no '/' at depth 2, so it's excluded
        assert!(!groups.contains_key("shallow"));
    }

    #[test]
    fn test_group_by_namespace_depth_1() {
        let memories = vec![
            ClusterableMemory {
                topic: "user/k0/preferences".to_string(),
                d_tag: "d1".to_string(),
                summary: "prefs".to_string(),
                detail: "detail".to_string(),
                tier: "public".to_string(),
            },
            ClusterableMemory {
                topic: "user/k0/timezone".to_string(),
                d_tag: "d2".to_string(),
                summary: "tz".to_string(),
                detail: "detail".to_string(),
                tier: "public".to_string(),
            },
            ClusterableMemory {
                topic: "project/nomen/architecture".to_string(),
                d_tag: "d3".to_string(),
                summary: "arch".to_string(),
                detail: "detail".to_string(),
                tier: "public".to_string(),
            },
        ];

        let groups = group_by_namespace(&memories, 1);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups["user"].len(), 2);
        assert_eq!(groups["project"].len(), 1);
    }

    #[test]
    fn test_derive_cluster_tier() {
        let public = ClusterableMemory {
            topic: "t".to_string(),
            d_tag: "d".to_string(),
            summary: "s".to_string(),
            detail: "d".to_string(),
            tier: "public".to_string(),
        };
        let personal = ClusterableMemory {
            tier: "personal".to_string(),
            ..public.clone()
        };
        let private = ClusterableMemory {
            tier: "private".to_string(),
            ..public.clone()
        };

        // All public -> public
        assert_eq!(derive_cluster_tier(&[&public, &public]), "public");
        // Mixed with personal -> personal
        assert_eq!(derive_cluster_tier(&[&public, &personal]), "personal");
        // Mixed with private -> private (most restrictive)
        assert_eq!(
            derive_cluster_tier(&[&public, &personal, &private]),
            "private"
        );
    }
}
