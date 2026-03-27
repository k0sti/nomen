//! CollectedEvent — thin wrapper over a kind 30100 Nostr event JSON.
//!
//! Provides accessor methods for tag-based metadata extraction.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::kinds::COLLECTED_MESSAGE_KIND;

/// A collected message event (kind 30100).
///
/// Thin wrapper over the raw Nostr event JSON with tag accessor methods.
/// The event IS the data — this struct just provides ergonomic access.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectedEvent {
    pub kind: u16,
    pub created_at: i64,
    pub pubkey: String,
    pub tags: Vec<Vec<String>>,
    pub content: String,
    /// Optional fields that may be present on signed events.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sig: Option<String>,
}

/// Parsed imeta tag values.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Imeta {
    pub url: Option<String>,
    pub mime_type: Option<String>,
    pub sha256: Option<String>,
    pub dim: Option<String>,
    pub alt: Option<String>,
}

/// Filter for querying collected events.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct CollectedEventFilter {
    /// Filter by platform.
    #[serde(rename = "#platform", default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<Vec<String>>,
    /// Filter by community_id (from `community` tag value[0]).
    #[serde(
        rename = "#community",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub community_id: Option<Vec<String>>,
    /// Filter by chat_id (from `chat` tag value[0]).
    #[serde(rename = "#chat", default, skip_serializing_if = "Option::is_none")]
    pub chat_id: Option<Vec<String>>,
    /// Filter by sender_id (from `sender` tag value[0]).
    #[serde(rename = "#sender", default, skip_serializing_if = "Option::is_none")]
    pub sender_id: Option<Vec<String>>,
    /// Filter by thread_id (from `thread` tag value[0]).
    #[serde(rename = "#thread", default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<Vec<String>>,
    /// Events created after this unix timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<i64>,
    /// Events created before this unix timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<i64>,
    /// Maximum number of events to return.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

impl CollectedEvent {
    /// Parse a CollectedEvent from a JSON value.
    pub fn from_json(value: &Value) -> Result<Self, String> {
        serde_json::from_value(value.clone()).map_err(|e| format!("invalid event JSON: {e}"))
    }

