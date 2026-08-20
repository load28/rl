# rl 구문의 Rust 수준 분석 — 격차 검토와 개선 계획

TASK-101의 검토 기록이다. **제안이며 규범이 아니다** — 채택된 항목은 구현
태스크에서 `docs/reference/`로 옮긴다.

TASK-096(typed match analysis)·TASK-097(coverage 단일 원천)은 `match`를
"필요할 때마다 부분 추론"에서 **타입이 붙은 공통 분석 결과**로 끌어올렸다.
이 문서는 같은 질문을 나머지 구문 전체에 던진다: rl이 러스트에서 가져온
구문들(`enum`·`match`·`try`·let-else·`if let`·`|>`/`flow`·`result`·`val`)이
**rustc가 자기 구문에 대해 아는 만큼**을 rlc도 아는가, 그리고 그 앎이
컴파일 정확도와 LSP(타입 추론·자동완성·이동·이름 바꾸기)로 이어지는가.

한 줄 결론:

> 바인딩의 **타입**은 이미 거의 다 맞다. 맞지 않는 것은 rl 자신의 **이름**이다.
> rlc에는 enum 이름·케이스 태그·페이로드 필드를 선언에 **해석(resolve)** 하는
> 단계가 없고, 그래서 (1) 오타가 rl 에러가 아니라 글루 위의 TS 에러로 새고,
> (2) 소진성 검사가 조용히 꺼지며, (3) 그 이름들에 대한 LSP 답을 에디터의
> 두 번째 구현(정규식 기반 `analysis.ts`)이 대신 지어내고 있다.

---

## 1. 기준선 — TASK-096/097이 세운 바

세 가지가 이번 검토의 잣대다.

1. **순수 단계로서의 분석.** `analysis.rs`는 소스 + 외부 선언만 받는 순수
   함수다. 파일 시스템도 TypeScript도 모르므로 rlc(sema)와 엔진(LSP)이 같은
   모델을 소비하고, 툴체인이 없어도 항상 계산된다.
2. **체커 우선, 분석은 폴백.** 타입의 권위는 TypeScript에 있다. 엔진은 먼저
   서비스에 묻고, 매핑이 없어 물을 수 없는 자리에서만 분석의 선언 타입으로
   답한다.
3. **한 규칙에 한 구현.** 소진성 규칙은 `analysis.rs`가 계산하고 `sema.rs`가
   보고한다. 두 번째 구현을 두지 않는다(TASK-097이 sema의 사본을 없앤 이유).

## 2. 실측 방법

`target/debug/rlc`(현재 main)와 tsc 5.9(`--strict --target es2022`)로 확인했다.
방출 결과는 `rlc --print`, 소스↔출력 매핑은 `rlc --emit-map`, 엔진 동작은
`src/engine/language.rs`·`src/typescript/mapper.rs`의 코드 경로를 읽어 판정했다.
아래 인용된 에러 코드·매핑 수치는 전부 그 실행 결과다.

## 3. 현재 상태 — 구문 × 기능 행렬

`✓` 정확 · `~` 부분적/근사 · `✗` 없음 · `—` 해당 없음

| 구문 | 바인딩 타입(hover/추론) | rl 이름 hover | definition | rename | 자동완성 | rl 진단 |
|---|---|---|---|---|---|---|
| `enum` 선언 | — | `~` 에디터 shadow | `~` shadow | `✗` 거부 | `Enum.` 멤버는 `✓`(TS) | 중복 케이스·필드 타입 `✓` |
| `match` | `✓` 체커→프로브→폴백 | `~` shadow(오탐 있음) | `~` shadow | `✗` 태그·필드 | `~` 암 태그(shadow) | 소진성·중복·or-집합 `✓` / 미지의 태그 `✗` |
| let-else | `✓` (체커, 매핑됨) | `✗` | `✗` | `✗` | `✗` | 발산·사용 위치만 |
| `if let` | `✓` (체커, 매핑됨) | `✗` | `✗` | `✗` | `✗` | 사용 위치만 |
| `try` | `✓` (체커, 매핑됨) | — | — | — | — | 사용 위치만 (`Result` 여부 `✗`) |
| `result` | `✓` (체커, 매핑됨) | — | — | — | — | 구조만 (`Result` 여부 `✗`) |
| `\|>` / `flow` | `✓` (체커, 문맥 타입) | — | — | `✓`(TS) | `✓` 프로브 | 구조 규칙 `✓` |
| `val` | — | — | — | `✓`(TS) | — | `✓` typed(symbol identity) |

