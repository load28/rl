# TASK-150: 전체 rl 구문의 Lowered IR 아키텍처 전환

- **상태**: 진행 중
- **시작일**: 2026-08-22
- **완료일**: —
- **커밋**: —

## 목적

모든 rl 구문의 의미 판단과 TypeScript 문자열 방출 사이에 전용 Lowered IR
경계를 둔다. codegen이 parser AST를 직접 해석하지 않고, HIR와 semantic facts를
소비해 만들어진 `LoweredFile`만 기계적으로 출력하도록 전환한다.

## 범위

- 포함: 전체 rl 구문의 lowering 모델, `LoweredFile` 생성·불변조건 검증, 임시 이름 사전 할당, emitter 입력 전환, 기존 출력·emit-map 호환 테스트
- 제외: TypeScript 전체 AST, Lowered IR 최적화, passthrough TypeScript 파싱, borrow checking·SSA·ABI 같은 rustc MIR 기능

## 의사결정

### 결정 1: 전체 MIR가 아니라 전체 rl 구문을 포괄하는 출력 전용 Lowered IR 도입

- **상황**: HIR·resolver·typed pattern analysis·flow facts는 이미 있으나 codegen은 모든 parser AST 구문을 직접 소비한다. match만 별도 경계로 옮기면 emitter의 이중 입력 구조가 고착되고 Phase 7의 전체 목표를 충족하지 못한다.
- **검토한 대안**: rustc 수준의 범용 MIR는 RL이 소유하지 않는 TypeScript 의미까지 모델링해야 하므로 과하다. match 전용 IR은 변경 폭이 작지만 전체 아키텍처 경계를 만들지 못한다. TypeScript AST를 먼저 도입하면 printing만 구조화되고 의미 판단은 codegen에 남는다.
- **선택과 근거**: 모든 rl 구문과 passthrough span을 포괄하는 출력 전용 `LoweredFile`을 도입한다. 내부 전환은 match부터 순차 진행하되 완료 조건은 emitter의 parser AST 직접 소비 제거다. rustc가 THIR에서 MIR를 만들고 codegen이 MIR primitive를 소비하는 단계 분리 원칙을 RL 규모에 맞게 적용한다.

### 결정 2: parser AST를 복제한 구문별 IR은 채택하지 않음

- **상황**: AST와 같은 `Enum`/`Match`/`Try` 변종을 별도 타입으로 복제하면 codegen의 입력 타입만 바뀌고, 구문별 의미 판단과 중복 방출 경로는 그대로 남는다.
- **검토한 대안**: AST 복제형 IR은 이관이 쉽지만 새 문법마다 lowering과 emitter 양쪽에 변종이 추가된다. 생성 문자열 중심 IR은 printer를 단순화하지만 구조 검증과 target-level 변환을 할 수 없다. 제어·데이터 중심 IR과 별도 TypeScript IR은 초기 비용이 크지만 여러 표면 구문을 공통 primitive로 정규화한다.
- **선택과 근거**: `SemanticFile`에서 이름·타입·flow 사실을 확정한 뒤, 표면 구문과 독립적인 control/data primitive로 낮춘다. 그 결과를 RL이 생성하는 범위만 표현하는 TypeScript IR로 다시 낮추고 printer는 해당 IR만 출력한다.

### 결정 3: 새 Core IR은 debug shadow lowering으로 먼저 검증

- **상황**: 기존 emitter를 한 번에 교체하면 compile output과 emit-map 회귀가 어느 lowering 단계에서 생겼는지 분리하기 어렵다. 반대로 사용하지 않는 IR을 release 경로에서 함께 만들면 컴파일 비용만 늘어난다.
- **검토한 대안**: 즉시 backend를 전환하면 중간 이중 경로는 없지만 회귀 원인 격리가 어렵다. 모든 build에서 두 경로를 실행하면 검증 표본은 넓지만 release 성능이 떨어진다. debug/test에서만 shadow lowering하면 기존 출력 계약을 유지하면서 validator와 구조 테스트를 먼저 축적할 수 있다.
- **선택과 근거**: debug/test build에서 기존 codegen 앞에 `SemanticFile → CoreFile`을 실행하고 release에서는 생략한다. TypeScript IR/printer가 출력 동등성 테스트를 통과하면 새 경로를 정식 경로로 바꾸고 legacy codegen을 삭제한다.

## 작업 내역

