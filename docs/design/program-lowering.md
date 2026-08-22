# SWC 전체 프로그램 lowering 아키텍처

TASK-154의 규범 설계다. 이 문서는 특정 `match` 출력 모양을 바꾸는 제안이 아니다.
TypeScript 전체 평가 문맥과 rl Core IR을 하나의 프로그램 모델에서 결합하고, 모든 rl
구문을 공통 제어·데이터 흐름으로 낮추는 다음 컴파일러 계층을 정의한다.

## 1. 결론

rl 구문이 하나라도 확정된 파일은 전체 TypeScript 구조를 SWC AST로 파악한다. 기존
`SemanticFile → CoreFile`은 rl 의미의 단일 원천으로 유지하고, SWC AST와 Core IR을
결합한 **Evaluation IR**에서 평가 순서·스코프·효과·제어 흐름을 확정한다. 출력은 SWC
printer가 아니라 원본 조각과 생성 조각을 함께 보존하는 source-preserving printer가
맡는다.

SWC AST는 최적화 가능 여부를 분류하는 보조 도구가 아니라 실제 변환 골격이다. RL 값을
포함한 최소 실행 owner 전체를 Evaluation IR로 선형화하고, 결과 슬롯·`if`/`switch`·직접
`return`·대입·선언으로 TypeScript를 다시 구조화한다. `match`나 `result`뿐 아니라 모든
Core primitive가 이 경로를 사용하며 IIFE 제거는 전체 최적 lowering의 한 결과다.

호출의 Reference, 기본 매개변수 환경, 클래스 초기화, 단락·반복 문맥은 closure fallback
사유가 아니다. SWC parent path에서 평가 프로토콜을 만들고 필요한 만큼 owner를 확장한다.
Reference는 receiver·property·callee의 평가 순서와 `thisValue`를 명시적인 IR 연산으로
보존한다. 독립 실행 환경은 해당 함수·클래스 owner 안에서 별도 region으로 구조화한다.

근거:

- ECMAScript `EvaluateCall`과 `ArgumentListEvaluation`:
  https://tc39.es/ecma262/multipage/ecmascript-language-expressions.html#sec-evaluatecall
- rustc의 AST → HIR → THIR → MIR 단계와 MIR CFG:
  https://rustc-dev-guide.rust-lang.org/overview.html
- rustc의 owner 단위 AST → HIR lowering:
  https://rustc-dev-guide.rust-lang.org/hir/lowering.html
- Svelte의 parse → analyze → transform 파이프라인:
  https://github.com/sveltejs/svelte/blob/main/packages/svelte/src/compiler/index.js
- SWC의 전체 AST visitor와 parent-path transform:
  https://rustdoc.swc.rs/swc_visit/index.html

## 2. 바뀌지 않는 계약

이 전환은 내부 아키텍처 변경이다. 다음은 호환성 검증 항목이 아니라 설계 불변조건이다.

- rl이 없는 유효한 TypeScript는 입력과 출력이 바이트 단위로 같다.
- rl이 있는 파일에서도 rl span 밖의 사용자 바이트는 한 번씩, 변경 없이, 원래 순서로
  출력된다. 생성 코드는 삽입할 수 있지만 사용자 코드를 재포맷하지 않는다.
- 사용자 코드의 평가 횟수, 좌우 평가 순서, conditional 실행 여부, `this`, `super`,
  optional-chain 단락, getter/proxy 관측, throw와 await 순서를 바꾸지 않는다.
- 기존 rl 진단의 코드·문안·primary span·owner·정렬 순서를 유지한다.
- 기존 `EmitMapping`, `EmitAnchor`, scrutinee/payload mark의 의미를 유지한다.
- SWC projection의 파싱 오류나 내부 lowering 실패를 새로운 사용자 진단으로 노출하지
  않는다.
- rlc가 만든 TypeScript 때문에 TypeScript 진단이 새로 생기면 최적화 실패가 아니라
  컴파일러 버그로 취급한다.

## 3. 전체 파이프라인