읽는 법: **가로 한 줄이 고르게 `✓`인 구문은 없지만, 세로로 보면 첫 열만
거의 다 `✓`다.** 값(바인딩)의 타입은 방출이 이미 해결했고, 나머지 열 —
rl 자신의 이름에 관한 모든 질문 — 이 비어 있다.

### 3.1 이미 바에 도달한 것 (손대지 않는다)

- **모든 구문의 바인딩 타입.** `try`·let-else·`if let`·`result`의 바인딩은
  전부 소스에서 **복사되어** 좁혀진 위치에 방출되므로(`const { radius } =
  $rl_t0;`) 체커가 제네릭 인스턴스화까지 정확히 답한다. or-패턴 바인딩만
  매핑이 없고, 그 자리는 TASK-096의 대안 격리 프로브가 덮는다.
- **파이프라인의 타입 정확도.** `$rl_ap`/`$rl_fl`의 인자 자리가 문맥 타입을
  주므로 커링 콤비네이터도 추론된다. 타입 에러도 **소스 위치**에 떨어진다:
  `n |> add`(2항 함수)는 `TS2345`를 소스의 `add`에, `"x" |> twice`는 소스의
  `"x"`에 보고했다.
- **`val`.** 판정이 checker의 symbol identity 기반이라 섀도잉·재선언에
  흔들리지 않는다.
- **소진성.** 규칙이 하나고(`Coverage`), typed 경로에서는 좁혀진 타입 기준의
  `TagQuery`가 더 정확히 답한다.

## 4. 격차

### GAP-1 — rl 이름을 선언에 해석하는 단계가 없다 (근본)

rl에는 TypeScript가 모르는 이름 공간이 셋 있다: **enum 이름**, **케이스
태그**, **페이로드 필드명**. `MatchAnalysis`는 subject를 해석해 필드 **타입**을
읽지만, 이름의 사용이 선언에 **닿지 않는다는 사실 자체는 아무도 보고하지
않는다.** rustc는 그 반대다 — 패턴 경로와 필드를 먼저 해석하고(E0599·E0026·
E0023), 해석에 실패하면 거기서 멈춘다.

실측(`enum Shape { Circle(radius: number), Empty }`):

| 쓴 것 | rlc | 실제로 나오는 것 |
|---|---|---|
| `const Circel(radius) = s else {...}` | 에러 **없음** | `TS2367` — 글루(`$rl_t0.kind !== "Circel"`) 위, 위치는 근사 |
| `const Circle(radiuz) = s else {...}` | 에러 **없음** | `TS2339: Property 'radiuz' does not exist...` — 위치는 정확, 문안은 TS의 것 |
| `match (s) { Circel(r) => r, Empty => 0 }` | 에러 **없음** | `TS2678` — 글루(`case "Circel":`) 위 |

세 번째 줄이 가장 나쁘다. 태그 오타 하나로 후보 표에서 **모든 태그를 포함하는
enum이 사라지고**, 그 결과 `coverage`가 `None`이 되어 — 빠진 `Circle` 케이스를
포함해 — **소진성 검사가 통째로 조용히 꺼진다.** rl의 간판 검사가 오타 하나에
무력화되고, 사용자는 그 사실을 통보받지 못한다. (typed 경로에서는 `TagQuery`가
좁혀진 타입으로 답하므로 "missing Circle"은 잡힌다. 오타 자체는 여전히 `TS2678`이다.)

