use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use tracing::warn;

pub use nomen_core::entities::EntityKind;
pub use nomen_db::{EntityRecord, RelationshipRecord};

/// An entity extracted from text.
#[derive(Debug, Clone)]
pub struct ExtractedEntity {
    pub name: String,
    pub kind: EntityKind,
    pub relevance: f64,
}

/// A typed relationship between two entities.
#[derive(Debug, Clone)]
pub struct ExtractedRelationship {
    pub from: String,
    pub to: String,
    pub relation: String,
    pub detail: Option<String>,
}

/// Trait for entity extraction strategies.
///
/// Implementations can be heuristic-only, LLM-powered, or composite.
#[async_trait]
pub trait EntityExtractor: Send + Sync {
    async fn extract(
        &self,
        text: &str,
        known_entities: &[ExtractedEntity],
    ) -> Result<(Vec<ExtractedEntity>, Vec<ExtractedRelationship>)>;
}

// -- Heuristic Extractor --

/// Wraps the existing heuristic extraction in the EntityExtractor trait.
/// Returns entities only (no relationships — heuristics can't infer those).
pub struct HeuristicExtractor;

#[async_trait]
impl EntityExtractor for HeuristicExtractor {
    async fn extract(
        &self,
        text: &str,
        known_entities: &[ExtractedEntity],
    ) -> Result<(Vec<ExtractedEntity>, Vec<ExtractedRelationship>)> {
        let entities = extract_entities_heuristic(text, known_entities);
        Ok((entities, vec![]))
    }
}

// -- LLM Entity Extractor --

/// LLM-powered entity and relationship extraction using OpenAI-compatible API.
pub struct LlmEntityExtractor {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
}

