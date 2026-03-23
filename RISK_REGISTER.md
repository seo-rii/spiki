# spiki Phase 1 리스크 레지스터 (1차)

작성일: 2026-03-23  
기준 커밋: `6695251` (`feat: add windows named-pipe daemon transport`)  
범위: `spiki` 레포지토리 (`README`, `docs`, `launcher`, `crates/spiki-core`, `crates/spiki-daemon`, `tests`)  
우선순위: 플랫폼 정합성, runtime ownership 정리 우선  
산출물 형식: finding + backlog + test_backlog 스키마 기반 리스크 레지스터

## 0) 스키마

모든 항목은 아래 분류 중 하나로 기록한다.

- `finding`
- `backlog`
- `test_backlog`

open 항목은 아래 필드를 사용한다.

- `id`
- `severity` (`P0`~`P3`)
- `area`
- `file`
- `evidence`
- `impact`
- `fix_option`
- `test_gap`
- `priority`

## 1) 현재 요약

- `spiki`는 Phase 1 reference build로서 전체 방향이 좋다. Node launcher와 Rust daemon의 역할 분리가 선명하고, public tool surface가 작아서 이해와 확장이 쉽다.
- 현재 public Phase 1 도구 8개(`ae.workspace.status`, `ae.workspace.read_spans`, `ae.workspace.search_text`, `ae.edit.prepare_plan`, `ae.edit.apply_plan`, `ae.edit.discard_plan`, `ae.semantic.status`, `ae.semantic.ensure`)는 코드와 테스트 흐름에서 대체로 일치한다.
- `apply_plan`은 `workspace_revision`, view ownership, file fingerprint를 다시 검증하고 있어서 최소한의 CAS 성격은 이미 확보했다.
- 직전 외부 정적 리뷰에서 지적된 README `docs/` 링크 깨짐은 로컬 `main` 기준으로 해소됐다. 지금은 `README.md`와 `docs/` 구조가 실제 저장소와 맞는다.
- 현재 open risk의 중심은 문서 품질이 아니라 환경별 검증 커버리지 쪽이다. launcher bootstrap race, write path의 인코딩 보존, 외부 edit flow의 미완결성, 기본 search 경로의 이중 스캔, overlapping edit/schema validation, roots fallback 정책, hardcoded scan policy, launcher package metadata 문제, launcher hotspot 문제, schema drift 문제, Windows transport gap, runtime ownership 집중 문제는 로컬 `main` 기준으로 해소됐다.
- 현재 남은 핵심 항목은 Windows host 실기기 smoke 검증 부재다.

## 2) 2026-03-22 외부 정적 리뷰 재평가

| 외부 리뷰 항목 | 현재 상태 | 현재 근거 |
|---|---|---|
| README가 `docs/`를 가리키지만 실제 디렉터리가 없음 | resolved locally | 현재 로컬 `main`에는 `docs/README.md`, `docs/architecture.md`, `docs/development.md`, `docs/language-profiles.md`가 존재한다. |
| npm 배포 모델이 스펙과 다름 | resolved locally | `package.json`은 publishable metadata와 `files` whitelist를 갖춘 launcher package 형태로 정리됐고 `npm pack --dry-run` 검증이 추가됐다. |
| Windows / non-Unix runtime model이 스펙과 다름 | resolved locally | daemon transport는 generic stream 처리로 올라갔고, Windows에서는 named pipe listener를 사용하도록 분기됐다. launcher runtime dir/bootstrap 경로도 플랫폼 공용으로 동작한다. |
| launcher startup race 가능성 | resolved locally | `ensureDaemonRunning()`은 bootstrap lock을 잡고 socket recheck 후 spawn으로 진행하며, concurrent bootstrap regression test가 추가됐다. |
| 파일 인코딩 보존 없이 write | resolved locally | `apply_plan()`은 원본 encoding/BOM을 보존해 temp file + rename 경로로 기록하고, BOM/UTF-16 roundtrip test가 추가됐다. |
| public edit flow 완결성 부족 | resolved locally | public surface에 `ae.edit.prepare_plan`이 추가됐고, prepare -> apply 흐름이 Rust/Node 테스트로 고정됐다. |
| `search_text`의 이중 스캔 | resolved locally | 기본 및 일반 scoped search는 기존 workspace index를 재사용하고, `includeIgnored`/`includeGenerated` 같은 확장 모드에서만 재스캔한다. |
| hardcoded default excludes | resolved locally | default exclude와 forced exclude가 분리됐고, `search_text.scope.includeDefaultExcluded`와 daemon env 설정으로 정책을 바꿀 수 있다. |
| edit overlap / schema validation이 느슨함 | resolved locally | overlapping edit는 `AE_INVALID_REQUEST`로 조기 거부되고, `range`/`scope`/`fingerprint` schema도 더 구체화됐다. |
| roots 없는 client에 `cwd` root 자동 주입 | resolved locally | launcher bridge는 기본적으로 roots 없는 `initialize`를 거부하고, `SPIKI_ALLOW_CWD_ROOT_FALLBACK=1`일 때만 명시 opt-in fallback을 허용한다. |

