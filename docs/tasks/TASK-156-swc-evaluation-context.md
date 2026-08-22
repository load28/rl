# TASK-156: SWC 평가 문맥 프로토콜

- **상태**: 완료
- **시작일**: 2026-08-22
- **완료일**: 2026-08-22
- **커밋**: —

## 목적

ProgramSyntax의 SWC parent path를 구문별 codegen 분기가 아니라 평가 빈도, 값 종류,
제어 owner, statement 구조화 가능성으로 정규화한다. 후속 Evaluation IR이 `match`와
`result`를 같은 규칙으로 직접 제어 흐름화하거나 의미 경계를 선택할 수 있게 한다.

## 범위

- 포함: 닫힌 평가 프로토콜 타입, parent-path 분석, conservative barrier, validator와
  문맥별 단위 테스트, shadow 컴파일 경로 연결
- 제외: 실제 Evaluation CFG, 기존 codegen 전환, IIFE 출력 변경, 효과 분석과 hygiene

## 의사결정

### 결정 1: 평가 문맥을 네 개의 직교 타입으로 분리한다

- **상황**: 하나의 `can_hoist` boolean은 반복 실행, parameter environment, 대입 대상,
  호출 Reference 문제를 구분하지 못한다.
- **검토한 대안**: SWC node 이름별 최적화 flag는 빠르지만 새 문법과 rl construct마다
  분기가 늘어난다. 모든 정보를 하나의 enum에 넣으면 가능한 조합 수가 커지고 후속
  validator가 누락을 찾기 어렵다.
- **선택과 근거**: `EvaluationFrequency`, `EvaluationOwner`, `ValueRole`,
  `StructureCapability`을 별도 닫힌 타입으로 만들었다. `BoundaryReason`은 의미 경계를
  parameter, class initialization, call Reference, assignment/pattern, unsupported host로
  구분한다.

### 결정 2: 확인할 수 없는 문맥은 추측하지 않고 명시적 barrier로 둔다

- **상황**: `AstParentKind::BinExpr(Right)`는 좌·우 위치는 보존하지만 논리 연산자인지는
  담지 않는다. 논리 RHS는 조건부이고 산술 RHS는 eager다.
- **검토한 대안**: 모든 binary RHS를 eager로 보면 `&&`·`||`·`??`를 잘못 hoist할 수
  있다. 모두 conditional로 보면 안전하지만 분석 사실이 실제 의미와 달라진다.
- **선택과 근거**: operator fact가 연결되기 전에는 `Indeterminate`와
  `UnsupportedHost` boundary를 기록한다. 후속 분석이 실제 SWC node fact를 공급할 때만
  barrier를 좁힌다.

### 결정 3: 실행 빈도는 가장 가까운 owner 안에서만 합성한다

- **상황**: 함수가 loop 안에 선언됐더라도 함수 body의 표현식은 함수 호출 기준으로
  실행된다. 바깥 loop를 그대로 상속하면 repeated로 잘못 분류한다.
- **검토한 대안**: module부터 전체 parent path를 합성하면 구현은 단순하지만 deferred
  boundary를 넘는다. source span만 보면 lexical containment와 evaluation owner를 구분할
  수 없다.
- **선택과 근거**: reverse parent path에서 가장 가까운 function body, parameter,
  constructor, class initializer, static block owner를 먼저 정하고 그 아래 edge만으로
  once·conditional·repeated를 계산한다.

## 작업 내역

- 2026-08-22: TASK-155의 실제 SWC parent path가 module부터 sentinel identifier까지
  field edge와 index를 보존함을 확인했다.
- 2026-08-22: 네 평가 프로토콜과 닫힌 boundary reason을 `ProgramSyntax` overlay에
  추가했다.
- 2026-08-22: function/parameter/class/static-block owner와 loop·conditional 실행 빈도를
  parent field edge에서 합성했다.
- 2026-08-22: direct return·expression statement, continuation, call Reference 및 보수적
  unsupported boundary를 구조화 capability로 분류했다.
- 2026-08-22: direct return, call argument, ternary branch, loop, default parameter, binary
  RHS를 단위 테스트로 고정하고 전체 회귀 게이트를 통과했다.

## 이슈 및 해결

### 이슈 1: parent path만으로 binary operator를 알 수 없음

- **증상**: `BinExprField::Right`만으로 해당 RHS가 항상 실행되는 산술식인지 단락되는
  논리식인지 판별할 수 없었다.
- **원인**: `AstParentKind`는 child field edge를 저장하며 parent node의 `BinaryOp` 값은
  저장하지 않는다.
- **해결**: 현재 단계에서는 `Indeterminate` frequency와 `UnsupportedHost` boundary를
  생성해 잘못된 최적화를 막았다. 실제 operator fact 연결은 Evaluation IR 분석 단계의
  입력으로 남겼다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`

## 결과

SWC parent path가 이제 구문 이름이 아니라 평가 프로토콜로 정규화된다. 후속 Evaluation
IR은 모든 Core expression에 동일한 owner·frequency·value-role·structure 계약을 적용할
수 있으며, 증명되지 않은 문맥은 이유가 있는 boundary로 남는다. 기존 codegen은 여전히
이 shadow 사실을 소비하지 않으므로 출력과 진단은 바뀌지 않았다.
