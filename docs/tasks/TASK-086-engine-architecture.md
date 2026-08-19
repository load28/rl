# TASK-086: Project/Snapshot 기반 Language Engine 아키텍처 재구성

- **상태**: 완료
- **시작일**: 2026-08-19
- **완료일**: 2026-08-19
- **커밋**: 9883527 · 56ea711 · 35599c8 · ecfaef9

## 목적

RL을 CLI/에디터 중심 구조에서 tsgo(typescript-go)와 유사한 지속형
Project/Snapshot 기반 Language Engine으로 재구성한다. rlc CLI, VSCode
LSP 서버, 향후 plugin/외부 API가 모두 하나의 engine을 소비하게 하고,
이원화된 semantic pipeline을 줄인다.

## 범위

- 포함: `rlc::engine`(Engine/Project/Snapshot/ProjectedDocument) 신설,
  typed CLI 경로의 엔진 소비자화(check.rs 해체), projection의 내용-해시
  증분 캐시, `rlc --server`(엔진 세션 서버)와 에디터 라우팅, 죽은 경로
  제거(P4), tsgo 실제 구현 분석 리포트(`docs/design/engine-architecture.md`).
- 제외: 언어 표면 변경 없음. 에디터의 tsgo LSP 직결(TsgoProject) 제거는
  후속 태스크(아래 "남은 부채").

## 절대 조건

현재 RL이 제공하는 모든 기능의 observable behavior를 동일하게 유지한다.

## 의사결정

### 결정 1: tsgo를 실제로 클론·빌드해 reference로 삼는다

- **상황**: 요청이 "tsgo 스타일"의 피상적 해석을 금지하고 실제 최신
  구현 분석을 요구했다.
- **검토한 대안**: 문서/블로그 기반 추정 vs 실제 HEAD 분석.
- **선택과 근거**: `microsoft/typescript-go` HEAD `c6b013f5`(2026-08-19,
  이 저장소 설계 문서들이 참조한 커밋과 동일)를 클론하고 Go 바이너리
  (`built/local/tsgo`, 7.1.0-dev)와 native-preview JS 클라이언트를 빌드해
  `internal/{api,project,lsp,compiler}`·`_packages/native-preview`를
  분석했다. 결과는 `docs/design/engine-architecture.md`의 B절과 D절
  (채택/변형/기각 표). 부수 효과로 **이 환경에서 native 테스트 23개가
  조용히 스킵되고 있었음을 발견**하고, 빌드된 체크아웃으로 전부 실행하는
  베이스라인을 확보했다.

### 결정 2: Session(가변)/Snapshot(불변) 분리를 채택하되 캐시 단위는 projection

- **상황**: tsgo의 ParseCache는 내용 해시 키의 AST 캐시다. RL의 비용
  구조는 다르다 — 조사 결과 typed 패스는 **파일당 5회 재파싱**(lower 1 +
  imports_std/literal/tag/val 각 1)을 매 패스 반복하고 있었다.
- **검토한 대안**: ① AST 캐시(파서 API 개편 필요) ② projection 캐시
  (원문·방출 TS·매핑·probe를 통째로 내용 비교로 재사용) ③ 캐시 없음(현행).
- **선택과 근거**: ②. `ProjectedDocument`가 한 파일의 파생물 전부를 한 번
  계산해 들고, `Project::update`가 텍스트 동일성으로 재사용한다. 파서
  API를 건드리지 않고 5회 파싱을 "내용 변경당 1회 × 5"로 줄이며, tsgo가
  ParseCache로 얻는 것(불변 스냅샷 간 구조 공유)과 같은 효과를 RL의 비용
  중심에서 얻는다. 정확성: 비교가 전체 텍스트 equality이므로 캐시 오염이
  구조적으로 불가능(요청 §30 "correctness 우선").

### 결정 3: 배치 빌드는 엔진 밖에 남긴다

- **상황**: 요청 §22는 CLI 전체의 엔진 소비자화를 제시했다.
- **검토한 대안**: untyped 빌드까지 엔진 경유 vs typed 경로만.
- **선택과 근거**: tsgo 자신이 배치 `tsc`(`internal/execute`)를 project
  시스템 **밖**에 둔다 — 상태가 필요 없는 1회 실행에 세션 기구를 태우지
  않는 분리다. rlc의 배치 빌드/`--check`/`--symbols`/`--emit-map`은 같은
  지위로 남기고, 지속 상태가 실존하는 typed 경로(`--check-types`/`--types`
  /watch/`--server`)만 엔진이 소유한다. 공용 조각(walk, `TS_EXTENSIONS`)은
  엔진으로 단일화했다.

### 결정 4: backend seam(Query/Answers)과 host 프로토콜은 불변

- **상황**: 요청 §16·17은 필요하면 seam과 host 재작성을 허용했다.
- **검토한 대안**: tsgo처럼 msgpack/persistent-project 프로토콜로 재작성
  vs 유지.
