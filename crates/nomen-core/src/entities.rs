use serde::{Deserialize, Serialize};

/// Kind of entity extracted from text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntityKind {
    Person,
    Project,
    Concept,
    Place,
    Organization,
    Technology,
}

impl EntityKind {
    pub fn as_str(&self) -> &str {
        match self {
            EntityKind::Person => "person",
            EntityKind::Project => "project",
            EntityKind::Concept => "concept",
            EntityKind::Place => "place",
            EntityKind::Organization => "organization",
            EntityKind::Technology => "technology",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "person" => Some(EntityKind::Person),
            "project" => Some(EntityKind::Project),
            "concept" => Some(EntityKind::Concept),
            "place" => Some(EntityKind::Place),
            "organization" => Some(EntityKind::Organization),
            "technology" => Some(EntityKind::Technology),
            _ => None,
        }
    }
}

impl std::fmt::Display for EntityKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}
