//! Canonical request/response types for the Nomen API v2.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::errors::ApiError;

// ── Response envelope ────────────────────────────────────────────────

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
            meta: ApiMeta { version: "v2", request_id: None },
        }
    }

    pub fn success_with_request_id(result: Value, request_id: Option<String>) -> Self {
        Self {
            ok: true,
            result: Some(result),
            error: None,
            meta: ApiMeta { version: "v2", request_id },
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
            meta: ApiMeta { version: "v2", request_id: None },
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
            meta: ApiMeta { version: "v2", request_id },
        }
    }

    /// Set the request_id on this response (pass-through from request).
    pub fn with_request_id(mut self, request_id: Option<String>) -> Self {
        self.meta.request_id = request_id;
        self
    }
}

// ── Canonical request ────────────────────────────────────────────────

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

// ── Retrieval tuning ─────────────────────────────────────────────────

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

// ── Visibility enum ──────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    Public,
    Group,
    Circle,
    Personal,
    Internal,
}

impl Visibility {
    /// Parse from string, supporting legacy "private" → Personal.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "public" => Some(Self::Public),
            "group" => Some(Self::Group),
            "circle" => Some(Self::Circle),
            "personal" | "private" => Some(Self::Personal),
            "internal" => Some(Self::Internal),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Group => "group",
            Self::Circle => "circle",
            Self::Personal => "personal",
            Self::Internal => "internal",
        }
    }

    /// Convert to legacy tier string used by Nomen internals.
    pub fn to_tier(&self, scope: &str) -> String {
        match self {
            Self::Public => "public".to_string(),
            Self::Group => format!("group:{scope}"),
            Self::Personal => "personal".to_string(),
            Self::Internal => "internal".to_string(),
            Self::Circle => format!("circle:{scope}"),
        }
    }
}

// ── Scope resolution ─────────────────────────────────────────────────

/// Resolve visibility and scope from params, with legacy fallback.
pub fn resolve_visibility_scope(
    params: &Value,
    nomen: &crate::Nomen,
    default_channel: &str,
) -> Result<(Option<Visibility>, Option<String>), ApiError> {
    // Try canonical fields first
    let vis = params
        .get("visibility")
        .and_then(|v| v.as_str())
        .and_then(Visibility::parse);
    let scope = params
        .get("scope")
        .and_then(|v| v.as_str())
        .map(String::from);

    if vis.is_some() {
        return validate_visibility_scope(&vis, &scope).map(|_| (vis, scope));
    }

    // Fallback: legacy tier field
    if let Some(tier_str) = params.get("tier").and_then(|v| v.as_str()) {
        let (v, s) = parse_legacy_tier(tier_str);
        return Ok((v, s.or(scope)));
    }

    // Fallback: session_id
    if let Some(sid) = params.get("session_id").and_then(|v| v.as_str()) {
        if let Ok(resolved) = nomen.resolve_session(sid, default_channel) {
            let v = Visibility::parse(&resolved.tier);
            let s = if resolved.scope.is_empty() {
                None
            } else {
                Some(resolved.scope)
            };
            return Ok((v, s));
        }
    }

    Ok((vis, scope))
}

fn parse_legacy_tier(tier: &str) -> (Option<Visibility>, Option<String>) {
    if let Some(group_id) = tier.strip_prefix("group:") {
        (Some(Visibility::Group), Some(group_id.to_string()))
    } else {
        (Visibility::parse(tier), None)
    }
}

fn validate_visibility_scope(
    vis: &Option<Visibility>,
    scope: &Option<String>,
) -> Result<(), ApiError> {
    let vis = match vis {
        Some(v) => v,
        None => return Ok(()),
    };
    let scope_empty = scope.as_ref().map_or(true, |s| s.is_empty());
    match vis {
        Visibility::Group | Visibility::Circle if scope_empty => Err(ApiError::invalid_scope(
            format!("scope is required when visibility={}", vis.as_str()),
        )),
        _ => Ok(()),
    }
}