- 2026-08-22: 첨부 분석, `docs/design/compiler-core.md`, HIR·analysis·codegen 구현을 대조했다. codegen이 모든 parser AST 구문을 직접 분기하는 현재 경계를 확인했다.
- 2026-08-22: 사용자 피드백에 따라 match 한정 범위를 폐기하고 전체 rl 구문의 Lowered IR 전환으로 범위를 확정했다.
- 2026-08-22: AST 구조를 복제하는 초기 Lowered IR 초안을 검토 후 폐기했다. 구문별 복제는 의미 정규화가 아니므로 구현에 포함하지 않는다.
- 2026-08-22: `SemanticFile`을 도입해 HIR·resolution·pattern analysis를 한 결과로 묶고, sema가 이 결과를 재사용하도록 변경했다.
- 2026-08-22: HIR에 binding mode, arm body kind, match extent, template raw chunk를 보존해 후속 backend가 parser AST를 다시 읽지 않아도 되게 했다.
- 2026-08-22: `src/core_ir`에 공통 `Decision`·`Propagate`·`Apply`·`Adt` primitive와 validator를 구현했다. `match`·`if let`·`let-else` 및 `try`·`result`가 각각 같은 IR로 낮아지는 구조 테스트를 추가했다.
- 2026-08-22: 기존 전체 `cargo test`와 Core IR 집중 테스트를 실행해 기존 출력·의미 계약이 유지됨을 확인했다.
- 2026-08-22: DNF 카테시안 곱이 tuple·or·중첩 pattern의 논리 구조와 mapping 귀속을 잃는 문제를 확인했다. `PatternPlan::AnyOf`·`AllOf`·`Test`·`Bind` decision tree로 교체해 조합 폭증 없이 구조를 보존했다.
- 2026-08-22: Core IR에 target anchor용 head/extent node, propagation/apply/result node, async 실행 형태, 단일/tuple decision 임시값 종류를 확정했다. target backend가 parser AST에서 정보를 복원하지 않게 했다.
- 2026-08-22: `src/codegen/core.rs`에 전체 Core primitive의 TypeScript target lowering을 구현했다. enum, import, match/tuple match, try, let-else, if-let, pipe/flow, result, template, passthrough를 모두 같은 backend 경계로 연결했다.
- 2026-08-22: debug shadow backend로 기존 backend와 전체 compile output·mapping·scrutinee/payload mark·anchor를 대조했다. compile 281개와 emit-map corpus의 byte 단위 동등성을 만든 뒤 정식 경로를 `SemanticFile + CoreFile` 입력으로 전환했다.
- 2026-08-22: parser AST 기반 `codegen/enums.rs`·`codegen/matches.rs`와 AST `Emitter`를 삭제했다. `src/codegen`과 `src/core_ir`에서 parser AST 참조가 없음을 `rg`로 확인했다.

## 이슈 및 해결

### 이슈 1: AST 복제형 Lowered IR이 의미를 정규화하지 못함

- **증상**: 초기 초안의 변종이 parser AST의 `Enum`·`Match`·`Try` 구조를 그대로 반복해 emitter 분기와 새 문법의 이중 수정 문제를 남겼다.
- **원인**: emitter 입력 타입 교체를 lowering 단계 분리로 잘못 간주했다.
- **해결**: 초안을 삭제하고 pattern 계열은 `Decision`, Result 계열은 `Propagate`로 통합하는 표면 독립 Core IR로 재설계했다.

### 이슈 2: 평탄화된 pattern 대안이 target에 필요한 구조를 소실함

- **증상**: tuple 안의 or-pattern을 카테시안 곱으로 만들면 원래의 conjunction/disjunction 경계와 shared binding의 mapping 귀속을 복원할 수 없었다.
- **원인**: Core IR이 decision tree가 아니라 대안 목록의 DNF만 보존했다.
- **해결**: boolean 구조를 직접 표현하는 `PatternPlan`으로 교체했다. backend는 tree를 재귀 방문해 조건, 무조건 arm, binding group과 mapping 귀속을 계산한다.

### 이슈 3: 해석된 이름과 복구 출력의 source spelling이 서로 다름

- **증상**: 편집 중 misspelling은 resolution identity를 얻을 수 있지만 projection 출력은 사용자가 쓴 철자를 유지해야 했다.
- **원인**: target lowering이 resolved declaration 이름을 출력 철자로 사용했다.
- **해결**: Core의 constructor/field가 resolution ID와 use node를 함께 보존한다. 의미 비교는 ID로 하고 출력 철자는 source-map node에서 가져온다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`
- [x] `cargo check --release`

## 결과

전체 rl 표면이 `SemanticFile → CoreFile → TypeScript target IR → printer` 경로를 사용한다.
codegen의 parser AST 직접 소비와 legacy emitter를 제거했고 기존 출력·emit-map·런타임
계약을 유지했다.
