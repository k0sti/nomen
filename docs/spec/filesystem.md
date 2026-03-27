# Filesystem Sync

Bidirectional sync between Nomen memories and a local directory of markdown files. Each memory becomes a file; changes in either direction are detected and synced.

## Commands

```bash
nomen fs init [--dir .]     # Initialize sync directory
nomen fs pull [--dir .]     # DB → filesystem
nomen fs push [--dir .]     # Filesystem → DB
nomen fs status [--dir .]   # Show sync status
nomen fs start [--dir .]    # Real-time bidirectional daemon (inotify + DB polling)
nomen fs stop [--dir .]     # Stop daemon
```

## Mapping

D-tag path segments map to directory structure:

```
public/rust-error-handling     → public/rust-error-handling.md
group/techteam/deploy-process  → group/techteam/deploy-process.md
private/agent-reasoning        → private/agent-reasoning.md
```

Content is the memory content (plain text/markdown). Frontmatter may include visibility, scope, importance.

## Real-Time Sync

The `nomen fs start` daemon watches for changes in both directions:

- **Filesystem → DB**: inotify watches for file creates/modifications/deletes, pushes changes to Nomen via `memory.put`/`memory.delete`
- **DB → Filesystem**: periodic polling detects new/updated memories and writes them as markdown files

Conflict resolution: last-write-wins based on timestamp comparison.
