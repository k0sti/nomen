// Centralized d-tag parsing — mirrors Rust memory.rs semantics.
//
// v0.2 format: {visibility}:{scope}:{topic}
//   - First colon separates visibility
//   - Last colon separates topic
//   - Everything between is scope (may contain colons)
//
// Examples:
//   "public::rust-error-handling"                         → { visibility: "public", scope: "", topic: "rust-error-handling" }
//   "group:techteam:deployment"                           → { visibility: "group", scope: "techteam", topic: "deployment" }
//   "group:telegram:-1003821690204:room/8485"             → { visibility: "group", scope: "telegram:-1003821690204", topic: "room/8485" }
//   "personal:a3f2b1c...:ssh-config"                     → { visibility: "personal", scope: "a3f2b1c...", topic: "ssh-config" }

export interface ParsedDTag {
  visibility: string;
  scope: string;
  topic: string;
}

const V2_PREFIXES = ['public', 'group', 'circle', 'personal', 'internal'];

/** Check if a d-tag uses the v0.2 format. */
export function isV2DTag(dTag: string): boolean {
  const prefix = dTag.split(':')[0] || '';
  return V2_PREFIXES.includes(prefix);
}

/** Parse a v0.2 d-tag into visibility, scope, and topic.
 *  For non-v0.2 d-tags, returns visibility="unknown", scope="", topic=dTag. */
export function parseDTag(dTag: string): ParsedDTag {
  if (!dTag) return { visibility: 'unknown', scope: '', topic: '' };
  if (!isV2DTag(dTag)) return { visibility: 'unknown', scope: '', topic: dTag };

  const firstColon = dTag.indexOf(':');
  if (firstColon === -1) return { visibility: dTag, scope: '', topic: '' };

  const lastColon = dTag.lastIndexOf(':');
  if (lastColon <= firstColon) {
    // Only one colon: visibility + topic, no scope
    return {
      visibility: dTag.substring(0, firstColon),
      scope: '',
      topic: dTag.substring(firstColon + 1),
    };
  }

  return {
    visibility: dTag.substring(0, firstColon),
    scope: dTag.substring(firstColon + 1, lastColon),
    topic: dTag.substring(lastColon + 1),
  };
}

/** Normalize legacy "private" to "personal". */
export function normalizeVisibility(vis: string): string {
  return vis === 'private' ? 'personal' : vis;
}

/** All 5 supported visibilities. */
export const ALL_VISIBILITIES = ['public', 'group', 'circle', 'personal', 'internal'] as const;
export type VisibilityType = (typeof ALL_VISIBILITIES)[number];
