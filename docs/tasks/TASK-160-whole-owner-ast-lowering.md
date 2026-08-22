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

### 결정 3: 최소 owner와 타입화된 target capability를 사용한다

- **상황**: 호출 인자·매개변수·클래스 초기화를 boundary로 분류하면 legacy wrapper가
  계속 남고 whole-AST 소유 목적이 사라진다.
- **검토한 대안**: 이유가 있는 boundary closure를 유지하는 방식은 의미 보존에는
  보수적이지만 최적 TS lowering을 완료할 수 없다. owner 전체를 변환하면 reference와
  독립 실행 환경을 IR에서 직접 보존해야 하지만 구문별 fallback이 사라진다.
- **선택과 근거**: 모든 값은 안정적인 `HostOwnerId` 아래 `PlannedValue`가 되고,
  소비 방식은 `ValueSlotId`와 host continuation으로 표현한다. statement를 허용하는
  호출·선언·return owner는 owner transform에 들어간다. 표준 TypeScript가 statement를
  허용하지 않는 매개변수·class field는 분석 실패 fallback이 아니라 명시적인
  expression-boundary target capability를 사용한다.

### 결정 4: 중첩 region은 부모의 value continuation을 직접 상속한다

- **상황**: initializer match를 statement control flow로 구조화해도 arm 값이 다시
  match이면 자식만 expression emitter로 돌아가 IIFE가 남았다.
- **검토한 대안**: 중첩 match만 재귀 출력하면 현재 사례는 해결되지만 다른 value
  primitive가 들어올 때 같은 분기가 반복된다. 자식 값을 먼저 임시 변수로 만든 뒤
  부모에 대입하면 불필요한 join과 이름이 늘어난다.
- **선택과 근거**: expression-valued 자식 region은 부모의 `ValueDestination::{Expression,
  Assign}`
  continuation을 그대로 받는다. 자식의 모든 정상 leaf가 같은 continuation을 소비하므로
  별도 expression boundary가 필요 없고 CFG의 value edge와 target 제어 흐름이 일치한다.

### 결정 5: concise arrow는 별도 closure가 아니라 ArrowReturn continuation이다

- **상황**: `(...) => match ...`는 이미 함수 실행 환경 안에 있지만 일반 `Compose`로
  분류되어 또 다른 closure를 만들었다.
- **검토한 대안**: 출력 문자열에서 `=>`를 찾는 방식은 타입·주석·괄호 변화에 취약하다.
  arrow 전체를 SWC printer로 다시 출력하면 원본 source piece 보존 계약을 깨뜨린다.
- **선택과 근거**: SWC `ArrowFunctionBody::Expr` parent edge를 `ArrowReturn` continuation으로
  타입화한다. 기존 arrow의 expression 조각만 `block + value slot + explicit return`으로
  교체하고 prefix·parameter·type annotation·suffix는 원본 조각을 그대로 사용한다.

### 결정 6: value continuation은 destination과 wrapper의 합성이다

- **상황**: `result` region은 실패 값은 그대로 전달하지만 성공 값만 `Ok(value)`로
  감싸야 한다. 단순 `Assign(target)`만으로는 nested decision/result leaf가 이 차이를
  표현할 수 없다.
- **검토한 대안**: result 전용 slot emitter는 빠르지만 decision과 propagation이 다시
  서로 다른 제어 흐름을 갖는다. 성공 값을 별도 temporary에 모은 뒤 `Ok`를 만들면
  중첩 단계마다 불필요한 join이 생긴다.
- **선택과 근거**: `ValueContinuation`을 `ValueDestination::{Expression, Assign}`와
  `ValueWrapper::ResultOk`의 합성으로 정의했다. 실패 edge는 기존 continuation을 소비하고
  성공 edge만 wrapper를 추가한다. wrapper는 중첩될 수 있으므로 nested result도 타입화된
  동일 규칙을 사용한다.

