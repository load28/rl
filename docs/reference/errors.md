# rlc 에러 레퍼런스

rlc가 내는 모든 진단의 형식과 해결 방법입니다. 언어 규칙은
[`language.md`](./language.md), CLI 동작은 [`cli.md`](./cli.md)를 보세요.

한 번의 검사에서 나온 진단은 파일과 원본 소스 위치 순서로 표시합니다. 같은 위치와
범위에서 같은 문안을 가진 진단은 검사기 코드가 달라도 한 번만 표시합니다. 패턴 이름을
해석하지 못한 `match`에서는 이름 진단만 보고하고, 그 결과로 생긴 소진성 진단은
억제합니다.

```
rlc: <파일>:<행>:<열>: <메시지>
```

행·열은 원본 `.rl` 기준 1-기반입니다. 위치를 특정할 수 없는 에러는
`rlc: <파일>: <메시지>`로 나옵니다.

rl 구문으로 완전히 파싱되지 않는 텍스트는 에러가 아니라 조용히 통과합니다 —
에러는 **rl 구문임이 확정된 뒤의 규칙 위반**에만 발생합니다. 통과 영역의 타입
에러는 tsc의 몫입니다.

## 다중 보고

한 파일의 rl 진단은 **전부, 소스 순서로** 보고됩니다 — tsc·rustc처럼
한 번의 실행이 파일의 문제를 다 보여 줍니다. 한 구문의 에러는 다음 독립
구문의 검사를 막지 않습니다. 복구 경계는 구문 단위입니다: 이름이 해석되지
않은 match는 **자기** 소진성 질문만 침묵하고(원인 위에 결과를 쌓지 않기
위해), 이웃 match는 제 답을 그대로 냅니다. 태그·리터럴이 섞인 match도
같은 이유로 자기 소진성만 침묵합니다.

`--check-types`/`--types`/에디터에서도 같습니다. 파서가 rl 구문으로 확정했지만
완성하지 못한 `match`·`enum`·`result`·`if let`·파이프라인과 배치가 잘못된
`try`는 정확한 구문 범위의 오류 노드로 남습니다. typed projection은 그 노드만
TypeScript placeholder로 바꾸고 나머지 원문을 계속 검사합니다. placeholder
범위에서 생긴 연쇄 진단은 억제하지만, 같은 파일의 다른 위치에 있는 타입 진단과
`val` 진단은 rl 오류와 **함께** 보고됩니다. 정상 컴파일은 이 placeholder를
사용하지 않으므로 오류가 있는 소스의 빌드 출력 계약은 바뀌지 않습니다.

모든 진단은 안정된 **코드**를 가집니다(예: `match-not-exhaustive`,
`unknown-case`, `val-mutation`). 코드는 규칙의 식별자이고 모든
소비자(CLI·`--server`·에디터, 기본·typed 경로)에서 같습니다. `--server`의
응답에 `code` 필드로 실립니다.

라이브러리의 `compile()`은 계약("코드를 내놓거나 실패하거나")을 유지합니다 —
소스 순서의 **첫** 에러를 돌려줍니다. 전체 목록은 `analyze()` /
`compile_report()`가 답합니다.

## 진단의 범위

CLI는 시작 위치만 찍지만, 진단은 그 안에 **범위**를 함께 담습니다 — 그
에러가 말하는 구문을 사용자가 쓴 그대로 덮는 구간입니다. 에디터의 밑줄이
이 범위입니다.

| 에러 | 덮는 범위 |
|------|-----------|
| 소진되지 않은 match | `match (스크루티니)` — 암은 사용자 코드이므로 제외 |
| `try` 관련 (위치 제약, 글루 타입 에러) | `try <식>` — 선언 형태여도 `const x = `는 제외 |
| let-else 위치 제약 | 선언 키워드부터 `else` 앞까지 |
| `if let` 위치 제약 | `if let <패턴> = <식>` (then 블록 제외) |
| 중복 암·중복 케이스·오타로 보이는 이름 | 그 이름/리터럴 |
| `val` 위반 | 그 바인딩 이름 |

