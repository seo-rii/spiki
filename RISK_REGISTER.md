# spiki Phase 1 리스크 레지스터 (1차)

작성일: 2026-03-22  
기준 커밋: `7f7459f` (`docs: reorganize project documentation`)  
범위: `spiki` 레포지토리 (`README`, `docs`, `launcher`, `crates/spiki-core`, `crates/spiki-daemon`, `tests`)  
우선순위: public edit flow 완결성, 스캔/validation 정교화, roots fallback 정책 우선  
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
- 현재 public Phase 1 도구 7개(`ae.workspace.status`, `ae.workspace.read_spans`, `ae.workspace.search_text`, `ae.edit.apply_plan`, `ae.edit.discard_plan`, `ae.semantic.status`, `ae.semantic.ensure`)는 코드와 테스트 흐름에서 대체로 일치한다.
- `apply_plan`은 `workspace_revision`, view ownership, file fingerprint를 다시 검증하고 있어서 최소한의 CAS 성격은 이미 확보했다.
- 직전 외부 정적 리뷰에서 지적된 README `docs/` 링크 깨짐은 로컬 `main` 기준으로 해소됐다. 지금은 `README.md`와 `docs/` 구조가 실제 저장소와 맞는다.
- 현재 open risk의 중심은 문서 품질이 아니라 correctness / completeness 쪽이다. launcher bootstrap race와 write path의 인코딩 보존은 로컬 `main` 기준으로 해소됐고, 이제 외부 edit flow의 미완결성이 가장 높은 우선순위다.
- 그 다음 순위는 repeated full scan, 과도한 default excludes, 느슨한 schema/overlap validation, roots 없는 client fallback의 안전성 문제다.

## 2) 2026-03-22 외부 정적 리뷰 재평가

| 외부 리뷰 항목 | 현재 상태 | 현재 근거 |
|---|---|---|
| README가 `docs/`를 가리키지만 실제 디렉터리가 없음 | resolved locally | 현재 로컬 `main`에는 `docs/README.md`, `docs/architecture.md`, `docs/development.md`, `docs/language-profiles.md`가 존재한다. |
| npm 배포 모델이 스펙과 다름 | valid / backlog | `package.json`은 여전히 `private: true`이고 publish-ready launcher package 상태는 아니다. |
| Windows / non-Unix runtime model이 스펙과 다름 | valid / backlog | 스펙은 Windows named pipe + single-instance mutex를 상정하지만 현재 구현은 non-Unix를 제품 수준으로 다루지 않는다. |
| launcher startup race 가능성 | resolved locally | `ensureDaemonRunning()`은 bootstrap lock을 잡고 socket recheck 후 spawn으로 진행하며, concurrent bootstrap regression test가 추가됐다. |
| 파일 인코딩 보존 없이 write | resolved locally | `apply_plan()`은 원본 encoding/BOM을 보존해 temp file + rename 경로로 기록하고, BOM/UTF-16 roundtrip test가 추가됐다. |
| public edit flow 완결성 부족 | valid / open | public tool surface에는 `apply_plan`/`discard_plan`만 있고 plan create/preview API는 없다. core에는 test-only `seed_plan_for_test()`만 있다. |
| `search_text`의 이중 스캔 | valid / open | `workspace::search_text()`는 `refresh_workspace()` 후 다시 `scan_workspace()`를 호출한다. |
| hardcoded default excludes | valid / backlog | `scan.rs`가 `vendor`, `dist`, `build`, `target`, `.next`, `.turbo`, `.cache`, `coverage` 등을 강하게 제외한다. |
| edit overlap / schema validation이 느슨함 | valid / open | `apply_edits_to_text()`는 reverse apply 중심이고 overlap validation이 없으며, tool schema의 `range`/`scope`는 느슨한 object 수준이다. |
| roots 없는 client에 `cwd` root 자동 주입 | valid / open | launcher bridge는 roots capability가 없으면 `process.cwd()`를 묵시적으로 root로 넣는다. |

## 3) 해소되었거나 stale인 항목

| id | 상태 | 현재 근거 |
|---|---|---|
| `R-001` README `docs/` mismatch | resolved locally | README와 `docs/` 디렉터리 구조가 일치하도록 정리됐다. |
| `R-002` semantic.status / ensure contract mismatch | resolved locally | lifecycle action과 leaf-profile 출력은 현재 코드와 테스트에서 맞춰져 있다. |
| `R-003` case-insensitive search offset corruption | resolved locally | case-insensitive literal/word search는 원문 기준 offset을 사용하도록 수정돼 있다. |
| `R-004` launcher startup race | resolved locally | bootstrap lock + recheck 경로가 들어갔고 concurrent `ensureDaemonRunning()` regression test가 추가됐다. |
| `R-005` apply_plan encoding/BOM loss | resolved locally | `apply_plan()`은 UTF-8 BOM, UTF-16LE, UTF-16BE를 보존해 쓰고 관련 regression test가 추가됐다. |

## 4) 현재 Open Finding

