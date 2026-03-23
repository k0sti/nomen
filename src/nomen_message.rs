//! impl Nomen — message ingestion, query, send, session resolution.

use anyhow::Result;

use crate::ingest::{MessageQuery, RawMessage, RawMessageRecord};
use crate::Nomen;

impl Nomen {
    /// Ingest a raw message for later consolidation.
    pub async fn ingest_message(&self, msg: RawMessage) -> Result<String> {
        crate::ingest::ingest_message(&self.db, &msg).await
    }

    /// Query raw messages with filters.
    pub async fn get_messages(&self, opts: MessageQuery) -> Result<Vec<RawMessageRecord>> {
        crate::ingest::get_messages(&self.db, &opts).await
    }

    /// Resolve a session ID to tier/scope/channel using the loaded groups.
    pub fn resolve_session(
        &self,
        session_id: &str,
        default_channel: &str,
    ) -> Result<crate::session::ResolvedSession> {
        crate::session::resolve_session(session_id, &self.groups, default_channel)
    }

    /// Send a message via relay.
    pub async fn send(&self, opts: crate::send::SendOptions) -> Result<crate::send::SendResult> {
        let relay = self
            .relay
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No relay configured for sending"))?;
        crate::send::send_message(relay, &self.db, &self.groups, opts).await
    }
}
