/// Custom Nostr event kinds for Nomen memory system.
///
/// Replaces generic NIP-78 kind 30078 with dedicated kinds
/// in the 30000-39999 replaceable range (d-tag addressable).

/// Named/consolidated memory event (replaceable).
pub const MEMORY_KIND: u16 = 31234;

/// Ephemeral memory / raw message (regular, future use).
pub const EPHEMERAL_MEMORY_KIND: u16 = 1234;

/// Legacy NIP-78 kind (read-only, for backward compatibility).
pub const LEGACY_APP_DATA_KIND: u16 = 30078;

/// Agent lesson (replaceable).
pub const LESSON_KIND: u16 = 31235;

/// Legacy lesson kind (read-only compat).
pub const LEGACY_LESSON_KIND: u16 = 4129;

/// Raw source event (regular, non-replaceable).
/// Append-only ground-truth layer for ingested messages from any provider.
pub const RAW_SOURCE_KIND: u16 = 1235;

/// Collected message (parameterized replaceable).
/// Bridged or native messages from any platform, stored as Nostr events.
pub const COLLECTED_MESSAGE_KIND: u16 = 30100;

/// Chat metadata (parameterized replaceable).
/// One per chat, updated when metadata changes.
pub const CHAT_METADATA_KIND: u16 = 30101;
