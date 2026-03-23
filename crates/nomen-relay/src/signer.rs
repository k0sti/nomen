//! KeysSigner: default NomenSigner implementation wrapping `nostr_sdk::Keys`.
//!
//! Used by CLI and tests. Library consumers implement [`NomenSigner`] directly
//! with their own key management.

use anyhow::Result;
use async_trait::async_trait;
use nostr_sdk::prelude::nip44;
use nostr_sdk::prelude::*;

use nomen_core::signer::NomenSigner;

/// Default signer wrapping `nostr_sdk::Keys` — used by CLI and tests.
pub struct KeysSigner {
    keys: Keys,
}

impl KeysSigner {
    pub fn new(keys: Keys) -> Self {
        Self { keys }
    }

    /// Get a reference to the underlying Keys.
    pub fn keys(&self) -> &Keys {
        &self.keys
    }
}

impl From<Keys> for KeysSigner {
    fn from(keys: Keys) -> Self {
        Self::new(keys)
    }
}

#[async_trait]
impl NomenSigner for KeysSigner {
    async fn sign_event(&self, unsigned: UnsignedEvent) -> Result<Event> {
        let event = unsigned
            .sign(&self.keys)
            .await
            .map_err(|e| anyhow::anyhow!("Event signing failed: {e}"))?;
        Ok(event)
    }

    fn public_key(&self) -> PublicKey {
        self.keys.public_key()
    }

    fn encrypt(&self, content: &str) -> Result<String> {
        let encrypted = nip44::encrypt(
            self.keys.secret_key(),
            &self.keys.public_key(),
            content,
            nip44::Version::default(),
        )
        .map_err(|e| anyhow::anyhow!("NIP-44 encryption failed: {e}"))?;
        Ok(encrypted)
    }

    fn decrypt(&self, encrypted: &str) -> Result<String> {
        let decrypted = nip44::decrypt(self.keys.secret_key(), &self.keys.public_key(), encrypted)
            .map_err(|e| anyhow::anyhow!("NIP-44 decryption failed: {e}"))?;
        Ok(decrypted)
    }

    fn encrypt_to(&self, content: &str, recipient: &PublicKey) -> Result<String> {
        let encrypted = nip44::encrypt(
            self.keys.secret_key(),
            recipient,
            content,
            nip44::Version::default(),
        )
        .map_err(|e| anyhow::anyhow!("NIP-44 encryption failed: {e}"))?;
        Ok(encrypted)
    }

    fn decrypt_from(&self, encrypted: &str, sender: &PublicKey) -> Result<String> {
        let decrypted = nip44::decrypt(self.keys.secret_key(), sender, encrypted)
            .map_err(|e| anyhow::anyhow!("NIP-44 decryption failed: {e}"))?;
        Ok(decrypted)
    }

    fn secret_key(&self) -> Option<&SecretKey> {
        Some(self.keys.secret_key())
    }
}