필요한 것은 타입이 아니라 **구조**다 — 후보 표는 이미 `analysis.rs`에 있다.
계약 위반 없이 rlc가 직접 보고할 수 있는 에러인데 보고하지 않고 있는 것이다.

### GAP-2 — enum 선언은 매핑 없는 글루다 (LSP 사각지대)

`codegen/enums.rs::emit_enum`은 선언 전체를 `format!`으로 **합성해**
`push_lit`으로 넣는다. `rlc --emit-map`으로 확인하면 위 파일의 첫 매핑은
`src:79` — 즉 `enum` 선언 78바이트에 대응하는 매핑이 **하나도 없다.**

엔진의 규칙과 겹치면 결과는 이렇다.

- `to_service`가 `None` → 선언 안의 어떤 위치에서도 서비스에 **질문할 수 없다.**
- `map_target`이 `None` → 선언으로 향하는 **모든 답이 버려진다.** 다른 파일에서
  `Shape`의 정의로 이동해도 목적지가 없다.
- `rename`은 편집이 하나라도 매핑되지 않으면 **전체를 거부**한다(반쪽 rename은
  프로그램을 깨뜨리므로 옳은 정책이다). 따라서 **enum 이름·케이스 태그·필드명은
  에디터에서 이름 바꾸기가 아예 불가능하다.**

여기서 중요한 판단: 이것은 "매핑을 붙이면 되는 버그"가 **아니다**. 케이스 태그는
방출 후 `"Circle"` **문자열 리터럴**과 프로퍼티 이름으로 흩어지고, 패턴 자리의
태그는 TypeScript 세계에 **대응물이 아예 없다**. 태그 이름 바꾸기를 완전하게
하려면 (a) 선언, (b) `Shape.Circle(...)` 호출부(TS가 아는 것), (c) 모든 패턴 자리
(rl만 아는 것)를 한 번에 고쳐야 한다. **(c)는 어떤 매핑으로도 tsc가 답할 수
없다** — TASK-096이 or-패턴에서 내린 결론과 같은 종류의 결론이다.

### GAP-3 — 에디터에 두 번째 rl 의미 구현이 있다

`editors/vscode/server/src/analysis.ts`(821줄)는 정규식·마스킹 기반으로
`parseEnums`/`parseMatches`/`inferEnum`/`armTags`/`BUILTIN_ENUMS`를 **다시**
구현한다. `server.ts`의 hover/definition/completion은 이 층을 **먼저** 물어보고,
답이 있으면 엔진(체커)에 가지 않는다.

문제는 셋이다.

1. **규칙이 다르다.** `inferEnum`은 "태그를 하나라도 가진 소유자가 유일하면
   그 enum"이다. `analysis.rs`의 규범 규칙("모든 arm 태그를 포함하는 후보 중
   커버 arm을 만족시키는 것, 없으면 결손 최소")과 다르므로, **hover가 컴파일러의
   판단과 다른 답을 할 수 있다.**
2. **오탐이 구조적이다.** `symbolAt`은 match 본문 안에서 "추론된 enum, 없으면
   **아무 enum이나**"의 태그와 이름이 같은 식별자를 케이스로 단정한다. 케이스
   이름과 같은 지역 변수(`Empty`)를 hover하면 enum 케이스 설명이 뜬다. 파일
   어디서든 enum 이름과 같은 식별자도 마찬가지로 가로챈다.
3. **덮는 범위가 match뿐이다.** let-else·`if let`의 패턴 태그는 Rust 쪽
   semantic tokens가 `enumMember`로 **색칠은 하면서**(`engine/tokens.rs`) hover는
   빈손이다. 패턴 필드명(`Some(value: user)`의 `value`)은 어느 구문에서도
   해석되지 않는다.

즉 TASK-097이 sema에서 없앤 "같은 규칙의 두 번째 구현"이 **에디터 층에 그대로
남아 있고**, 그쪽이 사용자 눈에 먼저 보인다.

### GAP-4 — 타입이 필요한 rl 에러가 TS 에러로 샌다

