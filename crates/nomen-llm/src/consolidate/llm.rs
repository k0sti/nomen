use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use tracing::warn;

use super::grouping::derive_topic_from_messages;
use super::types::{ConsolidationMessage, ExtractedMemory};

/// Trait for LLM-powered consolidation. Implementations call an LLM to
/// summarize and extract structured memories from canonical consolidation messages.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn consolidate(&self, messages: &[ConsolidationMessage]) -> Result<Vec<ExtractedMemory>>;

    /// Merge new information into an existing memory.
    /// Default implementation just returns the new extraction as-is.
    async fn merge(
        &self,
        existing_content: &str,
        messages: &[ConsolidationMessage],
    ) -> Result<Vec<ExtractedMemory>> {
        // Default: just consolidate the new messages (no merge logic)
        let _ = existing_content;
        self.consolidate(messages).await
    }
}

/// Noop LLM provider — creates a simple summary from message content
/// without calling any external service.
pub struct NoopLlmProvider;

#[async_trait]
impl LlmProvider for NoopLlmProvider {
    async fn consolidate(&self, messages: &[ConsolidationMessage]) -> Result<Vec<ExtractedMemory>> {
        if messages.is_empty() {
            return Ok(vec![]);
        }

        // Derive a semantic topic from the group key
        let topic = derive_topic_from_messages(messages);

        let content_lines: Vec<String> = messages
            .iter()
            .map(|m| format!("[{}] {}: {}", m.created_at, m.sender, m.content))
            .collect();
        let content = content_lines.join("\n");

        Ok(vec![ExtractedMemory {
            content,
            topic,
            importance: Some(5),
            contradicts_existing: false,
        }])
    }
}

/// OpenAI/OpenRouter-compatible LLM provider for real consolidation.
pub struct OpenAiLlmProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
}

impl OpenAiLlmProvider {
    pub fn new(base_url: &str, api_key: &str, model: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
        }
    }

    /// Create from config, returning None if API key is missing.
    pub fn from_config(config: &nomen_core::config::ConsolidationLlmConfig) -> Option<Self> {
        let api_key = std::env::var(&config.api_key_env).unwrap_or_default();
        if api_key.is_empty() {
            warn!(
                "Consolidation API key env {} not set, will use NoopLlmProvider",
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
struct LlmExtracted {
    memories: Vec<LlmMemory>,
}

#[derive(Deserialize)]
struct LlmMemory {
    topic: String,
    content: String,
    #[serde(default)]
    importance: Option<i32>,
    #[serde(default)]
    contradicts_existing: Option<bool>,
}

#[async_trait]
impl LlmProvider for OpenAiLlmProvider {
    async fn merge(
        &self,
        existing_content: &str,
        messages: &[ConsolidationMessage],
    ) -> Result<Vec<ExtractedMemory>> {
        if messages.is_empty() {
            return Ok(vec![]);
        }

        let mut transcript = String::new();
        for msg in messages {
            let container = if msg.container.is_empty() {
                "general".to_string()
            } else {
                msg.container.clone()
            };
            transcript.push_str(&format!(
                "[{}] #{} {}: {}\n",
                msg.created_at, container, msg.sender, msg.content
            ));
        }

        let system_prompt = "You are a memory consolidation agent. You are merging new information \
into an existing memory. Return JSON with this exact structure: {\"memories\": [{\"topic\": \"category/subcategory\", \
\"content\": \"full memory content\", \"importance\": 7, \
\"contradicts_existing\": false}]}. \
Merge the new information into the existing memory. Update what changed. Keep what's still true. \
Set contradicts_existing to true if the new information directly contradicts facts in the existing memory. \
Set importance 1-10: 1=trivial, 5=normal, 8=important decision, 10=critical fact. \
The topic should remain the same as the existing memory's topic.";

        let user_prompt = format!(
            "Existing memory:\n{existing_content}\n\n\
             New messages:\n{transcript}\n\n\
             Merge the new information into the existing memory."
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

        let extracted: LlmExtracted = serde_json::from_str(content).unwrap_or_else(|e| {
            warn!("Failed to parse LLM merge response as JSON: {e}");
            LlmExtracted { memories: vec![] }
        });

        Ok(extracted
            .memories
            .into_iter()
            .map(|m| ExtractedMemory {
                content: m.content,
                topic: m.topic,
                importance: m.importance.map(|i| i.clamp(1, 10)),
                contradicts_existing: m.contradicts_existing.unwrap_or(false),
            })
            .collect())
    }

    async fn consolidate(&self, messages: &[ConsolidationMessage]) -> Result<Vec<ExtractedMemory>> {
        if messages.is_empty() {
            return Ok(vec![]);
        }

        // Build message transcript
        let mut transcript = String::new();
        for msg in messages {
            let container = if msg.container.is_empty() {
                "general".to_string()
            } else {
                msg.container.clone()
            };
            transcript.push_str(&format!(
                "[{}] #{} {}: {}\n",
                msg.created_at, container, msg.sender, msg.content
            ));
        }

        let system_prompt = "You are a memory consolidation agent. Given a batch of collected conversation messages, \
extract significant facts, decisions, and context into structured memories. \
Return JSON with this exact structure: {\"memories\": [{\"topic\": \"category/subcategory\", \
\"content\": \"full memory content\", \"importance\": 7}]}. \
Use semantic topic names following this convention: \
- user/<name>/<aspect> for per-user knowledge (preferences, timezone, projects) \
- project/<name>/<aspect> for project knowledge \
- group/<id>/<aspect> for group context \
- fact/<domain>/<topic> for general knowledge \
Only extract genuinely significant information. \
Set importance 1-10: 1=trivial, 5=normal, 8=important decision, 10=critical fact. \
Return an empty memories array if nothing significant is found.";

        let body = serde_json::json!({
            "model": self.model,
            "messages": [
                { "role": "system", "content": system_prompt },
                { "role": "user", "content": format!("Extract memories from these messages:\n\n{transcript}") }
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

        let extracted: LlmExtracted = serde_json::from_str(content).unwrap_or_else(|e| {
            warn!("Failed to parse LLM response as JSON: {e}");
            LlmExtracted { memories: vec![] }
        });

        Ok(extracted
            .memories
            .into_iter()
            .map(|m| ExtractedMemory {
                content: m.content,
                topic: m.topic,
                importance: m.importance.map(|i| i.clamp(1, 10)),
                contradicts_existing: false,
            })
            .collect())
    }
}
