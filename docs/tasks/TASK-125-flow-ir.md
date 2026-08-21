# TASK-125: flow IR — let-else 발산을 제어 흐름으로 (Phase 5 1/n)

- **상태**: 완료
- **시작일**: 2026-08-21
- **완료일**: 2026-08-21
- **커밋**: —

## 목적

컴파일러 중심부([TASK-119](./TASK-119-compiler-core-design.md), §9)의 flow
계층 착수: rl 고유 제어 흐름 질문에 답하는 최소 CFG(`FlowBody`/
`BasicBlock`/`Terminator`)를 세우고, 첫 소비자로 let-else의 "else는 반드시
발산" 규칙을 옮긴다. 기존 판정은 마지막 문장의 첫 키워드를 보는 구문
휴리스틱이라 실제로 발산하는 블록(`if`/`else` 양쪽 return, 발산하는 중첩
블록, 발산 뒤 도달 불가 코드)을 거부했다 — rustc가 받아들이는 코드를 rl이
거부하던 정확성 격차(D7)의 해소다.

## 범위

- 포함: `src/flow/mod.rs` — CFG 타입과 `diverges`(entry에서 `End` 도달
  불가 판정), 문장 수준 lowering(문장 분할·`if`/`else` 체인·bare block·
  4개 발산 키워드), 파서의 `block_diverges`/`brace_opens_statement`류의
  flow 이동과 lets.rs의 위임, sema 문안 갱신, 언어 문서
  (language.md §6.4, errors.md, docs/ai/rl.md) 갱신, 테스트.
- 제외: 루프·`switch`·`try`의 모델링(보수적 fall-through — 오탐으로
  통과시키는 일은 없다), HIR body 위의 flow(조건 `ExprId` 연동 —
  `Terminator` 문서에 예약 명시), `try` 배치·`result` early-return·분기별
  초기화의 flow 이동(후속).

## 의사결정

### 결정 1: 첫 소비자는 토큰 스트림, HIR가 아니다

- **상황**: 지시 설계의 `Terminator::Branch { condition: ExprId }`는 HIR
  연동을 전제하지만, let-else else 블록의 내용물은 대부분 통과 TypeScript
  라 HIR에선 opaque다.
- **선택과 근거**: 판정에 필요한 것은 문장 경계와 제어 키워드뿐이므로
  lowering을 토큰 수준에 두고(재파싱 아님 — 이미 있는 lexer 산출물),
  ExprId 연동은 body가 HIR로 옮겨질 때로 예약한다(`Terminator` 문서에
  명시). CFG 구조 자체(`FlowBody`/`diverges`)는 설계 형태 그대로다.

### 결정 2: 모델 밖 구조는 전부 보수적 fall-through

- **상황**: 어디까지 이해할 것인가.
- **선택과 근거**: 이해하지 못하는 구조가 만들 수 있는 답은 "비발산"
  뿐이게 한다(false diverges 불가). 루프는 0회 실행 가능, `try`는 예외
  경로가 복잡, `switch`는 케이스 소진을 모름 — 전부 fall-through. else
  블록 안 함수 선언의 `return`은 그 함수를 벗어날 뿐이므로 문장 분할이
  함수 body 안으로 들어가지 않는 것(brace 규칙)으로 자연히 배제된다.

### 결정 3: 기존 오탐 고정 테스트는 갱신한다

- **상황**: `let_else_non_diverging_else_ending_in_a_brace_is_still_an_error`
  가 `{ return 1; }`(발산하는 bare block)를 에러로 고정하고 있었다.
- **선택과 근거**: 그 케이스는 휴리스틱의 한계를 고정한 것이지 언어의
  의도가 아니다(당시 문서도 "실제로 발산해도 거부됩니다"라고 한계로
  적었다). 발산 인정 목록으로 옮기고 규범 문서를 함께 갱신했다 — 언어
  표면 변경이며, 기존에 통과하던 코드는 전부 계속 통과한다(CFG는 구
  판정의 상위집합: 같은 문장 분할에서 마지막 문장의 발산 키워드는 CFG
  로도 발산이다).

## 작업 내역

- 2026-08-21: `src/flow/mod.rs` 신설 — CFG 타입, `diverges`, 문장 분할
  (블록/표현 brace 구분 포함 — lets.rs에서 이동), `if`/`else`(체인·
  단일문 then 포함) 파싱, 단위 테스트 9건.
- `src/parser/lets.rs` — 발산 판정을 `crate::flow::block_diverges`로
  위임, 옛 휴리스틱·상수 삭제.
- `src/sema.rs` — 문안을 새 규칙으로("every path through the `else`
  block must diverge …").
- 문서: language.md §6.4·요약표, errors.md 항목, docs/ai/rl.md.
- 테스트: compile.rs — 오탐 케이스 이동, 신규
  `let_else_divergence_is_a_flow_answer_not_a_last_keyword_check`(5형태).

## 이슈 및 해결

### 이슈 1: `else`가 별도 문장으로 쪼개짐

- **증상**: `if (c) { return 1; } else { return 2; }`가 비발산 판정.
- **원인**: 문장 분할이 블록문의 `}`(그리고 단일문 then의 `;`)에서
  끊는데, 뒤따르는 `else`는 같은 `if` 문의 연속이다.
- **해결**: 경계 다음 토큰이 `else`면 끊지 않는 연속 규칙 추가(양쪽).

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` (613건 — flow 9건·let-else 신규 포함, native 포함 전부
  통과)

## 결과

`src/flow/` 신설, let-else 발산이 CFG 답이 됐다. 후속: `try` 배치·
`result` early-return·분기별 초기화의 flow 이동, HIR body 연동.
