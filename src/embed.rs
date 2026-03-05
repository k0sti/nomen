use anyhow::Result;
use async_trait::async_trait;
use tracing::debug;

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

// ── NoopEmbedder (testing without API key) ───────────────────

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

// ── OpenAI-compatible embedder ───────────────────────────────

pub struct OpenAiEmbedder {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
    dims: usize,
    batch_size: usize,
}

impl OpenAiEmbedder {
    pub fn new(
        base_url: &str,
        api_key: &str,
        model: &str,
        dimensions: usize,
        batch_size: usize,
    ) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            dims: dimensions,
            batch_size,
        }
    }

    fn embeddings_url(&self) -> String {
        let url = reqwest::Url::parse(&self.base_url).ok();
        let path = url
            .as_ref()
            .map(|u| u.path().trim_end_matches('/'))
            .unwrap_or("");

        if path.ends_with("/embeddings") {
            self.base_url.clone()
        } else if !path.is_empty() && path != "/" {
            format!("{}/embeddings", self.base_url)
        } else {
            format!("{}/v1/embeddings", self.base_url)
        }
    }

    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let body = serde_json::json!({
            "model": self.model,
            "input": texts,
            "dimensions": self.dims,
        });

        let resp = self
            .client
            .post(self.embeddings_url())
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Embedding API error {status}: {text}");
        }

        let json: serde_json::Value = resp.json().await?;
        let data = json
            .get("data")
            .and_then(|d| d.as_array())
            .ok_or_else(|| anyhow::anyhow!("Invalid embedding response: missing 'data'"))?;

        let mut embeddings = Vec::with_capacity(data.len());
        for item in data {
            let embedding = item
                .get("embedding")
                .and_then(|e| e.as_array())
                .ok_or_else(|| anyhow::anyhow!("Invalid embedding item: missing 'embedding'"))?;

            #[allow(clippy::cast_possible_truncation)]
            let vec: Vec<f32> = embedding
                .iter()
                .filter_map(|v| v.as_f64().map(|f| f as f32))
                .collect();

            embeddings.push(vec);
        }

        Ok(embeddings)
    }
}

#[async_trait]
impl Embedder for OpenAiEmbedder {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let mut all_embeddings = Vec::with_capacity(texts.len());

        for chunk in texts.chunks(self.batch_size) {
            debug!(
                "Embedding batch of {} texts via {}",
                chunk.len(),
                self.model
            );
            let batch_result = self.embed_batch(&chunk.to_vec()).await?;
            all_embeddings.extend(batch_result);
        }

        Ok(all_embeddings)
    }

    fn dimensions(&self) -> usize {
        self.dims
    }
}

/// Create an embedder from config values.
pub fn create_embedder(
    provider: &str,
    base_url: Option<&str>,
    api_key: &str,
    model: &str,
    dimensions: usize,
    batch_size: usize,
) -> Box<dyn Embedder> {
    match provider {
        "openai" => Box::new(OpenAiEmbedder::new(
            base_url.unwrap_or("https://api.openai.com"),
            api_key,
            model,
            dimensions,
            batch_size,
        )),
        "openrouter" => Box::new(OpenAiEmbedder::new(
            base_url.unwrap_or("https://openrouter.ai/api/v1"),
            api_key,
            model,
            dimensions,
            batch_size,
        )),
        _ => Box::new(NoopEmbedder),
    }
}
