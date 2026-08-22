# TASK-160: SWC whole-owner 기반 RL→TS 최적 lowering

- **상태**: 진행 중
- **시작일**: 2026-08-22
- **완료일**: —
- **커밋**: —

## 목적

SWC 전체 AST를 실제 변환 골격으로 사용한다. 모든 RL 구문을 포함한 TypeScript
owner를 평가 순서대로 구조화하고, 각 Core primitive를 host 문맥에 가장 자연스러운
TypeScript 제어 흐름·표현식·선언으로 낮춘다. IIFE 제거는 이 최적 lowering의 한
결과로 취급한다.

## 범위

- 포함: SWC owner identity와 span, host expression 선형화, 공통 value slot
  continuation, 단락·반복·호출 reference 보존, `Decision`·`Propagate`·`Apply`·
  `Adt`·source edit의 TS-native lowering, source mapping과 기존 진단 보존,
  불필요한 wrapper/helper/temporary 제거 validator
- 제외: 사용자 TypeScript가 원래 작성한 IIFE 변경, 출력 포매팅 전면 변경,
  RL 언어 표면 변경

## 의사결정

### 결정 1: SWC AST는 분류기가 아니라 host rewrite와 최적화의 단일 골격이다

- **상황**: TASK-159는 SWC가 직접 return을 증명한 경우만 Core 조각을 바꿨다.
  이 방식은 호출 인자·선언 initializer·단락 표현식마다 예외가 늘어난다.
- **검토한 대안**: 위치별 최적화는 작게 적용할 수 있지만 완전한 AST 소유의
  목적을 달성하지 못한다. owner 전체를 선형화하면 구현량은 늘지만 모든 RL
  값 구문이 동일한 continuation 규칙을 사용한다.
- **선택과 근거**: SWC가 찾은 최소 실행 owner 전체를 Evaluation IR로 낮추고,
  target은 `prelude + value slot + rewritten host`를 기본 형태로 사용한다. 이후
  효과·사용 횟수·continuation을 근거로 slot과 temporary를 제거하거나 직접
  `return`·대입·분기로 합친다. Core primitive별 wrapper 선택은 허용하지 않는다.

### 결정 2: projected node와 source-backed owner를 별도 origin으로 관리한다

- **상황**: statement/item placeholder가 SWC 내부에 가짜 statement를 만들기 때문에
  가장 가까운 AST statement가 항상 원본 변환 owner인 것은 아니다.
- **검토한 대안**: statement 구문만 source span으로 fallback하면 해당 구문 전용
  예외가 된다. 모든 projected span을 선형 역투영하면 길이가 다른 placeholder 내부
  위치를 거짓 source 위치로 만들게 된다.
- **선택과 근거**: projection segment를 `Copied | Placeholder` origin으로 타입화하고,
  AST owner stack에서 원본으로 완전히 역투영되는 가장 안쪽 owner를 선택한다. 이
  규칙은 expression·statement·item에 동일하게 적용된다.

### 결정 3: boundary 대신 최소 owner와 값 target을 사용한다

- **상황**: 호출 인자·매개변수·클래스 초기화를 boundary로 분류하면 legacy wrapper가
  계속 남고 whole-AST 소유 목적이 사라진다.
- **검토한 대안**: 이유가 있는 boundary closure를 유지하는 방식은 의미 보존에는
  보수적이지만 최적 TS lowering을 완료할 수 없다. owner 전체를 변환하면 reference와
  독립 실행 환경을 IR에서 직접 보존해야 하지만 구문별 fallback이 사라진다.
- **선택과 근거**: 모든 값은 안정적인 `HostOwnerId` 아래 `PlannedValue`가 되고,
  소비 방식만 `Return | Slot(ValueSlotId)`으로 표현한다. 호출·매개변수·클래스도
  `Compose` continuation으로 owner transform에 들어간다.

## 작업 내역

- 2026-08-22: TASK-160을 등록하고 SWC whole-owner cutover를 시작했다.
- 2026-08-22: projection origin을 `Copied | Placeholder`로 분리하고 source-backed
  최소 owner 선택을 일반화했다.
- 2026-08-22: `HostOwnerId`, owner별 root 집합, source 순서의 `PlannedValue`,
  충돌 없는 `ValueSlotId`를 Evaluation IR 계약에 추가했다.
- 2026-08-22: `BoundaryReason`을 제거하고 모든 host 위치를
  `HostContinuation::{Return, Discard, Compose}`로 통합했다.
- 2026-08-22: SWC 실제 연산자와 child span에서 단락·삼항·호출·멤버·생성·태그
  템플릿·suspend 평가 프로토콜을 만들었다. 문자열이나 parent-kind 추측은 사용하지
  않았다.
- 2026-08-22: 선행 TypeScript 입력을 `Value | Reference`로 구분하고, 같은 owner의
  앞선 RL 입력을 원본 source가 아닌 `ValueSlotId` 의존성으로 정규화했다.
- 2026-08-22: protocol span이 원본 origin으로 역투영되지 않으면 단계를 생략하지 않고
  `UnmappedEvaluationSpan` 내부 오류로 실패하도록 했다.
- 2026-08-22: 중간 검증으로 `cargo fmt --check`, `cargo clippy --all-targets --
  -D warnings`, `cargo test`를 실행했다. 전체 테스트가 통과했다.
- 2026-08-22: codegen의 `.ok().unwrap_or_default()` 분석 실패 우회를 제거했다. host
  lowering이 필요한 Core 파일은 ProgramSyntax/Evaluation IR 오류를 내부 컴파일러 오류로
  처리하고, source edit만 있는 파일은 타입화된 `requires_host_lowering` 판정으로 분석을
  생략한다.
