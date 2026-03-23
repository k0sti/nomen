//! Canonical request/response types for the Nomen API v2.

pub use nomen_core::api::types::*;

use serde_json::Value;

use nomen_core::api::errors::ApiError;
use crate::NomenBackend;

// -- Scope resolution (needs NomenBackend) --

/// Resolve visibility and scope from params, with legacy fallback.
pub fn resolve_visibility_scope(
    params: &Value,
    nomen: &dyn NomenBackend,
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
