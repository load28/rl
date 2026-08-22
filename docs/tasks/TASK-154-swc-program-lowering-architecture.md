# TASK-154: SWC 전체 프로그램 lowering 아키텍처 설계

- **상태**: 완료
- **시작일**: 2026-08-22
- **완료일**: 2026-08-22
- **커밋**: —

## 목적

rl 구문이 포함된 파일의 전체 TypeScript 구조를 SWC AST로 이해하고, 모든 rl 구문을
평가 문맥과 효과에 따라 최적의 TypeScript 제어 흐름으로 낮추는 아키텍처를 설계한다.
IIFE 같은 범용 우회 표현을 기본 방출 전략에서 제거하되 기존 언어 의미, 진단, 원본
위치, passthrough 및 도구 계약은 바꾸지 않는다.

## 범위

- 포함: 전체 프로그램 AST 소유 모델, RL/Core IR 결합, 효과·평가 순서 분석,
  expression-to-statement 선형화, source-preserving 출력, 진단 provenance, 단계별 전환과
  동등성 검증 기준
- 제외: 이번 태스크에서의 Rust 구현, 새 rl 표면 문법, 사용자 진단 문안 변경,
  SWC 프린터를 통한 전체 파일 재출력, 번들러별 최적화 구현

## 의사결정

### 결정 1: lossless rl parser를 SWC로 교체하지 않고 합성 프로그램 모델을 만든다

- **상황**: 전체 TypeScript parent context가 있어야 expression lowering의 평가 순서와
  삽입 owner를 알 수 있지만, SWC는 rl 문법과 현재 parser의 무오류 claim/recovery 계약을
  알지 못한다.
- **검토한 대안**: SWC parser를 fork해 rl node를 직접 추가하면 하나의 AST가 되지만 SWC
  버전 추적과 TypeScript grammar 유지 비용을 프로젝트가 떠안는다. 현재 parser의 opaque
  span만 유지하면 host expression context를 계속 추측해야 한다. 기존 출력 TypeScript를
  다시 파싱하면 IIFE 같은 이미 선택된 target 모양이 원래 문맥을 가린다.
- **선택과 근거**: lossless parser가 확정한 rl span을 category-preserving sentinel로
  projection하고 전체 파일을 SWC로 파싱한다. `ProgramSyntax`가 SWC AST, RL/Core overlay,
  parent/scope/origin table을 함께 소유한다. Svelte의 parse/analyze/transform 분리와
  rustc의 AST/HIR 단계 분리를 참고하되 각 IR의 사실 소유권을 합치지 않는다.

### 결정 2: syntax별 rewrite가 아니라 공통 Evaluation IR을 도입한다

- **상황**: `match`만 statement로 hoist하면 `result`, pipe, 중첩 rl 구문에서 같은 평가
  순서와 temporary 문제를 다시 풀게 된다. 호출 callee, short-circuit, 반복 조건은 단순
  `prelude + value` 모델로 보존되지 않는다.
- **검토한 대안**: match visitor에서 parent 종류를 열거하는 방식은 빠르지만 새 AST node와
  rl 문법마다 예외가 늘어난다. 모든 TypeScript를 완전한 MIR로 낮추면 정확하지만 rl과
  관계없는 언어 전체를 재구현한다. Core primitive를 포함하는 최소 evaluation owner만
  CFG로 낮추면 공통 제어 흐름을 가지면서 opaque TypeScript를 유지할 수 있다.
- **선택과 근거**: `Decision`, `Propagate`, `Apply`, `Adt`를 SWC 평가 문맥에 배치하는
  Evaluation IR을 둔다. parent edge는 eager/conditional/repeated/deferred, value/reference,
  effect, control boundary 프로토콜을 제공한다. optimizer와 target structurer는 rl 표면
  문법 이름을 보지 않는다.

### 결정 3: 무조건 IIFE 0개보다 의미 보존을 우선한다

- **상황**: 사용자는 모든 기존 expression 위치를 지원하면서 IIFE 같은 wrapper를 없애길
  원한다. 그러나 ECMAScript expression에는 임의 statement가 없고, parameter initializer는
  별도 실행 환경이며, call callee의 Reference Record는 JavaScript 값으로 저장할 수 없다.
- **검토한 대안**: 모든 위치에서 owner 밖으로 hoist하면 default parameter 실행 시점과
  스코프 또는 call의 `this`가 달라진다. helper/`Reflect.apply`를 무조건 사용하면 IIFE를
  다른 우회 수단으로 바꿀 뿐이며 mutable intrinsic 가정도 생긴다. 해당 위치를 금지하면
  기존 언어 표면이 깨진다.
