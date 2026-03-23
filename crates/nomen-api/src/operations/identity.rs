//! Identity operations: authenticate a session with a per-client nsec.

use nostr_sdk::ToBech32;
use serde_json::Value;

use nomen_core::api::errors::ApiError;
use crate::NomenBackend;

/// Handle `identity.auth`: validate an nsec and return the corresponding pubkey.
///
/// This operation is intercepted by transports to set up a SessionBackend.
/// The dispatch layer handles validation; the transport uses the result to
/// create a KeysSigner and wrap the backend.
///
/// Params:
///   - `nsec`: nsec1... bech32-encoded secret key (required)
///
/// Returns:
///   - `pubkey`: hex-encoded public key
///   - `npub`: npub1... bech32-encoded public key
pub async fn auth(
    _nomen: &dyn NomenBackend,
    _default_channel: &str,
    params: &Value,
) -> Result<Value, ApiError> {
    let nsec = params
        .get("nsec")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::invalid_params("nsec is required"))?;

    // Parse and validate the nsec
    let keys = nostr_sdk::Keys::parse(nsec)
        .map_err(|e| ApiError::invalid_params(format!("invalid nsec: {e}")))?;

    let pubkey = keys.public_key();

    Ok(serde_json::json!({
        "pubkey": pubkey.to_hex(),
        "npub": pubkey.to_bech32().unwrap_or_default(),
    }))
}
