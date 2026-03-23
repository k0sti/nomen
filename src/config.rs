pub use nomen_core::config::*;

/// Extension trait for ContextVmConfig methods that need contextvm_sdk.
pub trait ContextVmConfigExt {
    fn encryption_mode(&self) -> contextvm_sdk::EncryptionMode;
}

impl ContextVmConfigExt for ContextVmConfig {
    /// Parse the encryption mode string into the SDK enum.
    fn encryption_mode(&self) -> contextvm_sdk::EncryptionMode {
        match self.encryption.to_lowercase().as_str() {
            "required" => contextvm_sdk::EncryptionMode::Required,
            "disabled" => contextvm_sdk::EncryptionMode::Disabled,
            _ => contextvm_sdk::EncryptionMode::Optional,
        }
    }
}

/// Extension trait for Config methods that need heavy deps (reqwest, nostr-sdk).
pub trait ConfigExt {
    fn build_embedder(&self) -> Box<dyn crate::embed::Embedder>;
    fn build_signer(&self) -> Option<std::sync::Arc<dyn crate::signer::NomenSigner>>;
}

impl ConfigExt for Config {
    /// Build an embedder from config. Returns NoopEmbedder if no config or no API key.
    fn build_embedder(&self) -> Box<dyn crate::embed::Embedder> {
        let Some(ref emb) = self.embedding else {
            return Box::new(crate::embed::NoopEmbedder);
        };

        let api_key = emb
            .api_key
            .clone()
            .unwrap_or_else(|| std::env::var(&emb.api_key_env).unwrap_or_default());
        if api_key.is_empty() {
            tracing::warn!(
                "Embedding API key env {} not set, using NoopEmbedder",
                emb.api_key_env
            );
            return Box::new(crate::embed::NoopEmbedder);
        }

        nomen_llm::embed::create_embedder(
            &emb.provider,
            emb.base_url.as_deref(),
            &api_key,
            &emb.model,
            emb.dimensions,
            emb.batch_size,
        )
    }

    /// Build a signer from the first nsec in config.
    ///
    /// Returns `None` if no nsec is configured (library callers provide their own signer).
    fn build_signer(&self) -> Option<std::sync::Arc<dyn crate::signer::NomenSigner>> {
        let nsecs = self.all_nsecs();
        let nsec = nsecs.first()?;
        let keys = nostr_sdk::Keys::parse(nsec).ok()?;
        Some(std::sync::Arc::new(crate::signer::KeysSigner::new(keys)))
    }
}
