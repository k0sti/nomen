/// Custom Nostr event kinds for Nomen memory system.

/// Named/consolidated memory event (replaceable).
pub const MEMORY_KIND: u16 = 31234;

/// Collected message (parameterized replaceable).
/// Bridged or native messages from any platform, stored as Nostr events.
pub const COLLECTED_MESSAGE_KIND: u16 = 30100;

/// Chat metadata (parameterized replaceable).
/// One per chat, updated when metadata changes.
pub const CHAT_METADATA_KIND: u16 = 30101;

/// Group definition (parameterized replaceable, NIP-78 app data).
/// Tags: d (nomen:group:<id>), name, member (pubkey hex), relay (optional).
pub const GROUP_KIND: u16 = 30078;
