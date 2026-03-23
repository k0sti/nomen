//! nomen-core — foundational types for the Nomen memory system.
//!
//! This crate contains pure types, traits, and logic with minimal dependencies
//! (no surrealdb, reqwest, axum). It is intended to be used by both the main
//! `nomen` crate and external consumers.

pub mod access;
pub mod api;
pub mod config;
pub mod embed;
pub mod entities;
pub mod groups;
pub mod ingest;
pub mod kinds;
pub mod memory;
pub mod session;
pub mod signer;
