# Agent Editor MCP 전체 스펙 초안 v0.1

상태: Draft  
문서 버전: 0.1.0  
기준 프로토콜: MCP 2025-11-25  
레퍼런스 구현 목표: `npx` launcher + Rust daemon  
문서 성격: 제품/프로토콜/런타임 통합 사양

---

## 1. 개요

### 1.1 한 줄 정의

Agent Editor MCP는 에이전트가 코드베이스를 **텍스트 패치 조각**이 아니라 **심볼(symbol), 구조(structure), 편집 계획(edit plan)** 단위로 다루게 해 주는 MCP 서버다.

### 1.2 제품 모토

**신경쓰지 않아도 알아서 작동한다.**

사용자는 daemon을 직접 관리하지 않는다.  
에이전트는 LSP를 직접 이해하지 않는다.  
대부분의 요청은 가벼운 인덱스/구조 계층에서 처리되고, 고정확도 의미 분석이 필요한 경우에만 semantic backend가 자동으로 올라온다.

### 1.3 배경 문제

기존 코딩 에이전트의 기본 루프는 보통 아래와 같다.

`search → read → patch`

이 방식은 작은 수정에는 충분하지만 다음 문제를 낳는다.

- cross-file rename이나 follow references 같은 작업이 불안정하다.
- 모델이 불필요하게 많은 파일 내용을 읽는다.
- 대규모 프로젝트에서 의미 단위 조작 대신 텍스트 근사에 의존한다.
- LSP를 직접 노출하면 상태 관리, 토큰 낭비, 장황한 응답이 커진다.

본 사양은 이 문제를 해결하기 위해 **텍스트 계층**, **구문/인덱스 계층**, **의미 계층**을 하나의 shared runtime 아래에 통합한다.

---

## 2. 설계 목표와 비목표

### 2.1 목표

1. 에이전트가 의미 단위 편집을 수행할 수 있어야 한다.
2. 설치와 실행은 `npx` 한 줄로 끝나야 한다.
3. 실제 무거운 실행체는 Rust 기반 native daemon이어야 한다.
4. 한 유저당 daemon 하나를 공유해 여러 에이전트 요청을 효율적으로 처리해야 한다.
5. LSP는 필요할 때만 자동으로 켜지고, idle 시 자동으로 쉬거나 종료되어야 한다.
6. 모든 비단순 수정은 preview-first 원칙을 따라야 한다.
7. roots 경계와 ACL은 항상 강제되어야 한다.
8. 같은 프로젝트를 여러 에이전트가 동시에 다뤄도 일관성과 충돌 처리가 보장되어야 한다.

### 2.2 비목표

1. 범용 IDE 대체품이 되는 것.
2. raw LSP 메서드를 MCP client에 그대로 노출하는 것.
3. 모든 언어에서 100% semantic refactor를 보장하는 것.
4. 사람이 확인하지 않아도 무조건 자동 적용되는 destructive editing.
5. 전체 파일 내용을 항상 LLM에 흘려보내는 것.

---

## 3. 규범 용어

이 문서의 MUST, MUST NOT, SHOULD, SHOULD NOT, MAY는 RFC 2119 의미로 해석한다.

- **client_session**: MCP client와 daemon 사이의 단일 MCP 연결.
- **daemon**: 유저 단위로 실행되는 공유 런타임.
- **shared_workspace**: 여러 client_session이 공유할 수 있는 실제 인덱스/캐시/semantic 상태 단위.
- **workspace_view**: 특정 client_session에 대해 roots ACL이 반영된 논리적 뷰.
- **snapshot**: 특정 시점의 읽기 일관성을 갖는 workspace 상태.
- **plan**: 아직 적용되지 않은 편집 계획.
- **semantic backend**: LSP 또는 동급 의미 분석 엔진.
- **engine**: text / syntax / semantic 중 어떤 계층이 결과를 냈는지 나타내는 실행 출처.

---

## 4. 외부 호환성 목표

### 4.1 MCP 호환성

본 서버는 MCP 최신 기준 버전인 2025-11-25를 타깃으로 한다.

- MCP base protocol과 lifecycle을 MUST 지원한다.
- tool schema는 JSON Schema 2020-12를 기본 dialect로 사용한다.
- tool output은 가능하면 `structuredContent`를 사용하고, backward compatibility를 위해 compact text 요약을 함께 제공한다.
- roots, progress, cancellation, resources를 지원한다.
- task-augmented execution은 optional extension으로만 지원한다.

### 4.2 배포 모델

외부 UX는 다음을 목표로 한다.

```bash
npx -y @org/agent-editor-mcp
```

- npm 패키지는 launcher/installer/doctor 역할을 맡는다.
- 실제 편집 엔진은 Rust daemon 바이너리다.
- 기본 transport UX는 stdio bridge다.
- direct local Streamable HTTP는 opt-in이다.

### 4.3 참조 UX

Codex CLI와 유사하게, 설치 UX는 npm 기반이지만 실제 핵심 실행은 native binary가 담당한다.

---

## 5. 제품 원칙

### 5.1 fast by default

가능한 한 text 또는 syntax 계층에서 먼저 처리한다.

### 5.2 semantic on demand

정확도나 범위가 요구될 때만 semantic backend를 올린다.

### 5.3 preview before apply

cross-file 또는 의미 기반 수정은 먼저 plan을 만들고, 실제 write는 별도 단계에서만 수행한다.

### 5.4 shared runtime, isolated views

무거운 캐시와 backend는 공유하되, 각 client의 roots ACL은 엄격히 분리한다.

### 5.5 stable contracts over raw text

가능한 한 `symbol_id`, `plan_id`, `workspace_revision` 같은 안정된 핸들을 사용한다.

### 5.6 explicit confidence

비단순 결과에는 항상 confidence와 warnings를 포함한다.

---

## 6. 전체 아키텍처

```text
MCP Host / Client
    │
    │ stdio
    ▼
Node launcher (npx)
    ├─ binary 존재 확인/설치
    ├─ per-user daemon 탐색/기동
    ├─ stdio <-> local socket bridge
    └─ doctor / upgrade / stop 관리
            │
            │ UDS / Named Pipe (기본)
            │ optional local Streamable HTTP
            ▼
Rust daemon
    ├─ MCP session acceptor
    ├─ workspace registry
    ├─ snapshot store
    ├─ text engine
    ├─ syntax/index engine
    ├─ semantic supervisor
    ├─ plan store
    ├─ apply engine
    ├─ resource publisher
    └─ metrics/logging
```

핵심 분리는 다음과 같다.

- **외부 MCP session** 은 연결 단위 lifecycle을 따른다.
- **내부 daemon** 은 유저 단위 shared runtime으로 오래 산다.
- **shared_workspace** 는 실제 캐시 단위다.
- **workspace_view** 는 ACL이 반영된 client별 뷰다.

즉, “세션 하나”가 아니라 **유저 daemon 하나 + 여러 MCP session** 모델이다.

### 6.1 v0.1 참조 구현 코드베이스 분해

실제 레퍼런스 구현은 다음 디렉터리 구조를 기준으로 한다.

```text
spiki/
  package.json
  bin/spiki.js
  launcher/
    cli.mjs
    runtime.mjs
  scripts/
    build-daemon.mjs
  Cargo.toml
  crates/
    spiki-core/
      src/
    spiki-daemon/
      src/
  tests/
```

역할 분리는 다음과 같다.

- `bin/spiki.js`: `npx` entrypoint
- `launcher/*`: daemon 탐색, stale 복구, stdio bridge, doctor command
- `crates/spiki-core`: workspace registry, text engine, plan/apply engine, MCP tool handler 공용 로직
- `crates/spiki-daemon`: socket listener, MCP JSON-RPC transport, per-session bootstrap

이 분해는 v0.1에서 MUST 간주한다.  
이후 semantic provider가 추가되더라도 edit/search 핵심 로직은 `spiki-core` 에 남아야 한다.

### 6.2 v0.1 내부 런타임 구성

v0.1 기준 daemon 내부 주 컴포넌트는 아래와 같이 고정한다.

- `RuntimeState`: daemon 전체 상태, active session 수, config, metrics
- `WorkspaceRegistry`: canonical roots 집합을 key로 shared_workspace를 재사용
- `WorkspaceState`: revision, 파일 메타, lazy text index, view 목록
- `TextEngine`: `read_spans`, `search_text`, fingerprint 계산
- `PlanStore`: opaque `plan_id` 기반 TTL 저장소
- `ApplyEngine`: workspace별 write mutex와 CAS 검증 담당
- `SemanticSupervisor`: Phase 1에서는 상태 조회용 skeleton만 제공

중요한 구현 원칙:

- file watcher가 아직 없더라도 각 tool 진입 시 mtime/size 재검사로 revision drift를 감지해야 한다.
- full semantic/syntax index가 없더라도 text 계층만으로 유효한 MCP 응답을 반환해야 한다.
- plan apply 직전 fingerprint 재검증은 `WorkspaceState` 캐시가 아니라 실제 파일시스템 기준으로 MUST 수행한다.

### 6.3 v0.1 초기 구현 범위

문서 전체는 최종 제품 목표를 서술하지만, **이번 참조 구현의 첫 번째 동작 가능한 마일스톤** 은 아래 범위로 고정한다.

- public MCP tools: `ae.workspace.status`, `ae.workspace.read_spans`, `ae.workspace.search_text`, `ae.edit.prepare_plan`, `ae.edit.apply_plan`, `ae.edit.discard_plan`, `ae.semantic.status`, `ae.semantic.ensure`
- `ae.semantic.*` 는 Phase 1에서 skeleton 응답만 제공하며, backend lifecycle state를 `off` 기준으로 노출한다.
- `ae.workspace.search_structure`, `ae.symbol.*`, `ae.refactor.rename_preview`, `ae.diagnostics.read` 는 reserved surface로 유지한다.
- launcher는 `doctor`, `daemon status`, `daemon stop`, 기본 stdio bridge까지 구현 범위에 포함한다.

즉, v0.1 첫 구현은 **text-first MCP + shared singleton runtime + safe apply skeleton** 을 완성하는 데 집중한다.

---

## 7. 프로세스 및 런타임 모델

### 7.1 구성 요소

#### launcher

역할:
- Rust binary 존재 확인
- daemon socket 탐색
- 필요 시 daemon 기동
- stdio ↔ local socket 브리지
- `doctor`, `daemon status`, `daemon stop` 제공

#### daemon

역할:
- 여러 MCP 연결 수용
- workspace 캐시 공유
- semantic backend 수명 주기 관리
- tools/resources 처리
- plan apply 직렬화

#### semantic backend

역할:
- 고정확도 definition/references/rename/diagnostics
- 필요 시에만 자동 기동
- idle 시 휴면 또는 종료

### 7.2 per-user singleton

daemon은 OS 사용자당 하나만 존재해야 한다.

- Linux/macOS: UDS + lock file
- Windows: named pipe + single-instance mutex

### 7.3 단일 인스턴스 획득 절차

launcher는 다음 절차를 MUST 수행한다.

1. runtime dir 계산
2. socket/pipe에 연결 시도
3. 실패 시 bootstrap lock 획득 시도
4. lock 획득 후 다시 연결 시도
5. 여전히 없으면 daemon spawn
6. readiness 대기
7. 연결 성공 후 bridge 시작
8. lock 해제

### 7.4 stale instance 복구

다음 조건에서 stale로 간주할 수 있다.

- PID file은 있지만 프로세스가 존재하지 않음
- socket path가 있으나 connect 불가
- daemon version이 호환 범위를 벗어남

