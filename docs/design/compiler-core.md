# 컴파일러 중심부 — rl 구문의 rustc 수준 프런트엔드 전환 설계

TASK-119의 설계 기록이다. **제안이 아니라 채택된 전환 계획**이며, 각 페이즈가
구현 태스크로 완료될 때마다 규범 내용은 `docs/reference/`로 옮긴다.

이 문서는 `rust-parity-analysis.md`(TASK-101)의 후속이다. 그 문서가 "격차가
어디인가"를 답했다면, 이 문서는 "그 격차를 없애는 컴파일러 구조가 무엇인가"를
답한다. 목표를 한 줄로:

> 현재의 lossless parser, pattern analysis, TypeScript backend, mapped
> codegen, Project/Snapshot을 **유지한 채**, 그 사이에
> `HIR → resolution → typed facts → flow → structured diagnostics`라는
> 명확한 컴파일러 중심부를 세운다. 새 컴파일러를 옆에 만들지 않는다.

"rustc 수준"의 뜻: LLVM·borrow checker·trait solver의 복제가 아니라, rl이
추가하는 구문(`enum`·pattern·`match`·`try`·let-else·`if let`·`result`·
`val`·`|>`/`flow`)에 대해 rustc가 자기 구문에 제공하는 것과 같은 **일관된
컴파일러 모델**을 제공하는 것이다: lossless parsing, 안정된 node/symbol
identity, 선언 수집과 이름 해석, TypeScript 타입 정보를 쓰는 typed semantic
analysis, usefulness/exhaustiveness, 최소 control-flow 분석, 구조화된 다중
진단, 컴파일러 소유의 에디터 semantic, Project/Snapshot 증분 query, 검증된
IR만 소비하는 codegen. TypeScript의 일반 타입 추론·overload·generic
inference·assignability는 계속 TypeScript(tsgo)가 담당한다 — rlc는
TypeScript 타입 체커를 재구현하지 않는다.

---

## 1. 지켜야 할 계약 (변경 불가)

1. 모든 유효한 TypeScript는 유효한 rl이고, rl 구문이 아닌 부분은
   byte-for-byte 통과한다 (`CLAUDE.md` 계약 1).
2. 파서는 무오류(infallible) 구조 파서로 유지한다. 완전히 파싱된 rl 구문만
   AST로 올린다.
3. 생성물은 runtime/type trick 없는 plain TypeScript다 (계약 2).
4. tsgo 세부사항은 `src/typescript/native.rs`·`service.rs` 밖으로 새지
   않는다 — `backend.rs`의 Query/Answers seam이 경계다.
5. CLI·server·editor는 최종적으로 동일한 engine semantic result를 소비한다.
6. 기존 `compile()` 공개 API와 현재 출력 형태는 마이그레이션 기간 동안
   호환을 유지한다.

## 2. 현재 구현 전면 검토 — 보존과 결함

### 2.1 보존하고 확장하는 자산 (교체 금지)

| 자산 | 이유 |
|---|---|
| `src/ast.rs` — lossless AST, 절대 byte span | 통과 계약의 구현체 |
| `src/parser/*` — 무오류 구조 파서 | 계약 2번 항목 그 자체 |
| `src/analysis/mod.rs` — `PatternSite`/`MatchAnalysis`/typed coverage | rustc THIR typed pattern에 대응 (TASK-096) |
| `src/analysis/usefulness.rs` — constructor matrix usefulness | Maranget 알고리즘, rustc와 동형 (TASK-103) |
| `src/codegen/*` + `EmitMapping`/`EmitAnchor`/`ScrutineeTemp`/`PayloadTemp` | plain TS lowering과 양방향/단방향 매핑의 규범 |
| `src/engine/project.rs`·`snapshot.rs` — overlay·projection cache·세션 | 증분의 뼈대 |
| `src/typescript/backend.rs` — rl 용어의 seam | 계약 4의 구현체 |
| 기존 compile/passthrough/stdlib/integration/native 테스트 | 회귀 방지선 |

`analysis`는 삭제·재작성하지 않는다. 새 HIR/resolution 층이 analysis에
**정확한 identity와 typed domain을 공급**하도록 이동시킨다.

### 2.2 결함 목록 — 무엇이 rustc 수준에 미달인가

실측 근거는 `rust-parity-analysis.md`와 `TASK-117`에 있다. 요약:

- **D1. 보고가 첫 에러에서 멈춘다** (TASK-117 증상 1). 계산은 이미 여러 개를
  들고 있는데(`unresolved`, `uncovered`, `stray_*` 전부 `Vec`) 보고 함수가
  `.first()`로 좁힌다. rustc·tsc는 전부 모아서 보고한다.
