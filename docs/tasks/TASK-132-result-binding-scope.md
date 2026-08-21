# TASK-132: result 바인딩의 early-return 범위 확정 (Phase 5 3/n)

- **상태**: 완료
- **시작일**: 2026-08-21
- **완료일**: 2026-08-21
- **커밋**: —

## 목적

`result` 블록의 `<-` 바인딩은 블록 IIFE의 early return으로 컴파일되므로
**블록 최상위 문장**만 될 수 있다 — codegen도 최상위 run만 낮춘다. 그런데
최상위 아래에 쓴 바인딩(`if (c) { const y <- g(); }`, 블록 안 함수 본문의
`<-`)은 진단 없이 원문 그대로 방출되어 verify 백스톱("invalid TypeScript
... or an rlc bug")으로 죽었다 — TASK-131이 봉합한 것과 같은 종류의 계약
구멍(설계 §9의 "`result` 바인딩의 early-return 범위"). 이 범위를 파서가
확정하고 rlc가 위치 있는 진단으로 보고한다.

## 범위

- 포함: `results::nested_binds`(후보 블록의 최상위 아래 bind-형 run 검출),
  `parse_result_block`이 attempt와 함께 반환, `Program.result_nested_binds`,
  sema 보고, 새 진단 코드 `result-nested-binding`(projection 차단),
  테스트 4건, 레퍼런스(§8.4 표·규칙, errors.md 행)·docs/ai/rl.md 갱신.
- 제외: 중첩 `<-`의 **지원**(중첩 제어 블록에서의 early return 방출) —
  바인딩 스코프가 어차피 그 블록에 갇혀 가치가 낮고, Rust do-notation
  계열 관례도 최상위 바인딩이다. match 암 본문 안의 bare `<-`(중첩 match
  영역은 스킵되므로 백스톱 유지 — 보수적 선택, 의사결정 3).

## 의사결정

### 결정 1: 진단이지 기능이 아니다

- **상황**: 중첩 `<-`를 지원(그 자리에 early-return 방출)할 수도 있다.
- **선택과 근거**: 진단. `if` 본문 안 바인딩의 스코프는 그 `if` 블록이라
  뒤 문장에서 쓸 수 없고(지원해도 쓸모가 제한적), 함수 안 `<-`는 의미상
  성립하지 않는다(`return`이 함수를 벗어남). "최상위 문장만"이 규칙이고,
  규칙 위반은 위치 있는 에러로 — 백스톱 아니라.

### 결정 2: 검출은 `scan_bind` 재사용 — 새 판별 규칙을 만들지 않는다

- **상황**: 중첩 깊이의 bind-형 run을 무엇으로 판정하나. 통과 계약이
  걸린다: `let x: Foo<-1>;`(제네릭 타입 인자)은 유효한 TS다.
- **선택과 근거**: 선언 키워드에서 그 run의 `;`까지를 잘라 기존
  `scan_bind`로 분류한다 — `<-` 인접성, 초기화 `=` 선행, 짝 없는 `>`
  (제네릭) 배제, run-on 배제가 전부 한 구현이다. 선언 키워드 뒤의 `<-`는
  어느 깊이에서도 TypeScript일 수 없으므로(§8.4) 후보 블록이 Pass여도
  보고가 통과 계약을 깨지 않는다.

### 결정 3: 중첩 `match`/`result` 영역은 스킵한다

- **상황**: 블록 안의 중첩 `result` 블록의 최상위 바인딩을 바깥 스캔이
  "중첩"으로 오인하면 안 된다.
- **선택과 근거**: `skip_braced_construct`로 중첩 `match (…) { … }`·
  `result { … }` 영역을 건너뛴다 — 그 영역은 자기 재귀 파스가 자기
  규칙으로 답한다(안쪽 `result`는 자기 중첩 바인딩을 스스로 검출).
  대가로 match 암 본문 안의 bare `<-` 오용은 이 진단 밖에 남는다(백스톱
  유지) — 오탐 없는 쪽을 택했다.

## 작업 내역

- 2026-08-21: `src/parser/results.rs` — `nested_binds`/`run_end` 추가,
  `parse_result_block`이 `(Attempt, Vec<usize>)` 반환(모든 경로에서 중첩
  목록 동반). `src/parser/mod.rs` — 호출처가 `result_nested_binds`로
  수집, `Program`에 필드 추가(`src/ast.rs`).
- `src/diagnostics.rs` — `ResultNestedBinding`("result-nested-binding"),
  `blocks_projection` 포함(방출물에 `<-`가 남으면 TS가 아니다).
- `src/sema.rs` — 목록 보고(문안: 최상위로 끌어올리거나 `match`).
- 테스트(compile.rs): `if` 본문·블록 안 함수·무판별(Pass) 후보의 세
  에러 경로 + 위치, 중첩 깊이의 `Foo<-1>` 통과, 중첩 `result` 블록의
  자기 답변(IIFE 2개).
- 문서: language.md §8.4(표에 행 추가, try/let-else 규칙 문구를
  TASK-131의 flow 규칙과 정합), errors.md 행 추가, docs/ai/rl.md 1곳.

## 이슈 및 해결

없음.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` (전 스위트 실패 0)

## 결과

`result` 바인딩의 early-return 범위가 규칙으로 확정되고 위반이 위치 있는
rl 진단이 됐다 — verify 백스톱으로 새던 마지막 알려진 `result` 구멍 봉합.
Phase 5 잔여는 분기별 초기화와 flow의 HIR body 연동이 남는다.
