# spiki

`spiki` is an editor-oriented MCP reference implementation for agents that need stable workspace operations instead of ad hoc text patching.

The current codebase implements the Phase 1 slice of [SPEC.md](./SPEC.md): workspace inspection, precise text reads, ignore-aware search, compare-and-swap edit application, and a lightweight semantic backend registry.

## Highlights

- Node.js launcher with MCP stdio bridging and on-demand daemon startup
- Rust daemon with shared workspace/runtime state across requests
- Local transport over Unix domain sockets on Unix-like hosts and current-user-scoped named pipes on Windows
- Ignore-aware workspace scanning, exact span reads, and text search
- CAS-style edit plan prepare/apply/discard flow
- Built-in language profile detection for common web and general-purpose stacks
- Phase 1 semantic lifecycle cache with `warm`, `refresh`, and `stop`

## Public MCP Tools

| Tool | Purpose |
| --- | --- |
| `ae.workspace.status` | Return roots, workspace revision, coverage, and backend summary for the active view. |
| `ae.workspace.read_spans` | Read exact ranges from files with optional surrounding context and fingerprints. |
| `ae.workspace.search_text` | Run literal, regex, or whole-word text search across the workspace, with `scope.includeDefaultExcluded` available when you need to traverse default-excluded directories. |
| `ae.edit.prepare_plan` | Validate and store a new edit plan for later apply or discard. |
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
By default, runtime state lives under the current user's local cache/runtime directory and can be overridden with `AGENT_EDITOR_RUNTIME_DIR` or `SPIKI_RUNTIME_DIR`.
Clients are expected to provide MCP roots during `initialize`.
If you need to support a roots-less client, opt in explicitly with `SPIKI_ALLOW_CWD_ROOT_FALLBACK=1` and run the launcher from the workspace directory you want to expose.
Default exclude components such as `dist`, `target`, or `coverage` are configurable defaults, not forced hides.
Use `scope.includeDefaultExcluded=true` for one search, or set `SPIKI_DEFAULT_EXCLUDE_COMPONENTS` / `SPIKI_FORCED_EXCLUDE_COMPONENTS` on the daemon process for global policy changes.

### Intended npm surface

The launcher package is prepared to publish as `spiki`.
Until an npm release is actually published, use the repository checkout or `npm pack` output from this repository.

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
- The current server capability surface is tools-only; resources, tasks, and progress flows remain out of scope for this phase.
- Roots-less clients are rejected by default to avoid implicit ACL expansion; the current launcher only allows `cwd` fallback behind `SPIKI_ALLOW_CWD_ROOT_FALLBACK=1`.
- The specification is intentionally ahead of the current implementation in some areas.