- **D2. rl 에러 하나가 그 파일의 typed 진단 전체를 가린다** (TASK-117 증상
  3 — 버그). `ProjectedDocument::project`가 `compile()`을 부르고, 회복 가능한
  에러(중복 arm)에도 projection을 포기한다(`Blocked`).
- **D3. typed/untyped 경로의 소진성 문안이 다르다** (TASK-117 증상 2).
  같은 규칙의 렌더러가 두 벌이다.
- **D4. symbol identity가 없다.** 태그·필드·enum 이름이 전부 **문자열 비교**로
  이어진다. `usefulness`의 constructor도 문자열 tag다. 다른 enum의 동명
  variant를 구분할 방법이 정의 동일성이 아니라 이름뿐이라, rename/references
  합성(GAP-2)이 원리상 성립하지 않는다.
- **D5. 이름 해석이 단계가 아니라 휴리스틱이다.** `analysis::Table`은 arm
  태그 집합으로 enum을 "추측"한다(`identify`). 선언 수집(모듈 스코프,
  import alias, shadowing)이 명시적 declaration table + scope graph로
  존재하지 않고, `Option`/`Result`도 builtin identity가 아니라 문자열
  특례다.
- **D6. 에디터에 두 번째 rl 의미 구현이 남아 있다** (GAP-3).
  `editors/vscode/server/src/analysis.ts`의 정규식/마스킹 구현이 컴파일러와
  다른 규칙으로 hover/definition을 지어낸다.
- **D7. 제어 흐름 판정이 구문 휴리스틱이다.** let-else 발산은 토큰 스캔
  (TASK-081의 오탐 수정 이력), `try`의 위치 제약은 문맥 enum(`Ctx`)으로
  근사한다. CFG가 없어 분기별 초기화·unreachable 판정이 없다.
- **D8. 진단이 구조화되어 있지 않다.** 코드(안정 식별자)·severity·
  secondary label·notes 없이 `message: String` 하나다. 같은 진단의
  typed/untyped 동일성 판정도 문자열 비교뿐이다.
- **D9. query 경계가 없다.** engine의 증분은 "파일 내용 해시가 같으면
  projection 재사용" 한 단계뿐이다. exported 선언 변경 시 importer만
  무효화하는 의존성 추적이 없다.
- **D10. codegen이 semantic 판단을 일부 재계산한다.** 방출이 AST 문자열
  위에서 자체 판단(예: or-패턴 첫 대안의 바인딩 채택)을 하고, 분석 결과를
  인자로 받지 않는다.

## 3. 목표 파이프라인

```
source
→ lexer/token stream                 (현행 유지)
→ lossless AST                       (현행 유지)
→ HIR lowering                       (신설: src/hir/)
→ declaration tables/module scopes   (신설: src/resolve/)
→ name resolution                    (신설: src/resolve/)
→ untyped rl diagnostics             (sema를 다중 진단으로 재구성)
→ TS projection + batched probes     (현행 유지, TypeRequestSet으로 정규화)
→ typed facts                        (신설: rl-owned TypedFacts)
→ typed pattern/usefulness           (analysis를 resolved identity 위로)
→ flow/effect analysis               (신설: 최소 CFG)
→ merged structured diagnostics      (Diagnostic {code, severity, ...})
→ TS codegen + mappings              (현행 유지, lowering plan 소비로 이동)
→ Project/Snapshot semantic APIs     (query 화)
```

AST는 원본 보존용 syntax tree, HIR는 분석용 representation이다. AST 문자열과
source offset을 **symbol identity로 쓰지 않는다** — offset은 SourceMap을
통해 위치로만 돌아간다.

## 4. HIR와 ID 체계 (Phase 1)

`src/hir/`에 최소 다음 newtype ID를 둔다: `FileId`, `NodeId`, `OwnerId`,
`DefId`, `LocalId`, `BodyId`, `ExprId`, `PatternId`, `PatternSiteId`,
`VariantId`, `FieldId`, `ScopeId`. index 기반 session-local identity로
시작한다. 모든 ID에서 원본 span으로 돌아가는 `HirSourceMap`
(`node_spans`/`def_spans`/`pattern_spans`/`ast_origins`)을 별도로 둔다.

HIR lowering이 정규화하는 sugar:

- `match`·tuple match·`if let`·let-else를 공통 `PatternSite`로.
- or-pattern을 alternatives를 가진 하나의 pattern으로.
- tag/payload field를 문자열이 아닌 **unresolved path node**로.
- guard와 arm body를 명시적으로 분리.
- `try`/`result` 바인딩에 single-evaluation 의미를 기록.
- `val` 선언/파라미터와 access path를 identity 연결 가능한 probe로.

