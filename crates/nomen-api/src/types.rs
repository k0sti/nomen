//! Canonical request/response types for the Nomen API v2.

pub use nomen_core::api::types::*;

use serde_json::Value;

use nomen_core::api::errors::ApiError;

// -- Scope resolution (needs NomenBackend) --

/// Resolve visibility and scope from params.
pub fn resolve_visibility_scope(
    params: &Value,
) -> Result<(Option<Visibility>, Option<String>), ApiError> {
    let vis = params
        .get("visibility")
        .and_then(|v| v.as_str())
        .and_then(Visibility::parse);
    let scope = params
        .get("scope")
        .and_then(|v| v.as_str())
        .map(String::from);

    if vis.is_some() {
        validate_visibility_scope(&vis, &scope)?;
    }

    Ok((vis, scope))
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