stale 판단 시 launcher는 안전하게 socket/lock/PID file을 정리하고 재기동해야 한다.

---

## 8. transport 모델

### 8.1 외부 transport

기본 모드는 stdio다.

- host는 launcher를 일반 MCP stdio 서버처럼 실행한다.
- launcher는 daemon과 연결되면 raw MCP JSON-RPC 메시지를 양방향으로 중계한다.
- launcher는 MCP payload를 해석하지 않는 얇은 bridge여야 한다.

### 8.2 내부 transport

기본은 다음과 같다.

- Linux/macOS: Unix domain socket
- Windows: named pipe

내부 transport에서도 MCP 메시지는 **그대로 JSON-RPC 2.0** 로 전달된다. 별도 envelope는 사용하지 않는다.

### 8.3 direct local HTTP 모드

옵션으로 daemon은 로컬 Streamable HTTP endpoint를 열 수 있다.

이 모드는 다음 조건을 MUST 만족한다.

- localhost bind only
- random bearer token 또는 동등 보안
- Origin 검증
- default off

### 8.4 브리지 버퍼링

daemon이 막 기동되는 동안 launcher는 stdio 입력을 최대 1 MiB까지 버퍼링 MAY 한다.  
초과 시 launcher는 연결 실패 오류를 반환해야 한다.

---

## 9. lifecycle 및 MCP session 모델

### 9.1 외부 MCP lifecycle

각 client_session은 표준 MCP lifecycle을 따른다.

1. initialize
2. notifications/initialized
3. operation
4. shutdown / connection close

### 9.2 daemon 내부 session 분리

daemon은 다음을 구분한다.

- **transport connection**: launcher와 daemon 사이의 실제 stream 연결
- **client_session**: 한 MCP initialize로 시작되는 논리 세션
- **workspace_view**: roots ACL을 반영한 논리 작업 범위
- **shared_workspace**: 물리 캐시/인덱스 단위

### 9.3 session 종료

client_session이 종료되어도 daemon은 살아남는다.  
semantic backend와 workspace cache도 다른 session이 있거나 idle timeout 전이면 유지된다.

---

## 10. 네이밍 및 식별자

### 10.1 식별자 종류

- `daemon_id`: 유저 런타임 식별자
- `client_session_id`: 연결 식별자
- `workspace_id`: shared workspace 식별자
- `view_id`: client roots ACL 기반 뷰 식별자
- `workspace_revision`: 해당 workspace의 현재 committed snapshot 식별자
- `symbol_id`: opaque 심볼 식별자
- `plan_id`: edit preview 식별자
- `backend_id`: semantic backend 인스턴스 식별자

### 10.2 식별자 원칙

- 모든 ID는 opaque 여야 한다.
- client는 ID의 내부 포맷을 해석해서는 안 된다.
- `symbol_id`는 최소한 해당 `workspace_revision` 범위 내에서만 유효하다고 간주해야 한다.
- `plan_id`는 TTL을 가지며 만료될 수 있다.

### 10.3 권장 생성 방식

- `plan_id`: UUIDv7 또는 ULID
- `workspace_revision`: monotonic counter + epoch 조합
- `symbol_id`: path/range/kind/name/signature hash를 포함한 opaque token

---

## 11. 파일시스템 접근 모델

### 11.1 roots 기반 접근

모든 file operation은 반드시 client가 제공한 roots 안에서만 수행되어야 한다.

### 11.2 canonicalization

모든 경로는 다음을 거친다.

1. 절대경로화
2. symlink canonicalization
3. URI 정규화
4. roots 경계 검증

### 11.3 symlink 정책

- roots 내부 symlink는 허용 가능하다.
- symlink가 roots 바깥을 가리키면 기본적으로 접근 금지다.
- `allowExternalSymlinks`는 v1에서 지원하지 않는다.

### 11.4 숨김/생성/벤더 파일 정책

기본 제외 대상 예시:

- `.git/`
- `node_modules/`
- `vendor/`
- `dist/`
- `build/`
- `target/`
- `.next/`
- `.turbo/`
- `.cache/`
- coverage 산출물
- 생성 코드로 판정된 파일

추가로 다음을 존중 SHOULD 한다.

- `.gitignore`
- `.ignore`
- `.fdignore`
- 언어/도구별 generated markers

### 11.5 binary 및 대형 파일 정책

- binary file은 기본적으로 index/search 대상에서 제외한다.
- text indexing 기본 최대 크기는 2 MiB/file이다.
- 더 큰 파일은 `read_spans`로는 명시 요청 시 읽을 수 있으나, index에는 포함하지 않아도 된다.

### 11.6 line ending 보존

편집 적용 시 파일별 원래 line ending을 MUST 보존한다.

### 11.7 인코딩 정책

- UTF-8, UTF-8 BOM, UTF-16 BOM은 지원 SHOULD 한다.
- 기타 인코딩은 binary 또는 unsupported로 처리 MAY 한다.

---

## 12. shared_workspace 와 workspace_view

### 12.1 shared_workspace

shared_workspace는 물리적 캐시 단위다.

포함 요소:
- 파일 메타데이터 캐시
- snapshot store
- text search index
- syntax trees / symbol index
- import/export graph
- semantic backend pool
- plan store

### 12.2 workspace_view

workspace_view는 특정 client_session에 대해 roots ACL이 반영된 논리 뷰다.

포함 요소:
- accessible roots 집합
- path filter
- view-local warnings/redactions
- view-visible resources

### 12.3 공유와 격리

daemon은 같은 유저의 여러 client_session 사이에서 shared_workspace를 공유 MAY 한다.  
그러나 모든 결과, edit plan, resource read는 반드시 `view_id` 를 기준으로 필터링되어야 한다.

### 12.4 redaction 원칙

shared cache가 더 넓은 파일 집합을 알고 있더라도, 특정 view 밖의 정보는 직접 노출해서는 안 된다.

허용 예시:
- "접근 불가 범위 때문에 confidence가 낮아졌다"
- "operation scope 바깥에 추가 영향 가능성이 있다"

금지 예시:
- "숨겨진 7개 파일이 더 있다"
- "../secret-package/foo.ts에 참조가 있다"

---

## 13. snapshot 및 revision 모델

### 13.1 snapshot

snapshot은 읽기 일관성을 보장하는 workspace 상태다.

포함 요소:
- 파일 내용 해시 테이블
- 파일 길이/mtime
- syntax index state
- symbol graph state
- diagnostic cache state

### 13.2 workspace_revision

workspace_revision은 committed snapshot 버전을 나타낸다.

다음 사건에서 revision은 증가 MUST 한다.

- daemon이 edit를 적용함
- 외부 파일 변경 감지
- roots 변경으로 가시 범위가 바뀜
- 프로젝트 설정 파일 변경으로 인덱스 의미가 바뀜

### 13.3 읽기 일관성

하나의 tool invocation은 기본적으로 하나의 snapshot을 기준으로 실행되어야 한다.  
long-running task에서는 중간에 snapshot이 바뀌더라도 최초 snapshot을 기준으로 계산하고, 결과에 `observedRevision` 과 `currentRevision` 을 함께 실을 수 있다.

---

## 14. 파일 fingerprint

`file_fingerprint`는 stale plan 검출에 쓰인다.

필드:
- `uri`
- `contentHash`
- `size`
- `mtimeMs`
- `lineEnding`
- `encoding`

권장 해시 알고리즘은 BLAKE3다.

---

## 15. 엔진 계층

### 15.1 text engine

책임:
- 파일 나열
- literal/regex 검색
- span 읽기
- diff 생성
- ignore-aware traversal

### 15.2 syntax/index engine

책임:
- incremental parse
- local symbol 추출
- import/export map 생성
- 구조 검색
- lightweight references/definition
- simple rename preview

### 15.3 semantic engine

책임:
- type-aware definition/references
- prepare rename
- high-confidence rename
- diagnostics
- framework-/toolchain-aware resolution

### 15.4 engine 선택 원칙

서버는 다음 순서로 시도 SHOULD 한다.

1. text
2. syntax/index
3. semantic

단, 다음 상황에서는 semantic 승격을 SHOULD 한다.

- exported/public symbol
- alias/re-export 체인 존재
- overload 또는 다형성 가능성
- 동적 property access 발견
- framework magic 가능성
- confidence threshold 미달
- 사용자가 accuracy 우선 모드 요청

---

## 16. 언어 어댑터 모델

레퍼런스 구현은 **base adapter** 와 **language profile** 을 분리해야 한다.

- **base adapter**: parser/index 구현 단위. 예: `javascript`, `typescript`, `python`, `go`, `rust`
- **language profile**: 사용자와 semantic binding이 보는 계층형 언어 정의 단위. 예: `javascript -> nodejs -> react`

이 문서에서 tool schema의 `language` 필드는 특별한 언급이 없으면 **resolved leaf `language_profile_id`** 를 의미한다.  
server가 아직 leaf profile을 확정하지 못한 경우에만 base adapter id를 반환 MAY 하며, 그 경우 `LANGUAGE_PROFILE_UNRESOLVED` warning을 함께 포함 SHOULD 한다.

### 16.1 base adapter 와 language profile

base adapter는 구문/인덱스 엔진의 구현을 담당하고, language profile은 탐지 규칙과 semantic binding을 담당한다.

```rust
struct LanguageProfile {
    profile_id: String,
    parent_profile_id: Option<String>,
    adapter_id: String,
    file_patterns: Vec<String>,
    project_markers: Vec<String>,
    detect: DetectionRules,
    semantic: SemanticBinding,
    priority: i32,
}

trait LanguageAdapter {
    fn adapter_id(&self) -> &'static str;
    fn base_profile_id(&self) -> &'static str;
    fn file_patterns(&self) -> &'static [&'static str];
    fn discover_projects(&self, roots: &[Root]) -> Vec<ProjectHandle>;
    fn parse(&self, file: &SourceFile) -> ParseResult;
    fn extract_symbols(&self, parse: &ParseResult) -> Vec<Symbol>;
    fn extract_import_edges(&self, parse: &ParseResult) -> Vec<ImportEdge>;
    fn prepare_rename_local(&self, target: &LocationRef, snap: &Snapshot) -> LocalRenameInfo;
    fn structural_search(&self, query: &StructureQuery, snap: &Snapshot) -> StructureMatches;
    fn supports_profile(&self, profile_id: &str) -> bool;
}
```

즉, `react` 나 `nodejs` 는 새로운 parser를 강제하지 않는다.  
보통은 `javascript` 또는 `typescript` adapter 위에 detection/semantic/toolchain 규칙을 덧씌운다.

### 16.2 계층 규칙

v0.1의 language profile 계층은 **단일 부모(single-parent) 트리** 여야 한다.

- profile은 최대 하나의 `parent_profile_id` 만 가질 수 있다.
- 다중 상속/DAG는 v0.1에서 지원하지 않는다.
- child는 parent의 file pattern, project marker, detection rule, semantic default를 상속한다.
- child는 parent 값을 additive merge 또는 replace override 할 수 있어야 한다.
- 가장 구체적인 child가 parent보다 우선한다.

이 계층은 반드시 “실행 런타임만의 분류” 일 필요는 없다.  
예를 들어 `javascript -> nodejs -> react` 는 “JS 문법 위에 Node 기반 프로젝트 규칙이 있고, 그 위에 React toolchain/JSX 규칙이 얹힌다” 는 specialization chain으로 해석한다.

### 16.3 profile 결정 절차

server는 파일/프로젝트에 대해 다음 순서로 language profile을 결정 SHOULD 한다.