| id | severity | area | file | evidence | impact | fix_option | test_gap | priority |
|---|---|---|---|---|---|---|---|---|
| `F-003` | `P1` | public API completeness | `crates/spiki-daemon/src/tools.rs`, `crates/spiki-core/src/runtime/plans.rs` | public MCP surface에는 `ae.edit.apply_plan`과 `ae.edit.discard_plan`만 있고, plan create/preview API가 없다. 내부적으로는 `seed_plan_for_test()`만 있다. | 외부 클라이언트 입장에서는 edit flow가 닫혀 있지 않아 Phase 1 edit contract를 온전히 사용할 수 없다. | `prepare_plan` 또는 `preview_plan` 성격의 public API를 추가하고 `apply_plan`과 연결한다. | 성공적인 public edit flow e2e test가 없다. | high |
| `F-004` | `P2` | workspace scan / latency | `crates/spiki-core/src/runtime/workspace.rs` | `search_text()`는 `refresh_workspace()`로 전체 스캔한 뒤 다시 `scan_workspace()`를 호출한다. | 요청당 full scan 비용이 커지고, 파일 수가 늘면 응답 지연과 stale churn이 함께 늘 수 있다. | `WorkspaceIndex`를 분리해 한 요청 내 중복 스캔을 없애고, scope-aware filtered view를 재사용한다. | scan count 또는 cached reuse를 고정하는 regression test가 없다. | medium |
| `F-005` | `P2` | validation | `crates/spiki-core/src/text/edits.rs`, `crates/spiki-daemon/src/tools.rs` | `apply_edits_to_text()`는 edit들을 뒤에서부터 적용하지만 overlapping range나 duplicate edit를 별도 검증하지 않는다. tool schema도 `range`/`scope`를 좁게 표현하지 않는다. | 다양한 클라이언트가 붙으면 잘못된 payload가 늦게 실패하거나, 디버깅 비용이 커질 수 있다. | overlap/duplicate validation을 write 전에 명시적으로 넣고, schema는 derive 기반으로 더 좁힌다. | malformed edit / overlapping range / nested schema error test가 부족하다. | medium |
| `F-006` | `P2` | access control / client compatibility | `launcher/runtime.mjs` | roots 미지원 `initialize` 요청에는 `process.cwd()`를 root로 묵시적으로 주입한다. | MCP host 실행 위치에 따라 작업 범위가 바뀌는 묵시적 ACL 확장이 생길 수 있다. | 기본은 reject 또는 explicit opt-in으로 바꾸고, convenience fallback은 설정 플래그 뒤로 숨긴다. | roots 미지원 client의 reject path / opt-in path test가 없다. | medium |

## 5) 남은 Backlog

| backlog | area | evidence | fix_option | priority |
|---|---|---|---|---|
| `B-001` publishable launcher package gap | productization | 스펙은 npm launcher UX를 상정하지만, 현재 `package.json`은 `private: true`이고 publish-ready metadata/packaging 단계가 아니다. | npm package surface, install docs, release workflow를 별도 제품화 라운드로 정리한다. | medium |
| `B-002` Windows / bootstrap model gap | platform | 스펙은 Windows named pipe + single-instance mutex와 Linux/macOS lock 절차를 상정하지만 구현은 그 수준까지 도달하지 못했다. | platform abstraction과 bootstrap lock 절차를 명시적으로 구현한다. | medium |
| `B-003` hardcoded scan policy | workspace policy | `scan.rs`의 default excludes는 안전한 기본값이지만 강제 제외처럼 동작한다. | default / recommended / forced exclude를 분리하고 config/profile에서 제어 가능하게 만든다. | medium |
| `B-004` launcher module hotspot | maintainability | `launcher/runtime.mjs`에 경로 계산, daemon lifecycle, stdio bridge가 함께 들어 있다. | `runtime-paths`, `daemon-bootstrap`, `mcp-bridge`로 나눠 race fix와 테스트를 쉽게 만든다. | low |
| `B-005` runtime ownership hotspot | maintainability | 현재 `Runtime`는 workspace scan, plan state, semantic skeleton, apply 흐름을 함께 들고 있다. | `WorkspaceIndex`, `PlanStore`, `SemanticSupervisor` 정도로 역할을 쪼갠다. | low |
| `B-006` schema automation gap | tooling | tool schema는 hand-written JSON이고 Rust 타입과 1:1 동기화가 보장되지 않는다. | Rust 타입에서 schema를 derive하는 방향으로 이동한다. | low |

## 6) 검증 기준선 (2026-03-22)

- `cargo test --workspace` : pass
- `npm run test:integration` : pass

## 7) 남은 테스트 Backlog

| test item | 목적 | priority |
|---|---|---|
| `T-003` public edit flow e2e | public `plan-create/preview -> apply/discard` 전체 흐름을 도구 surface 기준으로 고정한다. | high |
| `T-004` scan reuse regression | `workspace.status`, `search_text`, `apply_plan` 경로에서 중복 full scan이 줄어들었는지 계측 기반으로 고정한다. | medium |
| `T-005` roots-less client safety | roots 미지원 client에서 reject 또는 explicit opt-in path만 허용되는지 고정한다. | medium |
| `T-006` malformed schema / overlapping edit | 잘못된 `range`, 느슨한 `scope`, overlapping edit payload가 일찍 실패하는지 고정한다. | medium |

## 8) 권장 후속 순서

1. `F-003` public plan-create/preview API를 추가한다.  
   edit surface를 외부에서 실제로 쓸 수 있게 만들어야 Phase 1 tool story가 닫힌다.

2. `F-004` 이중 스캔 제거와 `B-003` scan policy 설정화를 묶는다.  
   성능과 사용성 문제가 같은 계층에 모여 있다.

3. `F-005` / `B-006`을 묶어 validation + schema 자동화를 진행한다.  
   correctness를 더 이른 단계에서 잡고 문서/구현 drift를 줄일 수 있다.

4. `F-006` roots fallback은 별도 policy 결정 후 고친다.  
   편의성과 ACL 안전성의 trade-off가 있으므로 기본 정책을 먼저 정해야 한다.
