# spiki

`spiki` is an editor-oriented MCP reference implementation for agents that need stable workspace operations instead of ad hoc text patching.

The current codebase implements the Phase 1 slice of [SPEC.md](./SPEC.md): workspace inspection, precise text reads, ignore-aware search, compare-and-swap edit application, and a lightweight semantic backend registry.

## Highlights

- Node.js launcher with MCP stdio bridging and on-demand daemon startup
- Rust daemon with shared workspace/runtime state across requests
- Ignore-aware workspace scanning, exact span reads, and text search
- CAS-style edit plan apply/discard flow
- Built-in language profile detection for common web and general-purpose stacks
- Phase 1 semantic lifecycle cache with `warm`, `refresh`, and `stop`

## Public MCP Tools

| Tool | Purpose |
| --- | --- |
| `ae.workspace.status` | Return roots, workspace revision, coverage, and backend summary for the active view. |
| `ae.workspace.read_spans` | Read exact ranges from files with optional surrounding context and fingerprints. |
| `ae.workspace.search_text` | Run literal, regex, or whole-word text search across the workspace. |
| `ae.edit.apply_plan` | Apply a previously prepared edit plan after compare-and-swap validation. |
| `ae.edit.discard_plan` | Discard a prepared edit plan without mutating files. |
| `ae.semantic.status` | Report detected leaf semantic profiles and their cached lifecycle state. |
| `ae.semantic.ensure` | Warm, refresh, or stop the cached semantic state for a language profile. |

## Quick Start

### Prerequisites

- A recent Node.js runtime
- A Rust toolchain with `cargo`

### Build the daemon

```bash
node ./scripts/build-daemon.mjs
```

### Run diagnostics

```bash
node ./bin/spiki.js doctor
```

### Start the MCP bridge

```bash
node ./bin/spiki.js
```

The launcher is the public entrypoint. It will start or reuse the per-user daemon automatically.

## Documentation

- [Documentation index](./docs/README.md)
- [Architecture](./docs/architecture.md)
- [Development guide](./docs/development.md)
- [Language profiles](./docs/language-profiles.md)
- [Full specification](./SPEC.md)

## Current Scope

- `spiki` is still a reference build, not a complete production editor runtime.
- Phase 1 focuses on reliable text and workspace operations.
- Semantic lifecycle support is currently a cached skeleton state, not a full semantic engine.
- The specification is intentionally ahead of the current implementation in some areas.
