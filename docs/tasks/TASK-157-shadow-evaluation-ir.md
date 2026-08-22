# TASK-157: shadow Evaluation IR과 CFG validator

- **상태**: 완료
- **시작일**: 2026-08-22
- **완료일**: 2026-08-22
- **커밋**: —

## 목적

ProgramSyntax의 host 평가 문맥과 CoreFile의 모든 의미 primitive를 하나의 Evaluation IR로
낮춘다. 기존 backend와 독립된 shadow CFG를 만들고 operation coverage, block 종료,
branch target, boundary reason을 validator로 보장한다.

## 범위

- 포함: Core root identity, Evaluation region/block/value ID, 공통 operation·placement·CFG,
  중첩 Core 순회, validator, shadow compile 연결, 단위·회귀 테스트
- 제외: Structured TypeScript IR, 실제 codegen 전환, IIFE 출력 변경, 효과·scope hygiene 분석

## 의사결정

### 결정 1: host root와 중첩 Core operation의 identity를 분리한다

- **상황**: pipeline이나 result가 host placeholder 하나를 차지해도 그 내부의 match와
  propagation은 별도 Core operation이다. 모든 중첩 노드에 가짜 SWC sentinel을 만들면
  실제 TypeScript parent context를 왜곡한다.
- **검토한 대안**: 모든 Core node를 projection에 강제로 노출하면 overlay 수는 맞지만
  synthetic call/argument 문맥이 원래 의미처럼 보인다. 최상위 placeholder만 추적하면
  내부 operation coverage를 잃는다.
- **선택과 근거**: `CoreRoot`가 실제 host placeholder만 연결한다. Evaluation lowering은
  Core graph를 재귀 순회해 내부 operation을 `RegionPlacement::Nested { parent }`로 둔다.
  따라서 SWC host 사실과 Core 내부 평가 관계가 섞이지 않는다.

### 결정 2: region과 CFG의 불변조건을 타입과 validator가 함께 소유한다

- **상황**: 후속 direct control-flow target은 모든 block 종료, 유효한 target, 정상 경로의
  결과 정의를 전제로 해야 한다.
- **검토한 대안**: `Option<Terminator>`를 builder 중간 상태로 유지하면 미종료 block이
  표현 가능하다. codegen에서 그때그때 검사하면 잘못된 IR이 여러 단계로 전파된다.
- **선택과 근거**: `RegionId`·`EvalBlockId`·`ValueId` newtype과 닫힌 `EvalTerminator`를
  사용했다. validator는 reachability, target 범위, operation 단일성, value-producing
  region의 모든 정상 종료 경로에서 동일 `ValueId` 정의를 검사한다.

### 결정 3: boundary는 closure 출력이 아니라 placement 사실로만 기록한다

- **상황**: 이 단계에서 IIFE를 생성하면 Evaluation IR이 기존 backend 선택을 답습한다.
- **검토한 대안**: boundary region을 즉시 closure target으로 낮추면 구현은 빠르지만
  Phase 3 source-preserving target과 Phase 4 direct control flow의 경계가 사라진다.
- **선택과 근거**: host context의 `StructureCapability::Boundary(reason)`을 region placement에
  그대로 보존한다. Evaluation IR은 이유를 잃지 않지만 어떤 TypeScript 구문도 선택하지
  않는다.

## 작업 내역

- 2026-08-22: TASK-154의 Phase 2 완료 조건과 현재 ProgramSyntax/CoreFile 경계를 확인했다.
- 2026-08-22: Rust Enterprise Type Programming Guide의 newtype, 닫힌 합 타입, typed error,
  불변 builder 원칙을 적용하기로 했다.
- 2026-08-22: ProgramSyntax overlay에 `CoreRoot` identity를 추가해 SWC host context와
  Core root를 문자열이나 span 비교 없이 연결했다.
- 2026-08-22: `evaluation_ir.rs`에 operation, placement, region, block, statement,
  terminator, result value 모델을 구현했다.
- 2026-08-22: Core root body부터 decision subject·guard·arm·miss, propagation value,
  apply head·step, result item·value, template interpolation을 재귀 순회했다.
- 2026-08-22: CFG target/reachability, operation 단일성, result path 정의, orphan·duplicate
  host와 duplicate operation validator를 구현했다.
- 2026-08-22: 실제 compile 진입에서 ProgramSyntax와 EvaluationFile을 함께 shadow로
  보유하도록 연결했다.
- 2026-08-22: 전체 포맷·clippy·테스트 게이트를 통과했다.

## 이슈 및 해결

### 이슈 1: coverage fixture가 일반 TypeScript 표면을 Core primitive로 기대함

- **증상**: 여섯 operation variant coverage 테스트에서 네 variant만 집계됐다. 출력된
  operation은 propagation, result, apply, decision뿐이었다.
- **원인**: payload가 없는 `enum E { A, B }`는 유효한 TypeScript enum으로 통과하며,
  `./load.js` import도 RL import rewrite 대상이 아니므로 Core `Adt`와 `Import`가 아니다.
- **해결**: fixture를 payload가 있는 RL enum과 `./load.rl` import로 바꿨다. 이후
  `Adt·Import·Decision·Propagate·Apply·ResultRegion` 여섯 variant가 모두 확인됐다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`

## 결과

`ProgramSyntax + CoreFile → EvaluationFile` shadow 경로를 구현했다. 모든 Core primitive는
하나의 operation region을 가지며, 중첩 primitive와 host boundary가 identity로 연결된다.
validator가 CFG와 결과 정의 불변조건을 확인하지만 기존 backend는 이 IR을 소비하지 않아
출력·진단·mapping은 바뀌지 않았다. 다음 태스크는 Phase 3 source-preserving target이다.
