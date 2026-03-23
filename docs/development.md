# Development Guide

## Prerequisites

- A recent Node.js runtime
- A Rust toolchain with `cargo`

## Build the daemon

```bash
node ./scripts/build-daemon.mjs
```

This produces the local Rust daemon binary used by the launcher.

On Unix-like hosts the launcher talks to the daemon over a Unix domain socket. On Windows it uses a named pipe.

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

## Testing

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