넓이를 알 수 없는 진단은 위치만 보고합니다 — 그때 밑줄의 넓이는 소비자가
정합니다(에디터는 그 위치의 단어를 씁니다). 엔진 서버는 이 범위를
`endLine`/`endCol`로 전달합니다 ([`cli.md`](./cli.md#엔진-서버---server)).

typed 진단은 생성 TypeScript의 시작점만 원문으로 되돌리지 않습니다. 진단 범위
전체가 하나의 verbatim 구간에 속할 때만 그 원문 범위를 그대로 씁니다. 범위가
rlc가 만든 glue와 걸치면 해당 lowering 구문의 기본 범위로 돌아갑니다. 따라서
`match`의 반환 타입 불일치는 `match (스크루티니)`를, `result` 바인딩의 타입
불일치는 `이름 <- 식`을 덮습니다. 어느 lowering에도 속하지 않는 생성 코드만
가장 가까운 원문 위치를 최후 수단으로 사용합니다.

진단의 **표시 범위**와 **원인 소유 범위**는 구분합니다. 예를 들어 mixed-pattern
오류는 잘못된 패턴을 표시하지만, 그 원인은 match 전체를 소유합니다. checker가
그 match의 생성 glue에서 낸 비교 오류는 같은 원인의 결과이므로 중복 표시하지
않습니다. 이 소유 관계는 오류 번호나 메시지 문구가 아니라 source-map과 lowering
anchor로 판정하며 CLI·서버·에디터가 공유합니다.
에디터의 빠른 TypeScript 잠정 검사도 같은 projection에서 수집한 RL 원인과
소유 범위를 먼저 적용하므로, 권위 있는 typed 검사를 기다리는 동안 연쇄 밑줄을
잠시 표시하지 않습니다.

## enum

| 메시지 | 원인과 해결 |
|--------|-------------|
| `enum <이름>: duplicate case "<태그>"` | 한 enum에 같은 태그가 두 번. 위치는 두 번째 태그. 합치거나 이름을 바꿉니다 |
| `enum <이름>: invalid type for field ``<필드>``: <상세>` | 필드 타입이 TypeScript 타입으로 파싱되지 않음. 서로 맞지 않는 괄호·대괄호도 enum 전체 parse 오류가 아니라 이 진단으로 보고합니다. 위치는 타입 시작 지점. 표기를 고치거나 `--no-verify`로 우회(이 경우 tsc 단계에서 드러납니다) |
| ``rl `enum` could not be parsed (...)`` | payload 괄호나 제네릭으로 rl enum임이 확정됐지만 선언이 완성되지 않음. 위치는 `enum` |

```rl
enum Shape { Circle(r: number), Circle(d: number) }
// rlc: file.rl:1:33: enum Shape: duplicate case "Circle"

enum E { A(x: number]) }
// rlc: file.rl:1:1: rl `enum` could not be parsed (...)
```

## 패턴의 이름 해석

패턴 안의 케이스 태그와 필드 이름은 선언에 대조됩니다 — `match`(튜플·중첩
포함), let-else, `if let` 모두 같은 규칙입니다.

| 메시지 | 원인과 해결 |
|--------|-------------|
| ``<enum> has no case `<태그>` — did you mean `<제안>`?`` | 패턴의 태그가 그 enum의 케이스가 아니고, 어떤 케이스의 오타로 보입니다. 위치는 태그 |
| ``<enum>: case `<태그>` has no field `<필드>` — did you mean `<제안>`?`` | 바인딩한 필드 이름이 그 케이스의 페이로드에 없고, 어떤 필드의 오타로 보입니다. 위치는 필드 이름 |

```rl
enum Shape { Circle(radius: number), Empty }
const a = match (s) { Circel(radius) => radius, Empty => 0 };
// rlc: file.rl:2:23: enum Shape has no case `Circel` — did you mean `Circle`?

const b = match (s) { Circle(radiuz) => radiuz, Empty => 0 };
// rlc: file.rl:5:29: enum Shape: case `Circle` has no field `radiuz` — did you mean `radius`?
```

**해석에 실패한 이름이 그 자체로 에러는 아닙니다.** 태그 패턴은 `kind` 문자열
필드를 가진 **모든** 태그드 유니언에 쓸 수 있고([`language.md` §3.2](./language.md#32-의미)),
손으로 쓴 유니언의 태그는 어떤 선언 표에도 없습니다. 그래서 rlc는 **고칠 이름을
댈 수 있을 때만** 보고합니다 — 대소문자만 다르거나 편집 거리가 가까운 이름
(글자 자리바꿈은 한 번의 편집으로 셉니다). 오타가 아닌 틀린 이름은 타입을 알아야
판정할 수 있으므로 보고하지 않습니다.

어느 enum에 대조할지는 사이트가 정합니다.

- **`match`**: 암들의 태그를 가장 많이 포함하는 유일한 enum. 후보가 없거나
  동점이면 검사하지 않습니다.
- **let-else·`if let`**: 태그가 **하나**뿐이면 근거가 얇으므로 편집 한 번
  거리의 케이스를 가진 enum이 유일할 때만 보고합니다. or-패턴의 여러 태그는
  match와 같은 근거라 match의 규칙을 그대로 씁니다. 태그가 정확히 해석되면
  그 케이스의 필드는 match와 똑같이 검사합니다.
- **중첩 패턴**: 바깥 필드의 **선언된 타입**이 가리키는 enum에 대조합니다.

태그가 해석되지 않아 보고된 match는 **소진성을 함께 보고하지 않습니다** —
원인(오타)을 고치면 그 답이 달라지기 때문입니다.

## match

| 메시지 | 원인과 해결 |
|--------|-------------|
| ``match: the wildcard arm `_` must be the last arm`` | `_` 뒤의 암은 도달 불가능. `_`를 마지막으로 옮깁니다 |
| `match: duplicate arm "<태그>"` | **무가드 암이 이미 덮은 태그**가 다시 나옴 — `A \| A`, `A \| B => .., B => ..`, `A => .., A if c => ..`. 가드 암끼리의 반복은 에러가 아닙니다. 위치는 두 번째 태그 |
| `match: or-pattern alternatives must bind the same names — <detail>` (let-else·`if let`에서는 접두사가 `let-else:`/`if let:`) | 대안들이 하나의 구조 분해를 공유하므로 같은 (필드, 이름) 집합을 바인딩해야 합니다. `A(x) \| B(x)`·`A(x, y) \| B(y, x)`는 되고 `A(x) \| B(y)`·`A \| B(x)`는 안 됩니다. `<detail>`은 어긋난 바인딩을 지목합니다 — 한쪽에만 있는 이름은 `` `y` is bound in `B(...)` but not in `A(...)` ``, 같은 이름을 다른 필드에서 가져오면 `` `v` is bound from field `v` in `A(...)` but from field `w` in `B(...)` `` |
| `match: nested patterns cannot be combined with or-patterns` (`if let`에서는 접두사가 `if let:`) | 중첩 패턴(`Tag(field: Inner(...))`)은 대안별 경로 조건이 필요해 공유 구조 분해와 양립하지 않습니다. 암을 나눕니다 |
| `` match: binding `<이름>` is used more than once in this pattern (rename one with `field: alias`) `` | 한 패턴(중첩 포함)이 같은 이름을 두 번 바인딩 — 한 스코프에 두 번 선언됩니다. 별칭으로 바꿉니다 |

### 리터럴 패턴

| 메시지 | 원인과 해결 |
|--------|-------------|
| `match: duplicate arm <리터럴>` | 무가드 암이 이미 덮은 리터럴이 다시 나옴. **값 기준**이라 `200`/`0xc8`, `"a"`/`'\x61'`은 같은 리터럴입니다. 위치는 두 번째 리터럴. 가드 암끼리의 반복은 에러가 아닙니다 |
| `match: cannot mix tag patterns and literal patterns in the same match ...` | 태그 match는 `$rl_m.kind`를, 리터럴 match는 `$rl_m`을 비교하므로 한 match에 섞을 수 없습니다. 두 match로 나눕니다 (`_`는 양쪽 모두 가능). 범위는 종류가 처음 달라진 패턴 전체 |
| `match: or-pattern alternatives must all be the same kind of literal (found <종류> after <종류>)` | `"a" \| 1`처럼 종류가 다른 리터럴을 한 or-패턴에 섞었습니다. 암을 나눕니다 |

```rl
const v = match (x) { "a" => 1, "a" => 2 };
// rlc: file.rl:1:33: match: duplicate arm "a"

const v = match (x) { Some(v) => v, "none" => 0 };
// rlc: file.rl:1:37: match: cannot mix tag patterns and literal patterns in the same match ...
```

`_` 없는 리터럴 match의 **소진성은 기본 경로에서 검사하지 않습니다** — 런타임
가드(`rl match: unexpected literal ...`)만 남습니다. 타입이 있는
`rlc --check-types`/`--types`가 검사합니다:

```
rlc: src/main.rl:3:10: match on literal union is not exhaustive: missing "south"
     (add the missing arms or a final `_` arm)
```

스크루티니 타입이 유한 리터럴 유니언으로 확정될 때만 나옵니다
([`language.md` §3.9](./language.md#39-리터럴-유니언-소진성---types)).

**enum 소진성 문안은 두 경로가 한 렌더러를 씁니다.** 형태는 하나 —
`match[ on <출처>] is not exhaustive: missing ... (add the missing arms or a
final \`_\` arm)` — 이고 차이는 데이터입니다: 기본 경로(`rlc`/`--check`)는
자기 선언 표에서 답하므로 enum 이름을 댈 수 있고(아래 표),
`--check-types`/`--types`는 match 위치의 *타입*에서 답하므로 출처 없이
`match is not exhaustive: ...`라고 하며, 대신 앞선 가드가 좁혀 낸 케이스는
요구하지 않습니다. 진단 코드는 양쪽 다 `match-not-exhaustive`입니다.

### 소진성

```
$ rlc shapes.rl
rlc: shapes.rl:12:25: match on enum Shape is not exhaustive: missing "Rect"
     (add the missing arms or a final `_` arm)
```

`_` 없는 match가 알려진 enum의 케이스를 다 덮지 않았습니다. 위치는 `match`
키워드이고(import된 enum이어도 컴파일 중인 파일 기준), 메시지가 enum의 출처를
표시합니다.

| 출처 | 메시지 |
|------|--------|
| 같은 파일의 rl enum | `match on enum <이름> is not exhaustive: ...` |
| import한 exported enum | `match on enum <이름> (imported from "<지정자>") is not exhaustive: ...` |
| 내장 `Option`/`Result` | `match on built-in enum <이름> is not exhaustive: ...` |

**가드 암은 케이스를 커버하지 못합니다** — 조건이 거짓일 수 있으므로 그 태그를
덮으려면 무가드 암이 따로 필요합니다. **중첩 패턴은 안쪽까지 검사되고**, 빠진
것은 태그가 아니라 **패턴으로** 지목됩니다 — 그대로 암으로 **붙여 넣을 수 있는**
형태입니다(중첩 자리의 유닛 케이스에 괄호가 붙는 이유입니다 — 패턴 안에서
`필드: 이름`은 매치가 아니라 별칭이므로):

```
rlc: r.rl:3:11: match on built-in enum Result is not exhaustive: missing "Ok(value: None())"
     (add the missing arms or a final `_` arm)
```

손으로 쓴 유니언이나 해석되지 않는 import의 enum은 이 검사를 받지 않고 런타임
가드만 남습니다 ([`language.md` §3.6](./language.md#36-소진성-검사)).

빠진 항목이 5개 이상이면 앞의 셋과 총 개수만 보고합니다 — 단일·튜플·typed
경로 공통 규칙입니다. 튜플 match는 곱집합으로 검사하고 빠진 **조합**을
보고합니다:

```
rlc: nav.rl:4:15: match on (Conn, Mode) is not exhaustive: missing (Offline, Manual)
     (add the missing arms or a final `_` arm)
```

### 튜플 match

| 메시지 | 원인과 해결 |
|--------|-------------|
| `match: tuple pattern has <n> elements but the match has <m> scrutinees` | 튜플 패턴의 원소 수가 스크루티니 수와 다름. 범위는 괄호를 포함한 튜플 패턴 전체 |
| `` match: binding `<이름>` is used more than once in this tuple pattern (rename one with `field: alias`) `` | 한 튜플 패턴의 두 원소가 같은 이름을 바인딩 — 한 스코프에 두 번 선언됩니다. 별칭으로 바꿉니다: `(Some(value), Some(value: other))` |

## try / let-else

| 메시지 | 원인과 해결 |
|--------|-------------|
| `` `try` is a statement, not an expression — bind its value first with `const value = try <expression>;` `` | `return try f()`·화살표 함수 본문·삼항식처럼 값을 요구하는 자리에 `try`를 직접 사용했습니다. `try` 선언문으로 값을 먼저 바인딩한 뒤 사용하세요. 위치는 `try <식>` |
| `` `try` cannot be used here — it compiles to a `return`, which would exit this construct's own IIFE ... `` | match(스크루티니·암 본문)·`result` 블록·템플릿 보간 안의 문장 위치: 그 자리의 `return`은 둘러싼 함수가 아니라 구성물의 IIFE에서 반환됩니다. 그 자리에 **함수를 쓰면** 그 안의 `try`는 허용됩니다(Rust의 클로저 안 `?`) — 판정은 제어 흐름 기반입니다. 로직을 함수로 추출하거나, `result` 블록 안이라면 `<-` 바인딩을 쓰세요 ([§5.4](./language.md#54-사용-위치-제약)). 위치는 `try` |
| `` `try` must be inside a function — it compiles to a `return` that propagates the `Err` ... `` | 모듈 최상위(또는 namespace 본문 등 함수 아닌 스코프)의 `try`: 방출되는 `return`이 나갈 함수가 없습니다. 함수 안으로 옮기세요 ([§5.4](./language.md#54-사용-위치-제약)). 위치는 `try` |
| `` let-else cannot be used here — its `else` block's exit ... would leave this construct's own IIFE ... `` | `try`와 같은 제어 흐름 판정 — rl 구성물의 문장 영역에 직접 쓴 let-else만 에러이고, 그 자리에 쓴 함수 안에서는 허용됩니다. 모듈 최상위는 허용 ([§6.4](./language.md#64-사용-위치와-발산-제약)). 위치는 선언 키워드 |
| ``let-else: every path through the `else` block must diverge — ...`` | `else`가 발산하지 않으면 뒤의 구조 분해가 케이스 미보장 상태로 실행됩니다. 판정은 제어 흐름 기반입니다: 네 발산 키워드 외에 모든 분기가 발산하는 `if`/`else`(체인 포함)와 발산하는 중첩 블록, 발산 뒤의 도달 불가 코드도 인정되고, 루프·`switch`·`try`는 보수적으로 비발산입니다. 객체 리터럴·화살표 본문의 `}`는 문장을 끝내지 않으므로 `return { … };`는 하나의 `return`입니다 ([§6.4](./language.md#64-사용-위치와-발산-제약)). 위치는 `else` |

```rl
function f(): number {
  const Some(v) = find() else { log(); };
  return v;
}
// rlc: file.rl:2:26: let-else: the `else` block must end with a `return`, ...
```

모듈 최상위의 `try`/let-else는 이 검사로 잡히지 않고, 최상위 `return`이 유효한
TS가 아니어서 아래 출력 검증 에러로 드러납니다.

## if let

| 메시지 | 원인과 해결 |
|--------|-------------|
| `` `if let` could not be parsed here (pattern parens are mandatory, and the `else` must be a block or another `if let`) `` | `if let`으로 시작했지만 문으로 완전히 파싱되지 않았습니다. `if` 뒤의 `let`은 유효한 TS에 없어 통과시킬 수 없으므로 위치와 함께 에러입니다. 흔한 원인: 패턴 괄호 누락(`if let Some = ...`), `else if (조건)` 이어 붙이기(else 블록 안으로 옮깁니다), `= 식` 최상위의 괄호 없는 블록 화살표. 위치는 `if` |
| `` `if let` cannot be used in expression position (...) — it compiles to a block statement ... `` | 템플릿 보간·스크루티니·가드·표현식 암 본문·`try` 식·파이프라인의 **표현식 자리에 직접** 쓸 수 없습니다. 문장 위치(match 암의 블록 본문 포함)로 옮기거나, 그 자리에 쓴 함수의 본문 안에 넣으세요(제어 흐름 판정 — [§6.5](./language.md#65-if-let-문--조건부-값-추출)) |
| `` match: binding `<이름>` is used more than once in this pattern ... `` | match와 같은 패턴 규칙 — 별칭으로 해소합니다 |

## 파이프라인

| 메시지 | 원인과 해결 |
|--------|-------------|
| `` pipeline: `|>` could not be parsed here (steps must be expressions; parenthesize ternaries and arrow functions) `` | `\|>`가 파이프라인으로 완전히 파싱되지 않았습니다. `\|>`는 유효한 TS에 존재할 수 없어 통과시킬 수 없으므로 위치와 함께 에러입니다. 흔한 원인: head/step 최상위의 삼항이나 괄호 없는 화살표(괄호로 감쌉니다 — `(c ? a : b) \|> f`, `x \|> (n => n + 1)`), 빈 스텝(`x \|>;`), `?.` 시작 스텝. 위치는 `\|>` |

| `` `flow`: the first step cannot be a method step — it is the composed function's input, so it must be a function (`flow \|> ((s: string) => s.trim()) \|> ...`) `` | 합성(`flow`)에는 아직 값이 없어 첫 스텝에 메서드 체인을 걸 수 없습니다. 입력을 받는 함수를 첫 스텝으로 쓰거나(`flow \|> parse \|> .trim()`), 괄호 화살표로 감쌉니다. 위치는 첫 스텝의 `.` ([`language.md` §7.5](./language.md#75-함수-합성-flow)) |

head/step 내부의 `try` 문은 위의 try 위치 제약 에러로 보고됩니다
([`language.md` §7.4](./language.md#74-구조-규칙)).

## `result` 블록

| 메시지 | 원인과 해결 |
|--------|-------------|
| `` `result` block could not be parsed here (every binding is `const <binding> <- <expression>;`, and the block must end with an expression) `` | Result 바인딩(`const x <- 식;`)이 있는 `result` 블록이 완전하게 파싱되지 않았습니다. 선언 키워드 뒤의 `<-`는 유효한 TS에 없어 통과시킬 수 없으므로 위치와 함께 에러입니다. 흔한 원인: 바인딩의 `;` 누락, 마지막 값 식 없음(또는 값 식에 `;`를 붙임), `<-` 뒤 식 없음, 바인딩 이름 없음. 위치는 `result` ([`language.md` §8.4](./language.md#84-구조-규칙)) |

| `` `<-` binding must be a top-level statement of the `result` block — ... `` | 바인딩을 블록 최상위가 아닌 곳(`if` 본문, 루프, 블록 안에 쓴 함수)에 썼습니다. 바인딩은 블록 IIFE의 early return으로 컴파일되므로 최상위 문장만 될 수 있습니다 — 끌어올리거나 `match`를 쓰세요. 위치는 선언 키워드 ([`language.md` §8.4](./language.md#84-구조-규칙)) |
| `` `result` binding is missing its declaration keyword (write `const <binding> <- <expression>;`, or `let`/`var`) `` | `b <- f();`처럼 선언 키워드 없이 바인딩을 썼습니다. 이 자리에 `const`(또는 `let`/`var`)를 붙입니다. `b < -f()` **비교**를 쓰려던 것이라면 `<`와 `-` 사이에 공백을 둡니다 — rl이 바인딩으로 보는 것은 붙여 쓴 `<-`뿐입니다. 위치는 이름 ([`language.md` §8.4](./language.md#84-구조-규칙)) |

블록 안의 `try` 문·let-else는 위의 위치 제약 에러로 보고됩니다 — 그 자리의
`return`은 둘러싼 함수가 아니라 블록에서 나가기 때문입니다. `Err` 전파는
`<-` 바인딩으로 씁니다.

```rl
const a = result {
  const user <- getUser(id);
};
// rlc: file.rl:1:11: `result` block could not be parsed here (every binding is
//      `const <binding> <- <expression>;`, and the block must end with an expression)
```

```rl
const a = result {
  const x <- f();
  y <- g();
  x + y
};
// rlc: file.rl:3:3: `result` binding is missing its declaration keyword
//      (write `const <binding> <- <expression>;`, or `let`/`var`)
```

```rl
const a = c ? x : y |> f;
// rlc: file.rl:1:21: pipeline: `|>` could not be parsed here (steps must be
//      expressions; parenthesize ternaries and arrow functions)
```

## `val`

| 메시지 | 원인과 해결 |
|--------|-------------|
| `` cannot mutate through val binding `<이름>` (the binding is declared with `val`, so every access path from it is read-only) `` | `val` 바인딩에서 시작하는 경로로 대입·증감·`delete`를 했습니다. 위치는 경로의 **루트 식별자**. 변경이 필요하면 `val`을 빼거나, 변경 가능한 다른 바인딩을 통하거나, 새 값을 만들어 교체합니다(`val let`이면 재할당은 가능) |
| `` cannot call mutating method `<메서드>` through val binding `<이름>` (...) `` | **`--check-types`/`--types`에서만** 나옵니다. `val` 경로로 호출한 메서드를 TypeScript가 **자신이 선언한 것**으로 확인했습니다 (`Array`/`Map`/`Set`/`WeakMap`/`WeakSet`/TypedArray의 변경 메서드). 같은 이름의 **사용자 정의 메서드는 걸리지 않습니다** — 판정 근거는 이름이 아니라 수신자의 선언입니다 ([`language.md` §10.4](./language.md#104-built-in-변경-메서드---types)) |
| `` cannot pass val binding `<이름>` to mutable parameter `<매개변수>` of `<함수>` (the parameter is not declared with `val`, so the function may mutate through it) `` | `val` 바인딩을 `val`이 아닌 매개변수로 넘겼습니다. 위치는 인자. 그 함수가 인자를 변경하지 않는다면 매개변수를 `val`로 선언합니다 ([`language.md` §10.5](./language.md#105-함수-경계)) |

```rl
val const user = { name: "Kim" };
user.name = "Lee";
// rlc: file.rl:2:1: cannot mutate through val binding `user` (the binding is
//      declared with `val`, so every access path from it is read-only)
```

`val`이 없는 바인딩에는 아무 검사도 걸리지 않습니다 — 기존 TypeScript 그대로
변경 가능합니다. `val` 바인딩 자체의 재할당(`x = v`)도 이 검사 대상이 아닙니다
(`const`면 tsc가 잡습니다). 안쪽 스코프의 같은 이름 선언은 바깥 `val`을 가리므로
섀도잉된 이름은 에러가 되지 않습니다 ([`language.md` §10.5](./language.md#105-스코프와-섀도잉)).

`val`은 매개변수와 선언 앞에서만 수식자입니다. 그 밖의 `val`은 평범한
식별자이므로 rl 구문으로 해석되지 않고 통과합니다 — `Ok(val user)` 같은 match
패턴에 쓰면 그 match가 파싱되지 않아 아래 **출력 검증** 에러로 드러납니다.

## 출력 검증

생성물 자가 검사 실패입니다 — ① 통과 영역의 소스가 애초에 유효한 TS가 아니었거나
(검증기가 아직 모르는 최신 문법 포함) ③ rlc의 버그.

swc는 **생성물**의 위치를 말하지만 사용자가 여는 파일은 `.rl`이므로, 그 위치는
방출 매핑을 타고 원본으로 돌아옵니다 — 글루에서 났다면 그 글루를 쓴 구문으로.
즉 이 에러도 다른 에러처럼 `.rl`의 행·열을 갖습니다.

`match`와 `enum`은 rl 구문으로 확정된 뒤 파싱에 실패하면 각각
``rl `match` could not be parsed (...)``와 ``rl `enum` could not be parsed (...)``를
직접 보고합니다. 출력 검증은 rl로 확정되지 않은 통과 영역의 백스톱입니다.

통과 영역 근처의 rl 키워드를 찾을 수 있으면 그 구문을 지목합니다:

```
`match` here did not parse as an rl `match`, so it was passed through as
TypeScript and the generated module no longer parses: <상세>
```

```rl
const a = match s { Circle(r) => r };   // 스크루티니 괄호가 없다
// rlc: file.rl:1:11: `match` here did not parse as an rl `match`, ...
```

그 외에는 원래 문장 그대로입니다:

```
generated TypeScript failed to parse: <상세>. This is either invalid TypeScript
passed through from the source or an rlc bug; use --no-verify to bypass.
```

소스가 유효한 TS인데도 발생하면 rlc 버그이므로 제보해 주세요.

## 타입 에러 (tsc)

타입 관계의 판정은 TypeScript checker가 합니다. rlc는 checker가 확인한
기대 타입·실제 타입·대입 불가능 관계를 구조화된 사실로 받아, 가장 작은 타입
차이를 **원본 `.rl`의 행·열**에서 표시합니다.

| 어디서 | 형식 | 비고 |
|--------|------|------|
| `rlc --check-types` / `--types` | `rlc: <파일>.rl:<행>:<열>: type mismatch: expected ...` | 타입 에러가 있어도 `--types`의 사이드카는 방출되고 종료 코드만 1 ([`cli.md`](./cli.md#타입-검사---check-types---types)) |
| VSCode 확장 | CLI와 같은 메시지, `source: ts`, `code: ts<번호>` | `rl.typeDiagnostics`로 끌 수 있음 |

두 경로 모두 `match` 암·`|>` 파이프라인·`try`/let-else/`if let` **안쪽**의
타입 에러까지 잡습니다. 방출물은 순수 TypeScript이므로 rl 구문이 타입 추론을
가리지 않습니다.

```rl
function parse(): Result<number, InputError> {
  return Result.Err(RangeError.TooLarge(70000, 65535));
}
// type mismatch: expected `InputError`, found `RangeError`
//   required type: `Result<number, InputError>`
```

제네릭·유니언의 바깥 모양을 반복하지 않고 checker가 증명한 첫 불일치 타입을
주 메시지로 냅니다. 전체 기대 타입이 맥락에 필요하면 `required type`으로 한 번만
붙입니다. 구조화할 수 없는 TypeScript 진단은 원문을 그대로 전달합니다.

### 생성된 코드에서 난 타입 에러

사용자 코드가 잘못됐을 때 checker가 **rlc가 쓴 글루**에서 여러 에러를 낼 수
있습니다. rlc는 같은 구문에서 확인된 기대/실제 타입 관계를 원인으로 선택하고,
그 결과로 생긴 프로퍼티·비교 에러는 표시하지 않습니다.

| 구문 | tsc 코드 | rlc가 하는 말 |
|------|----------|---------------|
| `try` | 2339·2551·2571 | `` `try` needs a `Result` — this expression is not one `` |
| 모든 구문 | 구조화된 대입 불일치 | ``type mismatch: expected `<타입>`, found `<타입>` `` |
| `result`의 `<-` | 2339·2551·2571 | `` `<-` needs a `Result` — this expression is not one `` |
| let-else / `if let` | 2339·2571 | `` ... needs a value with a `kind` discriminant — this expression has none `` |
| `match` | 2339·2571 | `` match on a tag pattern needs a value with a `kind` discriminant — this scrutinee has none (a plain TypeScript `enum` is not one) `` |
| `match` / let-else / `if let` | 2678·2367 | `this pattern's case is not one the value can be` |

```
rlc: f.rl:2:13: `try` needs a `Result` — this expression is not one
     (ts2339: Property 'kind' does not exist on type 'number'.)
```

- 구조화된 대입 불일치는 TypeScript 진단 문장을 파싱하지 않습니다. checker의
  타입 객체와 `isTypeAssignableTo` 결과를 사용합니다.
- 표에 없는 코드는 **옮기지 않고** 그대로 전달합니다(`(in code rlc generated
  for this construct)` 꼬리표가 붙습니다). 아닌 것을 아는 척하는 것보다
  못생긴 메시지가 낫습니다.
- 한 구문의 글루에서 원인 타입 불일치가 확인되면 그 원인 하나만 표시합니다.
- 매핑이 있는 자리(사용자가 쓴 텍스트)의 타입 에러는 **옮기지 않습니다** —
  같은 구조화 렌더러로 원래 위치에서 표시합니다.

#### 구조적 타입을 선언 이름으로

checker에는 rl 케이스를 가리킬 말이 없습니다. rlc는 구조화된 타입 사실을
선언 표와 대조해 낮아진 객체 타입을 `Wire.OutOfRange` 같은 이름으로 바꿉니다.

```
rlc: a.rl:12:13: type mismatch: expected `ParseError`, found `Wire.OutOfRange`
  required type: `Result<number, ParseError>`
```

이름은 선언 표(파일의 enum + 임포트가 이름 붙여 들여온 enum + 내장
`Option`/`Result`)가 **유일하게** 지목할 때만 붙습니다:

- 같은 태그를 두 enum이 선언했으면 **이름을 붙이지 않습니다**. 어느 쪽인지
  말하는 게 목적인데 둘 중 찍는 건 아무 말도 아닙니다.
- 필드 이름이 그 케이스의 페이로드와 정확히 같지 않으면 붙이지 않습니다 —
  태그만 우연히 같은 다른 타입입니다.
- 한 enum의 케이스 전부가 유니언으로 나오면 그 enum 이름 하나로 줄이고
  (`ParseError`), 일부만 나오면 케이스들의 유니언으로 씁니다
  (`ParseError.NotANumber | ParseError.Overflow`).
- 임포트한 enum은 **임포트가 준 이름**으로 부릅니다
  (`import { Wire as W }` → `W.OutOfRange`).

이름을 유일하게 확인할 수 없으면 checker가 준 구조적 타입 표기를 유지합니다.

## CLI

컴파일 이전 단계의 에러입니다. 전부 stderr로 나가고 종료 코드 1입니다.

| 메시지 | 원인 / 해결 |
|--------|-------------|
| `--out-dir requires a value` | `-o` 뒤에 디렉터리가 없음 |
| `--node requires a path to the node binary` | `--node` 뒤에 경로가 없음 |
| `--rewrite-imports requires a value (js, ts, or off)` / `... expects js, ts, or off (got <값>)` | 값이 없거나 셋 중 하나가 아님 |
| `--sidecar requires a directory of tsc-emitted .d.ts files` | `--sidecar` 뒤에 디렉터리가 없음 |
| `--emit-std takes no inputs (the build materializes @rl/std itself)` | stdout 전용 단독 모드 — 빌드에서는 자동 방출이 대신합니다 |
| `--types/--check-types does not combine with -p, --check, --symbols, --emit-map, or --sidecar` | 타입 검사 모드는 자체 파이프라인 (`-w`·`--project`·`-o`는 조합 가능) |
| `--overlay requires the path the buffer belongs to` | `--overlay` 뒤에 경로가 없음 |
| `--overlay and --rl-only require --check-types` | 편집 중인 버퍼를 묻는 옵션이므로 그 모드에서만 의미가 있습니다 |
| `--overlay and --rl-only work with --check-types, not --types` | `--types`는 사이드카를 씁니다 — 저장되지 않은 텍스트가 거기 들어가면 안 됩니다 |
| `--overlay does not combine with --watch` | 감시는 디스크를 다시 읽는데 stdin의 텍스트는 영원히 그대로입니다 |
| `--overlay <경로>: <이유>` | 오버레이가 대신할 파일이 실재하지 않음 — 아직 저장된 적 없는 버퍼는 프로젝트 그래프에 자리가 없습니다 |
| `cannot read the overlay from stdin: <이유>` | `--overlay`의 텍스트를 stdin에서 읽지 못함 |
| `unknown option <옵션>` | 알 수 없는 `-` 시작 인자. `rlc -h` 참조 |
| `` unknown help topic "<주제>" (run `rlc help` for the list) `` | `rlc help <주제>`의 주제가 목록에 없음. `rlc help`로 주제·별칭 확인 |
| `` help takes at most one topic (run `rlc help` for the list) `` | `rlc help`에 주제를 둘 이상 넘김 |
| `no such file or directory: <경로>` | 입력 경로가 없음 |
| `no sources found` | 입력에서 컴파일할 파일을 찾지 못함 |
| `<경로>: output would overwrite the input — pass -o <dir>` | 통과 `.ts`를 제자리 컴파일하면 소스를 덮어씀. `-o`로 출력 트리를 분리 |
| `no TypeScript compiler found — install one (npm i -D typescript@7)` | 타입 검사 모드가 구동할 TypeScript를 해석하지 못함 ([`cli.md`](./cli.md#컴파일러-해석)) |
| `rlc host: the resolved TypeScript has no declaration emit API` | 해석된 TypeScript로는 검사는 되지만 선언 방출이 안 됨 — `--check-types`는 되고 `--types`의 사이드카 쓰기만 막힙니다 |
| `<경로>: <OS 에러>` | 파일 IO 실패. 해당 파일만 건너뛰고 계속 진행한 뒤 1로 종료 |

인자 에러와 없는 경로는 즉시 종료하고, IO 에러는 파일 단위로 건너뛰며 계속
처리합니다.
