# TASK-120: 구조화 다중 진단 — Phase 0 (TASK-117 흡수)

- **상태**: 완료
- **시작일**: 2026-08-21
- **완료일**: 2026-08-21
- **커밋**: —

## 목적

컴파일러 중심부 전환([TASK-119](./TASK-119-compiler-core-design.md),
`docs/design/compiler-core.md` §8)의 선행 조건: `compile()`/`sema::check()`가
첫 `RlError`에서 종료되는 구조를 없애고, 파일의 **모든** rl 진단을 안정된
코드와 함께 소스 순서로 보고한다. [TASK-117](./TASK-117-multiple-rl-diagnostics.md)의
세 증상(두더지 잡기, 경로별 문안 비대칭, rl 에러 하나가 typed 진단 전체를
가리는 버그)을 전부 해소한다.

## 범위

- 포함: `DiagnosticCode`/`Severity`/`Diagnostic` 도입, sema·val의 누적 보고,
  `analyze()`/`compile_report()` 공개 API, projection의 `Blocked` 완화(복구
  가능 rl 에러는 typed 진단과 병행), typed/untyped 소진성 문안 렌더러 통일,
  CLI·`--server`의 다중 진단 출력과 `code` 필드, 문서 갱신.
- 제외: 파서의 무오류 계약 변경(파서는 여전히 에러를 내지 않는다), Warning
  severity의 실제 생산(변형만 예약), 에디터 확장 코드 변경(프로토콜이 추가
  필드만 갖므로 호환).

## 의사결정

### 결정 1: 보고 순서는 소스 순서, `compile()`은 그 첫 항목

- **상황**: TASK-117 쟁점 3 — 기존 순서(카테고리 순: stray → 구문 검사 →
  해석 → 소진성)를 유지할지, 소스 순서로 바꿀지. `compile()`의 첫 에러가
  달라지면 기존 테스트가 흔들린다.
- **검토한 대안**: (a) 카테고리 순 유지 — 기존 첫-에러 동작 보존, 대신 다중
  보고가 파일을 위에서 아래로 읽히지 않음. (b) 소스 순서 — tsc·rustc와 같고
  사용자가 파일을 한 번에 고칠 수 있음; 다중 에러 파일의 첫 에러가 바뀔 수
  있음.
- **선택과 근거**: (b). `cargo test` 실측 결과 흔들린 단언은 mixed-pattern
  테스트 2건뿐이었고, 그 원인은 순서가 아니라 **인과**였다(결정 2). 나머지
  246개 테스트는 그대로 통과 — 기존 테스트는 대부분 에러가 하나인 파일이라
  순서 변화의 영향이 없다.

### 결정 2: 에러 복구 경계는 match 단위, 원인 위에 결과를 쌓지 않는다

- **상황**: TASK-117 쟁점 1 — 어디서 멈추는가. 파일 단위로 자연히 성립하던
  "해석 실패 시 소진성 억제"를 다중 보고에서 다시 표현해야 했다.
- **검토한 대안**: (a) 억제 없이 전부 보고 — 오타 하나가 가짜 소진성 에러를
  달고 나옴. (b) 파일 단위 억제 유지 — match B의 답이 match A의 오타에
  가려짐. (c) match 단위 억제.
- **선택과 근거**: (c). `MatchAnalysis::has_unresolved`(분석이 그 match의
  해석 중 미해결을 기록)와 sema의 `coverage_suppressed`(태그·리터럴 혼합
  match)로 구현. 혼합 match의 소진성도 같은 원리로 억제한다 — 혼합 match는
  판별자가 하나가 아니므로 coverage 답 자체가 성립하지 않는다(실측: 소스
  순서 도입 시 mixed 테스트 2건이 이 억제 없이는 소진성 에러를 먼저 냈다).

### 결정 3: `Blocked`는 "산출물이 TypeScript일 수 없는" 진단으로만 좁힌다

- **상황**: TASK-117 쟁점 4 — 증상 3(rl 에러 하나가 typed 진단 전체를 가림)의
  근본. 어떤 rl 에러가 "방출해도 되는" 것인지 분류가 필요했다.
- **검토한 대안**: (a) 현행 유지(모든 rl 에러가 projection을 막음). (b) 전부
  방출 허용 — stray `|>`가 verbatim으로 나가 방출물이 TS가 아니게 됨. (c)
  코드 단위 분류: `DiagnosticCode::blocks_projection()`.
- **선택과 근거**: (c). 분류 기준은 "codegen이 이미 감당하는 입력인가"다 —
  에디터 경로의 `emit_mapped`는 무검사 방출이므로, 파서가 구조로 청구한
  구문은 어떤 rl 에러가 있어도 방출 가능함이 이미 증명되어 있다. 막는 것은
  청구되지 못해 verbatim으로 새는 텍스트(stray 계열), 타입 위치로 그대로
  방출되는 깨진 필드 타입, 출력 자가 검사 실패뿐. `try` 위치 제약 등은
  방출물이 구문상 유효하므로(의미만 다름) typed 검사를 막지 않는다 — 배치
  빌드는 에러가 있으면 어차피 산출물을 쓰지 않는다.

### 결정 4: 문안 통일은 렌더러 공유로, 데이터 차이는 인정한다

