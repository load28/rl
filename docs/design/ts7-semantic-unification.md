# TypeScript 7 semantic unification — 검토를 거친 개선 계획

이 문서는 **검토 기록이자 제안**이다. 규범 문서가 아니다 — backend 구조의
규범 서술은 `src/typescript/mod.rs`의 모듈 문서와
[`cli.md` §타입 검사](../reference/cli.md)에 있다.

**출처와 상태.** 원안은 "현재 main을 기준으로 TypeScript 7 API를 연동해
구조를 단순·정확하게 만들자"는 외부 제안서다(TASK-082). 이 문서는 그 원안을
현재 main 구현(TASK-070~081 완료 시점)과 2026-08-19의 `microsoft/typescript-go`
HEAD(`c6b013f5`)에 실제로 대조해 검증한 결과로 고쳐 쓴 것이다. 원안의 방향은
대부분 유효하지만, **원안이 "목표"로 서술한 것 중 상당수는 main에 이미
구현되어 있고**, 두 항목(P1의 범위, 항목 4의 Node 중심 전환)은 사실관계에
맞게 뒤집거나 좁혀야 한다.

한 줄 요약은 원안과 같고, 지금 구현의 문장이기도 하다:

> RL owns syntax and RL-only semantics. TypeScript owns TypeScript semantics.

**구현 상태 (TASK-083).** §3의 P1(host batch화 + metadata builtin 판정)과
P2(mutator 정책의 판정 시점 이동)는 구현됐다 — 방출 TypeScript와 진단이
바이트 단위로 불변임을 변경 전후 바이너리의 stderr diff로 확인했고, 측정치는
TASK-083에 있다. P2에는 한 가지 정련이 붙었다: 정책을 통과할 수 없는 메서드
호출은 **query 조립 단계에서 질문 자체를 생략**한다(답이 정해진 질문은
왕복할 가치가 없다 — correctness는 여전히 verdict의 것). P3은 §10.5의 규범
("같은 이름 다른 시그니처는 검사 제외")의 판정 결과를 바꾸므로 "동작 불변"
제약과 양립하지 않아 **보류** — 규범 갱신을 동반하는 별도 태스크가 필요하다.
P4(공개 API 제거)도 같은 이유로 보류.

---

## 0. 현재 구현 기준선 — 원안이 놓친 것

원안을 고치기 전에, main이 이미 어디까지 와 있는지부터 명확히 한다.
아래는 전부 **이미 구현된** 사실이다.

- **`TypeScriptBackend`는 이미 batch다.** `src/typescript/backend.rs`의
  `Query { modules, sources, literals, tags, symbols, emit_declarations }`가
  프로젝트 그래프 하나에 대한 모든 질문을 한 번의 Rust→Node 왕복으로
  나른다. 응답(`Answers`)은 질문 인덱스로 되돌아온다.
- **typed 경로의 `val` pairing은 이미 symbol identity다.** `--check-types`는
  `Options::defer_to_checker = true`로 컴파일하고, 이때 `val::check`(자체
  scope model)는 **아예 실행되지 않는다**(`lib.rs`). 대신
  `val::probes`가 binding과 mutation을 **짝짓지 않은 채** 수집하고
  (`ValProbes`), `check.rs`가 checker의 symbol id로 짝을 맺는다
  (`val_symbols.contains(&root.id)`). shadowing·redeclaration·destructuring이
  TypeScript의 resolution으로 판정되는 구조는 이미 도착해 있다.
- **built-in 판정도 이미 checker의 답이다.** host가 symbol의 모든 선언이
  TypeScript 자신의 lib 파일에 있는지를 답하고(`builtin`), 사용자 정의
  `set`은 그 판정을 통과하지 못한다(TASK-071, `tests/native.rs`의
  `val_mutation_is_decided_by_the_method_the_call_resolves_to`).
- **match exhaustiveness는 이미 narrowed type 기준이다.** scrutinee 위치의
  `getTypeAtPosition` 결과로 missing을 계산한다(원안 5절이 "유지하라"고 쓴
  그대로).
- **`.d.ts`는 이미 compiler의 declaration emit이다.** (원안 8절 그대로.)

따라서 이번 작업의 실체는 "val을 symbol identity로 통일"(원안 P1)이 아니라,
**typed 경로에 남은 마지막 이름 기반 근사 두 곳을 마저 걷어내고, host 내부의
IPC를 batch화하는 것**이다. 원안의 문제의식은 옳지만 대상 서술이 한 세대
이전 구현을 가리키고 있다.