`try`/`result`의 `<-`는 대상이 `Result`여야 하고, let-else·`if let`의 대상은
판별자를 가진 값이어야 한다. rlc는 이것을 검사하지 않고, 위반은 **글루 위의
TS 에러**로 나온다. 실측(`const b = try plain();`, `plain(): number`):

```
res.ts(9,40):  error TS2339: Property 'kind' does not exist on type 'number'.
res.ts(9,87):  error TS2551: Property 'value' does not exist on type 'number'.
```

두 위치 모두 rlc가 만든 글루다. 엔진은 TASK-089 규칙에 따라 가장 가까운 앞선
verbatim 바이트로 위치를 옮기고 `(in code rlc generated for this construct)`를
붙이지만, 사용자는 여전히 "`try`의 대상은 `Result`여야 한다"를 스스로 번역해야
한다. rustc라면 "the `?` operator can only be applied to values that implement
`Try`"다.

`language.md` §5.3은 이 동작을 이미 규범으로 적고 있고(의도된 현행), TASK-100은
`match`에 대해 같은 문제를 rl 진단으로 바꾸자고 등록되어 있다. **TASK-100은 이
부류의 첫 항목이지 유일한 항목이 아니다.**

### GAP-5 — rl 자리의 자동완성은 match 암 전용

지금 있는 것: match 암 시작 위치의 태그 완성과 `Enum.` 멤버 완성(둘 다 shadow
층), 그리고 미완성 파이프라인(`x |> .`)의 멤버 완성(엔진 프로브).

없는 것:

- let-else·`if let` 패턴 자리의 태그 완성 (`if let So|`)
- 패턴 안 **필드명** 완성 (`Some(va|` → `value`)
- 중첩 패턴 안쪽의 태그 완성 (`Ok(value: So|`)
- 튜플 match 원소 자리의 위치별 태그 완성

rust-analyzer는 이 자리 전부에서 변형·필드를 완성한다. 이 자리들은 방출 TS에
대응물이 없으므로 **프로브로도 해결되지 않는다** — rl이 직접 답해야 하는
자리다.

### GAP-6 — 잔여 항목

- **튜플 match에는 typed 소진성 프로브가 없다.** `probe.rs::walk`는
  `Segment::Match`에서만 `collect`를 부르고 `Segment::TupleMatch`에서는 부르지
  않는다. 따라서 튜플 match의 소진성은 선언 표로만 판정되고, 좁혀진 타입·TS
  유니언 스크루티니에서는 답하지 못한다.
- **중첩 패턴의 내부 소진성**은 설계상 v1 보류다
  (`type-inference-gaps.md` §4.3). rustc는 중첩 공간까지 검사한다.
- **let-else·`if let`에 or-패턴이 없다.** rustc에는 있다(언어 표면 격차).
- **미청구(missed-claim) 진단이 없다.** `x <- readNum();`처럼 `const`를 빠뜨린
  `result` 바인딩은 rl 구문으로 청구되지 않고 통과한 뒤, verify가
  `generated TypeScript failed to parse: ... (line 12, col 18 of the generated
  output)`으로 **생성물 좌표**를 들이민다. `|>`·`if let`이 받는 대접
  (`errors.md`의 rl-전용 구문 규칙)과 일관되지 않는다.
- **도달 불가 arm 검사**는 `match`의 중복 태그 검사에 한정된다.

## 5. 원인 — 왜 이렇게 갈렸나

두 문장으로 요약된다.

1. **분석에 타입은 붙였고 이름은 안 붙였다.** TASK-096의 동기가 "or-패턴
   바인딩의 타입"이었으므로 모델은 subject→필드 타입 방향만 갖췄다. 사용→선언
   방향(해석)과 그 실패(미해결 이름)는 모델에 자리가 없다.
2. **rl 이름에 대한 답의 소유자가 정해진 적이 없다.** 바인딩은 "체커 우선,
   분석 폴백"이라는 규범이 있지만(match-analysis.md §3), enum·태그·필드에는
   그런 표가 없어 에디터가 자기 구현으로 메웠다. `lsp-architecture.md` §33은
   "rl 구조 기능은 RL 자체(analysis.ts)"라고 적고 있는데, 그 "RL 자체"가
   엔진이 아니라 **Node 쪽 정규식 구현**을 가리킨 것이 드리프트의 출발점이다.

