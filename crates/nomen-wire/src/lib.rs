//! nomen-wire — Wire protocol types and codec for Nomen socket communication.
//!
//! Provides length-prefixed JSON framing over Unix domain sockets,
//! with types for request/response/event frames and a client implementation.

pub mod client;
pub mod codec;
pub mod types;

pub use client::{NomenClient, ReconnectingClient};
pub use codec::NomenCodec;
pub use types::{ErrorBody, Event, Frame, Request, Response};