- 2026-08-22: `try`의 의미 span과 전체 source owner를 HIR/Core에서 분리했다. 선언형과
  bare 형식 모두 완전한 statement owner로 projection되며 기존 진단·anchor는 기존
  `try <expr>` span을 계속 사용한다.
- 2026-08-22: SWC `VarDeclarator`의 initializer edge를 `Initialize` continuation으로
  타입화했다. owner가 하나의 expression-arm `Decision` 값을 초기화하는 경우 공통
  value slot을 만들고, statement 제어 흐름으로 값을 할당한 뒤 원본 initializer만
  slot 참조로 바꾸는 첫 whole-owner target 전환을 적용했다.
- 2026-08-22: decision leaf의 소비 방식을 `Expression | DirectReturn | Assign` 공통
  continuation으로 분리했다. switch와 guarded if-chain은 같은 구조화 함수를 사용하며,
  initializer 전환은 더 이상 별도 match 출력 template을 갖지 않는다.
- 2026-08-22: SWC가 수집한 전체 TypeScript identifier 집합을 Evaluation IR에 전달하고
  생성 value slot을 충돌 없이 할당했다. source-preserving rope는 source chunk 경계가
  아닌 임의의 원본 byte 위치에 owner prelude를 삽입할 수 있도록 event sweep으로
  일반화했다.
- 2026-08-22: compile 출력 284건과 match guard runtime, exhaustive match typecheck,
  emit mapping 15건을 중간 검증했다. whole variable initializer의 expression-arm match는
  IIFE 없이 방출되고 기존 의미·mapping 검사가 통과했다.
- 2026-08-22: `ParentCollector`의 익명 3중 결과를 `CollectedProgramSyntax` 단계 타입으로
  바꿨다. 최종 검증으로 `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test`를 실행했고 전체 게이트가 통과했다.
- 2026-08-22: main 작업 트리를 `cargo install --path . --force`로 전역 재설치했다.
  enum과 variable-initializer match 예제를 설치된 `~/.cargo/bin/rlc`로 변환해 IIFE 없는
  slot/switch 출력을 확인했고, 생성된 TypeScript를 Node.js로 실행해 결과를 검증했다.

## 이슈 및 해결

### 이슈 1: placeholder 내부 가짜 statement가 owner로 선택됨

- **증상**: statement/item RL 구문이 포함된 전체 surface 테스트에서
  `MissingOverlay`가 발생했다.
- **원인**: placeholder가 만든 내부 SWC statement span은 원본 source와 선형
  대응하지 않는데 가장 가까운 owner 하나만 저장했다.
- **해결**: 전체 owner stack을 보존하고 origin mapping이 성립하는 가장 안쪽
  source-backed owner를 선택하도록 일반화했다.

### 이슈 2: Evaluation IR 생성 실패가 legacy emitter로 조용히 우회됨

- **증상**: ProgramSyntax 또는 Evaluation IR 오류가 빈 `LoweringPlan`으로 바뀌어 기존
  emitter가 계속 실행됐다.
- **원인**: shadow 단계에서 사용하던 `.ok().unwrap_or_default()` 연결이 whole-owner
  전환 뒤에도 남아 있었다.
- **해결**: host lowering 필요 여부를 Core 구조에서 판정하고, 필요한 파일의 분석 실패는
  내부 컴파일러 오류로 중단하도록 변경했다. RL owner가 없는 파일만 분석하지 않는다.

### 이슈 3: 선언형 try의 의미 span만 투영되어 TS owner가 불완전해짐

- **증상**: `const n = try value;`가 projection에서 `($rl_syntax_expr) return n;`처럼
  선언 prefix를 잃었고 SWC가 다음 statement에서 parse error를 냈다.
- **원인**: 진단용 `try <expr>` span을 전체 TypeScript source owner로도 사용했다.
- **해결**: AST→HIR→Core에 별도 `TryOwner` identity를 전달했다. projection은 전체
  statement owner를 사용하고 진단·mapping은 기존 의미 span을 유지한다.

### 이슈 4: owner prelude가 HIR source chunk 경계에서만 삽입됨

- **증상**: enum이나 선행 공백 뒤의 variable initializer를 전환하면 value slot 선언이
  출력되지 않고 rewritten initializer만 남았다.
- **원인**: owner 시작 byte가 rope의 `Source` piece 시작과 같다는 가정을 사용했다.
  HIR source piece는 여러 TypeScript owner를 포함할 수 있으므로 그 가정이 성립하지 않았다.
- **해결**: source insertion과 exclusion을 원본 byte 위치의 event로 합성했다. 하나의
  source piece 내부에서도 cursor를 분할해 prelude를 정확한 owner 앞에 삽입한다.

### 이슈 5: guarded assignment leaf의 join이 false guard에도 실행됨

- **증상**: guarded if-chain에서 guard가 거짓이어도 뒤따르는 무조건 `break`가 실행되어
  다음 arm으로 fallthrough하지 못했다.
- **원인**: leaf의 assignment만 guard body에 넣고 assignment continuation의 join을
  guard 밖에 출력했다.
- **해결**: guarded assignment의 `assignment + break`를 하나의 block으로 묶었다.
  guard가 참인 경로만 join하고 거짓 경로는 다음 arm을 계속 검사한다.

### 이슈 6: collector 결과의 익명 튜플이 단계 계약을 숨김

- **증상**: `cargo clippy --all-targets -- -D warnings`가 `type_complexity`로 실패했다.
- **원인**: overlay, owner, occupied identifier를 익명 3중 튜플로 반환했다.
- **해결**: 세 결과를 이름 있는 `CollectedProgramSyntax`로 묶어 수집 단계의 출력 계약을
  명시했다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`

## 결과

진행 중.
