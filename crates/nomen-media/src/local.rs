//! Local filesystem media store — SHA-256 content-addressed storage.
//!
//! Files are stored as `{media_dir}/{sha256}.{ext}` where the extension
//! is derived from the MIME type. This naming convention is Blossom-compatible,
//! making future migration to a Blossom server a simple path swap.

use std::path::PathBuf;

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use tracing::debug;

use crate::{mime_to_ext, MediaRef, MediaStore};

/// Local filesystem media store.
///
/// Stores files by SHA-256 hash in a configurable directory.
/// Content-addressed: identical files are stored once.
pub struct LocalMediaStore {
    /// Root directory for media storage.
    media_dir: PathBuf,
}

impl LocalMediaStore {
    /// Create a new LocalMediaStore with the given directory.
    /// Creates the directory if it doesn't exist.
    pub fn new(media_dir: impl Into<PathBuf>) -> Result<Self> {
        let media_dir = media_dir.into();
        std::fs::create_dir_all(&media_dir)
            .with_context(|| format!("failed to create media dir: {}", media_dir.display()))?;
        Ok(Self { media_dir })
    }

    /// Get the default media directory (~/.nomen/media/).
    pub fn default_dir() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".nomen")
            .join("media")
    }

    /// Create a store using the default directory.
    pub fn default() -> Result<Self> {
        Self::new(Self::default_dir())
    }

    /// Build the file path for a given hash and extension.
    fn file_path(&self, sha256: &str, ext: &str) -> PathBuf {
        self.media_dir.join(format!("{sha256}.{ext}"))
    }

    /// Find an existing file by hash (trying common extensions).
    fn find_by_hash(&self, sha256: &str) -> Option<PathBuf> {
        let exts = [
            "jpg", "png", "gif", "webp", "svg", "bmp", "tiff", "mp4", "webm", "mov", "mp3",
            "ogg", "wav", "weba", "pdf", "txt", "bin",
        ];
        for ext in &exts {
            let path = self.file_path(sha256, ext);
            if path.exists() {
                return Some(path);
            }
        }
        None
    }
}

#[async_trait::async_trait]
impl MediaStore for LocalMediaStore {
    async fn store(&self, data: &[u8], mime_type: &str) -> Result<MediaRef> {
        let mut hasher = Sha256::new();
        hasher.update(data);
        let hash_bytes = hasher.finalize();
        let sha256 = hex::encode(hash_bytes);

        let ext = mime_to_ext(mime_type);
        let path = self.file_path(&sha256, ext);

        // Deduplication: if file exists, return existing ref
        if path.exists() {
            let metadata = std::fs::metadata(&path)?;
            debug!(sha256 = %sha256, "File already exists, skipping write");
            return Ok(MediaRef {
                sha256,
                path,
                size: metadata.len(),
                mime_type: mime_type.to_string(),
            });
        }

        // Also check if it exists with a different extension
        if let Some(existing) = self.find_by_hash(&sha256) {
            let metadata = std::fs::metadata(&existing)?;
            debug!(sha256 = %sha256, "File exists with different extension");
            return Ok(MediaRef {
                sha256,
                path: existing,
                size: metadata.len(),
                mime_type: mime_type.to_string(),
            });
        }

        std::fs::write(&path, data)
            .with_context(|| format!("failed to write media file: {}", path.display()))?;

        debug!(sha256 = %sha256, path = %path.display(), size = data.len(), "Stored media file");

        Ok(MediaRef {
            sha256,
            path,
            size: data.len() as u64,
            mime_type: mime_type.to_string(),
        })
    }

    async fn exists(&self, sha256: &str) -> Result<bool> {
        Ok(self.find_by_hash(sha256).is_some())
    }

    async fn get(&self, sha256: &str) -> Result<Option<Vec<u8>>> {
        match self.find_by_hash(sha256) {
            Some(path) => {
                let data = std::fs::read(&path)
                    .with_context(|| format!("failed to read media file: {}", path.display()))?;
                Ok(Some(data))
            }
            None => Ok(None),
        }
    }

    async fn path(&self, sha256: &str) -> Option<PathBuf> {
        self.find_by_hash(sha256)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn store_and_retrieve() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalMediaStore::new(dir.path()).unwrap();

        let data = b"hello world jpeg data";
        let result = store.store(data, "image/jpeg").await.unwrap();

        assert!(!result.sha256.is_empty());
        assert!(result.path.exists());
        assert_eq!(result.size, data.len() as u64);
        assert_eq!(result.mime_type, "image/jpeg");
        assert!(result.path.to_str().unwrap().ends_with(".jpg"));

        // Verify content
        let retrieved = store.get(&result.sha256).await.unwrap().unwrap();
        assert_eq!(retrieved, data);
    }

    #[tokio::test]
    async fn deduplication() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalMediaStore::new(dir.path()).unwrap();

        let data = b"identical content";
        let r1 = store.store(data, "image/png").await.unwrap();
        let r2 = store.store(data, "image/png").await.unwrap();

        assert_eq!(r1.sha256, r2.sha256);
        assert_eq!(r1.path, r2.path);

        // Only one file on disk
        let files: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
        assert_eq!(files.len(), 1);
    }

    #[tokio::test]
    async fn different_files() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalMediaStore::new(dir.path()).unwrap();

        let r1 = store.store(b"file one", "image/jpeg").await.unwrap();
        let r2 = store.store(b"file two", "image/jpeg").await.unwrap();

        assert_ne!(r1.sha256, r2.sha256);
        assert_ne!(r1.path, r2.path);

        let files: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
        assert_eq!(files.len(), 2);
    }

    #[tokio::test]
    async fn exists_and_path() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalMediaStore::new(dir.path()).unwrap();

        let result = store.store(b"test data", "text/plain").await.unwrap();

        assert!(store.exists(&result.sha256).await.unwrap());
        assert!(store.path(&result.sha256).await.is_some());

        assert!(!store.exists("nonexistent").await.unwrap());
        assert!(store.path("nonexistent").await.is_none());
    }

    #[tokio::test]
    async fn sha256_matches_filename() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalMediaStore::new(dir.path()).unwrap();

        let data = b"verify hash";
        let result = store.store(data, "image/jpeg").await.unwrap();

        let filename = result.path.file_stem().unwrap().to_str().unwrap();
        assert_eq!(filename, result.sha256);
    }
}