## 6. 개선 계획 (제안)

원리는 TASK-096/097을 그대로 한 칸 확장한 것이다:

> 패턴을 가진 **모든** 구문은 하나의 typed 분석을 통과한다.
> rl 이름의 해석은 그 분석의 일부이고, 그 이름들에 대한 LSP 답의 소유자는
> **엔진**이다.

### P1. 분석을 match 밖으로 — `PatternSite`로 일반화

`MatchAnalysis`의 arm 분석을 구문 중립적인 **패턴 사이트**로 끌어낸다:
match arm(단일·튜플), let-else, `if let`(중첩·else 체인 포함)이 같은
`pattern_bindings`/`body_bindings`/subject 해석을 갖는다. `try`와 `result`의
바인딩은 패턴이 아니므로 사이트가 아니고, **대상 식의 위치**만 기록한다
(P4가 쓴다).

- 층위: `analysis.rs` 그대로(순수 단계). 소비자는 sema·엔진.
- 위험: 낮음. 방출·매핑을 바꾸지 않는다.
- 효과: P2~P5 전부의 전제.

### P2. 이름 해석 진단 — 미지의 태그·필드·원소 수

P1의 사이트마다 태그·필드를 선언에 해석하고, 실패를 **rlc가 위치와 함께**
보고한다. 편집 거리 기반 "did you mean" 제안을 붙인다.

```
rlc: shape.rl:9:9: enum Shape has no case `Circel` — did you mean `Circle`?
rlc: shape.rl:14:16: case Shape.Circle has no field `radiuz` — did you mean `radius`?
```

- 계약 적합: 타입이 필요 없는 **구조** 판정이므로 계약 2에 정확히 맞는다.
- **GAP-1의 소진성 무력화가 여기서 함께 고쳐진다** — 후보를 못 찾는 원인이
  "미지의 태그"일 때 침묵하지 않고 그 태그를 지목한다.
- 보수성 규칙 필요: 해석할 subject 후보가 하나도 없을 때(외부 TS 유니언 등)는
  지금처럼 침묵한다. 오탐은 통과 계약보다 비싸다.
- 문서: `errors.md` 신규 항목, `docs/ai/rl.md` 반영.

### P3. rl 이름의 semantic 표면을 엔진으로 (GAP-2·GAP-3·GAP-5 해소)

엔진에 rl 이름 전용 표면을 추가하고, 에디터의 shadow 구현을 **폐기**한다.

- `rl_symbol_at(path, pos)` → enum/케이스/필드의 hover·definition. 재료는
  P1의 분석 + 기존 extern 수집(1-hop import).
- `rl_completions(path, pos)` → 패턴 자리의 태그·필드 완성(구문 중립, 미완성
  버퍼 내성). 커버된 태그는 제외하는 기존 arm 완성 동작을 유지·일반화한다.
- `references`/`rename`은 **합성**이다: rl이 아는 편집(선언의 태그, 모든 패턴
  자리)과 tsc가 아는 편집(`Shape.Circle(...)` 호출부, 프로퍼티 접근)을 합쳐
  하나의 원자적 편집 목록으로 낸다. 어느 한쪽이 불완전하면 지금처럼 **전체
  거부**한다.
- 답의 우선순위 표를 규범으로 못박는다(match-analysis.md §3의 확장):
  **rl 이름 = rl 소유, 그 밖 = 체커 소유.** shadow 층의 "아무 enum이나" 폴백은
  없앤다(모르면 답하지 않는다).
- 대안으로 검토했으나 기각: *enum 선언에 emit-map을 붙여 tsc가 답하게 하기* —
  패턴 자리에는 TS 대응물이 없어 rename이 원리상 완전해질 수 없고(§GAP-2),
  한 소스 span이 `type`/`const` 두 곳에 대응해 편집 중복 제거 규칙이 새로
  필요하다. 매핑은 이 문제를 풀지 못한다.

