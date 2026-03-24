//! nomen-core — foundational types for the Nomen memory system.
//!
//! This crate contains pure types, traits, and logic with minimal dependencies
//! (no surrealdb, reqwest, axum). It is intended to be used by both the main
//! `nomen` crate and external consumers.

pub mod access;
pub mod api;
pub mod collected;
pub mod config;
pub mod embed;
pub mod entities;
pub mod groups;
pub mod kinds;
pub mod memory;
pub mod ops;
pub mod search;
pub mod send;
pub mod session;
pub mod signer;

/// Options for creating a new memory directly (without relay event).
pub struct NewMemory {
    pub topic: String,
    /// Plain-text content (the full memory body).
    pub content: String,
    pub tier: String,
    /// Importance score (1-10). Optional.
    pub importance: Option<i32>,
    /// Source label (e.g. "api", "mcp", "contextvm"). Defaults to "api".
    pub source: Option<String>,
    /// Model label. Defaults to "nomen/api".
    pub model: Option<String>,
}