---

## 1. 검증된 TypeScript 7 API 표면

2026-08-19, `microsoft/typescript-go` HEAD `c6b013f5`의
`_packages/native-preview/src/api/sync/api.ts`에서 직접 확인한 사실:

- **position 기반 batch가 이미 있다.**
  ```
  getTypeAtPosition(file, position): Type | undefined
  getTypeAtPosition(file, positions[]): (Type | undefined)[]   // "getTypesAtPositions" 1 IPC
  getSymbolAtPosition(file, position): Symbol | undefined
  getSymbolAtPosition(file, positions[]): (Symbol | undefined)[] // "getSymbolsAtPositions" 1 IPC
  ```
  batch 단위는 **파일 하나**다. 파일별로 positions를 모으면 kind별
  파일당 1 IPC가 된다.
- **Node 기반 batch도 있다** (`getTypeAtLocation(nodes[])`,
  `getSymbolAtLocation(nodes[])`). 단, Node를 손에 쥐려면
  `program.getSourceFile()`로 **모듈 전체 AST를 바이너리로 전송**받아야
  한다(`RemoteSourceFile`). rlc는 emit-map으로 정확한 UTF-16 offset을 이미
  알고 있으므로, rlc의 용법에서 Node 경유는 순수한 추가 비용이다.
- `getTypeOfSymbol(symbols[])` batch가 있다. `getPropertyOfType(type, name)`은
  batch 형태가 **없다** (2026-08-19 기준).
- `isTypeAssignableTo(source: Type, target: Type): boolean`이 있다. 인자는
  Type 핸들이므로 `getTypesAtPositions`의 결과와 host 안에서 그대로
  합성된다.
- `api.runWithTemporaryFileUpdate(baseSnapshot, file, newText, cb)`가 있다.
  임시 snapshot을 만들고, 바뀌지 않은 파일의 source-file cache를 유지하며,
  콜백이 끝나면 정리한다.
- `program.getSourceFileMetadata(fileName)` /
  `isSourceFileDefaultLibrary(file)`: default-lib 여부는 **작은 metadata
  질의**(파일당 1회, Program에 캐시)로 답할 수 있다. 현재 host는 이
  판정을 위해 `getSourceFile()`을 불러 **lib 파일 전체 AST를 전송**받고
  있다 — 캐시가 있어 세션당 한 번이지만, metadata 질의로 바꾸는 편이
  명백히 싸다.
- `getResolvedSignature(node)`, `resolveName(name, meaning, location)`이
  있다(향후 후보, 이번 범위 아님).
- Symbol 응답은 `id`(snapshot 전역), `name`, `flags`,
  `declarations: NodeHandle[]`(path 포함, 추가 IPC 없음)를 나른다.

원안이 존재를 전제한 API(`batch getTypeAtLocation`, `isTypeAssignableTo`,
`runWithTemporaryFileUpdate`)는 전부 실재한다. 다만 **batch의 결이 원안과
다르다**: position 기반에도 1급 batch가 있고, batch 단위는 파일이다.

---

## 2. 원안 항목별 검토

### 원안 1 (P1) — "val binding resolution을 symbol identity로 완전히 통일"

**판정: typed 경로에서는 이미 완료. 남은 것은 두 곳이고, "완전 통일"은
untyped 경로까지 포함하면 불가능하며 해서도 안 된다.**

