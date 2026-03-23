#![recursion_limit = "256"]
//! nomen-transport — HTTP, socket, CVM, and MCP transport layers.
//!
//! All transports route requests through the canonical `nomen_api::dispatch`
//! layer via the [`NomenBackend`](nomen_api::NomenBackend) trait.

pub mod cvm;
pub mod http;
pub mod mcp;
pub mod socket;
