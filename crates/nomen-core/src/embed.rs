use anyhow::Result;
use async_trait::async_trait;

/// Trait for embedding providers — convert text to vectors.
#[async_trait]
pub trait Embedder: Send + Sync {
    /// Embed a batch of texts into vectors.
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;

    /// Embedding dimensions (0 means no embeddings).
    fn dimensions(&self) -> usize;

    /// Embed a single text.
    async fn embed_one(&self, text: &str) -> Result<Vec<f32>> {
        let mut results = self.embed(&[text.to_string()]).await?;
        results
            .pop()
            .ok_or_else(|| anyhow::anyhow!("Empty embedding result"))
    }
}

// -- NoopEmbedder (testing without API key) ---

pub struct NoopEmbedder;

#[async_trait]
impl Embedder for NoopEmbedder {
    async fn embed(&self, _texts: &[String]) -> Result<Vec<Vec<f32>>> {
        Ok(Vec::new())
    }

    fn dimensions(&self) -> usize {
        0
    }
}