### 결정 7: Sequence는 선행 효과와 최종 value를 분리하는 순차 region이다

- **상황**: result의 최종 match가 HIR `Sequence` 안에서 trivia·선행 statement와 함께
  보존되어 자식 continuation으로 합성되지 않았다.
- **검토한 대안**: 단일 expression만 든 sequence를 투명 처리하면 주석이 추가되거나
  선행 statement가 있는 순간 다시 wrapper가 생긴다.
- **선택과 근거**: 마지막 value statement 앞의 모든 statement를 순서대로 실행하고,
  마지막 value만 부모 continuation으로 전달한 뒤 후행 source trivia를 보존한다. 이는
  result 전용 처리가 아니라 모든 Core sequential value region의 실행 규칙이다.

### 결정 8: host return은 분기별 return이 아니라 명시적 value join을 사용한다

- **상황**: result의 실패·성공 edge를 각각 직접 return하면 실행 의미는 같지만 TypeScript가
  각 edge를 별도 표현식으로 검사한다. 기존 단일 result 표현식의 전체 contextual type과
  진단 경계가 분해되었다.
- **검토한 대안**: 진단 문자열을 result 전용으로 합치는 방식은 생성 형태에 의존한다.
  출력 IIFE를 유지하면 표현식 경계는 보존되지만 이번 구조 전환의 목적에 어긋난다.
- **선택과 근거**: 모든 host value에 충돌 없는 `ValueSlotId`를 할당한다. 구조화된 edge는
  slot에 값을 기록하고 하나의 원본 TypeScript return이 join 결과를 소비한다. slot 소비
  지점도 Core value의 source anchor를 상속하므로 전체 contextual type과 원문 진단 범위가
  함께 유지된다.

### 결정 9: host 평가 순서는 AST node의 ordered protocol로 합성한다

- **상황**: 호출 인자만 선형화하면 배열·객체·대입·시퀀스·단항식·template에서 같은
  IIFE 문제가 반복되고, 한 owner에 RL 값이 둘 이상 있으면 owner 전체를 구조화하지 못한다.
- **검토한 대안**: 구문별 출력 함수를 추가하면 현재 예제는 처리하지만 평가 순서와
  conditional 실행을 중복 구현하게 된다.
- **선택과 근거**: SWC node가 실제로 평가하는 child span을 `Ordered` protocol로 만들고,
  선행 source와 RL slot 의존성을 하나의 owner schedule로 합성한다. 같은 source 입력은
  owner 안에서 한 slot을 공유하며 여러 RL 값도 한 prelude에서 source 순서로 구조화한다.

### 결정 10: block arm의 return은 Core join exit로 낮춘다

- **상황**: block arm은 기존 익명 함수의 `return`에 의존해 match 값을 만들었으므로
  statement lowering에서 그대로 복사할 수 없었다.
- **검토한 대안**: block arm만 계속 expression boundary에 두면 동일한 Decision이 arm
  모양에 따라 다른 backend를 사용한다. 문자열로 `return`을 찾으면 중첩 함수의 return을
  구분하지 못한다.
- **선택과 근거**: decision projection이 arm 내부 TypeScript island를 SWC에 노출한다.
  SWC 함수 깊이로 현재 arm에 속한 `ReturnStmt`만 `HostExit`로 수집하고, target은 이를
  continuation assignment와 labeled break로 바꾼다. return 없는 완료 경로는 기존처럼
  `undefined` 값을 생산한다.

### 결정 11: statement 불가 owner는 이름 있는 expression boundary를 사용한다

- **상황**: parameter default와 class field initializer에는 표준 TypeScript statement를
  삽입할 위치가 없다. owner 밖으로 slot을 올리면 parameter scope, `this`, `arguments`,
  field 초기화 시점이 달라진다.
