//! NomenSigner trait: abstraction over key management for signing, encryption.
//!
//! Library consumers implement this trait to provide their own key management.

use anyhow::Result;
use async_trait::async_trait;
use nostr_sdk::prelude::*;

/// Trait for signing Nostr events and performing NIP-44 encryption.
///
/// Callers (e.g. Snowclaw) implement this to plug in their own key management.
/// Nomen never holds raw keys — it delegates all crypto to the signer.
#[async_trait]
pub trait NomenSigner: Send + Sync {
    /// Sign an unsigned event.
    async fn sign_event(&self, unsigned: UnsignedEvent) -> Result<Event>;

    /// Get the public key.
    fn public_key(&self) -> PublicKey;

    /// NIP-44 encrypt content (self-encrypt: encrypt to own pubkey).
    fn encrypt(&self, content: &str) -> Result<String>;

    /// NIP-44 decrypt content (self-encrypted).
    fn decrypt(&self, encrypted: &str) -> Result<String>;

    /// NIP-44 encrypt content to a specific recipient pubkey.
    fn encrypt_to(&self, content: &str, recipient: &PublicKey) -> Result<String>;

    /// NIP-44 decrypt content from a specific sender pubkey.
    fn decrypt_from(&self, encrypted: &str, sender: &PublicKey) -> Result<String>;

    /// Get the secret key if available (for nostr-sdk Client initialization).
    /// Returns None for remote/hardware signers.
    fn secret_key(&self) -> Option<&SecretKey>;
}