TypeScript passthrough expression은 다시 파싱하지 않는다 — rl 분석에 필요한
식은 `OpaqueTsExpr { span }`으로 보존하고 backend에 묻는다.

## 5. 선언 수집과 이름 해석 (Phase 2)

`src/resolve/`에 declaration collection과 reference resolution을 분리해
둔다. namespace는 최소: type / value / enum variant / payload field /
local pattern binding / module·import.

rl enum 하나는 type definition과 constructor value definition을 함께 만든다.
variant·field는 소유 enum에 연결된다(`DefKind::Variant { enum_def, variant }`).
`Res`는 `Def/Variant/Field/Local/Builtin/Unresolved/Ambiguous`를 구분한다.
local 선언이 imported enum을 shadow하고, import alias·namespace import가
정확히 반영된다. std의 `Option`/`Result`는 문자열 특례가 아니라 **builtin
declaration identity**로 등록한다.

unknown tag 진단의 suggestion 후보는 같은 enum domain의 variant로 한정한다.
다른 enum의 동명 variant를 자동 연결하지 않는다. 해석할 subject 후보가
하나도 없으면(외부 TS 유니언) 지금처럼 침묵한다 — 오탐은 통과 계약보다
비싸다.

## 6. Typed 패턴과 usefulness (Phase 3)

모든 pattern 구문(`match`·tuple match·nested·`if let`·let-else·향후
refutable binding)은 동일한 `PatternSite` 분석 경로를 쓴다. 분석 순서:
binding set → constructor resolution → field resolution → scrutinee type
domain → 호환성 → usefulness → unreachable → exhaustiveness(와일드카드
usefulness) → missing witness.

`analysis/usefulness.rs`는 유지하되 입력 constructor를 문자열 tag에서
**resolved constructor identity**로 바꾼다. or-pattern은 occurrence와 body
binding을 분리한다: `A(x) | B(x) => x`에서 pattern 위치의 `x`는 각 payload
타입, body의 `x`는 union — hover가 위치별로 다른 타입을 답한다(현행
`MatchAnalysis`가 이미 절반을 하고 있고, identity 위로 옮긴다). guard가 있는
arm은 coverage를 소모하지 않는다(현행 유지).

## 7. TypeScript 타입 경계 (Phase 4)

현재의 projection/probe 구조를 유지한다. expr마다 RPC를 쏘는 chatty
oracle을 만들지 않는다 — 한 snapshot의 타입 질문을 `TypeRequestSet`으로
수집해 **하나의 batch**로 보내고, 답을 rl-owned `TypedFacts`
(`expr_types`/`symbols`/`domains`/`payload_types`/`mutation_verdicts`)로
정규화한다. `TypeId`/`SymbolId`/`ConstructorDomain`은 rl-owned 타입이고,
tsgo protocol object·UTF-16 좌표는 backend/mapper 안에서 변환한다.

backend가 실패해도 rl semantic pass 전체를 중단하지 않는다: 해당 typed
fact를 `Unknown`으로 두고 독립적으로 판정 가능한 rl 오류는 계속 보고한다.

## 8. 구조화 진단 (Phase 0 — 선행 조건)

`compile()`/`sema::check()`가 첫 `RlError`에서 종료되는 구조를 먼저 없앤다
(D1·D2·D3, TASK-117). 핵심:

- `Diagnostic { code, severity, message, span, ... }` — 안정된
  `DiagnosticCode`와 severity. typed/untyped가 같은 code·renderer를 쓴다.
- semantic visitor는 진단을 누적하고 다음 독립 노드를 계속 검사한다.
  파서가 이미 가진 `stray_*`·`unresolved`·uncovered 목록도 전부 보고한다.
- 회복 가능한 rl 에러는 projection을 막지 않는다. 방출을 막아야 하는 것
  (통과 못 한 `|>`처럼 출력이 TS일 수 없는 것, 방출 의미가 어긋나는 위치
  제약 위반)만 `Blocked`로 남는다.
- 호환: `pub fn compile(...) -> Result<String, CompileError>`는 새 pipeline
  위에서 첫 error-severity 진단을 돌려주는 wrapper가 된다. 전체 결과는
  `analyze(...)`/`compile_report(...)`가 준다. engine·server·CLI는 다중
  진단 API를 소비한다.
- 에러 복구 경계: 해석 실패한 match는 그 match의 소진성 보고만 억제한다
  (파일 단위 억제 → match 단위 억제).

## 9. Flow IR (Phase 5)