- **검토한 대안**: 매개변수·constructor를 전면 재작성하면 body declaration visibility,
  함수 `length`, derived `super`, field define semantics까지 별도 JS lowering이 필요하다.
  기존 익명 IIFE는 의미는 맞지만 출력과 번들러 경계를 계속 구문마다 만든다.
- **선택과 근거**: EvaluationOwner가 expression-only이면 파일당 하나의 hygiene된
  `$rl_expr<T>(run: () => T): T` target intrinsic을 선택한다. Core CFG callback은 원래
  평가 위치에서 실행된다. 이는 조용한 legacy fallback이 아니라 검증되는 target
  capability이며, 생성 IIFE는 0개다.

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
- 2026-08-22: decision leaf의 소비 방식을 `ValueDestination::{Expression, Assign}` 공통
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
- 2026-08-22: expression arm의 자식 `Decision`이 부모 `ValueDestination`
  continuation을 상속하도록 target structuring을 재귀화했다. nested match가 더 이상
  자체 expression IIFE를 만들지 않는다.
- 2026-08-22: SWC concise-arrow body를 `ArrowReturn`으로 분류하고 expression body를
  source-preserving block body로 구조화했다. 311줄 commerce 예제의 initializer, arrow,
  nested decision에서 생성 IIFE가 0개임과 Node runtime 결과를 확인했다.
- 2026-08-22: 변경 후 compile 출력 테스트 286건을 실행해 모두 통과했다.
- 2026-08-22: 최종 중간 게이트로 `cargo fmt --check`, `cargo clippy --all-targets --
  -D warnings`, `cargo test`를 실행했다. passthrough 56건, integration 77건, native
  typecheck 38건, emit mapping 15건을 포함한 전체 검증이 통과했다.
- 2026-08-22: `ValueContinuation = destination + wrappers` 타입을 도입했다. Decision leaf와
  Result propagation/success가 같은 continuation을 소비하며 `ResultOk` wrapper는 성공
  edge에만 합성된다.
- 2026-08-22: initializer, direct return, concise arrow, match arm 안의 `ResultRegion`을
  `do/while(false)` join 또는 직접 return 제어 흐름으로 낮췄다. await는 기존 async host에
  남고 별도 async closure를 만들지 않는다.
- 2026-08-22: Core `Sequence`를 선행 statement·최종 value·후행 source piece로 구조화했다.
  result의 최종 match는 각 leaf에서 바로 `Ok` wrapper와 parent slot을 소비한다.
- 2026-08-22: result 출력 테스트를 statement-region 계약으로 갱신하고 direct-return 및
  match-arm continuation 회귀 테스트를 추가했다. compile 테스트 288건이 통과했다.
- 2026-08-22: 모든 host return을 owner-scoped value slot의 단일 join으로 통합했다.
  match/result의 각 edge는 slot을 정의하고 원래 return statement가 한 번만 소비한다.
- 2026-08-22: join slot 소비 지점에 Core value anchor를 전파했다. IIFE 제거 뒤에도
  `match (input)`과 `n <- test()`의 기존 타입 진단 범위 및 전체 Result 문맥이 유지됨을
  native 진단 회귀 테스트 2건으로 확인했다.
- 2026-08-22: 새 join 출력에 맞춰 direct-return compile 테스트 4건을 갱신했다.
  compile 출력 테스트 288건이 통과했다.
- 2026-08-22: 최종 중간 게이트로 `cargo fmt --check`, `cargo clippy --all-targets --
  -D warnings`, `cargo test`를 실행했다. unit 153건, compile 288건, mapping 15건,
  integration 77건, native 38건, passthrough 56건을 포함한 전체 검증이 통과했다.
- 2026-08-22: `ResultRegion`과 `Decision` projection이 내부 TypeScript island를 재귀
  투영하도록 바꿨다. 중첩 initializer/result/match/pipeline은 같은 source-backed owner의
  부모 continuation을 다시 상속한다.