- **선택과 근거**: 유지. TASK-083~085로 이 경계는 이미 batch 강제 + 증분
  snapshot(`fileChanges`) + symbol identity로 정리되어 있고, 측정된 병목
  (IPC 횟수)은 해소된 상태다. 전송 형식 교체는 측정된 필요 없이 하지
  않는다 — 교체 가능성 자체가 `native.rs` 격리의 존재 이유다. 대신 그
  위의 **Rust 쪽 상태 모델**(매 패스 전량 재계산)이 실제 문제였고 이번
  작업이 그것을 고쳤다.

### 결정 5: 에디터 마이그레이션은 컴파일러 경로부터, `rlc --server`로

- **상황**: 요청 §18·19의 최종 목표는 TsgoProject(`tsgo --lsp` 직결)와
  에디터 가상문서 저장소의 엔진 흡수다. 그러나 hover/completion/rename 등
  7개 기능은 에디터 테스트 76개가 잠근 표면으로, 한 번에 옮기면 §37의
  "단계마다 parity 증명"이 불가능하다.
- **검토한 대안**: ① LSP 기능 전부 포함 일괄 전환 ② 컴파일러 호출
  경로(check/emitMap/typedCheck)만 지속 서버로 전환하고 LSP 기능은 후속
  ③ 아무것도 안 함.
- **선택과 근거**: ②. `rlc --server`는 one-shot과 **동형의 답**을 주는
  JSON-lines 서버로, 에디터 `rlc.ts`가 폴백(서버 없으면 one-shot) 포함으로
  경유한다. 관측 결과 불변을 폴백 구조로 보증하면서, 가장 비싼 경로
  (typedCheck: 1.2초 디바운스마다 프로세스+컴파일러+프로젝트 열기)를 세션
  재사용으로 바꾼다. 측정: one-shot 665–776ms/회 → 서버 첫 요청 753ms,
  이후 **3–5ms** (소형 프로젝트, tsgo 빌드 체크아웃). ①은 후속 태스크의
  경로로 문서화했다(engine-architecture.md E.4).

### 결정 6: typedCheck의 overlay는 요청 스코프

- **상황**: 서버가 문서를 지속 보유하면(didOpen/didChange 모델) 에디터가
  close를 전달하지 않는 현 구조에서 닫힌 버퍼의 텍스트가 다른 파일 검사를
  오염시킨다(one-shot 대비 회귀).
- **선택과 근거**: 요청마다 `open_document → update/check →
  close_document`. 의미는 one-shot과 동일(무상태)하고, 성능은 projection
  캐시(텍스트 equality)와 컴파일러 세션이 요청 사이에 살아 있어 그대로
  얻는다. 에디터가 didOpen/didClose를 서버에 전달하게 되는 후속
  마이그레이션에서 지속 문서로 승격한다.

### 결정 7: P4(죽은 경로) 제거 단행

- **상황**: `Sink::Calls`/`val::method_calls`/`ValMethodCall`/
  `rlc::val_method_calls`는 TASK-083~085 이후 소비자가 자체 테스트뿐인
  공개 API로, ts7-semantic-unification.md가 "별도 결정"으로 보류해 뒀다.
- **선택과 근거**: 이번 재설계의 명시 목표가 중복 파이프라인 제거이므로
  제거했다. `val.rs`의 walk가 세 모드에서 두 모드로 줄었다. 공개 API
  제거지만 릴리스 전(0.x) 저장소이고 저장소 내 소비자가 없음을 확인했다.

## 작업 내역

- 2026-08-19: 조사 — 코어(전 모듈)·VSCode 서버(전 모듈)·tsgo HEAD 분석.
  tsgo 클론/빌드(`/workspace/microsoft/typescript-go`, Go 1.26 toolchain
  자동 다운로드 + `npx tsc -b _packages/native-preview`). 베이스라인:
  `RLC_TSGO_ROOT` 설정으로 Rust 전 스위트(통합 73·native 23 포함) +
  에디터 76/76 그린 확인.
- 2026-08-19: `docs/design/engine-architecture.md` 작성 (A 현재 흐름 /
  B tsgo 분석 / C 비교 / D 채택·변형·기각 표 / E 최종 아키텍처).
- 2026-08-19: 엔진 코어. `src/typescript/{backend,native,mapper,host.mjs}`를
  라이브러리로 이동(`rlc::` → `crate::`), `check.rs`·`project.rs` 해체.
  신설: `src/engine/{mod,project,snapshot,projection,semantics}.rs` —
  `Engine::open_project` → `Project`(overlay 문서·projection 캐시·
  `NativeBackend` 세션) → `Project::update`(불변 `Snapshot`, 텍스트
  동일성 재사용) → `Project::check`(RL-owned `Diagnostic`/`Checked`,
  문안·순서는 check.rs의 것을 그대로 이관). `main.rs`는
  `typed_check_mode`/`typed_pass`/`typed_watch`/`write_declarations`로
  출력·종료 코드만 소유. `collect_sources` 중복 제거(엔진으로 단일화).
