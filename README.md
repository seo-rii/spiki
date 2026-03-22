# spiki

`spiki`는 에이전트가 텍스트 패치 대신 구조적 편집 흐름으로 코드베이스를 다루게 하기 위한 editor MCP 참조 구현이다.

현재 구현 범위는 `SPEC.md`의 Phase 1 기준이다.

- Node launcher가 per-user daemon을 탐색/기동하고 stdio bridge를 제공
- Rust daemon이 shared workspace/runtime을 유지
- semantic status는 detected leaf profile과 skeleton lifecycle state를 노출
- public MCP tools:
  - `ae.workspace.status`
  - `ae.workspace.read_spans`
  - `ae.workspace.search_text`
  - `ae.edit.apply_plan`
  - `ae.edit.discard_plan`
  - `ae.semantic.status`
  - `ae.semantic.ensure`

## 개발

Rust daemon 빌드:

```bash
node ./scripts/build-daemon.mjs
```

smoke test:

```bash
npm run test:smoke
```

Codex integration test:

```bash
npm run test:codex
```

`test:codex`는 시스템의 `codex` CLI가 있으면 그 바이너리를 사용하고, 없으면 `npx @openai/codex`로 fallback 한다.

전체 integration test:

```bash
npm run test:integration
```
