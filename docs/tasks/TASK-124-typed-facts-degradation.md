# TASK-124: typed facts의 경계 확정 — 백엔드 실패는 rl 계층을 못 가린다 (Phase 4)

- **상태**: 완료
- **시작일**: 2026-08-21
- **완료일**: 2026-08-21
- **커밋**: —

## 목적

컴파일러 중심부([TASK-119](./TASK-119-compiler-core-design.md), §7)의
typed 경계 확정: TypeScript 타입 정보는 batch query와 rl-owned 답으로만
들어오고, **backend가 실패해도 rl semantic pass는 중단되지 않는다** — 해당
typed fact를 unknown으로 두고 독립 판정 가능한 rl 진단은 계속 보고한다.
TASK-120이 "rl 에러가 typed 진단을 가리지 않게" 했다면, 이 태스크는 그
역방향이다.

## 범위

- 포함: `Checked::backend_error` 도입, `Project::check`의 실패 강등(ask
  실패 → 빈 `Answers` + 오류 기록), 툴체인 발견 실패의 지연(`Project`가
  `Result<NativeBackend, String>`를 들고 열림 — `open_project`가 더는
  툴체인 부재로 실패하지 않음), CLI(`--check-types`)와 `--server`
  (`backendError`)의 소비, backend seam 문서에 TypeRequestSet/TypedFacts
  대응 명시, CLI 계약 테스트.
- 제외: `Query`/`Answers`의 개명·재구성 — 현 구조가 이미 지시 설계의
  구현이다(아래 결정 1). `ask::Property`/`Display` 등 일반 질문 추가는
  필요가 관측될 때(rust-parity-analysis.md §10.3의 현행 결론 유지).

## 의사결정

### 결정 1: TypeRequestSet/TypedFacts는 개명이 아니라 확인이다

- **상황**: 설계는 `TypeRequestSet`/`TypedFacts` 구조를 요구한다. 기존
  `backend::Query`/`Answers`와의 관계를 정할 것.
- **검토한 대안**: (a) 새 이름의 병렬 구조를 만들어 변환 — 같은 물건 두 벌.
  (b) 기존 구조가 그 설계의 구현임을 확인하고 문서로 못박기: 질문은
  snapshot 단위로 수집되어(`projection::assemble`) 하나의 batch로 가고,
  답은 rl-owned다(constructor domain = `TagMembers`, symbol identity =
  `Resolution.id`, mutation verdict = `builtin` 플래그). chatty per-expr
  oracle이 없다는 금지 조항까지 이미 성립한다.
- **선택과 근거**: (b). 빠져 있던 것은 형태가 아니라 **강등 규칙**(backend
  실패 시 pass 지속)이었고, 그것을 이번에 구현했다. seam 문서에 대응
  관계와 금지 조항을 명시했다.

### 결정 2: 툴체인 부재를 open 실패에서 check의 강등으로 옮긴다

- **상황**: 실측(추가한 CLI 테스트의 첫 실행)에서 백엔드 부재가
  `Engine::open_project`에서 이미 실패해 — `Project::check`의 강등 코드에
  도달조차 못 했다. rl 진단이 통째로 사라지는 것은 여기서도 마찬가지였다.
- **선택과 근거**: `Project`가 `Result<NativeBackend, String>`를 들고
  열리고, check 시점에 Err를 `backend_error`로 흘린다. 종료 코드는 2를
  유지한다 — "검사가 못 돌았다"는 사실은 변하지 않고, 달라진 것은 그
  상태에서도 rl의 답이 완전하다는 것뿐이다.

## 작업 내역

- 2026-08-21: `engine/semantics.rs` — `Checked.backend_error` 추가.
  `engine/project.rs` — backend 필드 타입 변경과 check의 강등.
  `engine/mod.rs` — `open_project`가 툴체인 부재로 실패하지 않음.
- `src/main.rs` — typed 경로: 진단 출력 뒤 backend 오류를 명명하고
  "the TypeScript layer did not run — only rl-level diagnostics are
  shown"과 함께 blocked(종료 2). `--types`의 선언 쓰기는 백엔드 없이는
  하지 않음. `src/server.rs` — `typedCheck` 응답에 `backendError`.
- `src/typescript/backend.rs` — 모듈 문서에 TypeRequestSet/TypedFacts
  대응과 no-chatty-oracle·강등 규칙 명시.
- `tests/cli.rs` — `a_missing_backend_still_reports_rl_diagnostics`:
  존재하지 않는 `RLC_TSGO_API`로 `--check-types` 실행 → 중복 암 진단 출력
  + 강등 문구 + 종료 2.
- `docs/reference/cli.md` — typed 진단 절에 역방향 비차폐 규칙 추가.

## 이슈 및 해결

### 이슈 1: 강등 코드가 실행되지 않음 (테스트 첫 실행 실패)

- **증상**: 테스트가 종료 코드 1을 받음 — "no TypeScript API client at
  ..."가 `open_project`에서 이미 에러.
- **원인**: `NativeBackend::new`가 생성 시점에 `Toolchain::resolve`를
  실행하고 `open_project`가 `?`로 전파.
- **해결**: 결정 2 — 실패를 들고 열리게 변경.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` (전 스위트 — native 31건 포함 — 통과)

## 결과

typed 계층과 rl 계층이 서로를 가리지 못한다: rl 에러는 typed 진단과 함께
나오고(TASK-120), typed 계층의 부재는 rl 진단을 지우지 못한다(이번).
후속: Phase 5 — flow/effect(최소 CFG).