```text
source bytes
  │
  ├─ lossless rl parse ─→ HIR ─→ resolution / typed facts / flow facts
  │                                      │
  │                                      └─→ SemanticFile ─→ CoreFile
  │
  └─ syntax projection ─→ SWC TypeScript Program
                              │
                  parent / scope / evaluation / effect tables
                              │
          SWC Program + CoreFile + OriginTable
                              │
                       Evaluation IR
                              │
               normalize → verify → optimize
                              │
                    Structured TypeScript IR
                              │
               source-preserving printer
                              │
       TypeScript + mappings + anchors + semantic marks
```

기존 lossless parser를 SWC로 교체하지 않는다. SWC는 rl 문법을 모르며, 기존 parser는
유효한 TypeScript를 잘못 청구하지 않는 언어 경계와 recovery 계약을 이미 소유한다.
두 front-end는 경쟁하지 않고 서로 다른 사실을 제공한다.

## 4. Syntax projection과 프로그램 소유권

### 4.1 Projection은 출력물이 아니라 SWC 입력이다

lossless parser가 확정한 rl node를 syntax category가 같은 sentinel로 치환한다.

- expression node → sentinel expression
- statement node → sentinel statement
- item node → sentinel declaration
- modifier node → 동일 위치를 추적할 수 있는 trivia

각 sentinel은 `RlNodeId`와 projection span을 가지며 `ProjectionMap`이 원본 span으로
되돌린다. 길이를 억지로 같게 만들지 않는다. 별도 좌표 변환을 사용해야 중첩 rl 구문과
멀티바이트 입력에서도 위치가 정확하다.

projection은 TypeScript checker나 사용자에게 노출되지 않는다. SWC parse diagnostic은
프로그램 모델의 recovery 사실일 뿐이며, 최종 사용자 오류는 현재 rlc/TypeScript 진단
경계를 그대로 따른다.

### 4.2 SWC AST만 단일 원천으로 삼지 않는다

`swc_ecma_ast::Expr`에는 rl variant를 추가할 수 없다. 프로그램 소유 모델은 다음의
합성 자료다.

```text
ProgramSyntax
  source: SourceFile
  projection: ProjectionMap
  typescript: swc_ecma_ast::Program
  rl_overlay: RlNodeId → CoreNodeId
  parents: SwcNodeId → ParentEdge
  scopes: SwcNodeId → ScopeId
  origins: OriginId → SourceOrigin
```

SWC AST는 TypeScript 구조와 parent context를 소유한다. Core IR은 rl 의미를 소유한다.
`rl_overlay`만 두 세계를 연결하며, SWC visitor가 다시 `match` 문자열을 찾거나 Core
lowering이 TypeScript 문맥을 추측하지 않는다.

### 4.3 이름은 scope identity로 생성한다

문자열 검색으로 `$rl_m` 충돌을 피하지 않는다. SWC scope tree와 기존 resolver를 결합해
`GeneratedLocalId`를 할당하고, target 직전에 hygiene된 spelling을 정한다. 생성 이름의
identity와 출력 철자를 분리한다.

## 5. Evaluation IR

Core IR은 rl 표면을 `Decision`, `Propagate`, `Apply`, `Adt`로 이미 정규화한다. 새
Evaluation IR은 이 연산을 TypeScript host context에 배치한다. 특정 rl 문법별 방출기를
추가하는 계층이 아니다.

```text
EvalRegion
  owner: EvaluationOwner
  entry: BlockId
  blocks: [EvalBlock]
  result: Option<ValueId>
  origin: OriginId

EvalBlock
  statements: [EvalStatement]
  terminator: EvalTerminator

EvalStatement
  Evaluate { source, output, mode }
  Assign { place, value }
  Bind { local, value }
  Core { operation }

EvalTerminator
  Goto | Branch | Switch | Return | Throw | Break | Continue | Complete
```

`mode`는 최소한 `Value`와 `Reference`를 구분한다. member call, `super`, private field,
assignment target은 단순 값으로 바꾸면 의미가 달라진다. 이 구분이 없는 expression
lifting은 `this` 또는 setter/getter 순서를 깨뜨릴 수 있으므로 허용하지 않는다.

Evaluation IR은 완전한 TypeScript MIR가 아니다. rl node를 포함하는 최소 owner만
구조화하며, rl과 관계없는 subtree는 `SourceExpr`/`SourceStmt`와 원본 span으로 유지한다.
다만 그 subtree의 평가 프로토콜과 효과는 분석한다.