1. 파일 확장자, 파일명, shebang으로 candidate base adapter를 고른다.
2. candidate adapter에 연결된 base profile에서 시작한다.
3. project marker, config file, dependency heuristic, YAML override를 적용해 matching child profile을 찾는다.
4. matching profile이 여러 개면 더 깊은(depth가 큰) profile을 우선한다.
5. depth가 같으면 `priority` 가 큰 profile을 우선한다.
6. 그래도 동률이면 parent profile로 degrade 하거나 ambiguity warning을 반환한다.

하나의 파일은 내부적으로 profile stack `[javascript, nodejs, react]` 처럼 추적할 수 있다.  
외부 tool output은 기본적으로 가장 leaf인 profile을 우선 노출 SHOULD 한다. 다만 phase 1 구현은 framework leaf와 별개 toolchain child(`java-jvm`, `node-ts`, `cargo-rust` 등)를 함께 노출 MAY 한다.

### 16.4 대표 built-in profile 정의

레퍼런스 구현은 최소한 다음 built-in profile tree를 SHOULD 제공한다.

- core language bases
- `javascript`
  - 일반 `.js`, `.mjs`, `.cjs`, `.jsx` 계열의 base profile
- `javascript -> nodejs`
  - `package.json`, Node module resolution, CJS/ESM heuristic를 반영하는 child profile
- `javascript -> nodejs -> react`
  - `react` dependency, JSX-centric toolchain을 반영하는 child profile
- `javascript -> nodejs -> react -> nextjs`
  - `next` dependency, `next.config.*`, app/pages router 관례를 반영하는 child profile
- `javascript -> nodejs -> react -> remix`
  - `@remix-run/*` dependency, `remix.config.*`, route module convention을 반영하는 child profile
- `javascript -> nodejs -> react -> gatsby`
  - `gatsby` dependency, `gatsby-config.*`, page/api route convention을 반영하는 child profile
- `javascript -> nodejs -> preact`
  - `preact` dependency와 lightweight JSX runtime을 반영하는 child profile
- `javascript -> nodejs -> vue`
  - `.vue` single-file component와 `vue` dependency를 반영하는 child profile
- `javascript -> nodejs -> vue -> nuxt`
  - `nuxt` dependency, `nuxt.config.*`, file-based route/app convention을 반영하는 child profile
- `javascript -> nodejs -> svelte`
  - `.svelte` component와 `svelte` dependency를 반영하는 child profile
- `javascript -> nodejs -> svelte -> sveltekit`
  - `@sveltejs/kit` dependency와 `src/routes` / kit config convention을 반영하는 child profile
- `javascript -> nodejs -> angular`
  - `@angular/core`, `angular.json`, workspace/project builder convention을 반영하는 child profile
- `javascript -> nodejs -> astro`
  - `.astro` component, `astro.config.*`, island/route convention을 반영하는 child profile
- `javascript -> nodejs -> solid`
  - `solid-js` dependency와 fine-grained JSX runtime을 반영하는 child profile
- `javascript -> nodejs -> solid -> solidstart`
  - `@solidjs/start` dependency와 route/server function convention을 반영하는 child profile
- `javascript -> nodejs -> qwik`
  - `@builder.io/qwik` / `@builder.io/qwik-city` dependency를 반영하는 child profile
- `javascript -> nodejs -> lit`
  - `lit` / `lit-html` / `lit-element` dependency를 반영하는 child profile
- `javascript -> nodejs -> ember`
  - `ember-source`, `ember-cli` dependency와 Ember app/addon 구조를 반영하는 child profile
- `javascript -> nodejs -> alpine`
  - `alpinejs` dependency와 HTML-first progressive enhancement convention을 반영하는 child profile
- `typescript`
  - 일반 `.ts`, `.tsx`, `tsconfig*.json` 계열의 base profile
- `typescript -> node-ts`
  - TS + Node module resolution + package boundary 규칙을 반영하는 child profile
- `typescript -> node-ts -> react-ts`
  - TSX + React dependency/toolchain을 반영하는 child profile
- `c`
  - 일반 `.c` 계열의 native base profile
- `c -> c-native`
  - `CMakeLists.txt`, `Makefile`, `compile_commands.json` 같은 local toolchain/project marker를 반영하는 child profile
- `cpp`
  - 일반 `.cc`, `.cpp`, `.cxx` 계열의 native base profile
- `cpp -> cpp-native`
  - local build graph/CMake/compile command 관례를 반영하는 child profile
- `java`
  - 일반 `.java` 계열의 JVM base profile
- `java -> java-jvm`
  - generic JVM semantics와 classpath/layout 관례를 반영하는 child profile
- `java -> java-maven`
  - `pom.xml`, Maven wrapper, Maven directory layout을 반영하는 child profile
- `java -> java-gradle`
  - `build.gradle*`, `settings.gradle*`, Gradle-based project 규칙을 반영하는 child profile
- `kotlin`
  - base Kotlin profile
- `kotlin -> kotlin-jvm`
  - Kotlin/JVM build/runtime 규칙을 반영하는 child profile
- `python`
  - 일반 `.py` 계열의 base profile
- `python -> pyproject-python`
  - `pyproject.toml`, `setup.cfg`, venv/tooling 규칙을 반영하는 child profile
- `go`
  - base Go profile
- `go -> go-module`
  - `go.mod` 기준 project/root semantics를 반영하는 child profile
- `rust`
  - base Rust profile
- `rust -> cargo-rust`
  - `Cargo.toml` / workspace semantics를 반영하는 child profile
- `csharp`
  - base C# profile
- `csharp -> dotnet-csharp`
  - `.csproj`, `.sln`, SDK-style .NET project 규칙을 반영하는 child profile
- `fsharp`
  - base F# profile
- `fsharp -> dotnet-fsharp`
  - .NET/F# project 규칙을 반영하는 child profile
- `vbnet`
  - base Visual Basic .NET profile
- `vbnet -> dotnet-vbnet`
  - .NET/VB project 규칙을 반영하는 child profile
- `swift`
  - base Swift profile
- `swift -> swift-package`
  - `Package.swift` 기준 SwiftPM semantics를 반영하는 child profile
- `ruby`
  - base Ruby profile
- `scala`
  - base Scala profile
- `scala -> scala-sbt`
  - `build.sbt` 기반 project semantics를 반영하는 child profile
- `haskell`
  - base Haskell profile
- `haskell -> haskell-cabal | haskell-stack`
  - Cabal/Stack project semantics를 반영하는 child profile
- `ocaml`
  - base OCaml profile
- `ocaml -> ocaml-opam`
  - Dune/opam 기반 project semantics를 반영하는 child profile
- `pascal`, `d`, `php`, `perl`, `lua`
  - single-root general language profile
- `shell -> bash`
  - shell script base profile과 Bash specialization
- `assembly`
  - generic assembly profile
- `objective-c`, `objective-cpp`, `fortran`, `scheme`, `ada`, `awk`, `tcl`, `r`, `julia`, `clojure`, `common-lisp`, `erlang`, `elixir`, `dart`, `nim`, `prolog`, `freebasic`, `haxe`, `systemverilog`
  - 일반 language base 또는 direct leaf로 제공 SHOULD 한다.

이 목록은 exhaustive하지 않다.  
그러나 TS/JS, systems/backend languages, VM/.NET languages, scripting languages에 대해서는 이와 같은 parent/child 모델을 기준으로 확장해야 한다.

### 16.5 지원 우선순위

v0.1 권장 우선순위:
- REQUIRED web/core: `javascript`, `nodejs`, `typescript`, `node-ts`
- REQUIRED major web frameworks: `react`, `react-ts`, `vue`, `svelte`, `angular`, `astro`, `solid`
- REQUIRED app framework leaves: `nextjs`, `remix`, `gatsby`, `nuxt`, `sveltekit`, `solidstart`
- REQUIRED medium-weight frameworks: `preact`, `qwik`, `lit`, `ember`, `alpine`
- REQUIRED general/core: `c`, `cpp`, `java`, `kotlin`, `python`, `go`, `rust`, `csharp`, `swift`
- SHOULD additional general: `fsharp`, `vbnet`, `scala`, `haskell`, `ocaml`, `pascal`, `d`, `php`, `perl`, `lua`, `shell`, `assembly`, `objective-c`, `objective-cpp`, `fortran`, `scheme`, `ada`, `awk`, `tcl`, `r`, `julia`, `clojure`, `common-lisp`, `erlang`, `elixir`, `dart`, `nim`, `prolog`, `freebasic`, `haxe`, `systemverilog`
- MAY ecosystem-specific children: `java-maven`, `java-gradle`, `swift-package`, `scala-sbt`, `haskell-cabal`, `haskell-stack`, `ocaml-opam`, `dotnet-csharp`, `dotnet-fsharp`, `dotnet-vbnet`

### 16.6 degraded mode

semantic provider가 없거나 실패하면 서버는 syntax/text만으로 동작해야 하며, 응답에 degraded warnings를 포함해야 한다.

---

## 17. semantic provider 모델

```rust
trait SemanticProvider {
    fn provider_id(&self) -> &'static str;
    fn start(&mut self, workspace: &WorkspaceContext) -> Result<()>;
    fn stop(&mut self) -> Result<()>;
    fn warm(&mut self, workspace: &WorkspaceContext) -> Result<()>;
    fn prepare_rename(&mut self, target: &TargetRef) -> Result<PrepareRenameResult>;
    fn rename(&mut self, target: &TargetRef, new_name: &str) -> Result<SemanticRenameResult>;
    fn definition(&mut self, target: &TargetRef) -> Result<Vec<LocationRef>>;
    fn references(&mut self, target: &TargetRef, include_declarations: bool) -> Result<Vec<LocationRef>>;
    fn diagnostics(&mut self, scope: &Scope) -> Result<Vec<Diagnostic>>;
}
```

semantic provider는 built-in provider일 수도 있고, YAML로 선언된 custom LSP provider일 수도 있다.

### 17.1 semantic binding

language profile은 semantic layer에 대해 다음 binding 중 하나를 가져야 한다.

- `none`: semantic provider 없음
- `builtin`: daemon 내부에 등록된 provider binding 사용
- `lsp`: 외부 LSP server command를 daemon이 subprocess로 관리

개념적으로는 아래 구조를 따른다.

```rust
struct SemanticBinding {
    kind: SemanticBindingKind, // none | builtin | lsp
    binding_id: String,
    transport: Option<TransportKind>, // v0.1 lsp는 stdio만 REQUIRED
    command: Option<Vec<String>>,
    root_markers: Vec<String>,
    initialization_options: JsonValue,
    workspace_configuration: JsonValue,
    environment: BTreeMap<String, String>,
}
```

### 17.2 source of truth 원칙

파일시스템 snapshot이 source of truth다.  
semantic provider는 snapshot을 반영한 projection일 뿐이다.

### 17.3 문서 동기화 원칙

daemon은 semantic provider에 대해 virtual editor 역할을 수행한다.

- 필요한 파일만 synthetic open
- daemon apply 후 synthetic change
- 외부 변경 감지 시 refresh 또는 invalidate
- provider 재시작 시 snapshot에서 재구성

### 17.4 provider 선택 정책

semantic provider는 language profile별/프로젝트별로 다를 수 있다.

server는 다음 순서로 provider binding을 선택 SHOULD 한다.

1. resolved leaf language profile의 explicit binding
2. leaf profile이 없으면 가장 구체적인 matching ancestor profile의 binding
3. workspace local YAML override
4. user global YAML override
5. built-in default binding

