# Lowered IR 아키텍처

TASK-150의 규범 설계다. 목표는 codegen 파일을 나누는 것이 아니라, parser AST에
남아 있는 표면 문법과 TypeScript 방출 사이에 검증 가능한 의미 경계를 세우는
것이다.

## 1. 파이프라인

```text
source
  → lossless AST
  → HIR
  → resolution + typed pattern facts + flow facts
  → SemanticFile
  → Core IR
  → TypeScript IR
  → printer
  → TypeScript source + emit map
```

rustc도 HIR을 직접 codegen하지 않는다. 타입이 붙고 더 desugar된 THIR에서 CFG
형태의 MIR를 만들며, backend는 MIR의 block·statement·terminator·place·operand·
rvalue를 codegen IR로 번역한다. RL은 borrow checking·drop·ABI·SSA를 소유하지
않으므로 이를 복제하지 않는다. 대신 RL이 추가한 의미만 Core IR로 정규화한다.

근거:

- Rust Compiler Development Guide, “Overview of the compiler” — HIR, THIR, MIR 단계
  https://rustc-dev-guide.rust-lang.org/overview.html
- Rust Compiler Development Guide, “MIR construction” — THIR에서 CFG·place·operand로 lowering
  https://rustc-dev-guide.rust-lang.org/mir/construction.html
- Rust Compiler Development Guide, “Lowering MIR” — backend가 MIR primitive별로 codegen IR 생성
  https://rustc-dev-guide.rust-lang.org/backend/lowering-mir.html

## 2. 단계별 소유권

### Lossless AST

원본 byte span과 parser recovery만 소유한다. passthrough TypeScript는 끝까지 opaque
span으로 유지한다. 이름 해석·소진성·방출 전략·임시 이름을 소유하지 않는다.

### SemanticFile

한 파일의 HIR, resolution, typed pattern facts, flow facts를 함께 소유한다. 모든
참조는 `DefId`·`VariantId`·`FieldId`·`LocalId`로 연결한다. 동일한 의미 분석을 sema,
lowering, editor가 다시 계산하지 않는다.

### Core IR

표면 구문 이름을 제거하고 다음 primitive만 소유한다.

- `Adt`: discriminant와 payload가 확정된 대수적 데이터 선언·생성자
- `Decision`: `AnyOf`·`AllOf`·`Test`·`Bind`로 구성한 패턴 decision tree
- `Propagate`: 값을 한 번 평가하고 success payload를 bind하거나 정해진 target으로 exit
- `Control`: block·branch·return·break와 result-producing region
- `Apply`: 값 pipeline과 함수 composition의 평가 순서가 확정된 호출 그래프
- `OpaqueTs`: RL이 해석하지 않는 TypeScript span

`match`, `if let`, `let-else`는 모두 `Decision`으로 낮춘다. `try`와 `result` binding은
모두 `Propagate`로 낮춘다. 새 표면 문법이 기존 primitive로 표현되면 emitter는
변경하지 않는다.

### TypeScript target IR

RL이 생성하는 TypeScript는 mapping-aware structured writer인 `Rope`로 낮춘다. 일반
TypeScript는 source span 조각으로, 생성 코드와 helper는 literal 조각으로 표현한다.
source mapping mark와 anchor는 별도 구조 조각이며 문자열 검색이나 출력 후 보정으로
만들지 않는다. 이 프로젝트는 TypeScript 전체를 소유하지 않으므로 전체 TypeScript
AST를 복제하지 않고, RL이 생성하는 범위만 구조화한다.

### Printer

target IR 조각을 평탄화하고 source span mapping·mark·anchor를 함께 계산한다. enum
이름 해석, pattern decision 구성, async 필요 여부, 임시 이름 생성은 판단하지 않는다.

## 3. 불변조건

각 단계는 다음 validator를 통과해야 한다.

- `validate_semantic`: 모든 HIR use가 resolution 또는 명시적 recovery이며 facts의 node ID가 유효함
- `validate_core`: decision leaf·exit target·place·temp가 유효하고 모든 임시 이름 ID가 유일함
- `validate_ts_ir`: statement/expression 위치, anchor 범위, source span, mapping mark가 유효함
- `verify_output`: 최종 TypeScript가 parser를 통과함

첫 세 validator의 실패는 사용자 진단이 아니라 internal compiler error다. 사용자
오류는 SemanticFile에 recovery fact로 명시되어야 하며 codegen이 새로 판단하지 않는다.

## 4. 전환 순서

1. 기존에 중복 생성되던 HIR·resolution·pattern analysis를 `SemanticFile` query로 통합한다.
2. `Decision`과 `Propagate`를 도입해 pattern 계열과 Result 계열을 각각 한 경로로 통합한다.
3. `Adt`·`Apply`·import·template를 Core IR에 편입하고 임시 ID를 lowering에서 할당한다.
4. mapping-aware TypeScript target IR과 printer를 도입한 뒤 기존 AST 기반 codegen을 삭제한다.

각 단계는 기존 compile output과 emit-map corpus의 byte 단위 동일성을 유지한다.

TASK-150에서 네 단계를 모두 완료했다. 현재 backend 경계는
`emit_with_map(&SemanticFile, &CoreFile, source, ...)`이며 `src/codegen`은 parser AST를
참조하지 않는다.

전체 TypeScript 평가 문맥과 Core IR을 결합해 owner별 최적 lowering을 선택하는 후속 계층은
[`program-lowering.md`](./program-lowering.md)에 정의한다. Core IR의 rl 의미 소유권은
유지하며, SWC AST는 host TypeScript의 구조와 평가 문맥을 제공한다.
