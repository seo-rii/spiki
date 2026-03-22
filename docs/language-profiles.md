# Language Profiles

## What Phase 1 does

Phase 1 can detect built-in language profiles from workspace files and return the detected leaf profiles through `ae.semantic.status`.

It can also cache a lightweight lifecycle state through `ae.semantic.ensure`:

- `warm` -> `ready`
- `refresh` -> `ready`
- `stop` -> `off`

This is a runtime state cache, not a full semantic engine.

## Detection model

The current runtime uses a mix of:

- file extensions
- common project markers
- `package.json` dependencies for JavaScript and framework ecosystems
- explicit requested profile names passed to `ae.semantic.ensure`

When multiple profiles exist in a hierarchy, Phase 1 prefers leaf profiles in outward-facing status results.

## Built-in coverage

### Web stacks

The runtime includes built-in detection for common JavaScript and TypeScript ecosystems, including profiles such as:

- `javascript`, `nodejs`, `typescript`, `node-ts`
- `react`, `react-ts`, `preact`
- `nextjs`, `remix`, `gatsby`
- `vue`, `nuxt`
- `svelte`, `sveltekit`
- `angular`
- `astro`
- `solid`, `solidstart`
- `qwik`, `ember`, `lit`, `alpine`

### General-purpose languages

The runtime also includes built-in coverage for common compiled and scripting languages, including families such as:

- C and C++
- Java and Kotlin
- Python
- Go
- Rust
- Ruby
- Swift
- .NET languages
- Scala
- Haskell
- OCaml
- PHP, Perl, Lua, shell, assembly
- Objective-C and Objective-C++
- Fortran, Scheme, Ada, Awk, Tcl, R, Julia, Clojure, Common Lisp, Erlang, Elixir, Dart, Nim, Prolog, FreeBASIC, Haxe, and SystemVerilog

The codebase uses toolchain-specific or framework-specific profiles when they can be inferred, for example `cargo-rust`, `pyproject-python`, or `react-ts`.

## Relationship to the specification

The broader hierarchical language model, including profile inheritance and custom YAML-defined bindings, is described in [SPEC.md](../SPEC.md).

The current runtime implements a practical Phase 1 subset of that model.

## Related documents

- [Architecture](./architecture.md)
- [Development guide](./development.md)
- [Specification](../SPEC.md)
