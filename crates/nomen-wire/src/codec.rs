//! Length-delimited JSON codec for Nomen wire protocol.

use bytes::{Bytes, BytesMut};
use tokio_util::codec::{Decoder, Encoder, LengthDelimitedCodec};

use crate::types::Frame;

/// Codec that wraps `LengthDelimitedCodec` with JSON serialization.
///
/// Wire format:
/// ```text
/// ┌──────────┬──────────────────────┐
/// │ len: u32 │ payload: JSON bytes  │
/// │ (BE)     │ (len bytes)          │
/// └──────────┴──────────────────────┘
/// ```
pub struct NomenCodec {
    inner: LengthDelimitedCodec,
}

impl NomenCodec {
    /// Create a new codec with default max frame size (16 MB).
    pub fn new() -> Self {
        Self {
            inner: LengthDelimitedCodec::builder()
                .max_frame_length(16 * 1024 * 1024) // 16 MB
                .new_codec(),
        }
    }

    /// Create a new codec with a custom max frame size.
    pub fn with_max_frame_size(max_bytes: usize) -> Self {
        Self {
            inner: LengthDelimitedCodec::builder()
                .max_frame_length(max_bytes)
                .new_codec(),
        }
    }
}

impl Default for NomenCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoder for NomenCodec {
    type Item = Frame;
    type Error = std::io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let bytes = match self.inner.decode(src)? {
            Some(b) => b,
            None => return Ok(None),
        };

        serde_json::from_slice(&bytes).map(Some).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("JSON parse error: {e}"),
            )
        })
    }
}

impl Encoder<Frame> for NomenCodec {
    type Error = std::io::Error;

    fn encode(&mut self, item: Frame, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let json = serde_json::to_vec(&item)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        self.inner.encode(Bytes::from(json), dst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;
    use bytes::BytesMut;
    use serde_json::json;

    fn roundtrip(frame: Frame) -> Frame {
        let mut codec = NomenCodec::new();
        let mut buf = BytesMut::new();
        codec.encode(frame.clone(), &mut buf).unwrap();
        codec.decode(&mut buf).unwrap().unwrap()
    }

    #[test]
    fn test_request_roundtrip() {
        let frame = Frame::Request(Request {
            id: "test1".to_string(),
            action: "memory.search".to_string(),
            params: json!({"query": "hello"}),
        });
        let decoded = roundtrip(frame);
        assert!(decoded.is_request());
        if let Frame::Request(req) = decoded {
            assert_eq!(req.id, "test1");
            assert_eq!(req.action, "memory.search");
            assert_eq!(req.params["query"], "hello");
        }
    }

    #[test]
    fn test_response_success_roundtrip() {
        let frame = Frame::Response(Response::success("test1".to_string(), json!({"count": 5})));
        let decoded = roundtrip(frame);
        assert!(decoded.is_response());
        if let Frame::Response(resp) = decoded {
            assert!(resp.ok);
            assert!(resp.result.is_some());
            assert!(resp.error.is_none());
        }
    }

    #[test]
    fn test_response_error_roundtrip() {
        let frame = Frame::Response(Response::error(
            "test1".to_string(),
            "not_found",
            "No memory with that topic",
        ));
        let decoded = roundtrip(frame);
        if let Frame::Response(resp) = decoded {
            assert!(!resp.ok);
            assert!(resp.result.is_none());
            assert!(resp.error.is_some());
            let err = resp.error.unwrap();
            assert_eq!(err.code, "not_found");
        }
    }

    #[test]
    fn test_event_roundtrip() {
        let frame = Frame::Event(Event {
            event: "memory.updated".to_string(),
            ts: 1741860000,
            data: json!({"topic": "test"}),
        });
        let decoded = roundtrip(frame);
        assert!(decoded.is_event());
        if let Frame::Event(evt) = decoded {
            assert_eq!(evt.event, "memory.updated");
            assert_eq!(evt.ts, 1741860000);
            assert_eq!(evt.data["topic"], "test");
        }
    }

    #[test]
    fn test_frame_discrimination() {
        // Request: has "action" field
        let req_json = json!({"id": "r1", "action": "memory.search", "params": {}});
        let frame: Frame = serde_json::from_value(req_json).unwrap();
        assert!(frame.is_request());

        // Response: has "ok" + "id" fields
        let resp_json = json!({"id": "r1", "ok": true, "result": null, "meta": {}});
        let frame: Frame = serde_json::from_value(resp_json).unwrap();
        assert!(frame.is_response());

        // Event: has "event" field
        let evt_json = json!({"event": "memory.updated", "ts": 12345, "data": {}});
        let frame: Frame = serde_json::from_value(evt_json).unwrap();
        assert!(frame.is_event());
    }

    #[test]
    fn test_oversized_frame_rejected() {
        let mut codec = NomenCodec::with_max_frame_size(100);
        let big_payload = "x".repeat(200);
        let frame = Frame::Request(Request {
            id: "big".to_string(),
            action: "test".to_string(),
            params: json!({"data": big_payload}),
        });
        let mut buf = BytesMut::new();
        // Encoding might succeed (codec encodes then inner checks)
        // But decode of oversized should fail
        let mut encode_codec = NomenCodec::new(); // Use unlimited for encoding
        encode_codec.encode(frame, &mut buf).unwrap();
        let result = codec.decode(&mut buf);
        assert!(result.is_err());
    }

    #[test]
    fn test_malformed_json_rejected() {
        let mut codec = NomenCodec::new();
        // Manually construct a length-delimited frame with bad JSON
        let bad_json = b"not valid json{{{";
        let len = bad_json.len() as u32;
        let mut buf = BytesMut::new();
        buf.extend_from_slice(&len.to_be_bytes());
        buf.extend_from_slice(bad_json);
        let result = codec.decode(&mut buf);
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_params_defaults() {
        let json_str = r#"{"id": "r1", "action": "memory.list"}"#;
        let req: Request = serde_json::from_str(json_str).unwrap();
        assert!(req.params.is_object());
        assert!(req.params.as_object().unwrap().is_empty());
    }
}