- **상황**: TASK-117 증상 2 — untyped `match on enum Shape is not
  exhaustive` vs typed `match is not exhaustive`.
- **검토한 대안**: (a) typed 경로도 enum 이름을 추정해 대기 — 체커의
  알파벳과 선언 표의 대응이 유일하지 않으면 오답 위험(TASK-118의 신중함과
  상충). (b) 렌더러 하나(`diagnostics::non_exhaustive_message`)를 공유하고,
  출처(subject)는 아는 쪽만 채운다.
- **선택과 근거**: (b). 형태·절단 규칙·코드가 하나가 되고, 남는 차이는
  "출처를 아는가"라는 데이터 차이뿐이다. 부수 효과: 5개 이상 절단 규칙이
  단일 match에도 적용된다(기존엔 튜플·typed만).

### 결정 5: `RlError`에 코드를 빌더로 부착, 기본값은 `Other`

- **상황**: 31개 생성 사이트의 시그니처를 전부 바꿀지.
- **선택과 근거**: `RlError::code(DiagnosticCode)` 빌더 추가. 모든 실제
  사이트에 코드를 부여했고, `Other`는 빠뜨린 사이트의 안전망으로만 존재한다
  (컴파일이 막히지 않아 마이그레이션이 안전).

## 작업 내역

- 2026-08-21: `src/diagnostics.rs` 신설 — `Severity`/`DiagnosticCode`(26개
  규칙 코드, `as_str`, `blocks_projection`)/`Diagnostic`(byte span,
  `to_compile_error`)/`non_exhaustive_message` 공유 렌더러 + 단위 테스트.
- `src/error.rs`: `RlError.code` 필드와 빌더. `src/verify.rs`:
  `at_source`가 `VerifyFailed` 코드를 단다.
- `src/sema.rs`: `check` → `check_all(Vec<RlError>)` 전면 개조 — Checker가
  누적하고 계속 방문, stray 목록 전부 보고, `report_resolution`/
  `report_coverage`가 전 항목 push, match 단위 억제(결정 2), 마지막에 소스
  순서 정렬.
- `src/analysis/mod.rs`: `MatchAnalysis::has_unresolved` 추가(단일·튜플
  match 분석에서 해석 전후 미해결 개수 비교로 설정).
- `src/val.rs`: `check` → `check_all` — `Sink::Report`가 수집기를 들고 walk
  전체가 무오류 진행(`Result` 제거), `ValMutation`/`ValPass` 코드 부여.
- `src/lib.rs`: `analyze()`/`CompileReport`/`compile_report()` 공개.
  `compile_mapped`는 `rl_errors`의 첫 항목을 돌려주는 wrapper로 재구성(에러
  시 방출 생략 — 기존과 동일 비용).
- `src/engine/projection.rs`: `ProjectedDocument.rl_diagnostics` 추가,
  `project()`가 `compile_report` 사용 — emit이 없을 때만 `Blocked`.
- `src/engine/semantics.rs`: `Diagnostic.code` 필드(rl 코드 또는 `tsNNNN`),
  `report()`가 파일별 rl 진단을 먼저 합류, typed 소진성(리터럴·태그)이 공유
  렌더러 사용.
- `src/main.rs`: 배치/`--check` 경로가 `compile_report`로 파일의 모든
  에러를 출력. `src/server.rs`: `check`가 전체 진단 + `code`,
  `typedCheck` 응답에도 `code`.
- 테스트: `tests/compile.rs`에 10건(다중 소진성·중복 암 비차폐·match 단위
  억제·sema/val 병합 순서·`compile()` 계약·recoverable 방출·stray 차단·전
  stray 보고·mixed 억제·코드 안정성), `src/engine/projection.rs`에 3건
  (recoverable projection 성공·해석 진단 동승·stray 차단).
- 문서: `errors.md` "다중 보고" 절 신설 + 소진성 문안 서술 갱신, `cli.md`
  (`--check` 행, typed 진단 절, `--server` 프로토콜 `code`), `docs/ai/rl.md`
  1행 요약 갱신, `lib.rs` crate 문서.

## 이슈 및 해결

### 이슈 1: 소스 순서 도입으로 mixed-pattern 테스트 2건 실패

- **증상**: `literal_and_tag_patterns_cannot_be_mixed`(2건)가 "match on
  built-in enum Option is not exhaustive"를 첫 에러로 받음.
- **원인**: 혼합 match에서 소진성 에러의 위치(`match` 키워드)가 혼합
  에러의 위치(두 번째 종류의 첫 arm)보다 앞서서, 소스 순서 정렬이 결과를
  원인보다 먼저 내놓았다.
- **해결**: 혼합 match의 coverage를 억제(결정 2의 `coverage_suppressed`).
  진단 하나를 지운 게 아니라 인과를 복원한 것 — 혼합 match는 판별자가
  하나가 아니므로 coverage 질문 자체가 성립하지 않는다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` (전 스위트 + doctests 통과; 통합 테스트 포함)

## 결과

한 파일의 rl 진단이 전부, 소스 순서로, 안정 코드와 함께 보고된다. 복구
가능한 rl 에러는 typed 경로의 진단(타입 에러·소진성·`val`)과 함께 나온다 —
TASK-117의 증상 1·2·3 해소. `compile()` 공개 계약은 유지. 후속: Phase 1
(HIR 기반, TASK-121).