## 6. 문맥은 syntax case가 아니라 평가 프로토콜로 분류한다

각 SWC parent edge는 다음 프로토콜을 합성한다.

- **순서**: eager left-to-right, conditional, repeated, deferred
- **값 종류**: value, reference, assignment target, pattern
- **제어 경계**: statement owner, function/parameter, class initialization, async/generator
- **단락**: `&&`, `||`, `??`, conditional, optional chain
- **관측 효과**: call, read, write, throw, suspend, allocate

새 ECMAScript/TypeScript AST node가 들어오면 이름으로 예외 처리하지 않는다. 해당 node의
평가 프로토콜과 owner structuring을 구현해야 한다. 알 수 없는 node를 legacy wrapper로
우회하지 않으며, 전체 AST corpus와 validator가 프로토콜 누락을 내부 오류로 검출한다.

Svelte에서 가져올 원칙은 parse/analyze/transform 분리, node별 scope map, 충돌 없는 이름
생성, transform state에 statement를 예약하는 방식이다. Svelte의 runtime thunk나
syntactic purity 판정을 그대로 가져오지는 않는다. rl은 TypeScript 실행 의미와
tree-shaking 안전성을 보존해야 하기 때문이다.

## 7. 공통 lowering 알고리즘

### 7.1 owner 형성

rl node에서 parent chain을 올라가면서 원래 실행 빈도와 스코프를 보존하는 가장 작은
`EvaluationOwner`를 찾는다. module/function/block/static block처럼 statement를 넣을 수
있는 owner가 일반적인 종료점이다. parameter initializer나 일부 class/decorator 위치처럼
독립적인 실행 환경은 별도 owner다.

### 7.2 continuation 기반 선형화

표현식을 단순히 `prelude + value`로 나누지 않는다. conditional/repeated 실행을 보존하기
위해 "이 값으로 다음 계산을 계속한다"는 continuation으로 lowering한다.

```text
lower(expr, continuation, evaluation protocol) → EvalRegion
```

이 방식에서는 `a && match ...`의 match block이 오른쪽 branch 안에 남고, loop 조건의
match는 반복 block 안에 남는다. `await` 경계를 넘는 hoist도 생기지 않는다.

### 7.3 직접 제어 흐름 선택

`Decision`이 값을 생산하면 result local을 하나 만들고 각 leaf가 그 local에 할당한 뒤
continuation으로 합류한다. owner가 곧 `return`이면 result local 없이 각 leaf를 직접
`return`할 수 있다. `Propagate`도 같은 exit edge를 공유하며 `result` 전용 함수 경계를
기본으로 만들지 않는다.

```ts
const value = match (input) { A(x) => f(x), B => 0 };
```

개념적인 target은 다음과 같다. 실제 printer는 원본 조각을 보존한다.

```ts
let $rl_value;
const $rl_subject = input;
switch ($rl_subject.kind) {
  case "A": $rl_value = f($rl_subject.x); break;
  case "B": $rl_value = 0; break;
}
const value = $rl_value;
```

이는 `match` 전용 rewrite가 아니다. 값을 생산하는 모든 `EvalRegion`이 같은 result-local
규칙을 쓴다.

### 7.4 실행 환경은 owner protocol로 구조화한다

평가 환경이 달라지는 위치는 closure 생성 사유가 아니다. parameter, class initializer,
static block, async/generator는 각각 독립 `EvaluationOwner`를 형성하고 해당 환경 안에서
statement slot과 CFG를 만든다. Reference도 값으로 강제 materialize하지 않고
`Reference` 입력으로 다음 call/member 연산까지 전달한다.

지원하지 않는 parent edge나 실행 환경을 만나면 legacy IIFE로 우회하지 않는다.
Evaluation IR validator가 누락된 protocol을 내부 컴파일러 오류로 검출한다. 따라서
backend의 유효한 상태에는 `BoundaryClosure`나 이유 코드가 존재하지 않는다.

## 8. 전체 rl 표면의 공통 배치

