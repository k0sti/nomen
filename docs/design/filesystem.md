# Filesystem Sync — Design

Bidirectional sync between Nomen memories and a local filesystem directory (markdown files).

## Concept

Each memory becomes a markdown file at `{sync_dir}/{d_tag_as_path}.md`. Changes in either direction are detected and synced.

## Commands

```bash
nomen fs init [--dir .]     # Initialize sync directory
nomen fs pull [--dir .]     # DB → filesystem
nomen fs push [--dir .]     # Filesystem → DB
nomen fs status [--dir .]   # Show sync status
nomen fs start [--dir .]    # Real-time bidirectional daemon
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

## Status

Implemented. See `nomen fs` CLI commands.