- `val.rs`의 scope model(`Frame`/`Var`/`lookup`)이 지금도 쓰이는 곳은
  **untyped 경로**(`rlc file.rl`, `--check` — `Sink::Report`)뿐이다.
  이 경로는 node도 tsgo도 없이 동작해야 하는 rlc의 기본 컴파일이다.
  여기서 scope model을 지우면 (a) untyped 컴파일에서 val 검사가 사라지거나
  (b) tsgo가 필수 의존성이 된다. (a)는 원안 자신의 완료 기준 7("기존 RL
  semantics 불변")과 모순이고 (b)는 rlc의 설계(독립 실행 컴파일러)와
  모순이다. **untyped 경로의 scope model은 문서화된 근사로 유지한다**
  (`Options::defer_to_checker` 문서가 이미 이 이중 구조를 규범으로
  서술한다).
- typed 경로에 실제로 남은 이름 기반 근사는:
  1. **call-capability 검사의 callee resolution.** `collect_signatures`가
     같은 파일의 함수 선언을 **이름으로** 표에 넣고(`HashMap<&str, _>`,
     동명이 signature가 다르면 검사 포기), `check_call`이 호출식의 callee
     **이름**으로 그 표를 찾는다. probes 모드에서도 이 이름 매칭은
     그대로다 — 인자 쪽 root만 symbol로 판정하고, "어느 함수를 불렀는가"는
     여전히 RL의 추정이다. → **callee identifier와 함수 선언 identifier에
     각각 SymbolQuery를 걸어 id로 짝지어라.** signature(어느 파라미터가
     `val`인가)는 `.rl` 원문에만 있는 RL 사실이므로 계속 RL이 소유하되,
     표의 키를 이름에서 선언 위치(byte offset)로 바꾼다. 이렇게 하면
     shadowing된 함수·재선언·블록 스코프 함수가 전부 checker의 답으로
     정리되고, 향후 cross-module 확장(NodeHandle.path + emit-map 역매핑)의
     자리도 생긴다.
  2. **built-in mutator 이름 prefilter** — 아래 원안 2 검토 참조.
- 부수 정리 후보: `Sink::Calls` / `val::method_calls` / 공개
  `val_method_calls`는 TASK-071 시절의 경로로, 현재 `check.rs`는 probes만
  쓴다. 저장소 안 사용처는 자체 테스트뿐이다. 제거하면 `val.rs`의 세 모드
  walk가 두 모드로 줄어든다. (공개 API 제거이므로 별도 결정으로.)

### 원안 2·P3 — "built-in mutator 이름 prefilter 의존도를 줄인다"

**판정: 방향 유효. 단, 원안의 서술에서 한 가지를 바로잡아야 한다 —
이 이름 목록은 prefilter이기 이전에 현재 유일한 mutator policy다.**

현재 구조: probes 수집 시 `is_builtin_mutator_name`을 통과한 메서드 호출만
`Mutation`이 되고, verdict(`check.rs`)는 **`builtin` 하나만** 본다. 즉
"builtin이면서 이름이 목록에 있으면 mutation"이라는 정책이 수집과 판정에
반씩 쪼개져 있다. 목록을 그냥 지우면 `map.get(k)`·`items.at(0)` 같은
**non-mutating builtin이 전부 오탐**이 된다 — TypeScript는 mutation effect를
말해주지 않으므로(원안 스스로 지적), 이름 정책은 없앨 수 없다.

올바른 리팩토링:

1. probes 모드는 val-rooted **모든** 메서드 호출을 수집한다(이름 필터
   제거).
2. verdict(`check.rs`)에서 `resolution.builtin && is_builtin_mutator_name`을
   적용한다 — 정책이 한 곳에 모인다.
3. 그 결과 목록은 correctness-critical한 수집 조건이 아니라 판정 시점의
   정책이 된다: 이름 누락은 오탐을 만들 수 없고 (문서화된) 미탐만 남는다.
   원안의 "prefilter가 누락되어도 typed analysis가 기능적으로 틀리지 않게"가
   정확히 이 형태다.
4. 비용: val 경로의 메서드 호출 수만큼 SymbolQuery가 늘어난다. val을 쓰는
   파일에 국한되고, 아래 batch화와 겹치면 IPC로는 파일당 1회에 흡수된다.

주의: 현재 목록에는 `Date`의 setter(`setHours` 등)와 `DataView.setX`가
없다 — 실제 미탐이다. 목록 보강은 언어 표면 변경(레퍼런스 §10 갱신 동반)
이므로 이 리팩토링과 분리해 별도 태스크로 등록한다(원안 11 "기능 추가로
범위 키우지 않기"와 일관되게).

### 원안 3 (P2) — "host 내부 semantic query를 batch API로"

**판정: 유효, 이번 작업의 가장 확실한 소득. 단 batch 단위는 파일이다.**

현재 host의 ask 1회당 IPC:

| 질문 | 현재 IPC | batch 후 |
|---|---|---|
| literal check | `getTypeAtPosition` 1 + union이면 `getTypesOfType` 1 | 파일당 type 1 + constituent 전개 |
| tag check | 위와 같음 + **constituent마다** `getPropertyOfType` 1 + `getTypeOfSymbol` 1 | property는 그대로, `getTypeOfSymbol`은 전 check를 모아 1 |
| symbol check | `getSymbolAtPosition` 1 | 파일당 1 |
| builtin 판정 | 선언 파일마다 `getSourceFile` (**전체 AST 바이너리 전송**, 캐시 있음) | `getSourceFileMetadata` (소형, 캐시) |

바꾸는 방법: host가 job의 checks를 module별로 모아
`getTypeAtPosition(file, positions[])` / `getSymbolAtPosition(file,
positions[])`를 호출하고, 결과를 원래의 전역 index로 흩뿌린다.
`getPropertyOfType`은 batch가 없으므로 constituent별 호출을 유지한다
(upstream에 batch 추가를 제안할 후보로 기록만 해 둔다).

**절대 조건: Rust 쪽 `Query`/`Answers`와 host 프로토콜의 index 계약은
그대로 둔다.** batch화는 host.mjs 내부 구현 세부이고, 흩뿌리기 순서가
깨지지 않는지는 파일 2개 × 질문 여러 개를 섞은 테스트로 고정한다
(원안 12 "batch 결과 순서" 그대로).

### 원안 4 (P4) — "position 중심에서 Node + Checker 중심으로"

**판정: 뒤집는다. rlc의 용법에서는 position 기반이 옳다.**

원안은 position 계열이 legacy로 정리되는 중이라고 전제했지만, HEAD의 API는
position 계열에 1급 batch(`getTypesAtPositions`/`getSymbolsAtPositions`)를
갖고 있다. Node 경유는 (a) `getSourceFile`로 모듈 전체 AST를 JS 쪽에
전송·구성하고 (b) node id를 얻어 (c) batch를 부르는 3단계인데, rlc는
emit-map으로 offset을 이미 알고 있으므로 (a)(b)가 순수 오버헤드다.
스크루티니 위치가 "식의 첫 토큰이 아니라 temp binding"이어야 한다는 문제도
codegen이 `ScrutineeTemp`로 이미 풀었다.

이 항목은 "TS7이 position API를 폐기하면 그때 Node로 옮긴다"는 **경계 안의
구현 교체 시나리오**로만 남긴다 — `TypeScriptBackend`가 offset을 나르는 한
Rust 쪽은 어느 쪽이든 무관하며, 그것이 이 경계의 존재 이유다.

### 원안 5 — match exhaustiveness 유지

동의. 이미 그 구조다. "RL 자체 타입 추론으로 되돌리지 않는다"는 금지
목록(§5)은 개선판에도 그대로 승계한다.

### 원안 6 (P5) — `isTypeAssignableTo`를 향후 primitive로

동의, 확인 완료. Type 핸들 기반이므로 `Query`에
`assignables: Vec<{source_position, target_position}>` 형태의 질의를 나중에
추가하면 host 안에서 `getTypesAtPositions` 결과와 그대로 합성된다.
이번 작업에서는 **설계 여지만 기록**하고 코드는 만들지 않는다(원안과 동일).
try/result를 assignability 기반으로 재작성하지 않는다는 금지도 승계.

### 원안 7 — `runWithTemporaryFileUpdate` 검토

동의(검토만, 재설계 없음). 확인한 사실과 한계:

- 이 API는 임시 snapshot을 만들고 미변경 파일의 source-file cache를
  유지한다. 목적은 rl의 `--overlay`와 겹친다.
- 그러나 rl의 overlay는 **lowering 이전의 `.rl` 원문**을 치환한다. host가
  serve하는 것은 lowering 결과인 virtual `.ts`이고, 이는 layered FS +
  `fileChanges`로 이미 증분 갱신된다. temporary update가 대체할 수 있는
  것은 "편집 중 버퍼의 ask가 세션 snapshot을 전진시키지 않게 하는" 부분
  정도다 — 이득은 snapshot 위생이지 기능이 아니다.
- 결론: layered FS와 project graph, lowered module mapping은 유지. 에디터
  경로의 snapshot lifecycle 단순화 여지로만 기록.

### 원안 8·9 — 유지할 설계, 책임 분리

동의. 현재 코드가 이미 그 경계 위에 있다. 한 가지 표현만 정확히 한다:
원안 9의 "identifier binding resolution — TypeScript owns"는 **typed 경로**의
서술이다. untyped 경로의 val 검사는 RL의 문서화된 근사로 남는다(위 P1
판정). 이 비대칭 자체가 규범이며 `Options::defer_to_checker`에 이미 그렇게
적혀 있다.

### 원안 11 — 피해야 할 것

전부 동의하고 두 개를 추가한다:

- untyped 컴파일 경로에서 val 검사를 없애거나, 그것을 위해 tsgo를 rlc의
  필수 의존성으로 만들기.
- mutator verdict를 `builtin`만으로 판정하기 — non-mutating builtin
  (`map.get`, `arr.at`, `slice`)이 전부 오탐이 된다. 이름 정책은 판정
  시점으로 옮기는 것이지 없애는 것이 아니다.

---

## 3. 수정된 구현 우선순위

원안의 P1~P5를 실제 남은 일로 재배열한 것. 각 단계는 독립적으로 머지
가능하고, 기존 테스트(특히 `tests/native.rs`·`tests/passthrough.rs`)를
전부 통과한 상태를 유지한다.

**P1 — host 내부 batch화 + builtin 판정 경량화** (`host.mjs`만 변경)
파일별 `getTypesAtPositions`/`getSymbolsAtPositions`로 전환,
tag의 `getTypeOfSymbol`을 전 check 취합 후 1회로, builtin 판정을
`getSourceFileMetadata`로. Rust 프로토콜 불변. index 흩뿌리기 테스트 추가.

**P2 — mutator 정책을 판정 시점으로** (`val.rs` 수집 + `check.rs` 판정)
probes가 val-rooted 모든 메서드 호출을 수집, verdict가
`builtin && mutator_name`. `is_builtin_mutator_name`의 "superset을
유지하라"는 주석 계약을 "판정 시점 정책" 서술로 교체. 레퍼런스 §10의
해당 서술 갱신.

**P3 — call-capability의 callee를 symbol identity로** (`val.rs` probes +
`project.rs` + `check.rs`)
함수 선언 identifier와 callee identifier에 SymbolQuery, id로 짝짓기.
signature 표의 키를 이름 → 선언 offset으로. 같은 파일 범위는 유지
(cross-module은 후속 태스크로만 기록).

**P4 — 죽은 경로 정리** (결정 필요)
`Sink::Calls`/`val_method_calls` 제거 여부. 공개 API이므로 별도 결정으로
분리하되, 제거 시 `val.rs`가 두 모드로 준다.

**P5 — 미래 확장 지점 기록만**
`assignables` batch 질의 스케치, temporary-snapshot 활용 시나리오,
position API 폐기 시의 Node 전환 시나리오. 코드 없음(이 문서가 그 기록).

## 4. 검증 테스트

원안 12의 목록 중 상당수는 `tests/native.rs`에 이미 있다:
shadowing(`a_shadowing_binding_is_a_different_binding`), parameter 경계
(`val_holds_on_a_parameter_and_across_a_function_boundary`), 사용자 정의
mutator 이름(`val_mutation_is_decided_by_the_method_the_call_resolves_to`),
literal/enum narrowing(`*_uses_the_narrowed_type_at_the_match`), any 수신자
(`an_any_receiver_is_never_called_a_mutation`), passthrough 계약
(`tests/passthrough.rs`). 이들은 **회귀 게이트로 그대로 쓴다**.

새로 추가할 것:

1. **batch 흩뿌리기 순서**: 모듈 2개 × literal/tag/symbol 질문이 섞인
   프로젝트에서 각 진단이 제 위치에 나오는지 (P1).
2. **non-mutating builtin 무보고**: `val const m = new Map(); m.get("a");`
   `val const xs = [1]; xs.at(0);` — P2 이후에도 오탐 없음.
3. **동명 함수 shadowing에서의 pass 검사**: 블록 안에서 같은 이름의 함수를
   재선언했을 때 callee가 올바른 선언의 signature로 판정되는지 (P3).
4. (별도 태스크) `Date#setHours` 등 정책 목록 보강 시의 미탐 회귀 테스트.

## 5. 완료 기준 (원안 대비 수정)

1. typed 경로에 남아 있던 이름 기반 resolution(callee 표, 수집 시점의
   mutator 이름 필터)이 제거되었는가 — untyped 경로는 명시적으로 제외.
2. symbol identity가 typed 경로 binding 판단의 단일 source of truth인가
   (binding·mutation root·callee 모두).
3. host↔tsgo IPC가 질문 수 비례에서 파일 수 비례로 줄었는가 —
   `--watch`의 재검사 시간으로 전후 측정치를 남긴다.
4. exhaustiveness가 약해지지 않았는가 — 기존 native 테스트 전부 통과.
5. `q.set` 류 false positive 방지가 유지되고, `map.get` 류 신규 false
   positive가 없는가.
6. unstable TS7 API가 `src/typescript/{native.rs,host.mjs}` 밖으로 새지
   않는가 — `backend.rs`의 타입에 tsgo 고유 개념(Node id, snapshot id)이
   등장하지 않는 것으로 확인.
7. 기존 RL semantics와 emitted TypeScript가 바이트 단위로 불변인가 —
   `tests/compile.rs` 스냅샷과 `tests/passthrough.rs`로 확인.
