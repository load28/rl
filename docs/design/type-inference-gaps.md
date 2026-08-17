# TypeScript↔Rust 타입 추론 격차와 rl 기능 제안

TypeScript의 타입 추론이 Rust에 미치지 못하는 지점들을 tsc 실측으로
분류하고, 그중 rl이 — `match`처럼 런타임 코드를 방출하는 구문을 포함해 —
메울 수 있는 것들을 기능으로 제안합니다. 이 문서는 제안이며 규범이 아닙니다.
채택된 항목은 구현 태스크에서 [`docs/reference/`](../reference/)로 옮깁니다.

실측 환경: tsc 6.0.2, `--strict --noEmit --target es2022`.

---

## 1. 판단 기준

rl이 격차를 "메울 수 있다"의 기준은 설계 계약([`CLAUDE.md`](../../CLAUDE.md))
그대로입니다:

1. rlc는 **타입을 모른다** — 구조(토큰·태그·바인딩 이름)만 안다. 타입이
   필요한 검사는 방출 형태를 통해 tsc가 하도록 유도하되, **방출 코드가 tsc
   에러를 만들면 안 된다.**
2. rl 수준 에러(소진성·중복 등)는 rlc가 `파일:행:열`로 직접 보고한다.
3. 모든 유효한 TS는 그대로 통과한다 — 새 구문은 유효 TS와 충돌하지 않는
   자리에만 놓을 수 있다.

이 기준으로 격차를 세 부류로 나눕니다:

- **[B] 이미 메움** — 기존 rl 구문이 해결.
- **[P] 제안** — 런타임 방출을 포함한 새 구문으로 메울 수 있음 (§3–§7).
- **[C] 전처리로 못 메움** — 타입 정보나 소유권 개념이 필요 (§8).

## 2. 격차 카탈로그 (실측)

### G1. 후방 추론(단일화) 부재 — [C]

Rust는 Hindley-Milner식 단일화로 **이후 사용처**가 타입을 확정합니다:

```rust
let mut v = Vec::new();   // 이 시점엔 Vec<?>
v.push(1);                // 여기서 Vec<i32>로 단일화
```

TS는 좌→우 단방향입니다. 제네릭 호출 시점에 추론 소스가 없으면 그 자리에서
타입 인자가 붕괴하고, 이후 사용처는 소급 적용되지 않습니다:

```ts
function make<T>() { return [] as T[]; }
const ys = make();          // T = unknown으로 즉시 확정
ys.push(1);                 // 소급되지 않음
const n: number = ys[0];    // error TS2322: Type 'unknown' is not assignable to type 'number'.
```

전처리기가 개입하려면 사용처의 타입을 알아야 하므로 rl 범위 밖입니다.
콜백 커링에서의 같은 붕괴(`TS18046`)는 파이프라인 제안
([TASK-013](../tasks/TASK-013-pipeline-operator-proposal.md))이 `$rl_ap` 헬퍼
방출로 우회한 바 있습니다 — "방출 형태를 바꿔 tsc의 문맥 추론이 작동하는
자리로 옮긴다"는 그 전략이 이 문서의 [P] 제안들의 공통 원리입니다.

### G2. 소진성 검사 부재 — [B], 리터럴로 확장은 [P3]

Rust의 `match`는 빠진 케이스가 컴파일 에러입니다. TS의 `switch`는 누락이
**에러 없이** 추론 반환 타입에 `| undefined`로 흡수되고, 멀리 떨어진
사용처에서야 다른 얼굴로 드러납니다:

```ts
type Dir = "n" | "s" | "e" | "w";
function label(d: Dir) {
  switch (d) {
    case "n": return "north";
    case "s": return "south";
  }                               // 에러 없음 — 반환 타입이 string | undefined로 조용히 넓어짐
}
const out: string = label("e");   // error TS2322 — 원인에서 멀리 떨어진 지점
```

태그드 유니언에 대해서는 rl `match`의 소진성 검사(§3.6, `언어.md`)가 이미
원인 지점에 `파일:행:열`로 보고합니다. 남은 격차는 **태그드 유니언이 아닌
스크루티니**(문자열·숫자 리터럴 유니언)로, §5의 리터럴 패턴 제안이 다룹니다.

### G3. 다중 값 동시 매치 불가 — [P1]

Rust는 `match (a, b)`로 두 값을 한 번에 매치하고 **곱집합 전체의 소진성**을
검사합니다. TS는 대응 구문이 없고, 튜플로 묶어 좁혀도 원본 변수에는
전파되지 않습니다:

```ts
declare const a: Opt<number>;
declare const b: Opt<string>;
const pair = [a, b] as const;
if (pair[0].kind === "Some" && pair[1].kind === "Some") {
  const x: number = a.value;   // error TS2339: Property 'value' does not exist on type 'Opt<number>'.
}
```

결국 2ⁿ개 조합의 if-체인을 손으로 쓰고, 조합 누락은 아무도 검사하지
않습니다. §3의 튜플 match 제안이 다룹니다.

### G4. 중첩 판별 — 내로잉은 되나 구문·소진성 격차 [P2]

정직하게: 조건 체인의 중첩 내로잉 자체는 TS가 **해냅니다** (실측 에러 없음):

```ts
declare const r: Res<Opt<number>, string>;
if (r.kind === "Ok" && r.value.kind === "Some") {
  const n: number = r.value.value;   // OK
}
```

격차는 추론이 아니라 (1) 한 단계마다 조건·분기가 하나씩 늘어나는 구문 비용,
(2) 중첩 공간 전체를 덮었는지 아무도 검사하지 않는다는 점입니다. Rust는
`Ok(Some(v))` 한 패턴과 소진성 검사로 둘 다 해결합니다. §4가 다룹니다.

### G5. 클로저 경계에서 내로잉 소실 — 원칙으로 완화, [P4]

TS 6 실측 결과는 통념보다 낫습니다 — 프로퍼티 내로잉이 함수 호출과 `await`
경계에서는 **유지**됩니다 (건전성을 희생한 편의). 소실되는 곳은 클로저
경계입니다:

```ts
function viaClosure(x: { v: string | null }) {
  if (x.v === null) return;
  [1].forEach(() => x.v.length);   // error TS18047: 'x.v' is possibly 'null'.
}
```

이후 재할당이 있는 `let` 지역 변수도 같은 이유로 소실됩니다(실측 동일).
Rust는 소유권/차용으로 이 문제 자체가 없습니다. 근본 해결은 [C]지만, rl은
방출 원칙으로 완화합니다: **rl 구문은 좁힌 값을 항상 새 `const`로 물질화**
합니다 — `match` 암의 `const { radius } = $rl_m`, let-else의 구조 분해가
그것이고, `const`는 클로저 안에서도 좁혀진 타입을 유지합니다. §6의 `if let`은
"조건부 스코프에서의 물질화"를 추가해 이 원칙을 완성합니다.

### G6. `catch`는 `unknown` — [B], 관용구는 [P5]

Rust는 `Result<T, E>`로 에러 타입이 추론에 참여합니다. TS의 `catch` 변수는
`unknown`이고(실측: `error TS18046: 'e' is of type 'unknown'`), 좁히기는
instanceof 사다리를 손으로 쌓아야 합니다. 값 수준 해법은 rl `try`/`Result`가
이미 제공합니다. 남은 것은 **어쩔 수 없이 `unknown`을 받는 자리**(catch,
이벤트, JSON)의 관용구로, §7의 `is` 패턴이 다룹니다.

### G7. 리터럴 확장(widening) — 대체로 [B]

```ts
function run(mode: "fast" | "slow") {}
const cfg = { mode: "fast" };   // { mode: string }으로 확장
run(cfg.mode);                  // error TS2345
```

Rust에는 대응 문제가 없습니다(리터럴이 아니라 enum을 쓰므로). rl의 답도
같습니다 — stringly-typed 유니언 대신 rl `enum`을 쓰면 확장 문제가 생기지
않습니다. 별도 기능은 제안하지 않습니다.

---

## 3. 제안 P1: 튜플 match — 다중 스크루티니와 곱집합 소진성

