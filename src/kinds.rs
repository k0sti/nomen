/// Custom Nostr event kinds for Nomen memory system.
///
/// Replaces generic NIP-78 kind 30078 with dedicated kinds
/// in the 30000-39999 replaceable range (d-tag addressable).

/// Named/consolidated memory event (replaceable).
pub const MEMORY_KIND: u16 = 31234;

/// Agent lesson / behavioral pattern (replaceable).
pub const LESSON_KIND: u16 = 31235;

/// Ephemeral memory / raw message (regular, future use).
pub const EPHEMERAL_MEMORY_KIND: u16 = 1234;

/// Legacy NIP-78 kind (read-only, for backward compatibility).
pub const LEGACY_APP_DATA_KIND: u16 = 30078;

/// Legacy lesson kind (read-only, for backward compatibility).
pub const LEGACY_LESSON_KIND: u16 = 4129;