- 2026-08-22: 호출·멤버·생성·배열·객체·대입·시퀀스·단항·template의 SWC 평가 child를
  공통 ordered protocol로 확장했다. 선행 getter·callee·인자·computed key를 owner slot에
  한 번만 저장하고 원래 좌우 순서로 소비한다.
- 2026-08-22: 한 HostOwner의 여러 RL value를 하나의 schedule로 합쳤다. RL source 입력은
  `ValueSlotId` 의존성으로 연결하고 동일 source capture는 owner 안에서 공유한다.
- 2026-08-22: SWC가 block arm의 함수 깊이별 `ReturnStmt`를 `HostExit`로 수집한다. target은
  return expression을 부모 continuation assignment와 labeled break로 낮추며 중첩 함수의
  return은 원문으로 보존한다.
- 2026-08-22: parameter initializer와 class initializer를 expression-only owner로
  타입화하고 hygiene된 `$rl_expr` intrinsic을 파일당 한 번만 방출했다. 일반 owner는
  계속 statement slot/CFG를 사용하며 생성 self-invoked closure/IIFE 경로를 제거했다.
- 2026-08-22: parameter의 `arguments`·함수 `length`, class field의 `this`, member getter와
  `this` binding, 선행/후행 인자 순서, block arm nested return을 Node runtime으로 검증했다.
- 2026-08-22: tagged template의 tag를 일반 call reference와 같은 `mode + receiver` 입력으로
  모델링했다. member tag는 receiver를 한 번 평가하고 bound reference로 만든 뒤 interpolation
  slot과 결합한다.
- 2026-08-22: optional call 인자의 부분 statement 승격을 실험하고 TypeScript의 분기별
  definite-assignment 상관관계가 보존되지 않음을 확인했다. operation 전체를 재구성하기 전에는
  expression-only target capability를 유지하도록 구조화 가능성 판정을 명시했다.
- 2026-08-22: Core operation의 직접 자식은 host owner가 달라도 `Nested` region으로 배치하는
  경로를 추가했다. propagation 안의 중첩 RL 값은 부모의 early-return continuation을 소비한다.
- 2026-08-22: 최종 게이트에서 unit 156건, compile 292건, mapping 15건, integration 80건,
  native 38건, passthrough 56건을 포함한 전체 테스트와 fmt/clippy가 통과했다.

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

### 이슈 7: arrow replacement가 하나의 source piece 안에 있다고 가정함

- **증상**: `ArrowReturn` 분류와 rewrite plan은 만들어졌지만 출력은 기존 expression
  IIFE로 남았다.
- **원인**: HIR source preservation은 arrow prefix, RL expression, suffix를 별도 piece로
  유지하는데 owner source piece 하나가 RL span 전체를 포함한다고 가정했다.
- **해결**: owner 문자열을 다시 자르지 않고 Core expression을 출력하는 `emit_expr`
  경계에서 `ArrowReturnRewrite`를 소비했다. 기존 source piece 분할과 mapping을 유지한다.

### 이슈 8: 구조화된 result 내부의 ordinary statement owner가 projection에서 가려짐

- **증상**: 바깥 result는 statement region으로 전환되지만, 그 ordinary statement의
  initializer에 있는 중첩 result는 기존 expression path에 남는다.
- **원인**: ProgramSyntax projection이 바깥 `ResultRegion` 전체를 하나의 placeholder로
  바꾸므로 내부 TypeScript statement의 SWC parent path가 만들어지지 않는다.
- **해결**: `ResultRegion`과 `Decision`을 분석용 synthetic call region으로 projection하고
  ordinary statement·arm body는 원본 source segment로 재귀 투영했다. 바깥 marker는 실제
  host continuation을 유지하고, 안쪽 RL value는 같은 source-backed owner면 부모 region을
  상속한다.

### 이슈 9: 분기별 direct return이 기존 타입 진단 경계를 분해함

