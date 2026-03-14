# Nomen API Reference

**Version:** 1.0
**Date:** 2026-03-14

> **This document has been fully replaced by the API v2 canonical dispatch model.** The legacy `nomen_*` MCP tool names, custom kind 21900/21901 ContextVM protocol, and per-interface parameter tables documented here no longer exist. All operations now route through the shared canonical dispatch layer (`src/api/dispatch.rs`).
>
> See [`api-v2-spec.md`](api-v2-spec.md) for the current specification covering all 21 operations, the response envelope format, and transport mappings for HTTP, MCP, ContextVM, and socket.

## CLI Commands

The CLI remains available for human operators and scripts. See `nomen --help` or the [architecture doc](architecture.md) for the full command list.