| Core primitive | rl 표면 | Evaluation IR 동작 |
|---|---|---|
| `Decision` | `match`, `if let`, `let-else` | branch/switch, bind, result join 또는 exit |
| `Propagate` | `try`, `result` binding | 단일 평가, 실패 edge, success payload bind |
| `Apply` | pipe, `flow` | reference-aware call graph와 좌우 평가 순서 |
| `Adt`/source edit | enum, import, `val`, template | 구조화 선언 또는 국소 source edit |

optimizer는 `match`라는 단어를 보지 않는다. `Decision`의 branch, `Propagate`의 exit,
`Apply`의 call처럼 공통 primitive만 본다. 새 rl 문법이 기존 primitive로 낮아지면 프로그램
분석과 target structuring을 다시 구현하지 않는다.

## 9. 효과 분석과 최적화

효과는 `pure/impure` boolean 하나로 두지 않는다.

```text
Effects
  may_read_mutable
  may_write
  may_call
  may_throw
  may_suspend
  may_allocate
  requires_reference
```

사용자 함수 호출, member access, computed key는 기본적으로 효과가 있을 수 있다. getter,
Proxy, TDZ, 사용자 정의 함수 때문에 TypeScript 타입만으로 purity를 증명할 수 없다.
tsgo는 타입과 심볼 사실을 제공하지만 runtime effect oracle로 쓰지 않는다.

최적화 순서는 다음과 같다.

1. block/branch 합치기와 불필요한 jump 제거
2. 평가 횟수와 순서가 그대로인 temporary 제거
3. statement가 필요 없는 `Decision`의 conditional expression 선택
4. 바로 `return`/`throw`로 이어지는 result local 제거
5. effect-free로 증명된 미사용 region 제거
6. proof가 있는 경우에만 PURE annotation 또는 bundler hint 허용

모든 IIFE에 PURE를 붙이는 방식은 채택하지 않는다. arm, scrutinee, getter, fallback throw의
효과를 지울 수 있기 때문이다. 출력 모양이 자연스러워져 번들러 분석이 쉬워지는 것과
프로그램이 순수하다는 것은 별개의 판단이다.

## 10. source-preserving target과 진단 provenance

SWC printer로 파일 전체를 다시 출력하지 않는다. target node는 다음 둘 중 하나다.

```text
TargetPiece
  Source { original_span, origin }
  Generated { node, origin }
```

변경하지 않은 TypeScript subtree와 trivia는 `Source`로 복사한다. 변환된 owner도 원본의
토큰·주석 조각을 가능한 한 그대로 재사용하고 필요한 glue만 `Generated`로 삽입한다.
printer는 모든 원본 non-RL span이 정확히 한 번, 원래 순서로 출력됐는지 검증한다.

모든 생성 node는 `OriginId`를 가진다.

```text
SourceOrigin
  Exact(original span)
  Construct(primary span, owner span, anchor kind)
  Synthetic(parent origin, reason)
```

- `Exact`는 기존 양방향 `EmitMapping`이 된다.
- `Construct`는 기존 단방향 `EmitAnchor`가 된다.
- `Synthetic` 진단은 가장 가까운 non-synthetic parent origin으로 올라간다.

projection span, SWC span, generated TypeScript span을 사용자 좌표로 직접 취급하지 않는다.
진단 정렬과 primary span은 기존 `SemanticFile` 결과를 사용하며, target lowering은 새 rl
진단을 만들지 않는다.

## 11. validator

각 단계는 다음을 검증한다.

- `validate_projection`: 모든 sentinel이 하나의 rl node에 대응하고 span 변환이 왕복됨
- `validate_program_syntax`: overlay node의 syntax category와 parent edge가 일치함
- `validate_eval`: 모든 block이 종료되고 value가 모든 정상 경로에서 한 번 정의됨
- `validate_order`: source evaluation ordinal과 conditional/repeated 영역이 보존됨
- `validate_reference`: reference-required operand가 value temporary로 강등되지 않음
- `validate_origin`: 모든 generated node가 source origin 또는 parent origin을 가짐
- `validate_source_preservation`: non-RL source span이 한 번씩 원래 순서로 출력됨
- `verify_output`: 최종 TypeScript가 SWC parser를 통과함

validator 실패는 사용자 오류가 아니라 internal compiler error다. release 경로에서
침묵하는 잘못된 최적화나 legacy backend 우회를 내보내지 않도록 모든 build에서 즉시
실패시킨다. 순수 TypeScript 또는 host lowering이 필요 없는 source edit만 타입화된
capability 판정으로 분석 대상에서 제외한다.