### P4. 타입이 필요한 rl 진단 — TASK-100의 일반화

typed 경로에서 대상 식의 타입을 물어, rl 수준 판정을 rl 문안으로 보고한다.

| 자리 | 지금 | 제안 |
|---|---|---|
| `match` scrutinee가 TS enum | `TS2339` on glue | TASK-100 |
| `try` 대상이 `Result`가 아님 | `TS2339`+`TS2551` on glue | `try`의 대상은 Result여야 합니다 — 여기서는 `number`입니다 |
| `result`의 `<-` 대상 | 같음 | 같은 문안 |
| let-else·`if let` 대상에 판별자 없음 | `TS2339` on glue | 같은 계열 |

- seam은 이미 있다(`backend.rs`의 `Query`/`Answers`). 필요한 것은 "이 위치의
  타입" 질문 하나를 태그/리터럴 전용에서 일반화하는 것.
- 배치(untyped) 빌드에서는 현행 유지 — 타입 없이는 알 수 없다.

### P5. 잔여 정리

- 튜플 match를 `probe.rs`의 typed 프로브에 포함(1줄 수준의 누락으로 보이나,
  튜플 소진성 답의 출처가 둘이 되므로 규범 확인 필요).
- 중첩 패턴 내부 소진성 v2(`type-inference-gaps.md` §4.3).
- let-else·`if let`의 or-패턴(언어 표면 확장 — 별도 제안 필요).
- 미청구 `result` 바인딩의 진단(`errors.md`의 rl-전용 구문 규칙과 일관되게).

## 7. 우선순위

| 순위 | 제안 | 근거 |
|---|---|---|
| 1 | **P2**(P1 위에) | 유일한 **정확성** 결함이다 — 오타가 소진성 검사를 조용히 끈다. 타입 불필요, 계약 적합, 구현 얕음 |
| 2 | **P1** | P2~P5의 전제. 순수 단계 확장이라 위험이 낮고 되돌리기 쉽다 |
| 3 | **P3** | 사용자 체감(자동완성·이동·이름 바꾸기)이 가장 큰 항목. 동시에 규칙 중복 하나를 없앤다 |
| 4 | **P4** | 에러 계층 계약의 마지막 누수. TASK-100이 이미 첫 조각으로 등록되어 있다 |
| 5 | **P5** | 각각 독립적이고 작다. 위 셋이 끝난 뒤 개별 판단 |

P1+P2를 한 태스크로 묶는 것이 자연스럽다(분석 확장과 그 첫 소비자). P3은
엔진 표면 추가와 에디터 폐기가 한 커밋에서 관측 동작을 유지해야 하므로
단독 태스크가 맞다.

## 8. 계약 점검

- **계약 1(유효한 TS는 그대로 통과).** P1~P5 어느 것도 파서의 청구 규칙을
  넓히지 않는다. P2의 새 에러는 **이미 rl 구문으로 청구된** 자리에서만
  발생한다.
- **계약 2(에러 계층 분리).** P2·P4는 계약을 **회복**하는 방향이다 — 지금
  글루 위의 TS 에러로 새고 있는 rl 수준 판정을 rlc의 문안·위치로 되돌린다.
- **"rlc는 TypeScript 타입 시스템을 기르지 않는다".** P2는 타입을 보지 않는다
  (이름과 선언 표만 본다). P4는 타입을 **직접 계산하지 않고** 체커에 묻는다.

---

## 9. "rustc 형태로 정확히" — 대응표와 옮길 수 없는 축

§6의 제안을 **rustc의 단계 구성 그대로** 놓을 수 있는지에 대한 답이다.
결론: **단계 구성은 그대로 옮길 수 있다. 옮길 수 없는 축은 하나뿐이고, 그
하나가 rl의 설계 계약 그 자체다.**

### 9.1 rustc의 패턴 처리 단계와 rl의 대응

rustc는 패턴을 네 단계로 나눠 처리한다. 각 단계가 답하는 질문이 다르고,
에러도 단계별로 다르다.

