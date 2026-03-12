//! Canonical API v2 — shared dispatch layer for MCP and CVM.

pub mod dispatch;
pub mod errors;
pub mod operations;
pub mod types;

pub use dispatch::dispatch;
pub use types::ApiResponse;
