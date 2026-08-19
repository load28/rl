# TASK-083: host 질의 batch화와 mutator 정책의 판정 시점 이동 — 동작 불변

- **상태**: 완료
- **시작일**: 2026-08-19
- **완료일**: 2026-08-19
- **커밋**: (커밋 후 기입)

## 목적

TASK-082 계획의 P1·P2를 구현한다: host↔tsgo IPC를 checker의 batch endpoint로
줄이고, built-in mutator 이름 목록을 수집 조건에서 판정 시점의 정책으로
옮긴다. **관찰 가능한 동작(방출 TypeScript, 진단 메시지·위치·유무)은 바이트
단위로 불변**이라는 제약 아래 진행한다.

## 범위

- 포함: P1(host.mjs batch화 + builtin 판정을 metadata 질의로), P2(수집
  무필터화 + verdict에서 `builtin && 정책` 적용 + 답이 정해진 질문의 생략),
  회귀·동등성 테스트, 레퍼런스 §10.4 문구 정합화.
- 제외: P3(callee symbol pairing)과 P4(`val_method_calls` 제거) — 아래
  결정 1·2로 보류. mutator 정책 목록 보강(`Date#setHours` 등)도 제외(언어
  표면 변경, 별도 태스크).

## 의사결정

### 결정 1: P3(callee symbol pairing)은 보류한다

- **상황**: 계획의 P3은 call-capability 검사의 callee 이름 매칭을 symbol
  identity로 바꾸는 것이었으나, 이번 작업의 제약이 "동작 불변"으로
  주어졌다.
- **검토한 대안**: ① 그대로 구현, ② 보류.
- **선택과 근거**: ②. `language.md` §10.5는 "같은 이름이 서로 다른
  시그니처로 두 번 선언되면 그 이름은 검사에서 제외"를 규범으로 명시한다.
  symbol pairing은 정확히 그 경우들(섀도잉·재선언)의 판정 결과를 바꾸므로
  — 그것이 목적이므로 — 동작 불변 제약과 양립할 수 없다. 규범 변경을
  동반하는 별도 태스크로 진행해야 한다.

### 결정 2: P4(`val_method_calls`/`Sink::Calls` 제거)도 보류한다

- **상황**: 저장소 안 사용처는 자체 테스트뿐인 legacy 경로다.
- **선택과 근거**: 공개 API 제거는 라이브러리 소비자 관점의 동작 변경이라
  이번 제약 밖이다. 이름 필터는 이 legacy 경로에만 남기고 문서로 표시했다.

### 결정 3: mutator 정책은 verdict에 두되, 질의는 정책으로 미리 거른다

- **상황**: 수집을 무필터로 넓히자(`val.rs`) 정책상 절대 보고될 수 없는
  호출(`items.at(0)` 등)까지 SymbolQuery가 만들어져, 메서드 호출이 많은
  파일에서 검사 시간이 오히려 늘었다(아래 이슈 1).
- **검토한 대안**: ① 수집 단계에서 다시 이름으로 거르기(원상복귀),
  ② 수집은 완전하게 두고 **query 조립**(`project.rs`)에서 "정책을 통과할
  수 없는 메서드 호출은 질문하지 않는다"로 거르기.
