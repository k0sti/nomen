//! Canonical request/response types for the Nomen API v2.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::errors::ApiError;

// -- Response envelope --

#[derive(Debug, Serialize)]
pub struct ApiResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ApiErrorBody>,
    pub meta: ApiMeta,
}

#[derive(Debug, Serialize)]
pub struct ApiErrorBody {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct ApiMeta {
    pub version: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

impl ApiResponse {
    pub fn success(result: Value) -> Self {
        Self {
            ok: true,
            result: Some(result),
            error: None,
            meta: ApiMeta {
                version: "v2",
                request_id: None,
            },
        }
    }

    pub fn success_with_request_id(result: Value, request_id: Option<String>) -> Self {
        Self {
            ok: true,
            result: Some(result),
            error: None,
            meta: ApiMeta {
                version: "v2",
                request_id,
            },
        }
    }

    pub fn error(err: ApiError) -> Self {
        Self {
            ok: false,
            result: None,
            error: Some(ApiErrorBody {
                code: err.code().to_string(),
                message: err.message().to_string(),
            }),
            meta: ApiMeta {
                version: "v2",
                request_id: None,
            },
        }
    }

    pub fn error_with_request_id(err: ApiError, request_id: Option<String>) -> Self {
        Self {
            ok: false,
            result: None,
            error: Some(ApiErrorBody {
                code: err.code().to_string(),
                message: err.message().to_string(),
            }),
            meta: ApiMeta {
                version: "v2",
                request_id,
            },
        }
    }

    /// Set the request_id on this response (pass-through from request).
    pub fn with_request_id(mut self, request_id: Option<String>) -> Self {
        self.meta.request_id = request_id;
        self
    }
}

// -- Canonical request --

#[derive(Debug, Deserialize)]
pub struct ApiRequest {
    pub action: String,
    #[serde(default)]
    pub params: Value,
    #[serde(default)]
    pub meta: Option<RequestMeta>,
}

/// Optional metadata on incoming requests.
#[derive(Debug, Deserialize)]
pub struct RequestMeta {
    pub request_id: Option<String>,
}

// -- Retrieval tuning --

#[derive(Debug, Deserialize)]
pub struct RetrievalParams {
    #[serde(default = "default_vector_weight")]
    pub vector_weight: f32,
    #[serde(default = "default_text_weight")]
    pub text_weight: f32,
    #[serde(default)]
    pub aggregate: bool,
    #[serde(default)]
    pub graph_expand: bool,
    #[serde(default = "default_max_hops")]
    pub max_hops: usize,
}

impl Default for RetrievalParams {
    fn default() -> Self {
        Self {
            vector_weight: 0.7,
            text_weight: 0.3,
            aggregate: false,
            graph_expand: false,
            max_hops: 1,
        }
    }
}

fn default_vector_weight() -> f32 {
    0.7
}
fn default_text_weight() -> f32 {
    0.3
}
fn default_max_hops() -> usize {
    1
}

// -- Visibility enum --

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    Public,
    Group,
    Circle,
    Personal,
    Private,
}

impl Visibility {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "public" => Some(Self::Public),
            "group" => Some(Self::Group),
            "circle" => Some(Self::Circle),
            "personal" => Some(Self::Personal),
            "private" => Some(Self::Private),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Group => "group",
            Self::Circle => "circle",
            Self::Personal => "personal",
            Self::Private => "private",
        }
    }

    /// Convert to tier string used by Nomen internals.
    pub fn to_tier(&self, scope: &str) -> String {
        match self {
            Self::Public => "public".to_string(),
            Self::Group => format!("group:{scope}"),
            Self::Personal => "personal".to_string(),
            Self::Private => "private".to_string(),
            Self::Circle => format!("circle:{scope}"),
        }
    }
}