> **상태: 구현됨** (TASK-044) — 규범은
> [`language.md` §3.7](../reference/language.md#37-튜플-match--다중-스크루티니).
> 방출은 중첩 switch 대신 if-체인으로 확정 (§3.4와 다름 — 구현 태스크 결정 2).

다섯 제안 중 가장 큽니다. **곱집합 소진성은 TS가 어떤 타입 트릭으로도
제공하지 못하는 것**이면서, rlc에게는 태그 집합의 곱이라는 순수 구조
검사입니다 — rl의 제약과 강점에 정확히 맞습니다.

### 3.1 문법

```
match-식     ::= "match" "(" 식 ("," 식)+ ")" "{" 튜플-암-목록 "}"
튜플-암      ::= 튜플-패턴 가드? "=>" 본문
튜플-패턴    ::= "(" 패턴 ("," 패턴)+ ")"     // 원소 수 = 스크루티니 수
패턴         ::= 기존 태그-패턴 | "_"          // 원소 자리의 _ 허용
```

```rl
const step = match (dir, speed) {
  (North, Fast) => 2,
  (North, Slow) => 1,
  (South, _)    => -1,
  (East | West, s) if isBlocked(s) => 0,
  (East | West, _) => 1,
};
```

### 3.2 판별과 하위호환

`match (a, b)`는 현재도 파싱됩니다 — 스크루티니가 콤마 식인 단일 match로.
재해석은 **암 패턴 주도**로 합니다: 암에 튜플-패턴(`(` 로 시작하는 패턴)이
하나라도 있어야 튜플 match이고, 그때 스크루티니의 최상위 콤마가 구분자가
됩니다. 튜플-패턴 없는 `match (a, b) { Tag => ... }`는 지금처럼 콤마 식
스크루티니입니다. 통과 계약은 영향 없습니다(`match (x) {...}` 자체가 유효
TS가 아니므로). 원소 수 불일치·`(_, _)`가 아닌 최상위 `_` 혼용은 rlc 에러.

### 3.3 소진성

각 원소 위치의 enum을 기존 규칙(로컬 > 임포트 > 내장)으로 해석해 태그
곱집합을 만들고, 무가드 암들의 커버 합집합과 대조합니다. 원소 `_`와
or-패턴은 그 위치의 전 태그를 커버합니다. 빠진 조합은 그대로 보고합니다:

```
rlc: nav.rl:4:14: match on (Dir, Speed) is not exhaustive: missing (South, Fast)
     (add the missing arms or a final `(_, _)` arm)
```

곱집합 크기는 태그 수의 곱이라 폭발하지 않습니다(커버 집합 연산은 비트셋).

### 3.4 컴파일 결과

스크루티니를 각각 한 번 평가한 뒤 첫 원소로 `switch`, 안에서 두 번째 원소로
`switch`(가드가 있으면 기존 규칙대로 if-체인 IIFE):

```ts
const step = ((() => {
  const $rl_m0 = (dir); const $rl_m1 = (speed);
  switch ($rl_m0.kind) {
    case "North": switch ($rl_m1.kind) {
      case "Fast": { return (2); }
      case "Slow": { return (1); }
    }
    ...
    default: { throw new Error("rl match: unexpected case " + JSON.stringify([$rl_m0, $rl_m1])); }
  }
})());
```

바인딩·구조 분해·`await` 감지·블록 본문 규칙은 단일 match와 동일합니다.

---

## 4. 제안 P2: 중첩 패턴 — `Ok(value: Some(v))`

> **상태: 구현됨** (TASK-045) — 규범은
> [`language.md` §3.2/§3.6](../reference/language.md#32-의미). 유닛 케이스
> 중첩은 별칭과의 문법 충돌 때문에 괄호 필수(`value: None()`)로 확정.

### 4.1 문법

바인딩 문법의 `":"` 우측을 별칭에서 **별칭 또는 패턴**으로 확장합니다:

```
바인딩 ::= 필드명 | 필드명 ":" 식별자 | 필드명 ":" 태그-패턴
```

```rl
const n = match (r) {
  Ok(value: Some(value: v)) => v,
  Ok(value: None)           => 0,
  Err(error)                => { log(error); return -1; },
};
```

식별자 하나는 지금처럼 별칭이고, `(`가 따라오면 중첩 패턴입니다 — 현재
문법에서 `필드명: 식별자(` 형태는 없으므로 하위호환 파손이 없습니다.

### 4.2 의미와 방출

중첩 패턴 불일치는 **다음 암으로 폴스루**합니다 — 가드 실패와 같은
의미이므로, 중첩 패턴이 있는 match는 가드 match와 같은 **if-체인 IIFE**로
방출합니다(기존 기계 재사용):

```ts
if ($rl_m.kind === "Ok" && $rl_m.value.kind === "Some") { const v = $rl_m.value.value; return (v); }
```

G4에서 봤듯 이 조건 체인 형태는 tsc가 완전하게 좁히므로 타입 트릭이
필요 없습니다.

### 4.3 소진성 (보수적 v1)

중첩 패턴이 달린 암은 **가드 암과 동일하게 케이스를 커버하지 못합니다**
(내부 태그가 다를 수 있으므로). 위 예시가 검사를 통과하려면
`Ok(value: Some(...))`·`Ok(value: None)`처럼 같은 외부 태그의 암들이 내부
공간을 덮어야 하는데, v1에서는 이를 검사하지 않고 무가드·무중첩 암(또는
`_`)이 커버를 결정합니다. 내부 곱 소진성(외부 태그별로 내부 태그 집합 검사)
은 구현 경험 후 v2로 미룹니다 — 보수적 방향의 오류는 "빠졌다고 잘못 보고"
쪽이라 안전합니다.

---

## 5. 제안 P3: 리터럴 패턴 — 태그드 유니언 밖의 match

### 5.1 문법과 판별

```rl
const n = match (key) {
  "ArrowUp"                => -1,
  "ArrowDown"              =>  1,
  "Home" | "End"           =>  0,
  _                        =>  NaN,
};
```

패턴 자리가 문자열·숫자 리터럴, `true`/`false`(예약어지만 패턴 자리 한정
허용) 또는 `-` 숫자면 리터럴 match입니다. 리터럴 패턴과 태그 패턴은 한
match에서 섞을 수 없습니다(rlc 에러). 현재 이런 입력은 rl 구문으로 파싱되지
않아 통과 후 verify에서 실패하므로, 기존 유효 프로그램의 의미 변화가
없습니다.

### 5.2 소진성: `_` 필수 (계약 유지)

rlc는 스크루티니의 타입을 모르므로 리터럴 유니언의 전체 집합을 알 수
없습니다. 검토한 대안:

- **(a) `_` 암 필수** — rlc가 구조로 강제 가능. G2의 "조용한 `| undefined`"
  는 막지 못하지만 표현식·단일 평가·물질화는 얻는다.
- (b) `_` 생략 시 `default`에 `const $rl_x: never = $rl_m;`을 방출해 tsc가
  누락을 보고 — Rust 수준 소진성을 얻지만 **방출 코드가 tsc 에러를 만들면
  안 된다**는 계약 2를 정면으로 깬다. 에러 위치도 생성물 기준이 된다.

**권고는 (a)입니다.** (b)는 계약 개정 없이는 불가하며, 개정 가치가 있는지는
(a) 사용 경험 후 별도 제안으로 판단합니다.

### 5.3 컴파일 결과

`kind` 접근 없이 값 자체를 switch합니다. 나머지(or-패턴 폴스루, 가드
if-체인, 블록 본문)는 기존과 동일합니다:

```ts
switch ($rl_m) { case "ArrowUp": { return (-1); } ... default: { return (NaN); } }
```

---

## 6. 제안 P4: `if let` — 조건부 스코프의 값 추출

> **상태: 구현됨** (TASK-046) — 규범은
> [`language.md` §6.5](../reference/language.md#65-if-let-문--조건부-값-추출).
> 제안과 달리 중첩 패턴도 지원하고, 표현식 위치 금지는 sema의 문맥 구분
> (Top/Stmt/Expr)으로 강제한다.

### 6.1 문법과 의미

```
if-let-문 ::= "if" "let" 패턴 "=" 식 블록 ("else" (블록 | if-let-문))?
패턴      ::= let-else와 동일 (태그 "(" 바인딩-목록? ")")
```

```rl
if let Some(value: user) = findUser(id) {
  greet(user);
} else if let Some(value: cached) = cache.get(id) {
  greet(cached);
} else {
  prompt();
}
```

let-else가 "불일치 시 반드시 이탈"이라면 `if let`은 이탈 의무가 없는 짝입니다.
`else` 생략·`else if let` 체이닝을 허용하고, 발산 검사는 없습니다.

### 6.2 통과 계약과 방출

유효 TS에서 `if` 뒤에는 반드시 `(`가 오므로 `if let`은 유효 TS와 충돌하지
않습니다. `let`이 예약어라 태그로 오인될 여지도 없습니다. 방출은 블록 스코프
+ 물질화(한 줄, `$rl_t` 공유):

```ts
{ const $rl_t0 = (findUser(id)); if ($rl_t0.kind === "Some") { const { value: user } = $rl_t0; greet(user); } else ... }
```

바인딩이 `const`로 물질화되므로 본문 안의 클로저에서도 좁혀진 타입이
유지됩니다 — G5 완화 원칙의 조건부 스코프 버전입니다. `try`/let-else와 같은
문장 위치 제약(§5.4, `언어.md`)을 따르되, IIFE가 없으므로 본문 내
`return`은 둘러싼 함수에서 그대로 동작합니다.

---

## 7. 제안 P5: `is` 패턴 — `unknown`과 클래스 계층 match

### 7.1 문법과 의미

```rl
const msg = match (err) {
  is SyntaxError(message)  => `bad syntax: ${message}`,
  is RangeError | is TypeError => "bad value",
  is Error(message)        => message,
  _                        => String(err),
};
```

패턴 앞의 `is` 키워드(문맥 키워드 — 태그 자리 뒤에 식별자가 또 오는 형태는
현재 문법에 없음)가 instanceof 매치를 나타냅니다. 선언 순서대로 검사하므로
서브클래스를 위에 씁니다. 클래스 계층은 열려 있어 소진성 검사가 원리상
불가능하므로 **`_` 암 필수**입니다(rlc 구조 검사). `is`와 태그 패턴은 한
match에서 혼용 불가.

### 7.2 방출

if-체인 IIFE(가드 기계 재사용) + instanceof + 물질화:

```ts
if ($rl_m instanceof SyntaxError) { const { message } = $rl_m; return (`bad syntax: ${message}`); }
```

instanceof는 tsc의 표준 내로잉이므로 구조 분해가 타입 트릭 없이 통과합니다.
바인딩은 태그 패턴과 같은 이름 기준이며, 좁혀진 클래스의 프로퍼티명과
일치해야 합니다(불일치는 tsc 책임 — G6의 값 자체가 `unknown`인 경우
`instanceof`가 유일한 표준 좁히기 수단이라는 점이 이 제안의 존재 이유입니다).

---

## 8. 전처리로 메울 수 없는 것 [C]

정직한 한계 목록입니다. 요청이 와도 rl 범위 밖으로 안내합니다.

| 격차 | 이유 |
|------|------|
| 후방 추론/단일화 (G1) | 사용처 타입 정보가 필요 — rlc는 타입을 모른다 |
| 정수 타입 (`i32`/`u8` 등) | 런타임 표현이 없고, 검사엔 타입 정보 필요. 방출로 흉내 내면 순수 TS 계약과 성능 모두 깨짐 |
| 클로저 내로잉의 일반해 (G5) | 소유권/차용 개념이 필요. rl은 물질화 원칙으로 완화만 한다 |
| 리터럴 match의 Rust 수준 소진성 (§5.2-b) | 가능은 하나 에러 계층 계약 개정이 선행 조건 |

## 9. 우선순위 권고

| 순위 | 제안 | 근거 |
|------|------|------|
| 1 | P1 튜플 match | TS로 불가능한 곱집합 소진성 = rl만의 가치. 기존 match 기반 위 확장이라 방출·검사 신설이 적다 |
| 2 | P4 `if let` | 구현이 가장 얇고(let-else 기계 재사용) let/try 가족을 완성. G5 완화의 마지막 조각 |
| 3 | P2 중첩 패턴 | 가드 if-체인 기계 재사용. 보수적 소진성 v1로 위험 낮음 |
| 4 | P5 `is` 패턴 | catch-`unknown` 관용구 해소. 소진성 없음이라 검사 신설 없음 |
| 5 | P3 리터럴 패턴 | 유용하나 `_` 필수 제약 하에서는 이득이 표현식화·물질화에 그침 |

각 제안은 독립적이라 개별 승인·개별 태스크로 진행 가능합니다.

## 10. 구현 시 공통 계획

- **테스트 세 계층**: compile.rs(방출 스냅샷 + rlc 에러), passthrough.rs
  (판별 경계 — 콤마 식 스크루티니, `if (`, `is` 없는 태그), integration.rs
  (tsc 타입체크 + node 실행, §2의 실측 케이스를 회귀 테스트로 이관).
- **문서**: `language.md`(문법·의미·방출), `errors.md`(신규 에러),
  필요시 `cli.md`. 레퍼런스와 어긋나면 버그라는 규칙 그대로.
- **AST/파서**: P1·P2·P3·P5는 `MatchExpr` 확장 + parser/matches.rs,
  P4는 parser/lets.rs 옆의 신규 모듈. 전부 무오류 구조 파싱 원칙 유지 —
  완전하게 파싱될 때만 변환, 아니면 통과.