동일한 provider command/root marker/init options를 공유하는 profile들은 하나의 backend instance를 재사용 MAY 한다.  
예를 들어 `javascript`, `nodejs`, `react` 가 모두 같은 TS/JS provider binding을 가리키면 supervisor는 하나의 backend pool로 묶을 수 있다.

### 17.5 YAML-defined custom LSP

server는 YAML 설정으로 새 language profile을 추가하거나 기존 profile의 semantic binding을 override 할 수 있어야 한다.

최소 요구:

- 새 `profile_id` 선언
- `extends` 로 부모 profile 지정
- `adapter` 지정
- detection rule 지정
- `semantic.provider.kind = lsp` 지정
- LSP 실행 command/args 지정
- root marker, init options, workspace config 지정 가능

custom LSP binding은 v0.1에서 **stdio transport** 를 REQUIRED 로 한다.  
TCP/stdio 이외 transport는 이후 버전에서 MAY 다룬다.

---

## 18. semantic supervisor

semantic supervisor는 workspace별 semantic backend 수명을 관리한다.

### 18.1 state

- `off`
- `starting`
- `warming`
- `ready`
- `hibernating`
- `stopping`
- `failed`

### 18.2 기본 정책

- 기본 모드는 `auto`
- 요청이 semantic을 필요로 하면 `starting` 또는 `warming` 으로 진입
- 일정 idle 후 `hibernating`
- 더 오래 idle이면 `off`

### 18.3 권장 타임아웃

- `warm_timeout = 5s`
- `hibernate_after = 2m`
- `kill_after = 10m`

### 18.4 hibernate 의미

실제 provider가 진짜 hibernate를 지원하지 않으면, hibernate는 다음으로 구현 가능하다.

- open documents 0개 유지
- 내부 캐시만 유지
- 프로세스 유지 또는 즉시 종료

즉, `hibernating` 은 추상 상태이며 구현체는 더 싼 근사로 대체 가능하다.

### 18.5 backend identity

semantic supervisor는 backend를 단순 `language` 문자열이 아니라 **`binding_id + project_root`** 기준으로 관리 SHOULD 한다.

- `react` 와 `nodejs` 가 같은 binding을 공유하면 같은 backend를 재사용 가능하다.
- 서로 다른 custom LSP YAML binding은 같은 adapter를 공유해도 별도 backend여야 한다.
- 외부 응답에는 이해를 위해 leaf `language_profile_id` 를 실을 수 있지만, 내부 pooling key는 binding 기준이 더 중요하다.

---

## 19. 인덱싱 모델

### 19.1 초기 인덱싱

workspace가 처음 보이면 daemon은 다음을 수행 SHOULD 한다.

1. 파일 스캔
2. ignore filtering
3. language detection
4. text metadata 수집
5. parse queue 적재
6. symbol/import graph 생성

### 19.2 incremental update

파일 watcher 이벤트가 오면 daemon은 다음을 수행한다.

1. 변경 파일 canonicalize
2. roots/ignore 재검사
3. fingerprint 갱신
4. parse 재실행
5. symbol/import graph patch
6. affected plans stale 처리
7. revision 증가
8. resource update publish

### 19.3 invalidation trigger

다음 파일은 특별 취급 SHOULD 한다.

- `tsconfig*.json`
- `package.json`
- `pyproject.toml`
- `setup.cfg`
- `go.mod`
- `Cargo.toml`
- lockfiles
- LSP/provider 설정 파일

이들이 바뀌면 semantic/index coverage가 달라질 수 있으므로 더 넓은 재분석이 필요하다.

### 19.4 부분 준비 상태

index가 완전히 끝나지 않아도 서버는 요청을 처리할 수 있다.  
단, 응답에 아래를 포함해야 한다.

- `coverage.partial = true`
- `coverage.filesIndexed`
- `coverage.filesTotalEstimate`

---

## 20. confidence 모델

### 20.1 목적

에이전트가 결과 신뢰도를 판단할 수 있게 한다.

### 20.2 표현

```json
{
  "score": 0.91,
  "level": "high",
  "reasons": [
    "resolved by semantic provider",
    "no dynamic access detected"
  ]
}
```

### 20.3 권장 레벨

- `high`: 0.90 이상
- `medium`: 0.75 이상 0.90 미만
- `low`: 0.75 미만

### 20.4 점수 구성 예시

권장 구성:
- + semantic corroboration
- + full index coverage
- + unique symbol resolution
- - dynamic access penalty
- - re-export ambiguity penalty
- - inaccessible roots boundary penalty
- - provider unavailable penalty

구체 가중치는 구현체 재량이지만, 동일한 의미를 유지해야 한다.

---

## 21. 동시성 및 일관성 모델

### 21.1 읽기

- snapshot 기반 병렬 읽기를 허용한다.
- search/read/definition/reference preview는 병렬 가능하다.

### 21.2 쓰기

- 한 `shared_workspace` 에 대한 실제 write는 직렬화되어야 한다.
- `apply_plan` 은 compare-and-swap(CAS) 의미를 가져야 한다.

### 21.3 CAS 규칙

`apply_plan` 시점에 다음을 검증 MUST 한다.

- `plan.workspaceRevision == expectedWorkspaceRevision`
- 또는 최소한 모든 `file_fingerprints` 가 여전히 일치

불일치 시 적용하지 않고 `AE_STALE_PLAN` 오류를 반환해야 한다.

### 21.4 긴 작업과 취소

- search/references/rename_preview/indexing은 취소 가능해야 한다.
- progress token이 있으면 주기적으로 진척을 보고해야 한다.
- cancellation이 오면 가능한 빨리 중단 SHOULD 한다.

### 21.5 공정성

daemon은 특정 client_session이 긴 작업을 독점하지 않도록 fairness queue를 유지 SHOULD 한다.

권장:
- client별 soft quota
- cooperative cancellation point
- background indexing의 낮은 우선순위

---

## 22. 상태 머신

### 22.1 daemon 상태 머신

상태:
- `absent`
- `bootstrapping`
- `starting`
- `running_active`
- `running_idle`
- `quiescing`
- `stopping`
- `stopped`
- `failed`

전이:

```text
absent -> bootstrapping -> starting -> running_idle
running_idle -> running_active      (client 연결 또는 작업 시작)
running_active -> running_idle      (활성 작업 0, 연결은 남아있음)
running_idle -> quiescing           (client 0, active task 0)
quiescing -> running_active         (새 연결/작업)
quiescing -> stopping               (idle_exit 만료)
stopping -> stopped
starting -> failed                  (초기화 실패)
running_* -> failed                 (복구 불가 오류)
failed -> absent                    (프로세스 종료 후)
```

상태 의미:
- `running_active`: 적어도 하나의 active client 또는 task 존재
- `running_idle`: 살아 있으나 유휴
- `quiescing`: 종료를 향한 countdown 상태

### 22.2 shared_workspace 상태 머신

상태:
- `unseen`
- `discovering`
- `indexing`
- `ready`
- `degraded`
- `invalidating`
- `disposing`

전이:

```text
unseen -> discovering
 discovering -> indexing
 indexing -> ready
 indexing -> degraded     (부분 실패)
 ready -> invalidating    (외부 변경/설정 변경)
 invalidating -> indexing
 ready -> disposing       (LRU eviction / daemon stop)
 degraded -> indexing     (재시도)
```

### 22.3 semantic backend 상태 머신

상태:
- `off`
- `starting`
- `warming`
- `ready`
- `hibernating`
- `stopping`
- `failed`

전이:

```text
off -> starting
starting -> warming
warming -> ready
ready -> hibernating      (idle)
hibernating -> ready      (새 semantic 요청)
ready -> stopping         (kill_after)
hibernating -> stopping   (kill_after)
stopping -> off
starting|warming|ready -> failed
failed -> off             (재시도 백오프 이후)
```

### 22.4 edit plan 상태 머신

상태:
- `creating`
- `ready`
- `applied`
- `discarded`
- `stale`
- `expired`
- `failed`

전이:

```text
creating -> ready
creating -> failed
ready -> applied
ready -> discarded
ready -> stale      (revision/fingerprint mismatch 감지)
ready -> expired    (TTL 만료)
stale -> expired    (보존기간 만료)
```

---

## 23. 편집 모델

### 23.1 기본 원칙

- 단일 파일의 단순 edit라도 plan을 만들 수 있다.
- 그러나 v1에서 cross-file/semantic edit는 항상 plan-first여야 한다.
- `apply_plan` 전에는 실제 파일 변경이 없어야 한다.

### 23.2 edit ordering

한 파일 내 edit는 byte offset 내림차순으로 적용해야 한다.  
이렇게 하면 앞선 edit가 뒤쪽 offset을 깨뜨리지 않는다.

### 23.3 merge 정책

v1에서는 자동 merge를 하지 않는다.  
conflict 또는 stale이면 무조건 재계산이다.

### 23.4 atomicity

한 `apply_plan` 은 workspace 관점에서 원자적이어야 한다.

권장 방법:
- temp file write
- fsync 필요 시 옵션
- atomic rename
- 전 파일 성공 후 revision commit

### 23.5 rollback

적용 도중 실패하면 server는 best-effort rollback을 해야 한다.  
완전 rollback 불가 시 `partialFailure` 를 명시하고 사용자介入이 필요함을 보고해야 한다.

---

## 24. rename preview 알고리즘

### 24.1 입력

- target location 또는 symbol_id
- `newName`
- mode: `auto | syntax | semantic`

### 24.2 절차

1. target resolve
2. prepare rename 영역 확인
3. 언어별 identifier rule 검증
4. syntax/index 기반 candidate 수집
5. ambiguity/conflict/dynamic usage 검사
6. confidence 계산
7. threshold 미달 또는 정책상 필요 시 semantic 승격
8. semantic 결과 병합 또는 대체
9. file edits 생성
10. optional validation (diagnostics)
11. plan 저장
12. preview 반환

### 24.3 semantic 승격 기준

다음 중 하나면 semantic 승격 SHOULD 한다.

- exported/public symbol
- package boundary crossing
- alias or re-export chain
- dynamic access 발견
- confidence < 0.85
- syntax coverage partial
- client가 `mode=semantic` 명시

### 24.4 충돌 규칙

다음 경우 rename preview는 실패하거나 low confidence여야 한다.

- target이 식별자 rename 불가 위치
- 새 이름이 언어 문법 위반
- 같은 scope 충돌 발생
- view 밖 영향이 있으나 안전한 redaction으로도 처리 불가

### 24.5 validation

semantic backend가 준비되어 있으면 rename preview 후 in-memory validation으로 diagnostics delta를 구할 수 있다.  
이 값은 advisory다.

---

## 25. tool surface

도구 이름은 MCP 최신 naming guidance를 따르는 짧고 고정된 이름을 사용한다.

### 25.1 도구 목록

1. `ae.workspace.status`
2. `ae.workspace.read_spans`
3. `ae.workspace.search_text`
4. `ae.workspace.search_structure`
5. `ae.symbol.find`
6. `ae.symbol.definition`
7. `ae.symbol.references`
8. `ae.refactor.rename_preview`
9. `ae.edit.prepare_plan`
10. `ae.edit.apply_plan`
11. `ae.edit.discard_plan`
12. `ae.semantic.status`
13. `ae.semantic.ensure`
14. `ae.diagnostics.read`

### 25.2 공통 규칙

모든 tool은 다음을 SHOULD 따른다.