- **선택과 근거**: 증명 가능한 owner에서는 direct control flow로 낮추고, 불가능한 곳은
  이유 코드가 있는 `BoundaryClosure`로 표현한다. backend의 구문별 IIFE 선택은 제거하되
  의미상 필요한 closure는 IR과 validator가 명시한다. ECMAScript `EvaluateCall` 명세로
  Reference와 argument evaluation의 구분을 확인했다.

### 결정 4: SWC printer 대신 source-preserving target을 유지한다

- **상황**: 전체 SWC AST를 printer로 출력하면 pure TypeScript passthrough, 주석·trivia,
  emit mapping과 진단 위치가 달라진다.
- **검토한 대안**: 전체 pretty-print와 source map은 구현이 단순하지만 바이트 계약을
  깨뜨린다. 문자열 edit만 사용하면 조건부/반복 문맥에서 원본 조각을 구조적으로 옮기기
  어렵다. 원본과 생성 조각을 함께 가진 target tree는 구현 비용이 있지만 현재 Rope의
  mapping 계약을 확장할 수 있다.
- **선택과 근거**: unchanged subtree는 원본 span으로, glue는 provenance가 있는 generated
  node로 출력한다. non-RL 원본 조각의 단일성·순서와 모든 generated node의 origin을
  validator가 검사한다. 기존 `EmitMapping`과 `EmitAnchor`의 서로 다른 계약도 유지한다.

### 결정 5: shadow 단계와 동등성 게이트로 전환한다

- **상황**: 출력 형태는 의도적으로 달라지므로 전체 문자열 동등성만으로 새 backend를
  검증할 수 없지만, 한 번에 교체하면 진단·평가 순서 회귀의 원인을 격리하기 어렵다.
- **검토한 대안**: 즉시 전환은 이중 경로가 없지만 silent miscompile 위험이 크다. 전체
  기간에 두 backend를 유지하면 부채가 고착된다. 단계별 shadow 검증은 중간 비용이 있지만
  projection, Evaluation IR, target printer의 계약을 따로 증명할 수 있다.
- **선택과 근거**: ProgramSyntax와 Evaluation IR을 각각 shadow로 도입하고, 기존 출력과
  byte-identical target을 먼저 만든 뒤 direct control flow를 켠다. 이후 출력은 runtime
  trace·진단·mapping 동등성으로 검증하며 pure TS는 계속 byte 비교한다.

## 작업 내역

- 2026-08-22: 현재 `SemanticFile → CoreFile → codegen/Rope` 경계, IIFE 기반
  `match`·`result` 방출, emit mapping·diagnostic anchor 계약을 확인했다.
- 2026-08-22: 기존 TASK-150/151의 Core IR 결정과 이번 전체 프로그램 AST 계층의
  관계를 검토했다.
- 2026-08-22: Svelte compiler의 parse/analyze/transform 단계와 scope map, rustc의
  AST/HIR/THIR/MIR 및 owner lowering, SWC visitor/path transform, ECMAScript
  `EvaluateCall`의 Reference 의미를 공식 자료에서 확인했다.
- 2026-08-22: `docs/design/program-lowering.md`에 합성 ProgramSyntax, Evaluation IR,
  evaluation protocol, continuation lowering, BoundaryClosure, source provenance, validator,
  단계별 전환과 완료 기준을 작성했다.
- 2026-08-22: `docs/design/lowered-ir.md`에서 후속 Evaluation IR 설계를 연결했다.
- 2026-08-22: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test`를 실행해 기존 코드와 테스트가 그대로 통과함을 확인했다.

## 이슈 및 해결

### 이슈 1: 무조건 IIFE 제거와 기존 완전한 expression 지원이 양립하지 않음

- **증상**: default parameter의 block arm은 statement slot이 없고, call argument를
  hoist하면 먼저 평가된 callee Reference를 값으로 보존할 수 없어 `this` 의미가 달라질
  수 있다.
- **원인**: TypeScript/ECMAScript는 statement expression과 Reference Record를 저장하는
  표면 문법을 제공하지 않는다. SWC AST는 이 차이를 보이게 하지만 새 runtime primitive를
  만들지는 못한다.
- **해결**: IIFE를 기본 codegen template에서 제거하고, 직접 구조화가 불가능하다는 증명이
  있는 경우만 `BoundaryClosure`를 생성한다. 이유 코드를 닫힌 합 타입으로 만들고 barrier
  개수를 측정해 후속 아키텍처 작업의 입력으로 삼는다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`

## 결과

`docs/design/program-lowering.md`에 특정 구문별 예외가 아닌 전체 프로그램 평가 모델을
기준으로 한 아키텍처를 확정했다. 기존 Core IR과 진단 소유권을 유지하면서 SWC AST가
TypeScript host context를 제공하고, Evaluation IR이 direct control flow 가능 여부를
증명한다. 구현은 후속 태스크로 분리한다.
