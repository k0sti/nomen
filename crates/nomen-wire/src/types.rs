//! Wire protocol frame types.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A request frame sent from agent to Nomen.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Request {
    /// Correlation ID (ULID recommended, UUID accepted).
    pub id: String,
    /// Action name matching api::dispatch (e.g. "memory.search").
    pub action: String,
    /// Action parameters. Defaults to empty object.
    #[serde(default = "default_object")]
    pub params: Value,
}

/// A response frame sent from Nomen to agent.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Response {
    /// Correlation ID matching the request.
    pub id: String,
    /// Whether the operation succeeded.
    pub ok: bool,
    /// Result payload on success.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// Error details on failure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorBody>,
    /// Response metadata.
    #[serde(default = "default_object")]
    pub meta: Value,
}

/// Error details in a response.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
}

/// A push event frame sent from Nomen to agent (unsolicited).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Event {
    /// Event type (e.g. "memory.updated").
    pub event: String,
    /// Unix timestamp.
    pub ts: u64,
    /// Event payload.
    #[serde(default = "default_object")]
    pub data: Value,
}

/// Wire frame enum — distinguished by field presence (untagged).
///
/// Discrimination logic:
/// - Presence of `action` field → Request
/// - Presence of `ok` + `id` fields → Response
/// - Presence of `event` field → Event
///
/// IMPORTANT: The order matters for serde untagged deserialization.
/// Request must come first (has unique `action` field).
/// Response second (has unique `ok` field).
/// Event last (has unique `event` field).
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum Frame {
    Request(Request),
    Response(Response),
    Event(Event),
}

impl Frame {
    /// Returns true if this is a request frame.
    pub fn is_request(&self) -> bool {
        matches!(self, Frame::Request(_))
    }

    /// Returns true if this is a response frame.
    pub fn is_response(&self) -> bool {
        matches!(self, Frame::Response(_))
    }

    /// Returns true if this is an event frame.
    pub fn is_event(&self) -> bool {
        matches!(self, Frame::Event(_))
    }
}

impl Response {
    /// Create a success response.
    pub fn success(id: String, result: Value) -> Self {
        Self {
            id,
            ok: true,
            result: Some(result),
            error: None,
            meta: serde_json::json!({"version": "v2"}),
        }
    }

    /// Create an error response.
    pub fn error(id: String, code: &str, message: &str) -> Self {
        Self {
            id,
            ok: false,
            result: None,
            error: Some(ErrorBody {
                code: code.to_string(),
                message: message.to_string(),
            }),
            meta: serde_json::json!({"version": "v2"}),
        }
    }
}

fn default_object() -> Value {
    Value::Object(serde_json::Map::new())
}