| rustc 단계 | 답하는 질문 | rl의 현재 대응 | 상태 |
|---|---|---|---|
| **resolve** (경로 → 정의) | 이 태그·필드는 무엇을 가리키나? | **없음** | 신설 (GAP-1) |
| **typeck의 `check_pat`** | 패턴이 기대 타입과 맞나, 각 바인딩의 타입은? | `MatchAnalysis` + TypeScript 체커 | 있음(match만) → P1이 일반화 |
| **THIR typed pattern** | 타입이 붙은 정규화된 패턴 | `MatchAnalysis`(TASK-096이 이 모델을 본떴다) | 있음 |
| **usefulness** (Maranget) | 도달 불가 arm은? 빠진 것은? 그 증거는? | `Coverage`(곱집합 odometer) | 부분 |

rustc가 이 단계들에서 내는 에러는 rl이 그대로 흉내 낼 수 있는 형태다:

| rustc | 언제 | rl의 현재 |
|---|---|---|
| `E0599` no variant named `Circel` | resolve | **침묵** (→ 글루 위 `TS2678`) |
| `E0026` variant does not have a field named `radiuz` | resolve/typeck | **침묵** (→ `TS2339`) |
| `E0023` this pattern has 2 fields, but the variant has 1 | resolve/typeck | **침묵** |
| `E0408` variable `y` is not bound in all patterns | resolve | **있음** (TASK-096이 지목형으로 개선) |
| `E0004` non-exhaustive patterns: `Circle(_)` not covered | usefulness | **있음** (증거 형태는 태그까지) |
| `unreachable_patterns` lint | usefulness | 중복 태그 한정 |

`E0408`을 이미 rustc와 같은 형태로 내고 있다는 사실이 중요하다 — **이 저장소는
이미 rustc의 단계 구성을 부분적으로 밟고 있고**, 빠진 것은 resolve 단계 하나와
usefulness의 나머지 절반이다.

### 9.2 옮길 수 있는 것 — 세 가지 구조

**(1) resolve를 별도 단계로 신설한다.** rustc가 타입을 보기 **전에** 경로를
정의에 묶고, 실패하면 거기서 멈추는 것과 같은 자리에 둔다. rl에서는
`analysis.rs`의 후보 표가 이미 "정의"의 역할을 하므로, 필요한 것은 그 표에
**닿지 못한 이름을 에러로 만드는 것**뿐이다. 타입이 필요 없다.

여기에 rustc의 `DefId`에 해당하는 **안정 식별자**를 붙인다. 지금 분석 결과는
span만 들고 있어 "이 태그와 저 태그가 같은 케이스인가"를 이름 비교로만 답할 수
있다. `EnumId`/`CaseId`/`FieldId`(모듈 경로 + 선언 순서)를 도입하면
references·rename이 이름 문자열이 아니라 정의 동일성 위에서 성립한다 — GAP-2의
rename 합성이 정확해지는 전제다.

**(2) 소진성을 진짜 usefulness 알고리즘으로 바꾼다.** 지금의 `Coverage`는
"태그 집합의 곱집합"이라 중첩 패턴을 다룰 수 없어 v1이 보수적으로 포기한
것(§GAP-6)이다. rustc는 Maranget의 usefulness 한 알고리즘으로 소진성·도달
불가·증거(witness)를 **동시에** 답하고, 중첩·or-패턴·와일드카드가 특별 케이스
없이 처리된다. rl의 패턴 문법은 rustc의 부분집합(구조체 변형 + or + 중첩 +
튜플)이므로 알고리즘이 그대로 성립한다.

이 교체는 세 가지를 한꺼번에 준다: 중첩 내부 소진성(v2), `Circle(_)` 형태의
증거, 그리고 도달 불가 arm 검사의 일반화.

