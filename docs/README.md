# Documentation

This directory contains project-facing documentation for the current `spiki` reference build.

`SPEC.md` in the repository root remains the normative design document. The files here are shorter operational guides for contributors and integrators.

## Guides

- [Architecture](./architecture.md): launcher, daemon, runtime boundaries, and current Phase 1 responsibilities
- [Development guide](./development.md): build, run, test, and local MCP integration workflow
- [Language profiles](./language-profiles.md): how Phase 1 detects language profiles and what semantic state means today
- [Specification](../SPEC.md): full product and protocol design

## Reading Order

1. Start with the repository [README](../README.md) for the high-level project overview.
2. Read [architecture](./architecture.md) to understand the runtime split.
3. Use [development](./development.md) for day-to-day local work.
4. Use [language profiles](./language-profiles.md) when touching semantic detection behavior.
5. Use [SPEC.md](../SPEC.md) when you need the broader contract or future-phase design.