- 2026-08-19: P4 제거 (`val.rs`/`lib.rs`/`tests/compile.rs`).
- 2026-08-19: `rlc --server`(`src/server.rs`) — check/emitMap/typedCheck를
  JSON 라인으로, 프로젝트 identity(`(tsconfig, root)`)당 `Project` 하나
  유지. `editors/vscode/server/src/rlc.ts`에 engine-server 클라이언트
  (지속 프로세스, 요청 타임아웃, 2-스트라이크 후 비활성, one-shot 폴백)
  를 추가하고 `runCheck`/`runEmitMap`/`runTypedCheck`를 경유시킴.
  outcome 매핑은 one-shot의 stderr 파싱 결과와 동치가 되도록 명시적으로
  재현(다른 파일에서만 보고된 실패 = unavailable 등).
- 2026-08-19: 문서 — `cli.md`(+`--server` 절), `CLAUDE.md`(아키텍처 맵 +
  typed 경로 규범), `compiler-architecture.md`(엔진 절),
  `typescript/mod.rs` 모듈 문서.
- 검증 명령: `cargo fmt --check` / `cargo clippy --all-targets -- -D
  warnings` / `RLC_TSGO_ROOT=… cargo test`(447 passed, 0 failed) /
  `editors/vscode`: `npx tsc -b && PATH=…debug RLC_TSGO_ROOT=… npm test`
  (76/76). 서버 스모크: check(소진성 에러 문안 동일)·emitMap·typedCheck
  (val 진단 문안 동일, 재검사 3–5ms) 수동 확인.

## 이슈 및 해결

### 이슈 1: native 테스트 23개가 환경에서 조용히 스킵

- **증상**: `cargo test`가 그린이지만 native 스위트가 0.00s — 툴체인
  가드(`RLC_TSGO_ROOT`/`../typescript-go`)가 없어 전부 skip.
- **원인**: CI/로컬에만 있던 빌드된 typescript-go가 이 환경에 없었음.
- **해결**: tsgo HEAD를 클론·빌드하고 `RLC_TSGO_ROOT`로 전 스위트를
  실행하는 것을 이 작업의 회귀 게이트로 삼았다.

### 이슈 2: 에디터 테스트 21개 cancelled (엔진 서버 도입 직후)

- **증상**: `npm test`가 exit 0인데 `# pass 55 / # cancelled 21`.
- **원인**: 요청 타임아웃 타이머까지 `unref` 해서, in-flight 요청 중
  이벤트 루프가 비어 node가 조기 정상 종료.
- **해결**: 타이머는 ref 유지(요청 진행 중에만 루프를 잡음), 자식
  프로세스·파이프만 unref(유휴 서버가 프로세스를 붙들지 않음). 76/76
  복구. 주석으로 이유를 남김.

### 이슈 3: `Project::update`와 `initial_files`의 borrow/패리티 충돌

- **증상**: `typed_pass(&mut project, &project.initial_files(), …)` E0502;
  또한 초기 `rl_sources()`에 폴백을 접었더니 watch의 "빈 스캔" 동작이
  현행(빈 목록으로 패스)과 달라질 수 있었다.
- **해결**: 초기 파일 목록은 open 시점에 확정해 저장(`initial_files()`는
  조회만), watch는 `scan()`(원시 결과)을 쓰도록 분리 — check.rs의 실행
  순서·오류 우선순위를 그대로 보존.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` — 447 passed / 0 failed (`RLC_TSGO_ROOT`로 native·
      typed 경로 포함 전부 실행)
- [x] 에디터: `npm test` 76/76 (rlc + tsgo 툴체인 포함)

## 결과

변경 요약: `src/engine/`(신설 5모듈) · `src/server.rs`(신설) ·
`src/typescript/`(라이브러리로 이동, check/project 해체) · `main.rs`
(typed 드라이버로 축소, collect_sources 이동) · `val.rs`/`lib.rs`(P4
제거) · `editors/vscode/server/src/rlc.ts`(엔진 서버 클라이언트) ·
문서(`engine-architecture.md` 신설, cli/CLAUDE/compiler-architecture 갱신).

observable behavior 불변(전 스위트 그린). typed 재검사: Rust 쪽도
증분화(projection 캐시), 에디터 typedCheck ~700ms → 3–5ms(세션 재사용).

**남은 부채 (후속 태스크 후보)**:
1. 에디터 LSP 기능(hover/definition/references/completion/rename/
   signatureHelp/TS 진단)의 엔진 이관과 `TsgoProject`(`tsgo --lsp` 직결)
   제거 — 경로는 engine-architecture.md E.4에 기록.
2. virtualDocs/diskVirtuals/analysis.ts/probe.ts의 엔진 흡수 (1에 종속).
3. `--server`의 문서 lifecycle 승격(didOpen/didChange/didClose)과
   emit-map/symbols의 스냅샷 기반 제공.
4. 외부 JS/TS API(요청 §26)와 서버 바이너리 안정화 (1·3 이후).
5. tsgo `runWithTemporaryFileUpdate` 채택 검토(completion probe의 임시
   스냅샷화 — 1에 종속).
6. Project의 동시성(현재 단일 스레드, 요청 §44의 "첫 구현은 correctness
   우선"에 따름) — 스냅샷은 이미 불변이므로 `Arc` 공유로 확장 가능.
