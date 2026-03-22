# Architecture

## Overview

`spiki` is split into a thin launcher layer and a long-lived daemon layer.

The launcher is written in Node.js and acts as the public MCP entrypoint. The daemon is written in Rust and owns the shared runtime.

## Runtime Split

### Node.js launcher

- Exposes the MCP server over stdio
- Starts the daemon on demand if no live instance is available
- Reuses an existing per-user daemon when possible
- Bridges client framing differences before forwarding requests
- Provides small operational commands such as `doctor`, `daemon status`, and `daemon stop`

### Rust daemon

- Maintains shared workspace/runtime state across requests
- Handles tool execution
- Owns workspace scanning, file reads, search, and edit plan application
- Tracks Phase 1 semantic backend state

### Core runtime

The core runtime is responsible for:

- canonical root handling
- ignore-aware workspace scanning
- exact span reads and fingerprints
- literal, regex, and whole-word search
- stored edit plan apply/discard flow
- built-in language/profile detection

## Phase 1 Boundaries

Phase 1 is intentionally text-first.

- Workspace inspection is real.
- Search and span reads are real.
- Edit application with revision checks and fingerprints is real.
- Semantic detection is real.
- Semantic execution is still shallow and represented as cached backend lifecycle state.

That means `spiki` can already support reliable workspace introspection and guarded edits, but it is not yet a full syntax or LSP-backed semantic engine.

## Why the daemon exists

The daemon avoids paying full startup and indexing cost on every MCP request. It also gives multiple client sessions a shared runtime without collapsing them into a single view.

The current design aims for:

- shared runtime
- isolated views
- fast repeated reads
- explicit lifecycle control

## Related documents

- [Development guide](./development.md)
- [Language profiles](./language-profiles.md)
- [Specification](../SPEC.md)