**(3) 컴파일러와 IDE가 같은 분석을 공유한다.** rustc의 usefulness 구현은
독립 크레이트(`rustc_pattern_analysis`)이고 **rust-analyzer가 그것을 그대로
가져다 쓴다.** 컴파일러와 IDE가 소진성에 대해 두 가지 답을 하지 않는 이유가
그 구조다. rl은 이미 절반 와 있다 — `analysis.rs`를 rlc와 엔진이 공유한다.
남은 절반이 GAP-3(에디터의 정규식 두 번째 구현)이고, P3은 정확히 rust-analyzer가
택한 그 구조로 되돌리는 작업이다.

### 9.3 옮길 수 없는 축 — 타입의 소유권

**rustc의 `check_pat`은 기대 타입을 알고 그 타입에 대해 패턴을 검사한다.
rlc는 타입을 모른다.** 이것이 유일하고 근본적인 차이이고, 설계 계약
(`CLAUDE.md`)이 그렇게 정한 것이므로 "고칠" 대상이 아니다. 결과적으로 rl의
패턴 검사는 rustc의 한 단계가 **둘로 쪼개진다**:

| | 재료 | 언제나 가능한가 |
|---|---|---|
| **구조 검사** (이름 해석, 필드 집합, 원소 수, 태그 소진성) | rlc의 선언 표 | ✔ 툴체인 없이도 |
| **타입 검사** (대상이 정말 그 enum인가, 좁혀진 타입, 제네릭 인스턴스화) | TypeScript 체커 | typed 경로에서만 |

여기서 파생되는 정직한 한계 셋:

1. **배치(untyped) 빌드에서는 절반만 답한다.** 이름 해석·필드·소진성은 답하고,
   "이 스크루티니가 정말 그 enum인가"는 답하지 않는다. rustc는 이 구분이 없다.
2. **세계가 닫혀 있지 않다.** rustc의 enum은 변형 목록이 닫혀 있지만, rl의
   스크루티니는 손으로 쓴 TS 유니언이거나 여러 rl enum의 합일 수 있다. 그래서
   resolve는 **후보를 하나도 못 찾으면 침묵**해야 한다 — 오탐은 통과 계약보다
   비싸다. rustc에는 이 규칙이 필요 없다.
3. **witness의 타입 표현은 선언 텍스트다.** `Circle(_)`까지는 rl이 만들 수
   있지만, 제네릭이 인스턴스화된 형태(`Some(value: string)`)는 체커만 안다 —
   TASK-098이 확정한 폴백의 한계 그대로다.

### 9.4 그래서 계획은 이렇게 조정된다

§6의 제안을 rustc 단계에 맞춰 다시 놓으면:

| 제안 | rustc 대응 | 조정 내용 |
|---|---|---|
| **P1** | THIR typed pattern | 그대로. 단, 사이트에 **안정 식별자**(EnumId/CaseId/FieldId)를 추가한다 |
| **P2** | resolve + `E0599`/`E0026`/`E0023` | 그대로. **독립 단계**로 두고, 해석 실패 시 그 사이트의 이후 판정(소진성)은 중단하되 다른 사이트는 계속 검사한다 (rustc의 에러 복구 방식) |
| **P3** | rustc ↔ rust-analyzer 공유 구조 | 그대로. 안정 식별자 위에서 rename/references를 합성한다 |
| **P4** | `check_pat`의 타입 절반 | rl 고유 — 체커에 위임하는 분업이 rustc에는 없는 층이다 |
| **P5** | usefulness 교체 | 잔여 정리가 아니라 **독립 제안으로 승격**한다: 곱집합 odometer → Maranget usefulness. 중첩 v2·witness·도달 불가를 한 번에 얻는다 |

순서는 §7과 같되 P5(usefulness)의 위치가 올라간다: **P1 → P2 → P5 → P3 → P4.**
P5를 P3보다 먼저 두는 이유는, 에디터가 소비할 분석이 최종 형태여야 같은 표면을
두 번 만들지 않기 때문이다.

한 줄로: **rustc의 단계 구성(resolve → typed pattern → usefulness)은 그대로
가능하고, 그중 타입을 보는 절반만 TypeScript에 위임된 형태로 남는다.**
