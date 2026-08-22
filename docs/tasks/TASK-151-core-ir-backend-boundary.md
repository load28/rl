# TASK-151: Core IR backend 의미 판단 제거

- **상태**: 완료
- **시작일**: 2026-08-22
- **완료일**: 2026-08-22
- **커밋**: —

## 목적

TypeScript backend가 Core IR의 패턴 구조와 HIR item을 다시 분류해 방출 전략을
결정하는 중복을 제거한다. 의미 lowering 결과가 backend에 필요한 결정을 명시적으로
전달하도록 경계를 강화한다.

## 범위

- 포함: decision 방출 형태, arm 조건 성격, let-else 직접 variant 판정, import item 정보의 Core IR 이전과 회귀 테스트
- 제외: source mapping을 위한 원문 slice 제거, enum 출력 모델 전면 재설계, TypeScript AST 도입

## 의사결정

### 결정 1: 원문 접근과 의미 재판단을 구분한다

- **상황**: backend는 `SemanticFile`, `CoreFile`, 원문을 함께 받는다. 원문 접근을 모두 제거하면 passthrough byte 동일성과 source mapping을 깨뜨리지만, 원문/HIR 구조를 다시 분류해 방출 형태를 고르는 것은 Core IR 경계를 약화한다.
- **검토한 대안**: 원문을 전부 Core IR 문자열로 복제하면 backend 입력은 줄지만 메모리 사용과 mapping 책임이 악화한다. 현재 구조를 유지하면 switch 선택과 item 복원이 backend에 남는다.
- **선택과 근거**: 원문은 spelling과 opaque span 출력에만 사용한다. match dispatch, statement decision 종류, let-else 직접 variant, import 종류, ADT 출력 데이터는 lowering에서 확정한다. `rg`로 codegen의 `hir.items`, `resolution.defs`, `pattern_has_nested_test`, `direct_variant_alternatives` 참조가 사라졌음을 확인했다.

### 결정 2: 방출 전략을 `DecisionKind`의 닫힌 합 타입으로 표현한다

- **상황**: backend가 arm action과 pattern tree를 검사해 match/if-let/let-else 및 switch/if-chain을 추론했다.
- **검토한 대안**: boolean 필드를 여러 개 두면 모순된 조합을 만들 수 있다. backend helper만 Core 모듈로 옮기면 계산 시점만 감춰지고 IR 계약은 생기지 않는다.
- **선택과 근거**: `DecisionKind::{Match, IfLet, LetElse}`와 `MatchDispatch`를 도입한다. 유효한 조합만 타입으로 표현하며 validator와 구조 테스트가 계약을 고정한다.

### 결정 3: ADT와 import는 backend가 HIR owner를 역참조하지 않게 한다

- **상황**: import는 `OwnerId`로 HIR item을 다시 열었고 ADT는 `DefId → resolution → origin node → HIR item`을 거슬러 출력 데이터를 복원했다.
- **검토한 대안**: ID 역참조를 유지하면 데이터 중복은 적지만 backend가 여러 상위 계층의 구조를 알아야 한다. 출력 문자열을 lowering에서 완성하면 backend는 단순하지만 target rendering까지 semantic lowering에 섞인다.
- **선택과 근거**: Core IR에 구조화된 `Import`와 `Adt` 데이터를 소유시킨다. 문자열 렌더링은 backend에 남기고, 중복 constructor 억제 여부는 lowering에서 `emit_constructor`로 확정한다.

## 작업 내역

- 2026-08-22: `src/codegen/core.rs`를 감사해 switch 가능성, literal/nested test, 무조건 arm, let-else 직접 variant와 import HIR item을 backend가 다시 판정하는 지점을 확인했다.
- 2026-08-22: `DecisionKind`와 `MatchDispatch`를 추가해 statement 종류, match dispatch, block arm label 필요 여부를 Core IR에 기록했다.
- 2026-08-22: let-else의 직접 variant 실패 조건을 lowering에서 계산하고 backend의 pattern 재분류 helper를 제거했다.
- 2026-08-22: import specifier·종류와 ADT 이름·generic·variant·field·constructor 회복 정책을 Core IR에 소유시켜 backend의 HIR/resolution 역참조를 제거했다.
- 2026-08-22: Core IR 구조 테스트 2개를 추가하고 전체 `fmt`, `clippy`, `test` 게이트를 통과했다.

## 이슈 및 해결

### 이슈 1: import variant 이름 불일치

- **증상**: 첫 `cargo check`에서 `ImportKind::RelativeRl`이 존재하지 않는다는 오류가 발생했다.
- **원인**: HIR의 실제 variant 이름은 `Relative`였다.
- **해결**: validator가 `ImportKind::{Std, Relative}`를 완전하게 열거하도록 수정했다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`

## 결과

codegen은 더 이상 HIR item이나 resolution definition을 거슬러 enum/import를 복원하지
않는다. match 방출 전략과 let-else 최적 실패 조건도 Core IR lowering에서 한 번만
결정한다. 언어 표면은 바뀌지 않아 `docs/ai/rl.md` 갱신은 필요하지 않다.