    /// Serialize to a JSON value.
    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).unwrap_or_default()
    }

    /// Validate that this is a proper kind 30100 event with a d-tag.
    pub fn validate(&self) -> Result<(), String> {
        if self.kind != COLLECTED_MESSAGE_KIND {
            return Err(format!(
                "expected kind {COLLECTED_MESSAGE_KIND}, got {}",
                self.kind
            ));
        }
        if self.d_tag().is_none() {
            return Err("missing d tag".to_string());
        }
        Ok(())
    }

    /// Get the `d` tag value (unique replaceable identifier).
    pub fn d_tag(&self) -> Option<&str> {
        self.tag_value("d", 0)
    }

    /// Get the platform. Checks the `platform` tag first, falls back to `proxy` tag protocol field.
    pub fn platform(&self) -> Option<&str> {
        self.tag_value("platform", 0)
            .or_else(|| self.tag_value("proxy", 1))
    }

    /// Get the community_id from the `community` tag (value[0]).
    pub fn community_id(&self) -> Option<&str> {
        self.tag_value("community", 0)
    }

    /// Get the community name from the `community` tag (value[1]).
    pub fn community_name(&self) -> Option<&str> {
        self.tag_value("community", 1)
    }

    /// Get the community type from the `community` tag (value[2]).
    pub fn community_type(&self) -> Option<&str> {
        self.tag_value("community", 2)
    }

    /// Get the chat_id from the `chat` tag (value[0]).
    pub fn chat_id(&self) -> Option<&str> {
        self.tag_value("chat", 0)
    }

    /// Get the chat name from the `chat` tag (value[1]).
    pub fn chat_name(&self) -> Option<&str> {
        self.tag_value("chat", 1)
    }

    /// Get the chat type from the `chat` tag (value[2]).
    pub fn chat_type(&self) -> Option<&str> {
        self.tag_value("chat", 2)
    }

    /// Get the sender_id from the `sender` tag (value[0]).
    pub fn sender_id(&self) -> Option<&str> {
        self.tag_value("sender", 0)
    }

    /// Get the sender display name from the `sender` tag (value[1]).
    pub fn sender_name(&self) -> Option<&str> {
        self.tag_value("sender", 1)
    }

    /// Get the thread_id from the `thread` tag (value[0]).
    pub fn thread_id(&self) -> Option<&str> {
        self.tag_value("thread", 0)
    }

    /// Get the thread name from the `thread` tag (value[1]).
    pub fn thread_name(&self) -> Option<&str> {
        self.tag_value("thread", 1)
    }

    /// Get the thread type from the `thread` tag (value[2]).
    pub fn thread_type(&self) -> Option<&str> {
        self.tag_value("thread", 2)
    }

    /// Get the provider-native message_id, derived from the final segment of the d-tag.
    pub fn message_id(&self) -> Option<&str> {
        self.d_tag()?.rsplit(':').next()
    }

    /// Get the message text content.
    pub fn text(&self) -> &str {
        &self.content
    }

    /// Get the reply d-tag from the `reply` tag.
    pub fn reply_to(&self) -> Option<&str> {
        self.tag_value("reply", 0)
    }

    /// Get the reply event id from `e` tags with "reply" marker.
    pub fn reply_to_event(&self) -> Option<&str> {
        for tag in &self.tags {
            if tag.len() >= 4 && tag[0] == "e" && tag[3] == "reply" {
                return Some(&tag[1]);
            }
        }
        None
    }

    /// Parse all `imeta` tags into structured media metadata.
    pub fn media(&self) -> Vec<Imeta> {
        self.tags
            .iter()
            .filter(|tag| !tag.is_empty() && tag[0] == "imeta")
            .map(|tag| {
                let mut imeta = Imeta {
                    url: None,
                    mime_type: None,
                    sha256: None,
                    dim: None,
                    alt: None,
                };
                for field in &tag[1..] {
                    if let Some(val) = field.strip_prefix("url ") {
                        imeta.url = Some(val.to_string());
                    } else if let Some(val) = field.strip_prefix("m ") {
                        imeta.mime_type = Some(val.to_string());
                    } else if let Some(val) = field.strip_prefix("x ") {
                        imeta.sha256 = Some(val.to_string());
                    } else if let Some(val) = field.strip_prefix("dim ") {
                        imeta.dim = Some(val.to_string());
                    } else if let Some(val) = field.strip_prefix("alt ") {
                        imeta.alt = Some(val.to_string());
                    }
                }
                imeta
            })
            .collect()
    }

    /// Get a tag value by tag name and value index (0-based, after the tag name).
    fn tag_value(&self, name: &str, index: usize) -> Option<&str> {
        for tag in &self.tags {
            if !tag.is_empty() && tag[0] == name {
                let actual_index = index + 1; // skip tag name
                if tag.len() > actual_index {
                    return Some(&tag[actual_index]);
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_event() -> Value {
        json!({
            "kind": 30100,
            "created_at": 1711540200,
            "pubkey": "abc123",
            "tags": [
                ["d", "telegram:-1003821690204:13943"],
                ["proxy", "telegram:-1003821690204:13943", "telegram"],
                ["community", "acme", "Acme", "workspace"],
                ["chat", "-1003821690204", "TechTeam", "group"],
                ["sender", "60996061", "kosti", "koshdot"],
                ["thread", "13939", "Message Bridge", "topic"],
                ["e", "event123", "", "reply"],
                ["reply", "telegram:-1003821690204:13939"],
                ["imeta", "url https://blossom.example/a1b2c3.jpg", "m image/jpeg", "x a1b2c3d4"]
            ],
            "content": "Explain 30100/30101 kinds"
        })
    }

    #[test]
    fn round_trip() {
        let event = CollectedEvent::from_json(&sample_event()).unwrap();
        let json = event.to_json();
        let event2 = CollectedEvent::from_json(&json).unwrap();
        assert_eq!(event.kind, event2.kind);
        assert_eq!(event.content, event2.content);
        assert_eq!(event.d_tag(), event2.d_tag());
    }

    #[test]
    fn accessors() {
        let event = CollectedEvent::from_json(&sample_event()).unwrap();
        assert_eq!(event.d_tag(), Some("telegram:-1003821690204:13943"));
        assert_eq!(event.platform(), Some("telegram"));
        assert_eq!(event.community_id(), Some("acme"));
        assert_eq!(event.community_name(), Some("Acme"));
        assert_eq!(event.community_type(), Some("workspace"));
        assert_eq!(event.chat_id(), Some("-1003821690204"));
        assert_eq!(event.chat_name(), Some("TechTeam"));
        assert_eq!(event.chat_type(), Some("group"));
        assert_eq!(event.sender_id(), Some("60996061"));
        assert_eq!(event.sender_name(), Some("kosti"));
        assert_eq!(event.thread_id(), Some("13939"));
        assert_eq!(event.thread_name(), Some("Message Bridge"));
        assert_eq!(event.thread_type(), Some("topic"));
        assert_eq!(event.message_id(), Some("13943"));
        assert_eq!(event.text(), "Explain 30100/30101 kinds");
        assert_eq!(event.reply_to(), Some("telegram:-1003821690204:13939"));
        assert_eq!(event.reply_to_event(), Some("event123"));
    }

    #[test]
    fn media_parsing() {
        let event = CollectedEvent::from_json(&sample_event()).unwrap();
        let media = event.media();
        assert_eq!(media.len(), 1);
        assert_eq!(
            media[0].url.as_deref(),
            Some("https://blossom.example/a1b2c3.jpg")
        );
        assert_eq!(media[0].mime_type.as_deref(), Some("image/jpeg"));
        assert_eq!(media[0].sha256.as_deref(), Some("a1b2c3d4"));
    }

    #[test]
    fn validate_ok() {
        let event = CollectedEvent::from_json(&sample_event()).unwrap();
        assert!(event.validate().is_ok());
    }

    #[test]
    fn validate_wrong_kind() {
        let mut val = sample_event();
        val["kind"] = json!(1);
        let event = CollectedEvent::from_json(&val).unwrap();
        assert!(event.validate().is_err());
    }

    #[test]
    fn validate_missing_d_tag() {
        let val = json!({
            "kind": 30100,
            "created_at": 1711540200,
            "pubkey": "abc123",
            "tags": [],
            "content": "no d tag"
        });
        let event = CollectedEvent::from_json(&val).unwrap();
        assert!(event.validate().is_err());
    }
}
