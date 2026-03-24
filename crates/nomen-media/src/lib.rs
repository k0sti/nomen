//! nomen-media — media storage abstraction for Nomen.
//!
//! Provides a `MediaStore` trait and a local filesystem implementation
//! using SHA-256 content-addressing (Blossom-compatible naming).

pub mod local;

use std::path::PathBuf;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Reference to a stored media file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaRef {
    /// SHA-256 hash of the file content (hex-encoded).
    pub sha256: String,
    /// Local filesystem path where the file is stored.
    pub path: PathBuf,
    /// File size in bytes.
    pub size: u64,
    /// MIME type of the file.
    pub mime_type: String,
}

/// Trait for media storage backends.
///
/// Implementations store binary data by content hash and provide retrieval.
/// The SHA-256 hash is the stable identifier across all backends.
#[async_trait]
pub trait MediaStore: Send + Sync {
    /// Store binary data with the given MIME type.
    /// Returns a reference with sha256, path, and size.
    /// Deduplicates: if the hash already exists, returns existing ref without re-writing.
    async fn store(&self, data: &[u8], mime_type: &str) -> Result<MediaRef>;

    /// Check if a file with the given SHA-256 hash exists.
    async fn exists(&self, sha256: &str) -> Result<bool>;

    /// Read the file content by SHA-256 hash.
    /// Returns None if the file doesn't exist.
    async fn get(&self, sha256: &str) -> Result<Option<Vec<u8>>>;

    /// Get the local filesystem path for a file by SHA-256 hash.
    /// Returns None if the file doesn't exist.
    async fn path(&self, sha256: &str) -> Option<PathBuf>;
}

/// A no-op media store that always returns errors.
/// Used when no media storage is configured.
pub struct NoopMediaStore;

#[async_trait]
impl MediaStore for NoopMediaStore {
    async fn store(&self, _data: &[u8], _mime_type: &str) -> Result<MediaRef> {
        anyhow::bail!("no media store configured")
    }

    async fn exists(&self, _sha256: &str) -> Result<bool> {
        Ok(false)
    }

    async fn get(&self, _sha256: &str) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }

    async fn path(&self, _sha256: &str) -> Option<PathBuf> {
        None
    }
}

/// Derive file extension from MIME type.
pub fn mime_to_ext(mime_type: &str) -> &str {
    match mime_type {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/svg+xml" => "svg",
        "image/bmp" => "bmp",
        "image/tiff" => "tiff",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "video/quicktime" => "mov",
        "audio/mpeg" | "audio/mp3" => "mp3",
        "audio/ogg" => "ogg",
        "audio/wav" => "wav",
        "audio/webm" => "weba",
        "application/pdf" => "pdf",
        "text/plain" => "txt",
        _ => "bin",
    }
}