- `structuredContent` 반환
- compact text summary도 함께 반환
- `engine`, `confidence`, `warnings`, `workspaceRevision` 등 공통 메타를 포함
- long-running tool은 progress 지원
- 실패 시 protocol error가 아니라면 `isError: true` 와 structured execution error를 반환

### 25.3 task support

다음 tool은 `execution.taskSupport = optional` 권장을 따른다.

- `ae.workspace.search_text`
- `ae.workspace.search_structure`
- `ae.symbol.find`
- `ae.symbol.references`
- `ae.refactor.rename_preview`
- `ae.diagnostics.read`

다만 tasks는 실험 기능이므로 v1 핵심 흐름은 task 비의존이어야 한다.

### 25.4 phase-gated exposure

본 절의 이름들은 최종 제품 기준 reserved surface다.  
그러나 구현 Phase에 따라 실제 `tools/list` 에 노출되는 subset은 달라질 수 있다.

- Phase 1 reference build는 `ae.workspace.status`, `ae.workspace.read_spans`, `ae.workspace.search_text`, `ae.edit.prepare_plan`, `ae.edit.apply_plan`, `ae.edit.discard_plan`, `ae.semantic.status`, `ae.semantic.ensure` 만 advertise MUST 한다.
- 아직 advertise하지 않은 이름은 문서상 reserved 상태로 유지한다.
- client는 advertised subset만 호출해야 하며, phase 미구현 tool에 대한 의존을 기본 흐름으로 두면 안 된다.

---

## 26. 공통 스키마 정의

이 절의 스키마는 JSON Schema 2020-12 기준 개념 정의다.

### 26.1 공통 정의 파일

아래 `ae-common.schema.json` 은 각 tool의 input/output schema에서 참조하는 공통 정의다.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "ae-common.schema.json",
  "$defs": {
    "Uri": {
      "type": "string",
      "minLength": 1
    },
    "Position": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "line": { "type": "integer", "minimum": 0 },
        "character": { "type": "integer", "minimum": 0 }
      },
      "required": ["line", "character"]
    },
    "Range": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "start": { "$ref": "#/$defs/Position" },
        "end": { "$ref": "#/$defs/Position" }
      },
      "required": ["start", "end"]
    },
    "LocationRef": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "uri": { "$ref": "#/$defs/Uri" },
        "range": { "$ref": "#/$defs/Range" }
      },
      "required": ["uri", "range"]
    },
    "TargetRef": {
      "oneOf": [
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "symbolId": { "type": "string", "minLength": 1 },
            "workspaceRevision": { "type": "string", "minLength": 1 }
          },
          "required": ["symbolId"]
        },
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "location": { "$ref": "#/$defs/LocationRef" },
            "workspaceRevision": { "type": "string", "minLength": 1 }
          },
          "required": ["location"]
        }
      ]
    },
    "Scope": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "uris": {
          "type": "array",
          "items": { "$ref": "#/$defs/Uri" },
          "uniqueItems": true
        },
        "includeIgnored": { "type": "boolean", "default": false },
        "includeGenerated": { "type": "boolean", "default": false },
        "includeDefaultExcluded": { "type": "boolean", "default": false },
        "excludeGlobs": {
          "type": "array",
          "items": { "type": "string" }
        },
        "maxFiles": { "type": "integer", "minimum": 1 }
      }
    },
    "Engine": {
      "type": "string",
      "enum": ["text", "syntax", "semantic"]
    },
    "Confidence": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "score": { "type": "number", "minimum": 0, "maximum": 1 },
        "level": { "type": "string", "enum": ["high", "medium", "low"] },
        "reasons": {
          "type": "array",
          "items": { "type": "string" }
        }
      },
      "required": ["score", "level"]
    },
    "Warning": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "code": { "type": "string" },
        "message": { "type": "string" },
        "severity": { "type": "string", "enum": ["info", "warning", "error"] }
      },
      "required": ["code", "message"]
    },
    "FileFingerprint": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "uri": { "$ref": "#/$defs/Uri" },
        "contentHash": { "type": "string" },
        "size": { "type": "integer", "minimum": 0 },
        "mtimeMs": { "type": "number", "minimum": 0 },
        "lineEnding": { "type": "string", "enum": ["lf", "crlf"] },
        "encoding": { "type": "string" }
      },
      "required": ["uri", "contentHash", "size"]
    },
    "SymbolRef": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "symbolId": { "type": "string" },
        "name": { "type": "string" },
        "kind": { "type": "string" },
        "language": { "type": "string" },
        "declaration": { "$ref": "#/$defs/LocationRef" },
        "exported": { "type": "boolean" }
      },
      "required": ["symbolId", "name", "kind", "language", "declaration"]
    },
    "TextSpan": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "uri": { "$ref": "#/$defs/Uri" },
        "range": { "$ref": "#/$defs/Range" },
        "before": { "type": "string" },
        "text": { "type": "string" },
        "after": { "type": "string" },
        "fingerprint": { "$ref": "#/$defs/FileFingerprint" }
      },
      "required": ["uri", "range", "text"]
    },
    "TextMatch": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "uri": { "$ref": "#/$defs/Uri" },
        "range": { "$ref": "#/$defs/Range" },
        "snippet": { "type": "string" },
        "score": { "type": "number", "minimum": 0 },
        "fingerprint": { "$ref": "#/$defs/FileFingerprint" }
      },
      "required": ["uri", "range", "snippet"]
    },
    "ReferenceMatch": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "location": { "$ref": "#/$defs/LocationRef" },
        "role": { "type": "string", "enum": ["declaration", "reference", "write", "read", "import", "export", "unknown"] },
        "snippet": { "type": "string" },
        "symbolId": { "type": "string" }
      },
      "required": ["location", "role"]
    },
    "Diagnostic": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "uri": { "$ref": "#/$defs/Uri" },
        "range": { "$ref": "#/$defs/Range" },
        "severity": { "type": "string", "enum": ["error", "warning", "information", "hint"] },
        "code": { "type": ["string", "integer"] },
        "source": { "type": "string" },
        "message": { "type": "string" }
      },
      "required": ["uri", "range", "severity", "message"]
    },
    "TextEdit": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "range": { "$ref": "#/$defs/Range" },
        "newText": { "type": "string" }
      },
      "required": ["range", "newText"]
    },
    "FileEdit": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "uri": { "$ref": "#/$defs/Uri" },
        "fingerprint": { "$ref": "#/$defs/FileFingerprint" },
        "edits": {
          "type": "array",
          "items": { "$ref": "#/$defs/TextEdit" },
          "minItems": 1
        }
      },
      "required": ["uri", "edits"]
    },
    "PlanSummary": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "filesTouched": { "type": "integer", "minimum": 0 },
        "edits": { "type": "integer", "minimum": 0 },
        "languages": { "type": "array", "items": { "type": "string" } },
        "blocked": { "type": "integer", "minimum": 0 },
        "requiresConfirmation": { "type": "boolean" }
      },
      "required": ["filesTouched", "edits", "requiresConfirmation"]
    },
    "Coverage": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "partial": { "type": "boolean" },
        "filesIndexed": { "type": "integer", "minimum": 0 },
        "filesTotalEstimate": { "type": "integer", "minimum": 0 }
      },
      "required": ["partial"]
    },
    "BackendState": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "language": { "type": "string" },
        "state": { "type": "string", "enum": ["off", "starting", "warming", "ready", "hibernating", "stopping", "failed"] },
        "provider": { "type": "string" },
        "idleForMs": { "type": "integer", "minimum": 0 },
        "lastError": { "type": "string" }
      },
      "required": ["language", "state"]
    },
    "ExecutionError": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "code": { "type": "string" },
        "message": { "type": "string" },
        "retryable": { "type": "boolean" },
        "details": {}
      },
      "required": ["code", "message", "retryable"]
    }
  }
}
```

---

## 27. tool 상세 스키마

아래 스키마는 `structuredContent` 기준이다.  
실제 MCP tool result는 compact text 요약을 `content` 로도 같이 제공해야 한다.

### 27.1 `ae.workspace.status`

목적: 현재 view, workspace, index, backend 상태를 요약한다.

#### inputSchema

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "includeBackends": { "type": "boolean", "default": true },
    "includeCoverage": { "type": "boolean", "default": true }
  }
}
```

#### outputSchema

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "clientSessionId": { "type": "string" },
    "viewId": { "type": "string" },
    "workspaceId": { "type": "string" },
    "workspaceRevision": { "type": "string" },
    "roots": { "type": "array", "items": { "$ref": "ae-common.schema.json#/$defs/Uri" } },
    "coverage": { "$ref": "ae-common.schema.json#/$defs/Coverage" },
    "backends": { "type": "array", "items": { "$ref": "ae-common.schema.json#/$defs/BackendState" } },
    "warnings": { "type": "array", "items": { "$ref": "ae-common.schema.json#/$defs/Warning" } }
  },
  "required": ["clientSessionId", "viewId", "workspaceId", "workspaceRevision", "roots"]
}
```

### 27.2 `ae.workspace.read_spans`

목적: 파일 전체가 아니라 필요한 범위만 읽는다.

#### inputSchema

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "spans": {
      "type": "array",
      "minItems": 1,
      "items": {
        "type": "object",
        "additionalProperties": false,
        "properties": {
          "uri": { "$ref": "ae-common.schema.json#/$defs/Uri" },
          "range": { "$ref": "ae-common.schema.json#/$defs/Range" },
          "contextLines": { "type": "integer", "minimum": 0, "default": 2 }
        },
        "required": ["uri", "range"]
      }
    }
  },
  "required": ["spans"]
}
```

#### outputSchema

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "workspaceRevision": { "type": "string" },
    "spans": { "type": "array", "items": { "$ref": "ae-common.schema.json#/$defs/TextSpan" } },
    "warnings": { "type": "array", "items": { "$ref": "ae-common.schema.json#/$defs/Warning" } }
  },
  "required": ["workspaceRevision", "spans"]
}
```

### 27.3 `ae.workspace.search_text`

목적: ignore-aware 텍스트 검색.

기본 exclude(`dist`, `target`, `coverage` 등)는 convenience default로 취급해야 하며, client는 필요할 때 `scope.includeDefaultExcluded=true`로 이를 우회할 수 있어야 한다.  
강제 제외는 별도 정책 축으로 유지하고, reference 구현에서는 최소한 `.git` 같은 경계성 디렉터리만 강제 제외로 두는 편이 안전하다.

#### inputSchema

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "query": { "type": "string", "minLength": 1 },
    "mode": { "type": "string", "enum": ["literal", "regex", "word"], "default": "literal" },
    "caseSensitive": { "type": "boolean", "default": false },
    "scope": { "$ref": "ae-common.schema.json#/$defs/Scope" },
    "contextLines": { "type": "integer", "minimum": 0, "default": 1 },
    "limit": { "type": "integer", "minimum": 1, "maximum": 10000, "default": 200 }
  },
  "required": ["query"]
}
```

#### outputSchema

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "workspaceRevision": { "type": "string" },
    "engine": { "$ref": "ae-common.schema.json#/$defs/Engine" },
    "matches": { "type": "array", "items": { "$ref": "ae-common.schema.json#/$defs/TextMatch" } },
    "truncated": { "type": "boolean" },
    "coverage": { "$ref": "ae-common.schema.json#/$defs/Coverage" },
    "warnings": { "type": "array", "items": { "$ref": "ae-common.schema.json#/$defs/Warning" } }
  },
  "required": ["workspaceRevision", "engine", "matches", "truncated"]
}
```

### 27.4 `ae.workspace.search_structure`

목적: 구조 검색.

#### inputSchema

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "language": { "type": "string" },
    "dsl": { "type": "string", "enum": ["auto", "sg", "tsquery"], "default": "auto" },
    "pattern": { "type": "string", "minLength": 1 },
    "scope": { "$ref": "ae-common.schema.json#/$defs/Scope" },
    "limit": { "type": "integer", "minimum": 1, "maximum": 10000, "default": 200 }
  },
  "required": ["language", "pattern"]
}
```

