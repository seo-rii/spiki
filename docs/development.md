# Development Guide

## Prerequisites

- A recent Node.js runtime
- A Rust toolchain with `cargo`

## Build the daemon

```bash
node ./scripts/build-daemon.mjs
```

This produces the local Rust daemon binary used by the launcher.

On Unix-like hosts the launcher talks to the daemon over a Unix domain socket. On Windows it uses a named pipe created with a current-user-only security descriptor plus `reject_remote_clients(true)`.

## Packaging and distribution

Current repository checkouts are development-oriented. If the launcher cannot find a daemon binary in the packaged native bundle or the local build output, it falls back to building `spiki-daemon` from source, which means local development still expects a Rust toolchain.

The current package flow is:

- `npm run prepare:package` builds the local daemon and stages it under `bin/native/<platform>-<arch>/`
- `npm pack` includes that current-platform daemon in the package payload
- `launcher/runtime-paths.mjs` resolves the packaged daemon first and falls back to `target/debug`

The intended longer-range release model is still broader:

- the npm package provides the public `spiki` launcher surface
- platform-specific native daemon artifacts are resolved first at runtime
- source builds remain a fallback for repository checkouts and unsupported targets, not the primary install path

Until prebuilt multi-platform daemon artifacts are wired into the release flow, `npm pack` output from this repository should still be treated as a developer snapshot rather than a zero-toolchain end-user release.

## Common commands

### Start the MCP bridge

```bash
node ./bin/spiki.js
```

### Inspect local runtime state

```bash
node ./bin/spiki.js doctor
node ./bin/spiki.js daemon status
```

### Stop the daemon

```bash
node ./bin/spiki.js daemon stop
```

## Project configuration

Workspace-local overrides are loaded from these files when present:

- `spiki.yaml`
- `.spiki/config.yaml`
- `spiki.languages.yaml`
- `.spiki/languages.yaml`

Runtime config can override index size limits, plan TTL, exclude defaults, and watcher policy.
Language config can override semantic bindings, including custom LSP command lines.

## Testing

### CI entrypoint

```bash
npm run ci
```

This is the merge-gated test entrypoint used by the GitHub Actions smoke workflows. It builds the daemon, runs `cargo test --workspace`, and then runs the launcher-focused Node test suite (`bootstrap-race`, `package-metadata`, `program-exec`, `runtime-paths`).

### Rust tests

```bash
cargo test --workspace
```

### Program execution integration

```bash
npm run test:smoke
```

### Codex integration

```bash
npm run test:codex
```

`test:codex` uses the system `codex` binary when available and falls back to `npx @openai/codex` otherwise.

### Plugin scaffold

```bash
node ./bin/spiki.js plugin scaffold codex ./tmp/spiki-codex-plugin
node ./bin/spiki.js plugin scaffold claude ./tmp/spiki-claude-plugin
```

Use `--allow-cwd-root-fallback` when you need a generated bundle to expose the current working directory as an implicit root for clients that do not send `initialize.params.roots`.

### Full integration pass

```bash
npm run test:integration
```

## MCP client setup

Use the launcher as the MCP entrypoint:

```json
{
  "mcpServers": {
    "spiki": {
      "command": "node",
      "args": ["/absolute/path/to/spiki/bin/spiki.js"],
      "cwd": "/absolute/path/to/workspace"
    }
  }
}
```

The launcher will start or reuse the daemon automatically.

## Project structure

- `bin/`: executable entrypoint
- `launcher/`: Node.js launcher and transport bridge
- `crates/spiki-core/`: workspace, text, edit, and language profile runtime
- `crates/spiki-daemon/`: daemon and MCP tool dispatch
- `tests/`: integration tests
- `SPEC.md`: normative design document

## Related documents

- [Architecture](./architecture.md)
- [Language profiles](./language-profiles.md)
- [Specification](../SPEC.md)