- **선택과 근거**: ②. verdict는 `builtin && is_builtin_mutator_name`이므로
  정책 밖 이름의 답은 묻기 전에 정해져 있다 — 생략은 순수 최적화이고
  correctness에 기여하지 않는다. 계획서의 목표 형태("prefilter는
  optimization일 뿐 correctness requirement가 아니게") 그대로다: 이름
  누락은 이제 어떤 경우에도 오탐을 만들 수 없고, 정책 변경은 `val.rs`의
  정책 함수 한 곳만 고치면 된다.

### 결정 4: batch 미지원 클라이언트는 세션당 1회 감지해 폴백한다

- **상황**: batch overload는 HEAD와 최근 native-preview에는 있지만,
  클라이언트·서버 조합에 따라 없을 수 있다(프로토콜에 버전 협상이 없다).
- **검토한 대안**: ① batch 전제(구버전은 세션 실패), ② `batched()` helper로
  첫 사용 시 감지, 실패하면 그 세션 동안 per-position 호출로 폴백.
- **선택과 근거**: ②. 두 경로는 같은 답을 다른 왕복 수로 낼 뿐이므로
  toolchain이 무엇이든 verdict가 달라지지 않는다. 검증은 양방향으로 했다
  (아래 작업 내역).

### 결정 5: builtin 판정은 source file 전송 대신 metadata 질의로

- **상황**: 기존 host는 선언 파일이 default lib인지 알기 위해
  `program.getSourceFile()`로 lib 파일 **전체 AST**를 바이너리 전송받았다
  (세션당 파일 1회, 캐시됨).
- **선택과 근거**: `program.getSourceFileMetadata(path)`가 같은 사실을 작은
  질의로 답하고 Program에 캐시된다. API가 없는 클라이언트는 기존 경로로
  폴백(`typeof` 검사). 판정 결과는 동일하다.

## 작업 내역

- 2026-08-19: `src/typescript/host.mjs` — 질문을 모듈별로 모아
  `getTypeAtPosition(file, positions[])`/`getSymbolAtPosition(file,
  positions[])` batch overload로 질의하고 원래 index로 흩뿌리는
  `perModule`/`batched` 추가. tag check의 `getTypeOfSymbol`은 전 check의
  kind 심볼을 모아 1회 batch. builtin 판정을 `getSourceFileMetadata`로
  (폴백 포함). 프로토콜·응답 형태 불변.
- 2026-08-19: `src/val.rs` — probes 수집에서 `is_builtin_mutator_name`
  게이트 제거(모든 메서드 호출 수집), 함수는 `pub`으로 승격하고 문서를
  "판정 시점 정책"으로 재서술. legacy `Sink::Calls`만 필터 유지.
  `src/lib.rs` — 재수출, `val_probes` 문서에 수집 계약 doctest 추가.
- 2026-08-19: `src/typescript/check.rs` — verdict를
  `resolution.builtin && rlc::is_builtin_mutator_name(&resolution.name)`으로.
  `src/typescript/project.rs` — 정책을 통과할 수 없는 메서드 호출은
  질문을 만들지 않는 생략 추가(결정 3).
- 2026-08-19: `docs/reference/language.md` §10.4 — "이름은 질문을 고르는
  필터"를 "표는 rl의 정책, 심볼 소속은 컴파일러의 답, 표 누락은 미탐만"
  으로 정합화(동작 서술은 불변). `docs/ai/rl.md`는 동작 서술만 담고 있어
  갱신 불필요를 확인.
- 2026-08-19: 테스트 — `tests/compile.rs`
  `val_probes_collect_every_method_call_for_the_verdict`,
  `tests/native.rs` `a_non_mutating_builtin_method_is_not_a_mutation`
  (정책 밖 built-in 무보고), `batched_answers_land_on_their_own_questions`
  (2개 모듈 × literal/val 질문의 흩뿌리기 순서).
- 2026-08-19: 실 toolchain 검증 — `@typescript/native-preview`
  7.0.0-dev.20260707.2 설치(`RLC_TSGO_API`), native 테스트 21개 전부 실행.
  batch 경로가 실제로 타는 것을 폴백을 예외로 바꿔 증명(전부 통과 = 폴백
  미사용), batch를 강제로 꺼서 폴백 경로의 동등성도 확인(전부 통과).
- 2026-08-19: 동작 불변 확인 — 변경 전(main)·후 바이너리로 4개 합성
  프로젝트의 `--check-types` stderr를 diff: 진단 0·120·600건 케이스 모두
  **바이트 동일**. 방출 TypeScript는 codegen 무변경 + 스냅샷/패스스루
  테스트로 확인.
- 2026-08-19: 측정 — 3회 평균, 같은 머신, cold start 포함:
  | 프로젝트 | 질문 규모 | 변경 전 | 변경 후 |
  |---|---|---|---|
  | literal 120 + at() 120 | 모듈 2 | 393 ms | 364 ms |
  | literal 120 | 모듈 2 | 383 ms | 367 ms |
  | val push 120 | 모듈 2 | 334 ms | 317 ms |
  | literal 600 + push 600 | 모듈 4 | 3990 ms | 3564 ms (−11%) |
  cold start 고정비가 지배적이라 세션을 유지하는 `--watch`/에디터 경로에서
  상대 효과가 더 크다. 남은 왕복: union constituent 전개(`getTypesOfType`)
  와 `getPropertyOfType`은 batch endpoint가 없어 check당 유지.

## 이슈 및 해결

### 이슈 1: 무필터 수집이 정책 밖 호출까지 질의해 검사가 느려짐

- **증상**: `items.at(i)` 120개짜리 벤치에서 변경 후가 371→534 ms로 역행.
- **원인**: 수집 확대로 절대 보고될 수 없는 메서드 호출에도 root+method
  SymbolQuery가 2개씩 생성됨. batch라 IPC는 적지만 서버의 심볼 해석과
  응답 직렬화 비용이 비례.
- **해결**: 결정 3 — query 조립에서 정책 밖 메서드 호출을 생략. 이후 전
  벤치에서 변경 후 ≤ 변경 전.

### 이슈 2: rustfmt가 수기 정렬을 되돌림

- **증상**: `cargo fmt --check` 실패(import 정렬, match arm 블록 형태 등).
- **해결**: `cargo fmt` 적용 후 게이트 재실행. 없음 수준의 이슈지만 기록.

## 검증 게이트

- `cargo fmt --check` — 통과
- `cargo clippy --all-targets -- -D warnings` — 통과
- `cargo test` — 전 스위트 통과 (native 21개 포함, `RLC_TSGO_API`로 실
  toolchain 구동; tsc/node 통합 테스트 포함)

## 변경 파일

- `src/typescript/host.mjs` — batch 질의·metadata builtin 판정·폴백
- `src/typescript/check.rs` — verdict에 mutator 정책 적용
- `src/typescript/project.rs` — 답이 정해진 질문 생략
- `src/val.rs`, `src/lib.rs` — 수집 무필터화, 정책 함수 공개·문서
- `docs/reference/language.md` — §10.4 문구 정합화
- `tests/compile.rs`, `tests/native.rs` — 계약·회귀 테스트 3개
- `docs/design/ts7-semantic-unification.md` — 구현 상태 표기