- **증상**: result IIFE를 제거한 뒤 오류 위치는 `<-` binding에 남았지만 진단이 전체
  `Result<number, InputError>` 불일치 대신 성공 payload의 `number/string` 불일치로
  축소되었다. 단일 join 도입 직후에는 마지막 slot 참조가 source anchor 없이 nearest
  위치로 보고되었다.
- **원인**: 하나의 source value가 여러 생성 return edge로 분해되었고, join slot 소비는
  합성 identifier라 원본 소유권을 자동으로 갖지 않았다.
- **해결**: Evaluation IR이 return value에도 slot을 할당하고 target이 모든 edge를 그
  slot로 합류시킨 뒤 원본 return에서 한 번 소비하도록 했다. slot 참조는 Decision이면
  Match anchor, ResultRegion이면 마지막 ResultBind anchor, Sequence이면 최종 value anchor를
  재귀 상속한다. 기존 두 native 진단 회귀 테스트가 수정 없이 통과했다.

### 이슈 10: decision island의 synthetic 연산이 거짓 평가 protocol을 만듦

- **증상**: arm 내부 중첩 RL과 `await`를 projection한 뒤 `UnmappedEvaluationSpan` 또는
  “await isn't allowed in non-async function” 내부 오류가 발생했다.
- **원인**: 분석용 `void` unary와 non-async synthetic arrow가 실제 source 평가 parent처럼
  수집되었다.
- **해결**: synthetic decision arrow가 Core의 async 사실을 상속하고, child는 별도 unary
  연산 없이 expression statement로 투영했다. marker call 자체는 protocol frame에서
  제외한다.

### 이슈 11: 같은 owner의 두 RL value가 서로의 source를 다시 출력함

- **증상**: constructor callee와 argument가 모두 match이면 빈 scrutinee와 원문 match가
  섞인 잘못된 TypeScript가 생성되었다.
- **원인**: owner당 값 하나를 가정했고 parenthesized callee span을 RL slot이 아닌 일반
  source input으로 분류했다.
- **해결**: transparent TS wrapper를 벗긴 reference value span을 protocol에 기록하고,
  owner의 모든 `PlannedValue`를 하나의 schedule로 합쳤다. source slot capture도 이름 있는
  `PlannedSourceSlot`으로 공유한다.

### 이슈 12: optional call 인자만 승격하면 TypeScript 타입이 달라짐

- **증상**: optional member call의 match 인자를 statement slot으로 승격하면 런타임 분기는
  맞지만 tsc가 인자 slot을 `T | undefined`로 추론했다.
- **원인**: 원래 구문은 callee가 nullish가 아닐 때만 인자를 평가한다. 부분 치환 출력에서는
  callee 조건과 이후 optional call 사이의 definite-assignment 관계를 TypeScript가 표현하지
  못한다.
- **해결**: `Conditional(OptionalCallArgument)`와 `MemberReference`의 조합은 부분 Compose
  대상에서 제외했다. 향후 conditional operation 전체가 result slot을 생산하는 target
  primitive로 모델링될 때만 statement CFG로 승격한다. direct optional reference의 조건은
  truthiness가 아니라 `!= null`로 기록했다.

### 이슈 13: opaque wrapper를 단순 sequence continuation으로 오인함

- **증상**: `try wrap(match (...))`가 `wrap({ switch ... })` 형태의 파싱 불가능한 TypeScript로
  방출되었다.
- **원인**: `Sequence`의 마지막 RL 값만 구조화 가능하면 주변 `Opaque` 조각의 내용과 무관하게
  부모 continuation을 직접 전달했다. 함수 호출의 prefix/suffix도 trivia처럼 취급되었다.
- **해결**: propagation 입력에서 주변 `Opaque` 조각이 실제로 공백뿐일 때만 continuation을
  직접 인라인한다. 실행 구문이 남은 sequence는 expression target으로 방출해 wrapper의 평가
  구조와 기존 진단 anchor를 보존한다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`

## 결과

진행 중.
