# TASK-159: 값 continuation과 direct-return lowering (Phase 4 1/n)

- **상태**: 완료
- **시작일**: 2026-08-22
- **완료일**: 2026-08-22
- **커밋**: —

## 목적

SWC가 파악한 TypeScript host 문맥을 실제 target lowering에 연결한다. 값 생성
연산의 결과 소비 방식을 공통 continuation으로 표현하고, `return` 문 전체를
소유할 수 있는 경우에는 `match`를 IIFE 없이 직접 제어 흐름으로 방출한다.

## 범위

- 포함: SWC `ReturnStmt` owner 범위, Evaluation IR의 typed value continuation,
  direct-return target lowering, 출력·타입·런타임·위치 회귀 테스트
- 제외: 대입문 continuation, 중첩 표현식의 임시 변수 승격, 다른 RL 구문의
  continuation 기반 최적화

## 의사결정

### 결정 1: continuation은 Evaluation IR의 명시적 계약으로 둔다

- **상황**: `match` 코드 생성기에서 `return` 문자열을 찾는 방식은 다른 값 생성
  구문에 재사용할 수 없고 TypeScript 구문 소유권도 보장하지 못한다.
- **검토한 대안**: Core 조각에서 `return` 접두사를 검색하는 방식은 작지만
  문자열 휴리스틱이 된다. SWC host owner와 Evaluation IR을 연결하면 구현량은
  늘지만 구문 문맥과 lowering 정책이 분리된다.
- **선택과 근거**: SWC가 증명한 owner와 Evaluation IR의 typed continuation을
  사용한다. direct lowering은 이 계약을 codegen이 소비할 때만 허용한다.

### 결정 2: ReturnStmt 전체를 target owner로 교체한다

- **상황**: Core IR에서는 `return `, `match`, `;`가 별도 source 조각이므로
  표현식만 바꾸면 원래 `return`을 남긴 채 statement 제어 흐름을 방출할 수 없다.
- **검토한 대안**: source 문자열에서 `return`을 검색하는 방식은 SWC 소유권을
  우회한다. SWC span을 원본 span으로 역투영하면 주석·괄호·세미콜론 유무를
  TypeScript parser가 결정한 범위대로 처리할 수 있다.
- **선택과 근거**: projection의 source 조각과 placeholder 양 끝에 mapping anchor를
  기록하고, SWC `ReturnStmt` span을 원본 owner span으로 변환했다. codegen은 owner
  중 RL 값 범위 바깥만 제외하고 직접 제어 흐름을 넣는다.

### 결정 3: 의미가 증명된 expression arm만 먼저 direct lowering한다

- **상황**: 기존 block arm은 IIFE 내부 label·break 의미를 사용하므로 곧바로
  함수 반환 제어 흐름으로 바꾸면 의미가 달라질 수 있다.
- **검토한 대안**: 모든 return 문맥을 즉시 전환하면 범위는 넓지만 block arm의
  별도 yield 계약이 필요하다. expression arm만 전환하면 SWC owner와 각 arm의
  `return`이 일대일로 대응한다.
- **선택과 근거**: `ArmBodyKind::Expression`인 decision만 활성화했다. 호출 인자,
  block arm, 기타 expression boundary는 typed plan에 남기되 기존 IIFE를 유지한다.

## 작업 내역

- 2026-08-22: TASK-159를 등록하고 direct-return lowering 범위를 확정했다.
- 2026-08-22: `ProgramSyntax`에 statement continuation과 SWC `ReturnStmt` owner
  span 역투영을 추가했다.
- 2026-08-22: `EvaluationFile`이 모든 값 region에 `Return` 또는
  `ExpressionBoundary` continuation을 부여하도록 했다.
- 2026-08-22: codegen이 lowering plan을 실제로 소비해 expression-arm
  `return match`를 IIFE 없는 block·switch/if 제어 흐름으로 방출하도록 했다.
- 2026-08-22: async guard·arm·tuple scrutinee, 세미콜론 없는 return, 중첩 호출
  boundary의 출력 회귀 테스트를 추가하고 전체 검증 게이트를 실행했다.

## 이슈 및 해결

### 이슈 1: 세미콜론 없는 return의 SWC 끝 위치가 placeholder 경계에 놓임

- **증상**: 복사된 source 조각만 선형 역투영하면 `return match ...` 뒤에
  세미콜론이 없는 경우 `ReturnStmt`의 끝 위치를 원본으로 변환할 수 없었다.
- **원인**: SWC statement 끝이 생성된 expression placeholder의 닫는 경계와
  같았고, 그 지점은 복사 source segment에 포함되지 않았다.
- **해결**: placeholder 전체의 시작·끝을 RL source span 시작·끝에 대응시키는
  endpoint anchor를 추가했다. 세미콜론 유무를 별도 문자열 규칙 없이 지원한다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`

## 결과

SWC host owner → Evaluation IR continuation → target lowering의 첫 실제 경로를
완성했다. 직접 반환하는 expression-arm `match`는 sync/async 모두 IIFE 없이
컴파일되며, 기존 expression boundary와 block arm은 그대로 유지된다.
