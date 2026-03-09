use serde::{Deserialize, Serialize};

/// Kind of entity extracted from text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntityKind {
    Person,
    Project,
    Concept,
    Place,
    Organization,
}

impl EntityKind {
    pub fn as_str(&self) -> &str {
        match self {
            EntityKind::Person => "person",
            EntityKind::Project => "project",
            EntityKind::Concept => "concept",
            EntityKind::Place => "place",
            EntityKind::Organization => "organization",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "person" => Some(EntityKind::Person),
            "project" => Some(EntityKind::Project),
            "concept" => Some(EntityKind::Concept),
            "place" => Some(EntityKind::Place),
            "organization" => Some(EntityKind::Organization),
            _ => None,
        }
    }
}

impl std::fmt::Display for EntityKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// An entity extracted from text.
#[derive(Debug, Clone)]
pub struct ExtractedEntity {
    pub name: String,
    pub kind: EntityKind,
    pub relevance: f64,
}

/// An entity record from SurrealDB.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityRecord {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub kind: String,
    pub attributes: Option<serde_json::Value>,
    pub created_at: String,
}

/// Heuristic (pattern-based) entity extraction.
///
/// Detects:
/// - @mentions → Person
/// - Capitalized multi-word phrases (Title Case) → Person or Organization
/// - Known entities matched case-insensitively
/// - URLs → Project
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

    // 2. @mentions → Person
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
}