#### outputSchema

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "workspaceRevision": { "type": "string" },
    "engine": { "$ref": "ae-common.schema.json#/$defs/Engine" },
    "matches": { "type": "array", "items": { "$ref": "ae-common.schema.json#/$defs/TextMatch" } },
    "truncated": { "type": "boolean" },
    "warnings": { "type": "array", "items": { "$ref": "ae-common.schema.json#/$defs/Warning" } }
  },
  "required": ["workspaceRevision", "engine", "matches", "truncated"]
}
```

### 27.5 `ae.symbol.find`

목적: 이름/종류 기준 심볼 후보 탐색.

#### inputSchema

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "query": { "type": "string", "minLength": 1 },
    "kind": { "type": "string" },
    "language": { "type": "string" },
    "scope": { "$ref": "ae-common.schema.json#/$defs/Scope" },
    "fuzzy": { "type": "boolean", "default": true },
    "limit": { "type": "integer", "minimum": 1, "maximum": 1000, "default": 50 }
  },
  "required": ["query"]
}
```

#### outputSchema

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "workspaceRevision": { "type": "string" },
    "engine": { "$ref": "ae-common.schema.json#/$defs/Engine" },
    "confidence": { "$ref": "ae-common.schema.json#/$defs/Confidence" },
    "symbols": { "type": "array", "items": { "$ref": "ae-common.schema.json#/$defs/SymbolRef" } },
    "truncated": { "type": "boolean" },
    "warnings": { "type": "array", "items": { "$ref": "ae-common.schema.json#/$defs/Warning" } }
  },
  "required": ["workspaceRevision", "engine", "confidence", "symbols", "truncated"]
}
```

### 27.6 `ae.symbol.definition`

목적: definition 찾기.

#### inputSchema

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "target": { "$ref": "ae-common.schema.json#/$defs/TargetRef" },
    "mode": { "type": "string", "enum": ["auto", "syntax", "semantic"], "default": "auto" }
  },
  "required": ["target"]
}
```

#### outputSchema

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "workspaceRevision": { "type": "string" },
    "engine": { "$ref": "ae-common.schema.json#/$defs/Engine" },
    "confidence": { "$ref": "ae-common.schema.json#/$defs/Confidence" },
    "definitions": { "type": "array", "items": { "$ref": "ae-common.schema.json#/$defs/LocationRef" } },
    "warnings": { "type": "array", "items": { "$ref": "ae-common.schema.json#/$defs/Warning" } }
  },
  "required": ["workspaceRevision", "engine", "confidence", "definitions"]
}
```

### 27.7 `ae.symbol.references`

목적: 참조 찾기.

#### inputSchema

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "target": { "$ref": "ae-common.schema.json#/$defs/TargetRef" },
    "mode": { "type": "string", "enum": ["auto", "syntax", "semantic"], "default": "auto" },
    "includeDeclarations": { "type": "boolean", "default": false },
    "scope": { "$ref": "ae-common.schema.json#/$defs/Scope" },
    "limit": { "type": "integer", "minimum": 1, "maximum": 20000, "default": 1000 }
  },
  "required": ["target"]
}
```

#### outputSchema

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "workspaceRevision": { "type": "string" },
    "engine": { "$ref": "ae-common.schema.json#/$defs/Engine" },
    "confidence": { "$ref": "ae-common.schema.json#/$defs/Confidence" },
    "references": { "type": "array", "items": { "$ref": "ae-common.schema.json#/$defs/ReferenceMatch" } },
    "truncated": { "type": "boolean" },
    "warnings": { "type": "array", "items": { "$ref": "ae-common.schema.json#/$defs/Warning" } }
  },
  "required": ["workspaceRevision", "engine", "confidence", "references", "truncated"]
}
```

### 27.8 `ae.refactor.rename_preview`

목적: rename을 실제 적용하지 않고 plan으로 반환.

#### inputSchema

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "target": { "$ref": "ae-common.schema.json#/$defs/TargetRef" },
    "newName": { "type": "string", "minLength": 1 },
    "mode": { "type": "string", "enum": ["auto", "syntax", "semantic"], "default": "auto" },
    "scope": { "$ref": "ae-common.schema.json#/$defs/Scope" },
    "validate": { "type": "boolean", "default": true },
    "sampleLimit": { "type": "integer", "minimum": 1, "maximum": 200, "default": 20 }
  },
  "required": ["target", "newName"]
}
```

#### outputSchema

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "planId": { "type": "string" },
    "workspaceId": { "type": "string" },
    "viewId": { "type": "string" },
    "workspaceRevision": { "type": "string" },
    "engine": { "$ref": "ae-common.schema.json#/$defs/Engine" },
    "confidence": { "$ref": "ae-common.schema.json#/$defs/Confidence" },
    "summary": { "$ref": "ae-common.schema.json#/$defs/PlanSummary" },
    "fileEdits": { "type": "array", "items": { "$ref": "ae-common.schema.json#/$defs/FileEdit" } },
    "sampleEdits": {
      "type": "array",
      "items": {
        "type": "object",
        "additionalProperties": false,
        "properties": {
          "uri": { "$ref": "ae-common.schema.json#/$defs/Uri" },
          "before": { "type": "string" },
          "after": { "type": "string" }
        },
        "required": ["uri", "before", "after"]
      }
    },
    "diagnostics": { "type": "array", "items": { "$ref": "ae-common.schema.json#/$defs/Diagnostic" } },
    "coverage": { "$ref": "ae-common.schema.json#/$defs/Coverage" },
    "warnings": { "type": "array", "items": { "$ref": "ae-common.schema.json#/$defs/Warning" } },
    "resources": {
      "type": "array",
      "items": { "type": "string" }
    },
    "expiresAt": { "type": "string", "format": "date-time" }
  },
  "required": ["planId", "workspaceId", "viewId", "workspaceRevision", "engine", "confidence", "summary", "fileEdits", "warnings", "expiresAt"]
}
```

### 27.9 `ae.edit.prepare_plan`

목적: client가 제안한 file edits를 roots/fingerprint/range 기준으로 검증하고 apply/discard 전에 보관 가능한 plan으로 승격.

#### inputSchema

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "fileEdits": {
      "type": "array",
      "minItems": 1,
      "items": { "$ref": "ae-common.schema.json#/$defs/FileEdit" }
    }
  },
  "required": ["fileEdits"]
}
```

#### outputSchema

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "planId": { "type": "string" },
    "workspaceId": { "type": "string" },
    "workspaceRevision": { "type": "string" },
    "summary": { "$ref": "ae-common.schema.json#/$defs/PlanSummary" },
    "warnings": { "type": "array", "items": { "$ref": "ae-common.schema.json#/$defs/Warning" } }
  },
  "required": ["planId", "workspaceId", "workspaceRevision", "summary"]
}
```

### 27.10 `ae.edit.apply_plan`

목적: 준비된 plan을 실제 파일에 반영.

#### inputSchema

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "planId": { "type": "string", "minLength": 1 },
    "expectedWorkspaceRevision": { "type": "string", "minLength": 1 }
  },
  "required": ["planId", "expectedWorkspaceRevision"]
}
```

#### outputSchema

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "applied": { "type": "boolean" },
    "workspaceId": { "type": "string" },
    "previousRevision": { "type": "string" },
    "newRevision": { "type": "string" },
    "filesTouched": { "type": "integer", "minimum": 0 },
    "editsApplied": { "type": "integer", "minimum": 0 },
    "warnings": { "type": "array", "items": { "$ref": "ae-common.schema.json#/$defs/Warning" } }
  },
  "required": ["applied", "workspaceId", "previousRevision", "newRevision", "filesTouched", "editsApplied"]
}
```

### 27.11 `ae.edit.discard_plan`

목적: plan 폐기.

#### inputSchema

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "planId": { "type": "string", "minLength": 1 }
  },
  "required": ["planId"]
}
```

#### outputSchema

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "discarded": { "type": "boolean" },
    "planId": { "type": "string" }
  },
  "required": ["discarded", "planId"]
}
```

### 27.12 `ae.semantic.status`

목적: semantic backend 상태 조회.

Phase 1 reference build는 detected leaf profile을 우선 반환하고, warm/refresh/stop으로 바뀐 skeleton lifecycle state를 함께 반환 SHOULD 한다.

#### inputSchema

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "language": { "type": "string" }
  }
}
```

#### outputSchema

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "workspaceId": { "type": "string" },
    "backends": { "type": "array", "items": { "$ref": "ae-common.schema.json#/$defs/BackendState" } }
  },
  "required": ["workspaceId", "backends"]
}
```

### 27.13 `ae.semantic.ensure`

목적: semantic backend를 명시적으로 warm/stop/refresh.

Phase 1 skeleton semantics:
- `warm`: backend state를 `ready` 로 전이
- `refresh`: backend state를 `ready` 로 재설정
- `stop`: backend state를 `off` 로 전이

#### inputSchema

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "language": { "type": "string" },
    "action": { "type": "string", "enum": ["warm", "stop", "refresh"], "default": "warm" }
  },
  "required": ["language"]
}
```

#### outputSchema

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "workspaceId": { "type": "string" },
    "backend": { "$ref": "ae-common.schema.json#/$defs/BackendState" }
  },
  "required": ["workspaceId", "backend"]
}
```

### 27.13 `ae.diagnostics.read`

목적: 현재 diagnostics 조회.

#### inputSchema

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "scope": { "$ref": "ae-common.schema.json#/$defs/Scope" },
    "minSeverity": { "type": "string", "enum": ["hint", "information", "warning", "error"], "default": "warning" },
    "limit": { "type": "integer", "minimum": 1, "maximum": 20000, "default": 5000 },
    "mode": { "type": "string", "enum": ["auto", "semantic"], "default": "auto" }
  }
}
```

#### outputSchema

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "workspaceRevision": { "type": "string" },
    "engine": { "$ref": "ae-common.schema.json#/$defs/Engine" },
    "diagnostics": { "type": "array", "items": { "$ref": "ae-common.schema.json#/$defs/Diagnostic" } },
    "truncated": { "type": "boolean" },
    "warnings": { "type": "array", "items": { "$ref": "ae-common.schema.json#/$defs/Warning" } }
  },
  "required": ["workspaceRevision", "engine", "diagnostics", "truncated"]
}
```

---

## 28. resource surface

본 서버는 tools 외에 resources도 제공해야 한다.

### 28.1 URI scheme

커스텀 scheme:

- `ae://workspace/{workspaceId}/status`
- `ae://workspace/{workspaceId}/plans`
- `ae://workspace/{workspaceId}/plans/{planId}`
- `ae://workspace/{workspaceId}/plans/{planId}/diff`
- `ae://workspace/{workspaceId}/diagnostics`
- `ae://workspace/{workspaceId}/symbols/{symbolId}`

### 28.2 리소스 목적

- status: 현재 workspace 상태
- plans: 살아 있는 plan 목록
- plan: 개별 plan detail
- diff: plan diff preview
- diagnostics: 현재 diagnostic snapshot
- symbol: 심볼 메타데이터

### 28.3 resources/list