## 3) 해소되었거나 stale인 항목

| id | 상태 | 현재 근거 |
|---|---|---|
| `R-001` README `docs/` mismatch | resolved locally | README와 `docs/` 디렉터리 구조가 일치하도록 정리됐다. |
| `R-002` semantic.status / ensure contract mismatch | resolved locally | lifecycle action과 leaf-profile 출력은 현재 코드와 테스트에서 맞춰져 있다. |
| `R-003` case-insensitive search offset corruption | resolved locally | case-insensitive literal/word search는 원문 기준 offset을 사용하도록 수정돼 있다. |
| `R-004` launcher startup race | resolved locally | bootstrap lock + recheck 경로가 들어갔고 concurrent `ensureDaemonRunning()` regression test가 추가됐다. |
| `R-005` apply_plan encoding/BOM loss | resolved locally | `apply_plan()`은 UTF-8 BOM, UTF-16LE, UTF-16BE를 보존해 쓰고 관련 regression test가 추가됐다. |
| `R-006` public edit flow completeness | resolved locally | `ae.edit.prepare_plan`이 추가됐고 public prepare -> apply 흐름이 tool surface 기준으로 동작한다. |
| `R-007` default search double scan | resolved locally | 기본 및 일반 scoped `search_text`는 indexed file set을 재사용하고 scan-count regression test가 추가됐다. |
| `R-008` overlapping edit / loose input schema | resolved locally | overlapping edit는 조기 거부되고 `read_spans`, `search_text`, `prepare_plan` schema가 더 구체화됐다. |
| `R-009` roots-less client implicit `cwd` fallback | resolved locally | 기본은 reject로 바뀌었고, roots 없는 client 지원은 `SPIKI_ALLOW_CWD_ROOT_FALLBACK=1`에서만 허용되며 reject/opt-in 테스트가 추가됐다. |
| `R-010` hardcoded scan policy | resolved locally | default exclude는 `RuntimeConfig`와 daemon env로 설정 가능하고, MCP search는 `scope.includeDefaultExcluded`로 per-request override를 제공한다. |
| `R-011` publishable launcher package gap | resolved locally | `package.json`에서 `private`를 제거하고 publish metadata / files whitelist / license를 추가했으며 `npm pack --dry-run` test가 들어갔다. |
| `R-012` launcher runtime hotspot | resolved locally | `runtime.mjs`는 `runtime-paths`, `daemon-bootstrap`, `mcp-bridge`로 분리됐고 bootstrap / bridge regression test가 그대로 통과한다. |
| `R-013` schema automation gap | resolved locally | 주요 tool input schema는 Rust 타입에서 derive되고, `deny_unknown_fields`로 런타임 파서와 advertised schema의 기준을 맞췄다. |
| `R-014` Windows named-pipe transport gap | resolved locally | `spiki-daemon`은 더 이상 Unix-only compile guard에 묶이지 않고 generic stream + Windows named pipe listener 경로를 가진다. launcher는 runtime dir를 플랫폼 공통으로 만들고 같은 bootstrap lock 절차를 사용한다. |
| `R-015` runtime ownership hotspot | resolved locally | shared workspace index 책임은 `runtime/index.rs`로 분리돼 `new`, `upsert_view`, `refresh_workspace`, `current_revision` 경로가 workspace tool logic에서 분리됐다. |

## 4) 현재 Open Finding

현재 open finding은 없다. 남은 항목은 Windows host 검증용 test backlog뿐이다.

## 5) 남은 Backlog

현재 별도 backlog는 없다.

## 6) 검증 기준선 (2026-03-23)

- `cargo test --workspace` : pass
- `node ./scripts/build-daemon.mjs` : pass
- `node --test ./tests/program-exec.test.mjs ./tests/bootstrap-race.test.mjs` : pass
- `node --test ./tests/package-metadata.test.mjs` : pass
- `npm run test:integration` : 환경 의존(Codex CLI quota/availability)으로 변동 가능

## 7) 남은 테스트 Backlog

| test item | 목적 | priority |
|---|---|---|
| `T-001` Windows host smoke | 실제 Windows host에서 named pipe listener bootstrap, attach, tool call, stop 흐름이 end-to-end로 동작하는지 확인한다. | medium |

## 8) 권장 후속 순서

1. `T-001` Windows host smoke를 별도 환경에서 먼저 확인한다.  
   transport path는 들어갔지만 실제 Windows host e2e는 아직 이 환경에서 재확인하지 못했다.
