//! nomen-wire — Wire protocol types and codec for Nomen socket communication.
//!
//! Provides length-prefixed JSON framing over Unix domain sockets,
//! with types for request/response/event frames and a client implementation.

pub mod types;
pub mod codec;
pub mod client;

pub use types::{Frame, Request, Response, ErrorBody, Event};
pub use codec::NomenCodec;
pub use client::{NomenClient, ReconnectingClient};