client는 현재 view에서 접근 가능한 resource만 봐야 한다.

### 28.4 resources/subscribe

daemon은 다음 리소스에 대해 subscribe를 SHOULD 지원한다.

- workspace status
- diagnostics
- plan detail
- plan diff

### 28.5 update notifications

다음 사건에서 `notifications/resources/updated` 를 SHOULD 발행한다.

- workspace revision 변경
- backend state 변경
- diagnostics 갱신
- plan 생성/폐기/만료/적용

### 28.6 list_changed notifications

plan 목록이나 workspace-visible resource 집합이 바뀌면 `notifications/resources/list_changed` 를 SHOULD 발행한다.

---

## 29. MCP capabilities 선언

### 29.1 server capabilities

서버는 최소 다음을 광고해야 한다.

```json
{
  "capabilities": {
    "tools": {
      "listChanged": false
    },
    "resources": {
      "subscribe": true,
      "listChanged": true
    }
  }
}
```

추가로 구현한다면:
- logging
- completion
- tasks (optional)

### 29.2 client capabilities 기대치

권장 client capabilities:
- roots
- progress
- cancellation
- resources

서버는 roots capability가 없는 client도 동작시킬 수 있으나, 이 경우 기본 작업 범위를 명시적으로 제한해야 한다.  
v1에서는 roots 미지원 client를 기본 비지원으로 두는 것이 안전하다.
현재 `spiki` launcher reference 구현도 기본적으로 roots 없는 `initialize`를 거부하며, 편의용 `cwd` fallback은 명시 opt-in(`SPIKI_ALLOW_CWD_ROOT_FALLBACK=1`) 뒤에서만 허용한다.

---

## 30. progress 및 cancellation

### 30.1 progress

다음 작업은 progress를 보내야 한다.

- 초기 인덱싱
- 대규모 text search
- structure search
- references
- rename preview
- diagnostics full refresh
- semantic backend warmup

### 30.2 progress message 규칙

메시지는 사람도 이해 가능한 간단한 문장이어야 한다.

예:
- `Scanning workspace files`
- `Parsing TypeScript files`
- `Starting semantic backend`
- `Computing rename edits`

### 30.3 cancellation

- non-task long request는 `notifications/cancelled` 로 취소 가능해야 한다.
- task-augmented 요청은 `tasks/cancel` 을 사용한다.
- 취소 후 가능한 빨리 처리 중단 SHOULD 한다.

### 30.4 부분 결과

취소된 요청은 부분 결과를 commit해서는 안 된다.  
plan 생성 중 취소되면 plan은 생성되지 않거나 `failed`/`discarded` 상태여야 한다.

---

## 31. 오류 모델

### 31.1 protocol error와 execution error 구분

- 요청 JSON 구조가 틀리면 protocol error
- tool arguments schema 불일치면 protocol error
- 실제 작업 중 실패는 execution error (`isError: true`)

### 31.2 표준 execution error 코드

다음 문자열 코드를 MUST 또는 SHOULD 사용한다.

- `AE_ROOT_VIOLATION`
- `AE_PERMISSION_DENIED`
- `AE_BINARY_FILE`
- `AE_FILE_TOO_LARGE`
- `AE_UNSUPPORTED_LANGUAGE`
- `AE_INDEX_NOT_READY`
- `AE_SEMANTIC_UNAVAILABLE`
- `AE_INVALID_TARGET`
- `AE_RENAME_CONFLICT`
- `AE_STALE_PLAN`
- `AE_PLAN_EXPIRED`
- `AE_PLAN_NOT_FOUND`
- `AE_CONFLICT`
- `AE_TIMEOUT`
- `AE_CANCELLED`
- `AE_INTERNAL`

### 31.3 execution error 예시

```json
{
  "isError": true,
  "structuredContent": {
    "code": "AE_STALE_PLAN",
    "message": "Workspace changed since preview was created",
    "retryable": true,
    "details": {
      "planId": "plan_01J...",
      "expectedWorkspaceRevision": "ws_12",
      "currentWorkspaceRevision": "ws_13"
    }
  },
  "content": [
    {
      "type": "text",
      "text": "AE_STALE_PLAN: Workspace changed since preview was created. Recompute the plan and try again."
    }
  ]
}
```

### 31.4 재시도 규칙

- `AE_STALE_PLAN`, `AE_INDEX_NOT_READY`, `AE_TIMEOUT`, `AE_SEMANTIC_UNAVAILABLE` 는 보통 retryable
- `AE_ROOT_VIOLATION`, `AE_INVALID_TARGET`, `AE_RENAME_CONFLICT` 는 보통 non-retryable

---

## 32. 보안 및 ACL

### 32.1 기본 원칙

- no roots, no access
- view 밖 결과 반환 금지
- socket 접근은 현재 사용자로 제한
- direct HTTP는 기본 비활성화

### 32.2 로컬 socket 보안

- runtime dir 권한 0700
- socket 파일 권한 0600
- Windows pipe ACL을 현재 사용자 SID로 제한

### 32.3 HTTP 보안

HTTP 모드에서 daemon은 반드시 다음을 지켜야 한다.

- localhost bind only
- Origin 검증
- 인증 토큰 요구
- session hijacking 완화

### 32.4 path traversal 방지

모든 입력 URI/path는 canonicalize 후 roots 포함 여부를 검사해야 한다.

### 32.5 cross-view 정보 누설 방지

server는 shared cache의 존재를 이유로 다른 client의 roots 범위를 유추할 수 있는 통계를 직접 노출해서는 안 된다.

### 32.6 tool safety

`ae.edit.apply_plan` 은 destructive tool이다.  
host/client는 human confirmation을 제공 SHOULD 한다.

---

## 33. 자동 발견과 프로젝트 모델

### 33.1 프로젝트 루트 발견

다음 표식을 기준으로 project root 후보를 찾는다.

- `.git`
- `package.json`
- `.spiki/languages.yaml`
- `spiki.languages.yaml`
- `tsconfig*.json`
- `pyproject.toml`
- `go.mod`
- `Cargo.toml`

### 33.2 root와 project root의 관계

client roots는 ACL 경계이고, project root는 인덱싱/semantic 구성 단위다.  
둘은 같을 수도, 다를 수도 있다.

### 33.3 monorepo

monorepo에서는 여러 package/project를 하나의 shared_workspace 아래 둘 수 있다.  
단, rename/reference 계산은 scope와 language/project boundary를 고려해야 한다.

---

## 34. ignore 정책 세부

### 34.1 우선순위

기본 권장 우선순위:

1. roots ACL
2. explicit scope include/exclude
3. hardcoded unsafe/huge directories
4. VCS ignore rules
5. generated file heuristics

### 34.2 generated file heuristics

예시:
- 파일 헤더에 `generated`, `do not edit`
- minified bundle 특징
- build output 디렉토리 위치

### 34.3 opt-in flags

`Scope` 에 다음 opt-in을 둔다.

- `includeIgnored`
- `includeGenerated`

기본은 둘 다 false다.

---

## 35. 실제 apply 절차

`ae.edit.apply_plan` 구현은 아래를 따라야 한다.

1. workspace write lock 획득
2. plan 존재 여부 확인
3. plan 만료 여부 확인
4. `expectedWorkspaceRevision` 확인
5. file fingerprints 재검증
6. 각 파일에 대해 edit ordering 계산
7. temp write 또는 in-memory patch
8. 전체 성공 시 commit
9. revision 증가
10. 관련 plan stale 처리
11. resources/updated 발행
12. lock 해제

실패 시:
- commit 전이면 전체 abort
- commit 중 실패면 best-effort rollback 후 `AE_INTERNAL` 또는 `AE_CONFLICT`

---

## 36. observability

### 36.1 logging

daemon은 다음 범주의 로그를 SHOULD 남긴다.

- startup/shutdown
- workspace discover/index
- backend lifecycle
- tool invocation latency
- apply outcome
- stale/conflict events

### 36.2 metrics

권장 메트릭:

- active clients
- active workspaces
- backend states by language
- hot attach latency
- cold start latency
- search latency p50/p95
- rename preview latency p50/p95
- apply success/failure
- stale plan rate
- cache hit ratio

### 36.3 doctor

launcher는 최소 아래 명령을 제공 SHOULD 한다.

```bash
npx -y @org/agent-editor-mcp doctor
npx -y @org/agent-editor-mcp daemon status
npx -y @org/agent-editor-mcp daemon stop
```

---

## 37. 설정 파일

### 37.1 위치

권장 위치:

- Linux/macOS config: `$XDG_CONFIG_HOME/agent-editor/config.toml` 또는 `~/.config/agent-editor/config.toml`
- Linux/macOS global language profiles: `$XDG_CONFIG_HOME/agent-editor/languages.yaml` 또는 `~/.config/agent-editor/languages.yaml`
- Linux/macOS cache: `$XDG_CACHE_HOME/agent-editor` 또는 `~/.cache/agent-editor`
- Linux/macOS runtime: `$XDG_RUNTIME_DIR/agent-editor`
- Windows config: `%APPDATA%\\AgentEditor\\config.toml`
- Windows global language profiles: `%APPDATA%\\AgentEditor\\languages.yaml`
- Windows cache: `%LOCALAPPDATA%\\AgentEditor\\Cache`

workspace local override 위치는 다음 둘 중 하나를 권장한다.

- `<repo>/.spiki/languages.yaml`
- `<repo>/spiki.languages.yaml`

### 37.2 기본 설정 예시

```toml
[daemon]
idle_exit = "20m"
max_workspaces = 12

[index]
watch = true
respect_gitignore = true
max_index_file_size_mb = 2
exclude = [".git", "node_modules", "dist", "build", "target", ".next", ".turbo", ".cache", "coverage"]

[semantic]
mode = "auto"
warm_timeout = "5s"
hibernate_after = "2m"
kill_after = "10m"

[plans]
ttl = "30m"
max_retained = 100

[http]
enabled = false
bind = "127.0.0.1"
```

### 37.3 language profile YAML

`config.toml` 은 daemon/runtime 설정용이고, language profile과 custom LSP binding은 별도 YAML로 관리 SHOULD 한다.

merge 순서:

1. built-in profile registry
2. user global `languages.yaml`
3. workspace local `languages.yaml`

workspace local YAML이 가장 높은 우선순위를 가진다.

권장 스키마 예시:

```yaml
version: 1

profiles:
  - id: javascript
    builtin: true
    adapter: javascript

  - id: nodejs
    extends: javascript
    detect:
      any:
        - path_exists: package.json
        - package_json_field: engines.node
    semantic:
      provider:
        kind: builtin
        binding: ts-js-default

  - id: react
    extends: nodejs
    file_patterns:
      - "**/*.jsx"
    detect:
      any:
        - package_json_dependency: react
        - package_json_dependency: preact
    semantic:
      provider:
        kind: builtin
        binding: ts-js-react

  - id: nextjs
    extends: react
    detect:
      any:
        - package_json_dependency: next
        - path_exists: next.config.js
        - path_exists: next.config.mjs
        - path_exists: next.config.ts
    semantic:
      provider:
        kind: builtin
        binding: ts-js-next

  - id: vue
    extends: nodejs
    file_patterns:
      - "**/*.vue"
    detect:
      any:
        - package_json_dependency: vue
        - path_glob_exists: "src/**/*.vue"
    semantic:
      provider:
        kind: builtin
        binding: ts-js-vue

  - id: nuxt
    extends: vue
    detect:
      any:
        - package_json_dependency: nuxt
        - path_exists: nuxt.config.ts
        - path_exists: nuxt.config.js
    semantic:
      provider:
        kind: builtin
        binding: ts-js-nuxt

  - id: svelte
    extends: nodejs
    file_patterns:
      - "**/*.svelte"
    detect:
      any:
        - package_json_dependency: svelte
        - path_exists: svelte.config.js
    semantic:
      provider:
        kind: lsp
        transport: stdio
        command:
          - svelteserver
          - --stdio
        root_markers:
          - svelte.config.js
          - package.json
        initialization_options:
          configuration:
            typescript:
              tsdk: node_modules/typescript/lib

  - id: sveltekit
    extends: svelte
    detect:
      any:
        - package_json_dependency: "@sveltejs/kit"
        - path_glob_exists: "src/routes/**"
    semantic:
      provider:
        kind: builtin
        binding: ts-js-sveltekit

  - id: astro
    extends: nodejs
    file_patterns:
      - "**/*.astro"
    detect:
      any:
        - package_json_dependency: astro
        - path_exists: astro.config.mjs
        - path_exists: astro.config.ts
    semantic:
      provider:
        kind: builtin
        binding: ts-js-astro

  - id: solid
    extends: nodejs
    detect:
      any:
        - package_json_dependency: solid-js
    semantic:
      provider:
        kind: builtin
        binding: ts-js-solid

  - id: solidstart
    extends: solid
    detect:
      any:
        - package_json_dependency: "@solidjs/start"
    semantic:
      provider:
        kind: builtin
        binding: ts-js-solidstart

  - id: cpp
    builtin: true
    adapter: cpp

  - id: cpp-native
    extends: cpp
    detect:
      any:
        - path_exists: CMakeLists.txt
        - path_exists: compile_commands.json
    semantic:
      provider:
        kind: builtin
        binding: native-cpp-default

  - id: python
    builtin: true
    adapter: python

  - id: pyproject-python
    extends: python
    detect:
      any:
        - path_exists: pyproject.toml
        - path_exists: requirements.txt
    semantic:
      provider:
        kind: builtin
        binding: python-default

  - id: rust
    builtin: true
    adapter: rust

  - id: cargo-rust
    extends: rust
    detect:
      any:
        - path_exists: Cargo.toml
    semantic:
      provider:
        kind: builtin
        binding: rust-default

  - id: systemverilog
    builtin: true
    adapter: systemverilog
```

동일한 built-in registry에는 `preact`, `remix`, `gatsby`, `angular`, `qwik`, `lit`, `ember`, `alpine` 같은 web framework profile도 같은 패턴으로 포함 SHOULD 한다.

동일한 built-in registry는 mainstream language pack도 SHOULD 제공한다. 이 pack은 `c`, `cpp`, `java`, `kotlin`, `python`, `go`, `rust`, `.NET`, `swift`, `scala`, `haskell`, `ocaml`, `php`, `lua`, `shell`, `systemverilog` 같은 일반 사용 빈도가 높은 언어를 explicit profile id로 바로 참조할 수 있게 해야 한다.

최소 필드 규칙:

- `id`: unique profile id
- `extends`: 부모 profile id, 없으면 root profile
- `adapter`: root profile에서는 REQUIRED, child profile에서는 부모에서 상속 가능
- `file_patterns`: optional
- `detect`: optional but child profile에서는 strongly recommended
- `semantic.provider.kind`: `none | builtin | lsp`
- `semantic.provider.binding` 또는 `semantic.provider.command`: kind에 따라 REQUIRED

YAML parse/validation 실패 시 server는 해당 profile만 무시하고 warning을 보고 SHOULD 한다.  
전체 daemon 초기화를 실패시키는 것은 문법적/보안적으로 치명적인 경우에만 허용한다.

### 37.4 환경 변수

권장:

- `AGENT_EDITOR_CONFIG`
- `AGENT_EDITOR_LANGUAGES_FILE`
- `AGENT_EDITOR_CACHE_DIR`
- `AGENT_EDITOR_RUNTIME_DIR`
- `AGENT_EDITOR_LOG_LEVEL`
- `AGENT_EDITOR_HTTP_TOKEN`

---

## 38. 성능 목표

다음 값은 비규범적 목표치다.

- 기존 daemon 재사용 attach: p50 < 50ms
- `read_spans` hot path: p50 < 30ms
- ignore-aware text search (100k 파일 규모): p50 < 300ms
- symbol find hot path: p50 < 150ms
- local/private rename preview (syntax path): p50 < 1.5s
- semantic rename preview (warm backend): p50 < 4s

핵심은 평균 absolute 수치보다 **cold/hot 차이**와 **daemon 재사용 이점**을 크게 만드는 것이다.

---

## 39. 테스트 및 수용 기준

### 39.1 기능 테스트

- roots 밖 파일 read/search/edit 금지 확인
- symlink escape 차단 확인
- text search ignore 정책 검증
- rename preview → stale → recompute 흐름 검증
- concurrent apply conflict 검증
- backend idle shutdown 검증

### 39.2 회귀 테스트

- TS monorepo rename
- re-export chain references
- partial index 상태에서 degraded output
- 외부 파일 변경 후 plan invalidation
- daemon crash 후 launcher 재연결

### 39.3 스트레스 테스트

- 여러 에이전트 동시 연결
- 대규모 repo initial indexing
- repeated semantic warm/kill cycles
- thousands of reference results with truncation

---

## 40. 구현 우선순위

### Phase 1

- launcher
- singleton daemon
- roots/ACL
- `ae.workspace.status`
- `ae.workspace.read_spans`
- `ae.workspace.search_text`
- `ae.semantic.status`
- `ae.semantic.ensure`
- plan store / apply skeleton
- `ae.edit.discard_plan`

Phase 1 구현 체크포인트:

- Node launcher와 Rust daemon이 실제 stdio ↔ UDS/pipe bridge로 연결된다.
- `WorkspaceRegistry` 는 roots hash 기준으로 shared workspace를 재사용한다.
- text search/read는 ignore-aware traversal과 ACL 검증을 수행한다.
- `apply_plan` 은 외부 생성 plan이 아직 제한적이더라도 CAS/fingerprint/write-lock 절차를 완성해야 한다.
- semantic 기능은 실제 provider 없이도 상태 머신 skeleton을 노출해야 한다.

### Phase 2

- syntax index
- `ae.symbol.find`
- `ae.symbol.definition`
- `ae.symbol.references` fast path
- `ae.workspace.search_structure`

### Phase 3

- semantic supervisor
- `ae.refactor.rename_preview` auto mode
- `ae.diagnostics.read`
- backend idle management

### Phase 4

- direct local HTTP
- richer resources/subscriptions
- optional task-augmented requests
- more languages

---

## 41. 예시 응답

### 41.1 rename preview 성공

```json
{
  "structuredContent": {
    "planId": "plan_01JY6K4V7X1Q",
    "workspaceId": "ws_7f0b",
    "viewId": "view_12c4",
    "workspaceRevision": "rev_142",
    "engine": "semantic",
    "confidence": {
      "score": 0.96,
      "level": "high",
      "reasons": [
        "resolved by semantic provider",
        "all indexed files covered"
      ]
    },
    "summary": {
      "filesTouched": 18,
      "edits": 73,
      "languages": ["node-ts", "react-ts"],
      "blocked": 0,
      "requiresConfirmation": true
    },
    "fileEdits": [
      {
        "uri": "file:///repo/src/user.ts",
        "edits": [
          {
            "range": {
              "start": {"line": 10, "character": 15},
              "end": {"line": 10, "character": 19}
            },
            "newText": "displayName"
          }
        ]
      }
    ],
    "sampleEdits": [
      {
        "uri": "file:///repo/src/user.ts",
        "before": "user.name",
        "after": "user.displayName"
      }
    ],
    "warnings": [],
    "expiresAt": "2026-03-21T10:10:00Z"
  },
  "content": [
    {
      "type": "text",
      "text": "Rename preview ready: 18 files, 73 edits, confidence high (0.96)."
    }
  ],
  "isError": false
}
```

### 41.2 stale plan 실패

```json
{
  "structuredContent": {
    "code": "AE_STALE_PLAN",
    "message": "Workspace changed since preview was created",
    "retryable": true,
    "details": {
      "planId": "plan_01JY6K4V7X1Q",
      "expectedWorkspaceRevision": "rev_142",
      "currentWorkspaceRevision": "rev_143"
    }
  },
  "content": [
    {
      "type": "text",
      "text": "AE_STALE_PLAN: Workspace changed since preview was created. Recompute the plan and try again."
    }
  ],
  "isError": true
}
```

---

## 42. 구현 메모

### 42.1 왜 raw LSP passthrough가 아닌가

에이전트에 raw LSP를 그대로 주면 stateful editor 계약, verbose payload, capability negotiation 부담이 크다.  
본 서버는 LSP를 내부 구현 세부사항으로 숨기고, 작은 semantic actions만 노출한다.

### 42.2 왜 shared daemon인가

- 같은 repo 인덱스를 여러 에이전트가 재계산하지 않게 하기 위해
- backend warm state를 재사용하기 위해
- cold start 비용을 amortize하기 위해
- install/launch UX를 단순화하기 위해

### 42.3 왜 plan-first인가

에이전트의 실수는 보통 한 번의 잘못된 large write로 비가역적 피해를 만든다.  
plan-first는 preview, approval, stale detection, diff resource를 모두 가능하게 만든다.

---

## 43. 미정 항목

다음은 v0.1에서 열어 둘 수 있다.

1. 구조 검색 DSL을 `sg` 로 고정할지 `auto` 추상화로 유지할지
2. TS semantic provider를 tsserver adapter로 할지 LSP adapter로 할지
3. plan diff를 unified diff string으로만 줄지, richer AST-aware diff를 같이 줄지
4. workspace discovery를 repo root 기준으로 강하게 통합할지, subroot 중심으로 느슨하게 갈지
5. direct local HTTP를 v0.1에 넣을지 v0.2로 미룰지

---

## 44. 최종 요약

이 서버는 **“에이전트가 raw text patch 대신 semantic edit plan으로 일하게 만드는 per-user shared editing runtime”** 이다.

핵심 계약은 다음 네 줄로 요약된다.

1. 외부에는 MCP session, 내부에는 shared daemon.
2. 기본은 빠른 text/syntax, 필요할 때만 semantic.
3. 모든 비단순 수정은 preview 후 apply.
4. 모든 결과는 roots ACL, confidence, stale detection을 따른다.

이 네 줄이 흔들리지 않으면 구현 세부는 바뀌어도 제품 방향은 유지된다.

---

## 45. 참고 기준 문서

- MCP Specification 2025-11-25: https://modelcontextprotocol.io/specification/2025-11-25
- MCP Base Overview: https://modelcontextprotocol.io/specification/2025-11-25/basic
- MCP Lifecycle: https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle
- MCP Transports: https://modelcontextprotocol.io/specification/2025-11-25/basic/transports
- MCP Tools: https://modelcontextprotocol.io/specification/2025-11-25/server/tools
- MCP Resources: https://modelcontextprotocol.io/specification/2025-11-25/server/resources
- MCP Roots: https://modelcontextprotocol.io/specification/2025-11-25/client/roots
- MCP Progress: https://modelcontextprotocol.io/specification/2025-11-25/basic/utilities/progress
- MCP Cancellation: https://modelcontextprotocol.io/specification/2025-11-25/basic/utilities/cancellation
- MCP Tasks: https://modelcontextprotocol.io/specification/2025-11-25/basic/utilities/tasks
- Codex CLI docs: https://developers.openai.com/codex/cli