전체 TypeScript MIR를 만들지 않는다. rl 고유 제어 흐름 검증에 필요한 최소
IR(`FlowBody`/`BasicBlock`/`Terminator{Goto,Branch,Return,Throw,Match,End}`)
만 만들어 다음을 판정한다: let-else else의 실제 divergence, `try`가 반환
가능한 body 안인지, `result` 바인딩의 early-return 범위, 분기별 초기화,
`val` 파생 mutation access path, unreachable branch. 함수 호출의 임의
effect는 추론하지 않는다 — built-in mutator 정책 + symbol identity의 현행
`val` 정책을 유지한다.

## 10. Codegen 경계 (Phase 7)

codegen은 최종적으로 raw AST 문자열 추측이 아니라 검증된 lowering plan
(`LoweredFile { hir, resolutions, pattern_analyses, flow }`)을 소비한다.
첫 단계에서는 `codegen::emit_with_map(&Program, ...)`을 유지하되 semantic
판단을 인자로 전달받게 하고, 안정화된 HIR node부터 순차 이동한다. 출력
계약(discriminated union + constructor object, switch/IIFE, single
evaluation과 early return, 사용자 바이트만 양방향 매핑, glue는
`EmitAnchor`)은 그대로다.

## 11. Query와 증분 (Phase 6)

`Project`/`Snapshot` 위에 `parse`/`lower`/`module_declarations`/
`module_scope`/`resolve`/`untyped_diagnostics`/`type_requests`/
`typed_facts`/`pattern_analysis`/`flow_body`/`diagnostics`/`emit` query를
단계적으로 추가한다. 처음에는 snapshot-local memoization, 이후 dependency
기록으로 무효화를 좁힌다(파일 text 변경 → 그 파일의 parse 이후; exported
선언 변경 → importer의 scope/resolution/analysis; body만 변경 → 타 파일
선언 query 유지). rustc의 disk cache·red-green까지는 만들지 않는다 — stable
ID·query key·dependency 경계를 먼저 확정하고 측정 가능한 cache hit 테스트를
추가한다.

## 12. IDE 통합

hover/definition/references/rename/completion의 답 순서: ① rl-owned
symbol이면 HIR/resolution DB, ② pattern body binding이면 `PatternAnalysis`,
③ passthrough TS symbol이면 language service 위임, ④ 좌표는 항상 mapper를
거쳐 `.rl` 좌표. rename은 `DefId`/`LocalId` 단위로 references를 수집한다 —
동명 이형 variant, shadowed local, generated glue는 건드리지 않는다.
compiler API가 대체한 뒤 VSCode 확장의 정규식/마스킹 rl 구현(D6)을
삭제한다.

## 13. 페이즈 → 태스크 매핑

| Phase | 내용 | 태스크 |
|---|---|---|
| 0 | 구조화 다중 진단, recoverable projection, 문안 통일 (TASK-117 흡수) | TASK-120 |
| 1 | HIR 기반: ID, arena, lowering, source map | TASK-121 |
| 2 | 선언 수집·이름 해석: scope graph, DefId, builtins | TASK-122 |
| 3 | typed pattern: usefulness를 resolved HIR 위로 | TASK-123 |
| 4 | 타입 query: TypeRequestSet/TypedFacts 정규화 | TASK-124 |
| 5 | flow/effect: 최소 CFG로 try/result/let-else/if let/val 이동 | TASK-125 |
| 6 | query engine: snapshot query cache + dependency invalidation | TASK-126 |
| 7 | codegen 정리: lowering plan 소비, 중복 판단 제거 | TASK-127 |

각 페이즈는 독립 태스크·독립 테스트·독립 커밋으로 완료 가능해야 하고, 다음
페이즈를 위해 저장소를 깨진 상태로 두지 않는다. 태스크 번호는 착수 시점의
INDEX 상태에 따라 조정될 수 있다 — 확정 번호는 INDEX가 진실이다.

## 14. 완료 기준

- 모든 rl declaration과 reference가 symbol identity로 연결된다.
- 모든 pattern syntax가 동일한 `PatternSite` 분석 경로를 사용한다.
- exhaustiveness·unreachable·hover·completion이 같은 분석 결과를 소비한다.
- TypeScript 타입 정보는 batch query와 rl-owned `TypedFacts`로만 들어온다.
- 하나의 rl 오류가 같은 파일의 나머지 진단을 가리지 않는다.
- CLI·server·editor가 동일한 diagnostic code와 renderer를 사용한다.
- codegen은 이름 해석·exhaustiveness를 다시 계산하지 않는다.
- regex 기반 editor shadow semantics가 제거된다.
- 수정되지 않은 파일과 무관한 importer는 다시 분석되지 않는다.
- TypeScript passthrough와 기존 plain TS 출력 계약이 유지된다.