## 12. TypeScript 엔진의 역할

SWC와 tsgo의 책임을 섞지 않는다.

- SWC: TypeScript syntax AST, parent path, scope 형성, 평가 문맥
- rlc Semantic/Core: rl 이름·패턴·소진성·flow·의미 primitive
- tsgo: 프로젝트 타입·심볼·narrowing 사실과 최종 TypeScript 진단
- Evaluation IR: runtime 평가 순서와 target 제어 흐름

타입 정보가 없어도 correctness는 유지되어야 한다. tsgo 질의 실패는 최적화 기회를
줄일 수 있지만 다른 코드를 만들거나 기존 rl 진단을 없애면 안 된다.

## 13. 전환 계획

### Phase 0 — 호환성 기준선

현재 전체 compile output, mapped emit, 진단, runtime trace corpus를 고정한다. 평가 순서,
getter/proxy, `this`, throw, short-circuit, async, default parameter, class initialization,
decorator, optional chain, template/JSX와 중첩 rl 구문을 포함한다.

### Phase 1 — shadow ProgramSyntax

projection, SWC AST, parent/scope/origin table을 만들되 출력에 사용하지 않는다. rl node와
sentinel의 완전 대응 및 원본 span 왕복만 검증한다.

### Phase 2 — shadow Evaluation IR

모든 Core primitive를 Evaluation IR로 낮추고 validator를 실행한다. 기존 backend와 함께
돌리되 release 출력은 바꾸지 않는다. syntax별 emitter가 아니라 공통 primitive coverage가
100%여야 다음 단계로 간다.

### Phase 3 — source-preserving target

Structured TypeScript IR과 printer를 도입한다. 먼저 기존 출력과 byte-identical한 target을
만들어 mapping·anchor·진단 경계를 검증한다.

### Phase 4 — whole-owner structuring과 최적화

모든 HostOwner를 continuation·ANF·CFG로 구조화한다. Reference, parameter, class,
async/generator를 공통 평가 프로토콜로 낮추고 결과 슬롯·temporary·분기를 효과 정보에
따라 제거한다. 특정 테스트 하나를 통과시키기 위한 syntax 분기는 허용하지 않는다.

legacy emitter는 전체 owner corpus가 새 경로에서 동일하게 동작한 뒤 원자적으로 삭제한다.

첫 cutover는 `Initialize` continuation이다. 하나의 expression-arm `Decision`을 소비하는
variable initializer owner를 `value slot + statement decision + rewritten initializer`로
구조화한다. slot 이름은 SWC가 수집한 identifier 집합과 충돌하지 않으며, decision leaf는
`Expression | DirectReturn | Assign` 공통 continuation을 사용한다. 이 cutover의 capability가
증명되지 않은 owner를 legacy로 되돌리는 것이 아니라, 아직 전환되지 않은 primitive가
기존 target path에 남아 있는 과도기 상태로 명시한다.

## 14. 완료 기준

- 모든 rl 구문이 `ProgramSyntax + CoreFile → Evaluation IR` 한 경로를 사용한다.
- backend에 `match`/`result` 전용 IIFE 선택 로직이 없다.
- 모든 Core primitive가 host owner에 맞는 TypeScript 제어 흐름·표현식·선언으로 출력된다.
- backend에는 RL 구문을 감싸는 생성 closure/IIFE 경로가 없다.
- 기존 rl 진단 snapshot과 source 위치가 바이트 단위로 같다.
- pure TypeScript passthrough와 non-RL source-piece 보존 검사가 통과한다.
- runtime trace corpus에서 평가 횟수·순서·`this`·throw·await가 기존과 같다.
- SWC parse 검증과 tsgo 타입 검증에 새 오류가 없다.
- Rolldown/Rollup/esbuild fixture에서 effect-free 미사용 region은 제거되고 effectful region은
  유지된다.

이 기준은 “IIFE 문자열이 안 보인다”에 그치지 않는다. 평가 프로토콜·효과·continuation을
근거로 불필요한 wrapper와 temporary를 만들지 않는 컴파일러 구조를 요구한다.