impl LlmEntityExtractor {
    pub fn new(base_url: &str, api_key: &str, model: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
        }
    }

    /// Create from entity config, returning None if API key is missing.
    pub fn from_config(config: &nomen_core::config::EntityExtractionConfig) -> Option<Self> {
        let api_key = std::env::var(&config.api_key_env).unwrap_or_default();
        if api_key.is_empty() {
            warn!(
                "Entity extraction API key env {} not set, will use HeuristicExtractor",
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

/// JSON response structures for LLM entity extraction.
#[derive(Deserialize)]
struct LlmEntityResponse {
    #[serde(default)]
    entities: Vec<LlmEntity>,
    #[serde(default)]
    relationships: Vec<LlmRelationship>,
}

#[derive(Deserialize)]
struct LlmEntity {
    name: String,
    kind: String,
    #[serde(default)]
    #[allow(dead_code)]
    attributes: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct LlmRelationship {
    from: String,
    to: String,
    relation: String,
    #[serde(default)]
    detail: Option<String>,
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

const ENTITY_SYSTEM_PROMPT: &str = r#"Extract entities and relationships from this memory text.

Return JSON with this exact structure:
{
  "entities": [
    {"name": "k0", "kind": "person", "attributes": {"role": "owner"}},
    {"name": "Nomen", "kind": "project", "attributes": {"language": "rust"}}
  ],
  "relationships": [
    {"from": "k0", "to": "Nomen", "relation": "works_on", "detail": "primary developer"},
    {"from": "Tommi", "to": "Alhovuori", "relation": "manages_finances", "detail": "handles business plan"}
  ]
}

Entity kinds: person, project, concept, place, organization, technology
Relationship types: works_on, collaborates_with, decided, contradicts, depends_on, member_of, located_in, hired_by, manages, owns, uses, created

Rules:
- Only extract clearly mentioned entities, do not infer
- Normalize entity names (consistent casing)
- Deduplicate: if the same entity appears with different names, use the most specific one
- Return empty arrays if nothing significant is found
- Keep relationship detail concise (under 10 words)"#;

#[async_trait]
impl EntityExtractor for LlmEntityExtractor {
    async fn extract(
        &self,
        text: &str,
        known_entities: &[ExtractedEntity],
    ) -> Result<(Vec<ExtractedEntity>, Vec<ExtractedRelationship>)> {
        let known_context = if known_entities.is_empty() {
            String::new()
        } else {
            let names: Vec<&str> = known_entities.iter().map(|e| e.name.as_str()).collect();
            format!(
                "\n\nKnown entities (reuse these names if they appear): {}",
                names.join(", ")
            )
        };

        let user_prompt =
            format!("Extract entities and relationships from this text:{known_context}\n\n{text}");

        let body = serde_json::json!({
            "model": self.model,
            "messages": [
                { "role": "system", "content": ENTITY_SYSTEM_PROMPT },
                { "role": "user", "content": user_prompt }
            ],
            "temperature": 0.2,
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
            anyhow::bail!("Entity extraction LLM API error {status}: {text}");
        }

        let chat_resp: ChatResponse = resp.json().await?;
        let content = chat_resp
            .choices
            .first()
            .map(|c| c.message.content.as_str())
            .unwrap_or("{}");

        let parsed: LlmEntityResponse = serde_json::from_str(content).unwrap_or_else(|e| {
            warn!("Failed to parse LLM entity extraction response: {e}");
            LlmEntityResponse {
                entities: vec![],
                relationships: vec![],
            }
        });

        let entities: Vec<ExtractedEntity> = parsed
            .entities
            .into_iter()
            .map(|e| ExtractedEntity {
                name: e.name,
                kind: EntityKind::from_str(&e.kind).unwrap_or(EntityKind::Concept),
                relevance: 0.8,
            })
            .collect();

        let relationships: Vec<ExtractedRelationship> = parsed
            .relationships
            .into_iter()
            .map(|r| ExtractedRelationship {
                from: r.from,
                to: r.to,
                relation: r.relation,
                detail: r.detail,
            })
            .collect();

        Ok((entities, relationships))
    }
}

// -- Composite Extractor --

/// Runs heuristic extraction first, then LLM for refinement.
///
/// The heuristic pass provides fast, cheap entity detection.
/// The LLM pass adds relationships and catches entities the heuristic misses.
/// Results are merged with deduplication by normalized name.
pub struct CompositeExtractor {
    heuristic: HeuristicExtractor,
    llm: LlmEntityExtractor,
}

impl CompositeExtractor {
    pub fn new(llm: LlmEntityExtractor) -> Self {
        Self {
            heuristic: HeuristicExtractor,
            llm,
        }
    }
}

#[async_trait]
impl EntityExtractor for CompositeExtractor {
    async fn extract(
        &self,
        text: &str,
        known_entities: &[ExtractedEntity],
    ) -> Result<(Vec<ExtractedEntity>, Vec<ExtractedRelationship>)> {
        // Phase 1: Heuristic extraction
        let (heuristic_entities, _) = self.heuristic.extract(text, known_entities).await?;

        // Phase 2: LLM extraction (feed heuristic results as known entities)
        let mut combined_known: Vec<ExtractedEntity> = known_entities.to_vec();
        combined_known.extend(heuristic_entities.iter().cloned());

        let (llm_entities, relationships) = self.llm.extract(text, &combined_known).await?;

        // Merge entities: LLM results take priority, dedup by normalized name
        let mut seen = std::collections::HashSet::new();
        let mut merged = Vec::new();

        // LLM entities first (higher quality)
        for entity in llm_entities {
            let key = entity.name.to_lowercase();
            if seen.insert(key) {
                merged.push(entity);
            }
        }

        // Then heuristic entities (fill gaps)
        for entity in heuristic_entities {
            let key = entity.name.to_lowercase();
            if seen.insert(key) {
                merged.push(entity);
            }
        }

        Ok((merged, relationships))
    }
}

// -- Builder --

/// Build the appropriate EntityExtractor from config.
///
/// Returns a CompositeExtractor if LLM is configured, otherwise HeuristicExtractor.
pub fn build_entity_extractor(config: &nomen_core::config::Config) -> Box<dyn EntityExtractor> {
    if let Some(ref entity_config) = config.entities {
        if entity_config.provider == "heuristic" || entity_config.provider == "none" {
            return Box::new(HeuristicExtractor);
        }

        if let Some(llm) = LlmEntityExtractor::from_config(entity_config) {
            return Box::new(CompositeExtractor::new(llm));
        }
    }

    Box::new(HeuristicExtractor)
}

// -- Heuristic extraction (original logic) --

/// Heuristic (pattern-based) entity extraction.
///
/// Detects:
/// - @mentions -> Person
/// - Capitalized multi-word phrases (Title Case) -> Person or Organization
/// - Known entities matched case-insensitively
/// - URLs -> Project
pub fn extract_entities_heuristic(
    text: &str,
    known_entities: &[ExtractedEntity],
) -> Vec<ExtractedEntity> {
    let mut entities = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // 1. Match known entities (case-insensitive)
    let text_lower = text.to_lowercase();
    for known in known_entities {
        let name_lower = known.name.to_lowercase();
        if text_lower.contains(&name_lower) && seen.insert(name_lower) {
            entities.push(ExtractedEntity {
                name: known.name.clone(),
                kind: known.kind.clone(),
                relevance: known.relevance,
            });
        }
    }

    // 2. @mentions -> Person
    for word in text.split_whitespace() {
        if let Some(name) = word.strip_prefix('@') {
            let name = name.trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
            if !name.is_empty() {
                let key = name.to_lowercase();
                if seen.insert(key) {
                    entities.push(ExtractedEntity {
                        name: name.to_string(),
                        kind: EntityKind::Person,
                        relevance: 0.9,
                    });
                }
            }
        }
    }

    // 3. Capitalized word sequences (2+ words, likely proper nouns)
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut i = 0;
    while i < words.len() {
        // Skip @mentions and URLs
        if words[i].starts_with('@') || words[i].starts_with("http") {
            i += 1;
            continue;
        }

        if is_capitalized_word(words[i]) && !is_common_word(words[i]) {
            let start = i;
            let mut end = i + 1;

            // Collect consecutive capitalized words
            while end < words.len()
                && is_capitalized_word(words[end])
                && !is_common_word(words[end])
            {
                end += 1;
            }

            // Only multi-word sequences or single words not at sentence start
            if end - start >= 2 || (start > 0 && end - start == 1) {
                let phrase: String = words[start..end]
                    .iter()
                    .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
                    .collect::<Vec<_>>()
                    .join(" ");

                if !phrase.is_empty() {
                    let key = phrase.to_lowercase();
                    if seen.insert(key) {
                        let kind = if end - start >= 2 {
                            EntityKind::Organization
                        } else {
                            EntityKind::Person
                        };
                        entities.push(ExtractedEntity {
                            name: phrase,
                            kind,
                            relevance: 0.6,
                        });
                    }
                }
            }

            i = end;
        } else {
            i += 1;
        }
    }

    entities
}

fn is_capitalized_word(word: &str) -> bool {
    let trimmed = word.trim_matches(|c: char| !c.is_alphanumeric());
    trimmed
        .chars()
        .next()
        .map(|c| c.is_uppercase())
        .unwrap_or(false)
}

fn is_common_word(word: &str) -> bool {
    let w = word
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase();
    matches!(
        w.as_str(),
        "the"
            | "a"
            | "an"
            | "and"
            | "or"
            | "but"
            | "in"
            | "on"
            | "at"
            | "to"
            | "for"
            | "of"
            | "with"
            | "by"
            | "from"
            | "is"
            | "are"
            | "was"
            | "were"
            | "be"
            | "been"
            | "being"
            | "have"
            | "has"
            | "had"
            | "do"
            | "does"
            | "did"
            | "will"
            | "would"
            | "could"
            | "should"
            | "may"
            | "might"
            | "shall"
            | "can"
            | "this"
            | "that"
            | "these"
            | "those"
            | "it"
            | "its"
            | "i"
            | "we"
            | "they"
            | "he"
            | "she"
            | "not"
            | "no"
            | "if"
            | "then"
            | "so"
            | "as"
            | "use"
            | "using"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_mentions() {
        let entities = extract_entities_heuristic("talked to @alice about the project", &[]);
        assert!(entities
            .iter()
            .any(|e| e.name == "alice" && e.kind == EntityKind::Person));
    }

    #[test]
    fn test_extract_known_entities() {
        let known = vec![ExtractedEntity {
            name: "Nomen".to_string(),
            kind: EntityKind::Project,
            relevance: 1.0,
        }];
        let entities = extract_entities_heuristic("working on nomen today", &known);
        assert!(entities
            .iter()
            .any(|e| e.name == "Nomen" && e.kind == EntityKind::Project));
    }

    #[test]
    fn test_extract_capitalized_phrases() {
        let entities = extract_entities_heuristic("met with Atlantis Labs about the deal", &[]);
        assert!(entities.iter().any(|e| e.name == "Atlantis Labs"));
    }

    #[test]
    fn test_entity_kind_technology() {
        assert_eq!(
            EntityKind::from_str("technology"),
            Some(EntityKind::Technology)
        );
        assert_eq!(EntityKind::Technology.as_str(), "technology");
    }

    #[tokio::test]
    async fn test_heuristic_extractor_trait() {
        let extractor = HeuristicExtractor;
        let (entities, relationships) = extractor
            .extract("talked to @bob about the project", &[])
            .await
            .unwrap();
        assert!(!entities.is_empty());
        assert!(relationships.is_empty()); // heuristic doesn't produce relationships
    }
}
