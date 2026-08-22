# TASK-155: SWC ProgramSyntax shadow 계층

- **상태**: 완료
- **시작일**: 2026-08-22
- **완료일**: 2026-08-22
- **커밋**: —

## 목적

rl 구문이 있는 파일을 category-preserving TypeScript projection으로 만들고 전체 SWC AST를
shadow로 보유한다. 현재 출력·진단에는 사용하지 않으면서 원본 rl node, projection span,
SWC parent context를 연결하는 ProgramSyntax 기반을 구현한다.

## 범위

- 포함: projection과 좌표 타입, RL node identity, SWC 전체 module parse, overlay와 parent
  context 수집, validator, shadow 실행, 단위·회귀 테스트
- 제외: Evaluation IR, 실제 codegen 전환, IIFE 출력 변경, scope/effect 분석, 사용자 진단 변경

## 의사결정

### 결정 1: SWC는 원본 RL이 아니라 category-preserving projection을 파싱한다

- **상황**: SWC는 rl 구문을 파싱할 수 없지만 전체 TypeScript parent context는 SWC AST에서
  얻어야 했다.
- **검토한 대안**: 기존 방출 결과를 파싱하면 IIFE 등 과거 backend 선택이 원래 host
  context를 가린다. SWC parser를 fork하면 TypeScript grammar 유지 비용과 현재 parser의
  claim 계약을 함께 떠안는다.
- **선택과 근거**: Core IR이 소유한 expression·statement·item을 같은 문법 범주의 sentinel로
  투영하고 나머지 원본 조각은 그대로 복사한다. 그 결과 호출 인자의 `match`가
  `CallExpr` 아래에 있다는 원래 host context를 IIFE 방출 전에 얻는다.

### 결정 2: 원본 좌표와 projection 좌표를 별도 타입으로 유지한다

- **상황**: sentinel 길이는 원본 rl 구문 길이와 다르며 UTF-8 멀티바이트 입력도 존재한다.
- **검토한 대안**: 동일한 `usize`를 사용하거나 길이를 맞춘 placeholder를 만들면 좌표를
  실수로 섞기 쉽고 중첩 구문에서 길이 보정이 누적된다.
- **선택과 근거**: `SourceByte`·`ProjectedByte`와 각각의 span을 newtype으로 분리했다.
  SWC span은 해당 source file의 시작 위치를 뺀 뒤 projection 좌표로만 변환한다.

### 결정 3: shadow 실패는 기존 출력과 진단을 바꾸지 않는다

- **상황**: 이 단계에는 Evaluation IR 소비자가 없고 ProgramSyntax는 아직 호환성 검증용
  그림자 계층이다.
- **검토한 대안**: projection 오류를 사용자 오류로 반환하면 기존에 컴파일되던 입력이 새
  내부 계층 때문에 실패한다. 그림자 실행을 테스트에서만 하면 실제 컴파일 corpus의 host
  문맥을 지속적으로 검증하지 못한다.
- **선택과 근거**: 실제 codegen 진입에서 ProgramSyntax를 만들되 결과를 기존 emitter와
  분리했다. projection 불능은 사용자 진단으로 노출하지 않으며 전용 validator와 테스트가
  overlay 완전성·중복·span 범위를 검사한다.

## 작업 내역

- 2026-08-22: TASK-154 설계와 현재 AST/HIR/Core/codegen/verify 경계를 확인했다.
- 2026-08-22: Rust Enterprise Type Programming Guide와 연결된 타입·에러·소유권·계층
  가이드를 적용해 좌표와 node identity를 newtype으로 분리하기로 했다.
- 2026-08-22: `swc_ecma_visit`의 parent-path visitor를 추가하고 `ProgramSyntax`가 전체 SWC
  `Module`, projection, RL overlay를 함께 소유하도록 구현했다.
- 2026-08-22: Core `Adt`에도 원본 `NodeId`를 보존해 item projection이 문자열 재탐색 없이
  source map을 사용하도록 했다.
- 2026-08-22: expression·statement·item sentinel과 템플릿 interpolation 재귀 projection을
  구현하고 실제 codegen 진입에 shadow build를 연결했다.
- 2026-08-22: 호출 인자 parent path, 세 syntax category, 멀티바이트 좌표 분리, 순수 TS의
  byte-identical projection을 단위 테스트로 확인했다.
- 2026-08-22: 전체 포맷·clippy·테스트 게이트를 통과했다.

## 이슈 및 해결

### 이슈 1: 공개 범위가 다른 오류 payload 타입으로 clippy가 실패함

- **증상**: `ProgramSyntaxError`는 `pub(crate)`인데 variant가 담은 `RlNodeId`와
  `SourceByte`가 private여서 `private_interfaces` 경고가 발생했다.
- **원인**: 오류 타입의 가시성보다 payload newtype의 가시성이 좁았다.
- **해결**: 두 domain newtype을 `pub(crate)`로 맞춰 내부 API가 primitive 좌표로 후퇴하지
  않으면서 가시성 계약을 일치시켰다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`

## 결과

기존 parser/HIR/Core 의미 소유권을 바꾸지 않고, rl 구문이 놓인 전체 TypeScript host
문맥을 SWC AST parent path로 조회할 수 있는 shadow `ProgramSyntax` 계층을 추가했다.
기존 emitter는 그대로이며 출력·진단·mapping·passthrough 계약은 전체 회귀 테스트에서
변하지 않았다. 다음 단계는 이 parent context를 평가 프로토콜로 바꾸는 Evaluation IR이다.
