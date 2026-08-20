# rl 언어 레퍼런스

rl 문법과 의미의 규범 문서입니다. 구현이 이 문서와 어긋나면 버그입니다.
CLI는 [`cli.md`](./cli.md), 에러 메시지는 [`errors.md`](./errors.md), 표준
라이브러리 API는 [`std.md`](./std.md)를 보세요.

1. [기본 원칙](#1-기본-원칙) · 2. [`enum`](#2-enum-선언) ·
3. [`match`](#3-match-표현식) · 4. [표준 라이브러리](#4-표준-라이브러리와-내장-enum) ·
5. [`try`](#5-에러-전파-try-문) · 6. [`let-else`·`if let`](#6-값-추출-let-else-문) ·
7. [`|>`](#7-파이프라인-연산자-) · 8. [`result` 블록](#8-result-계산-블록) ·
9. [모듈](#9-모듈-rl-import-지정자-재작성) · 10. [`val`](#10-바인딩-수식자-val) ·
11. [제한사항](#11-제한사항)

---

## 1. 기본 원칙

`.rl` 파일은 TypeScript에 일곱 구문 — `enum` 선언, `match` 표현식, `try` 문,
let-else 문, `if let` 문, 파이프라인 연산자 `|>`, `result` 계산 블록 — 과 바인딩
수식자 `val` 하나를 더한 것입니다.

> **모든 유효한 TypeScript 파일은 그대로 유효한 `.rl` 파일이며, 자기 자신으로
> 컴파일됩니다.** 유일한 예외는 상대 경로 `.rl` import 지정자의 재작성입니다
> ([§9](#9-모듈-rl-import-지정자-재작성)).

컴파일러는 rl 구문으로 **완전하게 파싱되는** 부분만 변환하고 나머지는 바이트
그대로 통과시킵니다.

| 대상 | 결과 |
|------|------|
| 문자열·주석·정규식·템플릿 텍스트 안의 `enum`/`match` | 통과 |
| `str.match(...)` 등 `.` 뒤의 `match` | 통과 |
| TypeScript 자체의 모든 `enum` 형태 | 통과 ([§2.2](#22-rl-enum과-ts-enum의-구분)) |
| 템플릿 리터럴 `${ ... }` 보간 내부 | rl 구문 사용 가능 |

rl 구문 안의 식별자(태그·필드명·바인딩)는 ASCII(`[A-Za-z_$][A-Za-z0-9_$]*`)만
지원합니다. 그 밖의 위치에서는 유니코드를 자유롭게 씁니다.

에러는 원본 `.rl` 기준 `파일:행:열`(1-기반)로 보고됩니다.

### 1.1 예약어

다음 단어는 enum 이름, 케이스 태그, 필드명, match 패턴 태그, 바인딩, 별칭이
될 수 없습니다. 이 단어가 들어간 구문은 rl 구문으로 해석되지 않고 통과합니다
(에러가 아님).

```
async await break case catch class const continue debugger default delete
do else enum export extends false finally for function if import in
instanceof let new null of return static super switch this throw true try
typeof var void while with yield
```

---

## 2. `enum` 선언

### 2.1 문법

```
rl-enum      ::= "export"? "enum" 식별자 제네릭? "{" 케이스-목록 "}"
케이스-목록  ::= 케이스 ("," 케이스)* ","?
케이스       ::= 태그                         // 유닛
               | 태그 "(" 필드-목록? ")"      // 페이로드 (빈 괄호 허용)
필드-목록    ::= 필드 ("," 필드)* ","?
필드         ::= 이름 "?"? ":" 타입
```

제네릭은 제약·기본값·`const`/`in`/`out` 한정자를 그대로 지원하고, 타입 자리에는
임의의 TypeScript 타입 표기를 씁니다. 공백·주석·후행 콤마는 자유입니다.

### 2.2 rl enum과 TS enum의 구분

`enum` 선언은 다음 **둘 중 하나 이상**일 때만 rl enum입니다.

1. 케이스에 페이로드 괄호 `(...)`가 하나라도 있다 (빈 괄호 `Tag()` 포함)
2. 선언에 제네릭이 있다

그 외의 모든 `enum`(그리고 `const enum` / `declare enum`)은 TypeScript enum으로
통과합니다. 유효한 TS enum이 rl enum으로 오인되는 경우는 없습니다.

```rl
enum Color { Red, Green, Blue }        // TS enum — 통과
enum Status { Active(), Inactive }     // rl enum
```

### 2.3 컴파일 결과

rl enum 하나는 **같은 이름의 타입 별칭과 생성자 객체**로 컴파일됩니다.
`export`가 있으면 둘 다 export됩니다.

```rl
export enum Shape {
  Circle(radius: number),
  Rect(width: number, height: number),
  Point,
}
```

```ts
export type Shape =
  | { kind: "Circle"; radius: number }
  | { kind: "Rect"; width: number; height: number }
  | { kind: "Point" };
export const Shape = {
  Circle: (radius: number): Shape => ({ kind: "Circle", radius }),
  Rect: (width: number, height: number): Shape => ({ kind: "Rect", width, height }),
  Point: { kind: "Point" } as const,
};
```

| 케이스 형태 | 방출 |
|-------------|------|
| 필드 있음 `Circle(radius: number)` | 생성자 함수 — `Shape.Circle(1)` |
| 괄호 없음 `Point` | 싱글턴 값 — `Shape.Point` |
| 빈 괄호 `Active()` | 인자 없는 생성자 함수 — `Status.Active()` |

판별 필드는 항상 `kind`이고 값은 태그 문자열입니다. 제네릭은 생성자로 이어지며
(`Some: <T>(value: T): Option<T> => ...`), 제네릭 유닛 케이스(`Option.None`)는
모든 `Option<T>`에 대입 가능합니다.

### 2.4 컴파일 시점 검사

- **중복 케이스** — 같은 태그가 두 번 나오면 에러.
- **필드 타입 검증** — 타입 표기가 TypeScript 타입으로 파싱되지 않으면 에러
  (`--no-verify` / `Options { verify: false }`로 끌 수 있음).

---

## 3. `match` 표현식

### 3.1 문법

```
match-식   ::= "match" "(" 식 ")" "{" 암-목록 "}"
암-목록    ::= 암 ("," 암)* ","?
암         ::= 패턴 가드? "=>" 본문
가드       ::= "if" 식                     // `_`를 제외한 모든 패턴에
패턴       ::= 태그-패턴 ("|" 태그-패턴)*     // or-패턴
             | 리터럴 ("|" 리터럴)*          // 리터럴 or-패턴
             | "_"                         // 반드시 마지막 암
태그-패턴  ::= 태그 | 태그 "(" 바인딩-목록? ")"
리터럴     ::= 문자열 | 숫자 | "true" | "false"
바인딩     ::= 필드명 | 필드명 ":" 별칭 | 필드명 ":" 태그-패턴   // 중첩 패턴
본문       ::= 식 | "{" 문장* "}"
```

스크루티니 괄호는 필수이고 비어 있을 수 없습니다.

### 3.2 의미

`match`는 **표현식**입니다. 스크루티니를 한 번만 평가해 분기하고 선택된 암의
값으로 평가됩니다. 분기 기준은 패턴의 종류가 정합니다.

| 패턴 종류 | 분기 기준 | 대상 |
|-----------|-----------|------|
| 태그 패턴 | `kind` 필드 | rl enum 값과 **`kind` 문자열 필드를 가진 모든 태그드 유니언** |
| 리터럴 패턴 | 스크루티니 값 자체 (`===`) | 문자열·숫자·불리언 리터럴 유니언 ([§3.8](#38-리터럴-패턴)) |

**한 match 안에서 두 종류를 섞을 수 없습니다** — 비교 대상이 다르기 때문입니다
(`$rl_m.kind` vs `$rl_m`). `_`는 양쪽 모두에 쓸 수 있습니다.

| 요소 | 규칙 |
|------|------|
| 바인딩 | **이름 기준**(위치 아님). 선언된 필드명과 일치해야 하고, 일부만·순서 무관하게 바인딩 가능. `Tag(field: alias)`로 이름 변경 |
| 중첩 패턴 | `Tag(field: Inner(...))` — 필드 값을 내부 태그 패턴과 대조. **괄호 필수**라 유닛 케이스는 `field: None()`로 쓰고, 괄호 없는 `field: name`은 지금처럼 별칭입니다. 내부 불일치는 가드 실패처럼 **다음 암으로 폴스루**. or-패턴과 조합 불가, 한 패턴 안에서 같은 이름 중복 바인딩 불가(별칭으로 해소) |
| or-패턴 | `A \| B => ...` — 대안들이 한 본문 공유. 바인딩이 있으면 **모든 대안이 같은 (필드, 이름) 집합**을 바인딩해야 함(순서 무관). `\|\|`는 구분자가 아님 |
| 가드 | `패턴 if 조건 => ...` — 태그가 맞아도 조건이 참일 때만 선택, 거짓이면 다음 암. 조건은 rlc가 해석하지 않고 그대로 방출 |

암 검사 (전부 컴파일 에러):

- **무가드 암이 이미 덮은 태그**를 다시 쓰면 중복 암 — `A | A`,
  `A | B => .., B => ..`, `A => .., A if c => ..`. 가드 암끼리는 같은 태그를
  반복할 수 있습니다 (`A if c1 => .., A if c2 => .., A => ..`).
- or-패턴 대안들의 바인딩 집합이 다르면 에러.
- 중첩 패턴이 or-패턴 대안에 섞이면 에러. 한 패턴이 같은 이름을 두 번
  바인딩하면 에러.
- `_` 암은 반드시 마지막.

### 3.3 본문 형태

| 형태 | 규칙 |
|------|------|
| 표현식 본문 `Tag => 식` | 객체 리터럴은 괄호 필수: `Tag => ({ a: 1 })` |
| 블록 본문 `Tag => { ... }` | 값은 `return`으로. `return` 없이 끝나면 `undefined` |

본문 안에서 match 중첩, rl `enum` 선언, 템플릿 보간 내 사용이 모두 됩니다.

### 3.4 컴파일 결과

`kind`를 판별하는 `switch` 기반 즉시 실행 함수입니다.

```rl
const area = match (shape) {
  Circle(radius) => Math.PI * radius * radius,
  Rect(width: w, height) => w * height,
  Point => 0,
};
```

```ts
const area = ((() => {
  const $rl_m = (shape);
  switch ($rl_m.kind) {
    case "Circle": { const { radius } = $rl_m; return (Math.PI * radius * radius); }
    case "Rect": { const { width: w, height } = $rl_m; return (w * height); }
    case "Point": { return (0); }
    default: { throw new Error("rl match: unexpected case " + JSON.stringify($rl_m)); }
  }
})());
```

| 형태 | 방출 |
|------|------|
| `_` 암 | `default` 분기 |
| `_` 없음 | fail-fast 런타임 가드 `default` (타입 시스템을 우회해 들어온 값 대비) |
| or-패턴 | `case` 폴스루 — `case "Escape": case "Tab": { ... }` (구조 분해는 공유) |
| 가드가 하나라도 있는 match | switch로 "가드 실패 시 다음 암"을 표현할 수 없어 **if-체인 IIFE**로 방출 |
| 리터럴 패턴 match | `switch ($rl_m)` — 값 자체로 분기 ([§3.8](#38-리터럴-패턴)) |
| 중첩 패턴이 있는 match | 같은 사유로 if-체인 방출 — 경로 조건을 잇고(`$rl_m.kind === "Ok" && $rl_m.value.kind === "Some"`) 각 단계의 바인딩을 그 경로에서 구조 분해(`const { value: v } = $rl_m.value;`). tsc의 제어 흐름 분석이 경로를 좁히므로 타입 트릭 없음 |
| 블록 본문이 있는 match | 전체가 라벨 블록 `$rl_b: { ... }`로 감싸이고 블록 본문은 `break $rl_b`로 끝남 |

### 3.5 `await`와 async

스크루티니·가드·암 본문에 `await`가 있으면 async 함수로 방출되고 전체가
`await`됩니다. 감지는 토큰 단위이므로 중첩 함수 안의 `await`도 async를
유발합니다 ([§11](#11-제한사항)).

### 3.6 소진성 검사

**태그 패턴 match**의 규칙입니다 (리터럴 match는
[§3.8](#38-리터럴-패턴)·[§3.9](#39-리터럴-유니언-소진성---types)).

`_` 없는 match는 알려진 enum과 대조해 검사하고, 빠진 케이스는 컴파일
에러입니다 (선언 순서 무관).

```
rlc: shapes.rl:12:25: match on enum Shape is not exhaustive: missing "Rect"
     (add the missing arms or a final `_` arm)
```

검사 대상 enum의 출처 셋 — 같은 이름이면 **로컬 > 임포트 > 내장** 순으로
섀도잉됩니다.

1. 같은 파일에 선언된 rl enum
2. 상대 경로 `.rl` import로 가져온 exported rl enum
   ([§9.3](#93-선언-수집과-프로젝트-단위-소진성))
3. 내장 `Option`/`Result` ([§4.2](#42-내장-enum과-소진성-검사))

- or-패턴 암은 모든 대안 태그를 커버한 것으로 인정됩니다.
- **가드 암은 케이스를 커버하지 못합니다** (조건이 거짓일 수 있으므로).
  `Some(v) if v > 0 => v` 하나만 있는 match는 `Some`·`None` 둘 다 빠진 것으로
  보고됩니다.
- **중첩 패턴은 안쪽까지 검사합니다.** `Ok(value: Some(v))`와
  `Ok(value: None())`로 내부 공간을 다 덮으면 소진입니다. 덜 덮으면 빠진
  값을 패턴 그대로 지목합니다:

  ```
  rlc: r.rl:3:11: match on built-in enum Result is not exhaustive:
       missing "Ok(value: None)" (add the missing arms or a final `_` arm)
  ```

  안쪽 위치의 enum은 필드의 **선언된 타입**으로, 그것이 enum을 지목하지 않으면
  (제네릭 페이로드 `T` 등) **그 자리에 쓰인 패턴들**로 정합니다 — match의
  스크루티니를 암 태그로 정하는 것과 같은 규칙입니다. 둘 다 실패하면 그 자리는
  알 수 없는 것으로 두고 `_`만 커버로 인정합니다.
- `_` 암이 있는 match는 검사하지 않습니다.
- 어느 출처에도 없는 태그의 match는 검사 없이 컴파일되고 런타임 가드만
  남습니다.
- 빠진 값이 아주 많으면 목록이 잘릴 수 있습니다(읽을 수 없는 목록보다 낫습니다).

### 3.7 튜플 match — 다중 스크루티니

두 개 이상의 값을 콤마로 나열하고 **조합**으로 매치합니다.

```
튜플-match ::= "match" "(" 식 ("," 식)+ ")" "{" 튜플-암-목록 "}"
튜플-암    ::= 튜플-패턴 가드? "=>" 본문
             | "_" "=>" 본문                  // 전체 와일드카드, 반드시 마지막
튜플-패턴  ::= "(" 원소 ("," 원소)+ ")"
원소       ::= 태그-패턴 ("|" 태그-패턴)* | "_"
```

```rl
enum Conn { Online(latency: number), Offline }
enum Mode { Auto(), Manual(level: number) }

const speed = match (conn, mode) {
  (Online(latency), Auto) if latency < 50 => 10,
  (Online, Auto)          => 5,
  (Online, Manual(level)) => level,
  (Offline, _)            => 0,
};
```

**판별은 암 주도입니다**: 모든 암이 튜플-패턴(또는 마지막 bare `_`)이고
스크루티니가 최상위 콤마로 나뉠 때만 튜플 match입니다. 암에 튜플-패턴이
없으면 `match (a, b)`는 지금까지처럼 **콤마 식 스크루티니의 단일 match**라
기존 프로그램의 의미가 바뀌지 않습니다.

| 요소 | 규칙 |
|------|------|
| 원소 | 태그 패턴(or-패턴·바인딩 포함) 또는 `_`. 리터럴 패턴은 v1에서 원소로 쓸 수 없습니다. 원소 수는 스크루티니 수와 일치해야 함 (불일치는 컴파일 에러) |
| 바인딩 | 단일 match와 동일 (이름 기준, `field: alias`). 한 튜플 패턴 안에서 같은 이름을 두 번 바인딩하면 에러 — 별칭으로 바꿉니다 |
| 가드 | 튜플-패턴 암에만. 가드 암은 소진성 커버 불인정 (단일 match와 동일) |
| bare `_` 암 | 모든 조합 커버, 반드시 마지막 |

**컴파일 결과**: 스크루티니를 각각 한 번씩 좌→우로 평가해 `$rl_m0`,
`$rl_m1`, ...에 담고, 조합 조건의 if-체인으로 분기합니다 (가드 있는 단일
match와 같은 형태 — 구조 분해는 각 임시에서, `await` 규칙은 §3.5와 동일).

```ts
const speed = ((() => {
  const $rl_m0 = (conn);
  const $rl_m1 = (mode);
  if ($rl_m0.kind === "Online" && $rl_m1.kind === "Auto") { const { latency } = $rl_m0; if ((latency < 50)) return (10); }
  ...
  throw new Error("rl match: unexpected case " + JSON.stringify([$rl_m0, $rl_m1]));
})());
```

**소진성은 곱집합입니다**: 각 위치의 enum을 §3.6의 규칙(로컬 > 임포트 >
내장)으로 해석해 태그 조합 전체를 검사하고, 빠진 조합을 보고합니다 —
TypeScript로는 어떤 방법으로도 얻을 수 없는 검사입니다.

```
rlc: nav.rl:4:15: match on (Conn, Mode) is not exhaustive: missing (Offline, Manual)
     (add the missing arms or a final `_` arm)
```

원소 `_`와 or-패턴은 그 위치의 해당 태그들을 커버합니다. 모든 암이 `_`인
위치는 검사에서 제외되고 메시지에 `_`로 표시됩니다. 어떤 위치든 알려진
enum으로 해석되지 않으면 그 match는 검사 없이 컴파일됩니다 (단일 match의
미지 유니언과 동일).

### 3.8 리터럴 패턴

패턴 자리에 **문자열·숫자·불리언 리터럴**을 쓰면 스크루티니 값 자체를 `===`로
비교합니다. TypeScript에 흔한 리터럴 유니언을 그대로 다루기 위한 것입니다.

```rl
type Direction = "north" | "south" | "east" | "west";

const label = match (dir) {
  "north" => "N",
  "south" => "S",
  "east"  => "E",
  "west"  => "W",
};
```

```rl
const message = match (status) {
  200 | 201 | 204 => "success",
  400 | 404       => "client error",
  500             => "server error",
  _               => "unknown",
};

const flag = match (on) { true => 1, false => 0 };
```

| 요소 | 규칙 |
|------|------|
| 리터럴 | 문자열(`"a"`, `'a'`), 숫자(`0`, `404`, `-1`, `0xff`, `1_000`, `1.5e2`, `0b1010`, `10n`), `true`/`false`. v1은 이 셋뿐 — 객체·배열·범위 패턴은 없습니다 |
| or-패턴 | `200 \| 201 \| 204` — 대안들이 한 본문 공유. **대안은 모두 같은 종류**여야 합니다 (`"a" \| 1`은 에러). `\|\|`는 구분자가 아님 |
| 가드 | 태그 패턴과 동일 — `200 if retry => ...`. 가드 암은 케이스를 커버하지 못하므로 같은 리터럴을 여러 가드 암이 반복할 수 있습니다 |
| 바인딩 | 없음 (리터럴은 값을 꺼낼 것이 없습니다). 값이 필요하면 스크루티니를 그대로 씁니다 |
| `_` | 태그 match와 동일 — 반드시 마지막 |
| 튜플 match | v1에서는 튜플 원소에 리터럴을 쓸 수 없습니다 (원소는 태그 패턴이나 `_`) |

**동치 판정은 값 기준입니다.** `switch`가 `===`로 비교하므로 `200`과 `0xc8`,
`"a"`와 `'\x61'`은 같은 케이스이고, 한 match에 둘 다 있으면 중복 암 에러입니다.
`1n`은 `1`과 다릅니다 (`1n === 1`이 거짓이므로).

**컴파일 결과**는 태그 match와 같은 IIFE이고, `switch`의 대상만 `$rl_m.kind`가
아니라 `$rl_m`입니다 — 리터럴은 **원본 표기 그대로** `case` 라벨에 복사되므로
`0xff`는 `0xff`로 남습니다.

```ts
const label = ((() => {
  const $rl_m = (dir);
  switch ($rl_m) {
    case "north": { return ("N"); }
    case "south": { return ("S"); }
    default: { throw new Error("rl match: unexpected literal " + JSON.stringify($rl_m)); }
  }
})());
```

| 형태 | 방출 |
|------|------|
| `_` 암 | `default` 분기 |
| `_` 없음 | fail-fast 런타임 가드 — 메시지는 `rl match: unexpected literal ...` |
| or-패턴 | `case` 폴스루 (`case 200: case 201: { ... }`) — 본문은 한 번만 |
| 가드가 있는 match | 태그 match와 같은 사유로 if-체인 IIFE (`if ($rl_m === 200) ...`) |
| 스크루티니 | 태그 match와 동일하게 **정확히 한 번** 평가 (`const $rl_m = (...)`) |

#### 소진성

기본 컴파일 경로는 리터럴 match의 소진성을 **검사하지 않습니다**. 검사하려면
`dir`의 타입이 `"north" | "south" | ...`라는 사실을 알아야 하는데, 그 정보는
TypeScript 타입 체커 안에 있고 rlc는 TypeScript 타입 시스템을 부분 구현하지
않는다는 설계 계약을 지킵니다 (`docs/design/match-literal-patterns.md`).
`_` 없는 리터럴 match는 위의 런타임 가드만 받고 그대로 컴파일됩니다.

대신 **`rlc --check-types`/`--types`**가 이미 돌리는 TypeScript 체커에게 스크루티니 타입을
물어 검사합니다 ([§3.9](#39-리터럴-유니언-소진성---types)).

### 3.9 리터럴 유니언 소진성 (`--types`)

`rlc --check-types`/`--types`는 낮춘 모듈을 실제 TypeScript 프로젝트에 넣으므로,
`_` 없는 리터럴 match마다 스크루티니의 타입을 `getTypeAtLocation`으로 조회할 수
있습니다. 그 타입이 **유한한 리터럴 유니언으로 확정될 때만** 빠진 리터럴을
보고합니다.

```
rlc: src/main.rl:3:10: match on literal union is not exhaustive: missing "south"
     (add the missing arms or a final `_` arm)
```

진단 위치는 생성된 `.ts`가 아니라 원본 `.rl`의 `match` 키워드입니다.

같은 경로가 **enum 소진성**도 다시 답합니다 — match 위치에서 좁혀진 타입을
쓰므로 앞선 가드가 제거한 케이스는 요구하지 않고, 다른 모듈의 enum도 선언을
모아 오지 않아도 됩니다. 대신 답이 *타입*에서 오므로 enum 이름을 댈 수 없어
메시지가 `match is not exhaustive: missing ...`입니다 (기본 경로는 자기 선언
표에서 답하므로 이름을 댑니다 — [`errors.md`](./errors.md)).

정확히는, 체커는 **스크루티니 타입의 구성원 목록**을 답하고 소진성 계산은
기본 경로와 **같은 알고리즘**이 합니다([§3.6](#36-소진성-검사)). 그래서 이
경로도 중첩 패턴 안쪽의 구멍을 봅니다:

```
rlc: nest.rl:4:18: match is not exhaustive: missing "Wrap(inner: No)"
```

중첩 자리의 enum도 이 경로에서는 **체커가** 답합니다. 중첩 패턴이 방출하는
조건(`$rl_m.inner.kind === "Yes"`)의 필드 이름 자리를 물으면 그 페이로드 타입의
구성원이 나오므로, 페이로드 타입이 rl 선언과 무관해도(손으로 쓴 유니언, 제네릭
인자) 안쪽까지 검사됩니다.

체커도 확정된 답을 주지 못하는 타입(유한한 리터럴 유니언이 아닌 것)에서는
기본 경로가 보수적으로 보고하는 반면 이 경로는 **보고하지 않습니다** — 여기서는
모른다고 말하는 편이 정직합니다.

| 스크루티니 타입 | 검사 |
|-----------------|------|
| `"a" \| "b"`, `1 \| 2 \| 3`, `boolean`, `typeof values[number]` (as const) | 검사함 |
| `string`, `number`, `unknown`, `any`, `T`, `T extends string`, `"a" \| string`, `string \| number` | **검사하지 않음** |
| TS `enum` 멤버 타입 | 검사하지 않음 (멤버는 `E.A`로 쓰지 리터럴 패턴으로 쓰지 않으므로) |

원칙은 보수적입니다: 체커 결과를 완전한 유한 리터럴 집합으로 바꿀 수 없으면
검사하지 않습니다 — **잘못된 진단보다 검사하지 않는 편이 낫습니다**.

- `_` 암이 있으면 이미 소진이므로 검사 대상이 아닙니다.
- **가드 암은 커버하지 못합니다** (태그 match와 동일). `"a" if ok => .., "b" => ..`는
  `"a"`가 빠진 것으로 보고됩니다.
- 유니언에 없는 리터럴을 쓴 경우(`"c"`)는 rlc가 아니라 tsc가
  `Type '"c"' is not comparable to ...`로 보고하고, `case` 라벨이 원본에서
  복사된 덕에 그 진단도 `.rl`의 해당 리터럴 위치로 매핑됩니다.
- 중복 리터럴·태그와의 혼합·`_` 위치 같은 의미 검사는 기본 경로에서 그대로
  수행됩니다.

### 3.10 패턴 이름 해석

패턴의 케이스 태그와 필드 이름은 선언에 대조됩니다 — `match`·let-else·`if let`이
같은 규칙을 씁니다. 대조에 실패했고 **고칠 이름을 댈 수 있으면** 컴파일
에러입니다 (`errors.md`의 [패턴의 이름 해석](./errors.md#패턴의-이름-해석)):

```rl
enum Shape { Circle(radius: number), Empty }
const a = match (s) { Circel(radius) => radius, Empty => 0 };
// rlc: file.rl:2:23: enum Shape has no case `Circel` — did you mean `Circle`?
```

규칙이 조건부인 이유는 [§3.2](#32-의미)에 있습니다: 태그 패턴은 `kind` 필드를
가진 **모든** 태그드 유니언에 쓸 수 있고, 손으로 쓴 유니언의 태그는 rlc의 선언
표에 없습니다. 그래서 "해석되지 않았다"가 아니라 **"오타로 보인다"** 가 보고의
조건입니다 — 대소문자만 다르거나 편집 거리가 가까울 때(글자 자리바꿈은 한 번의
편집). 오타가 아닌 틀린 이름은 타입을 알아야 알 수 있으므로 보고하지 않습니다.

- 검사 대상 enum은 사이트가 정합니다: `match`는 암들의 태그를 가장 많이 포함하는
  유일한 enum, let-else·`if let`은 태그가 하나뿐이라 **편집 한 번** 거리의
  케이스를 가진 enum이 유일할 때만. 중첩 패턴은 바깥 필드의 선언된 타입입니다.
- 태그가 정확히 해석되면 그 케이스의 **필드 이름**은 어느 구문에서든 검사됩니다.
- 이 에러가 나면 그 match의 소진성은 함께 보고되지 않습니다 — 오타를 고치면
  답이 달라지기 때문입니다.
- 임포트한 enum은 태그만 수집되므로(§9.3) 필드 검사가 되지 않습니다. 태그
  검사는 됩니다.

---

## 4. 표준 라이브러리와 내장 enum

### 4.1 표준 라이브러리 모듈

rl은 런타임을 주입하지 않습니다. `Option<T>`/`Result<T, E>`와 콤비네이터가 담긴
**순수 TypeScript 모듈 하나**를 컴파일러가 제공하고, 소스에서는 bare 지정자로
가져옵니다. 모듈의 실체화·해석은 소비 층이 맡습니다
([`cli.md`](./cli.md), [`std.md`](./std.md)).

```rl
import { Option, Result } from "@rl/std";

const half = (n: number): Option<number> =>
  n % 2 === 0 ? Option.Some(n / 2) : Option.None;

const msg = match (half(4)) {
  Some(value) => `half=${value}`,
  None => "odd",
};
```

모듈이 만드는 **값**은 같은 이름의 rl enum을 컴파일한 결과와 **바이트 단위로
같은 형태**이므로 `match`가 그대로 동작합니다. `Result`의 두 생성자만 타입이
다릅니다 — `Result.Ok(v)`는 `Ok<T>`, `Result.Err(e)`는 `Err<E>`를 돌려줍니다
([`std.md` §값의 형태 계약](./std.md#값의-형태-계약)).

### 4.2 내장 enum과 소진성 검사

`Option`(`Some`/`None`)과 `Result`(`Ok`/`Err`)는 **내장 enum**입니다 — 파일에
선언이 없어도 소진성 검사의 대상입니다.

```rl
const f = (o: Option<number>) => match (o) { Some(value) => value };
// rlc: file.rl:1:34: match on built-in enum Option is not exhaustive:
//      missing "None" (add the missing arms or a final `_` arm)
```

- 같은 이름의 rl enum을 직접 선언하면 그 선언이 내장을 **대체**합니다.
- 내장은 검사에만 관여하고 **선언을 만들어주지 않습니다** — 값과 타입은
  import하거나 직접 선언해야 합니다.
- 손으로 쓴 유니언이 `Some`/`None`/`Ok`/`Err` 태그의 일부만 쓰면 검사에 걸릴
  수 있습니다. 의도적이면 `_` 암을 두세요.

---

## 5. 에러 전파: `try` 문

### 5.1 문법

```
try-문   ::= "try" 식 ";"                                      // 전파만
           | ("const" | "let" | "var") 바인딩 "=" "try" 식 ";"  // 값 바인딩
바인딩   ::= 식별자 | 구조 분해 패턴      // 타입 주석 허용
```

- **세미콜론 필수** — 없으면 rl 구문이 아니고 통과합니다.
- 식은 `(`나 `<`로 시작할 수 없습니다 (인터페이스의 `try(x);` /
  `try<T>(x);` 멤버 시그니처와 구분 불가). `try (식);` 대신 `try 식;`.
- TypeScript의 `try { ... } catch`와 멤버 이름 `try`는 전부 통과합니다.

### 5.2 의미

Rust의 `?`입니다. 식은 `Result`여야 하고, `Ok`면 값을 풀고 `Err`면 그 값을
**둘러싼 함수에서 즉시 `return`**합니다.

```rl
function readPort(cfg: string): Result<number, string> {
  const parsed = try parseNum(cfg);   // Err면 여기서 바로 return
  try validateRange(parsed);          // 값이 필요 없으면 전파만
  return Result.Ok(parsed);
}
```

### 5.3 컴파일 결과

IIFE 없이 둘러싼 함수 스코프에 문장으로 방출됩니다 (한 줄).

```ts
const $rl_t0 = (parseNum(cfg)); if ($rl_t0.kind !== "Ok") return $rl_t0; const parsed = $rl_t0.value;
```

- 임시 변수는 파일 단위로 유일합니다 (`$rl_t0`, `$rl_t1`, ...).
- IIFE가 없으므로 식 안의 `await`가 그대로 동작합니다
  (`const data = try await fetchData();`).
- 검사는 구조적(`kind !== "Ok"`)입니다. `Result`가 아닌 값에 쓰면 typed
  경로에서 `` `try` needs a `Result` `` 로 **`try` 자리에** 보고됩니다
  ([`errors.md`](./errors.md#생성된-코드에서-난-타입-에러)). `Option` 전파는
  지원하지 않습니다 — `Option.okOr`로 바꾸세요.
- 함수 반환 타입은 식의 `Err` 타입과 호환되는 `Result`여야 합니다
  (Rust의 `From` 같은 자동 변환 없음).
- 반환 타입을 **적지 않으면** tsc가 조기 return들과 마지막 `Result.Ok(...)`의
  합집합으로 추론합니다. `Err` 타입이 서로 다른 `try`를 여러 번 써도
  `Ok<T> | Err<E1> | Err<E2>` — 즉 `Result<T, E1 | E2>` — 가 됩니다.
  rlc는 에러 타입을 모으지 않습니다; 추론은 전적으로 tsc의 몫입니다
  ([`std.md` §여러 `try`의 에러 타입](./std.md#여러-try의-에러-타입)).

### 5.4 사용 위치 제약

`try`는 **함수 본문의 문장 위치**에서만 씁니다.

| 위치 | 결과 |
|------|------|
| match 내부(스크루티니·암 본문), 템플릿 보간, 다른 try의 식 내부 | 컴파일 에러 — 그 자리의 `return`은 둘러싼 함수가 아니라 IIFE에서 반환됩니다. 헬퍼 함수로 추출하세요 |
| 모듈 최상위 | 사용 불가 — 최상위 `return`은 유효한 TS가 아니라 출력 자가 검사에서 실패합니다 |

---

## 6. 값 추출: `let-else` 문

### 6.1 문법

```
let-else-문 ::= ("const" | "let" | "var") 패턴 "=" 식 "else" 블록 ";"
패턴        ::= 태그 "(" 바인딩-목록? ")"   // 괄호 필수, 바인딩은 match와 동일
블록        ::= "{" 문장* "}"
```

**괄호와 세미콜론이 필수**입니다 — 없으면 rl 구문이 아니고 통과합니다. 일반
TypeScript 선언과 `if/else`는 전부 통과합니다(유효한 TS에서 선언 키워드 뒤에
`식별자(`가 오는 일은 없습니다).

### 6.2 의미

Rust의 `let ... else`입니다. 식을 한 번만 평가해 `kind`가 태그와 맞으면 필드를
풀고, 아니면 `else` 블록을 실행합니다. `else` 블록은 **반드시 발산**해야
합니다.

```rl
function greet(id: number): string {
  const Some(value: user) = findUser(id) else { return "who?"; };
  return `hello, ${user}`;
}
```

`try`가 "`Err`를 전파"하는 한 가지 이탈만 제공한다면, let-else는 대상 enum과
이탈 방법을 사용자가 정합니다.

### 6.3 컴파일 결과

`try`와 같은 문장 방출 스타일입니다 (IIFE 없음, 한 줄, `$rl_t` 공유).

```ts
const $rl_t0 = (findUser(id)); if ($rl_t0.kind !== "Some") { return "who?"; } const { value: user } = $rl_t0;
```

`else` 블록이 발산하므로 tsc의 제어 흐름 분석이 뒤의 `$rl_t0`를 좁혀, 구조
분해가 타입 트릭 없이 검사를 통과합니다.

### 6.4 사용 위치와 발산 제약

- 사용 위치 제약은 `try`와 같습니다 ([§5.4](#54-사용-위치-제약)).
- 발산 검사는 **구문 검사**입니다: else 블록의 마지막 최상위 문장이
  `return`/`throw`/`break`/`continue`로 시작해야 합니다.
  `if (c) return a; else return b;`는 실제로 발산해도 거부됩니다 — 블록을
  발산 키워드로 끝내도록 재구성하세요.
- 문장 경계는 최상위 `;`와 **블록 문**(`if`/`for`/`while`/`try`/`switch`/
  함수·클래스 본문 등)의 닫는 `}`입니다. 식의 중괄호 — 객체 리터럴,
  화살표 함수 본문 — 는 문장을 끝내지 않으므로
  `else { return { kind: "Err", error: e }; };`는 그대로 `return`으로
  읽힙니다:

  ```rl
  const Some(value: user) = findUser(id) else {
    log("missing");
    return { kind: "Err", error: "no user" };   // 발산으로 인정
  };
  ```
- `= try 식 else { ... };` 조합은 지원하지 않습니다.

### 6.5 `if let` 문 — 조건부 값 추출

let-else의 비발산 짝입니다. 패턴이 맞으면 바인딩과 함께 본문을, 아니면
`else` 부분을 실행하고, 이탈 의무는 없습니다.

```
if-let-문 ::= "if" "let" 태그 "(" 바인딩-목록? ")" "=" 식 블록
              ("else" (블록 | if-let-문))?
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

- 유효한 TS에서 `if` 뒤에는 반드시 `(`가 오므로 `if let`은 rl 전용
  구문입니다. 따라서 **파싱에 실패한 `if let`은 통과되지 않고** `파일:행:열`
  과 함께 에러로 보고됩니다 (`|>`와 같은 원리 — [`errors.md`](./errors.md)).
- 바인딩 문법은 match 패턴과 같고 **중첩 패턴도 됩니다**:
  `if let Ok(value: Some(value: v)) = r { ... }`. or-패턴은 없습니다.
- `else`는 블록 또는 또 다른 if-let만 됩니다 — 일반 `else if (조건)`을
  이어 붙이려면 else 블록 안에 쓰세요.

**컴파일 결과**: 자기 완결적인 블록 문장입니다 (IIFE 없음, `$rl_t` 공유).

```ts
{ const $rl_t0 = (findUser(id)); if ($rl_t0.kind === "Some") { const { value: user } = $rl_t0; greet(user); } else ... }
```

바인딩이 `const`로 물질화되므로 본문 안의 클로저에서도 좁혀진 타입이
유지됩니다. `try`/let-else와 달리 자체 `return`을 방출하지 않으므로 **모든
문장 위치**(match 암의 블록 본문, let-else의 else 블록 포함)에서 쓸 수
있고, 표현식 위치(템플릿 보간, 스크루티니, 표현식 암 본문 등)에서는 컴파일
에러입니다.

---

## 7. 파이프라인 연산자 `|>`

### 7.1 문법

```
파이프라인 ::= head ("|>" step)+
head       ::= 식                     // 임의의 TypeScript 표현식
             | "flow"                 // 함수 합성 (§7.5)
step       ::= 식                     // 적용 스텝: 단항 함수로 평가되는 식
             | "." 포스트픽스-체인    // 메서드 스텝: 파이프 값에 대한 체인
```

`|>`(`|` 바로 뒤 `>`)는 유효한 TypeScript 어디에도 등장할 수 없는 토큰 열이라
통과 계약과 충돌하지 않습니다. 문자열·주석·정규식·템플릿 텍스트 안의 `|>`는
그대로 통과합니다.

### 7.2 의미

파이프라인은 **표현식**입니다. 값이 왼쪽에서 오른쪽으로 흐르고, 평가 순서도
head → 각 step 순서(좌→우)입니다.

| 스텝 형태 | 의미 |
|-----------|------|
| 적용 스텝 `x \|> f` | `f(x)` — F# 스타일. 다인자는 커링(`x \|> add(2)`, std의 `*P` 콤비네이터 — [`std.md`](./std.md)) 또는 괄호 화살표(`x \|> (n => f(n, 2))`) |
| 메서드 스텝 `x \|> .m(a)` | `(x).m(a)` — 파이프 값에 대한 포스트픽스 체인. `x \|> .trim().split(",")`처럼 한 스텝에서 체인 가능 |

```rl
import { Option } from "@rl/std";

const label = half(4)
  |> Option.mapP(x => x + 1)
  |> Option.unwrapOrP(0)
  |> .toFixed(1);
```

### 7.3 컴파일 결과

파이프라인이 있는 파일에는 2인자 적용 헬퍼가 **파일 끝에 한 번** 방출되고
(함수 선언은 호이스팅되므로 원본 행 위치가 유지됩니다), 적용 스텝은 그 헬퍼
호출로 중첩되며 메서드 스텝은 포스트픽스로 이어집니다.

```ts
const label = ($rl_ap($rl_ap((half(4)), (Option.mapP(x => x + 1))), (Option.unwrapOrP(0)))).toFixed(1);
function $rl_ap<A, B>(v: A, f: (v: A) => B): B { return f(v); }
```

- step이 `$rl_ap`의 **인자 위치**에 놓이므로 반환 타입 문맥 추론이 작동해,
  커링 콤비네이터 스텝의 화살표 인자까지 주석 없이 추론됩니다.
- IIFE가 없으므로 head/step 안의 `await`는 둘러싼 async 컨텍스트에서 그대로
  동작합니다 (match의 async 감지 휴리스틱이 필요 없습니다).
- 인자 평가 규칙에 따라 head → step 순서(좌→우)로 평가됩니다.
- step이 단항 함수가 아니면 그 텍스트 위치에서 tsc가 표준 에러로 보고합니다.

문맥 추론은 `$rl_ap`의 `A`, 즉 **head의 타입에서 출발합니다**. head의 타입이
정해지지 않으면(타입 주석 없는 파라미터 등) 스텝의 화살표 인자도 추론되지
않고, head의 타입이 스텝과 맞지 않으면 커링 콤비네이터의 타입 인자가 `unknown`
으로 떨어집니다.

```rl
const a = (v: number) => v |> Result.mapP((n) => n);
// n: unknown — head가 number라 Result<T, E>에 붙지 않습니다.
// rlc: file.rl:1:26: Argument of type 'number' is not assignable to
//      parameter of type 'Result<unknown, unknown>'.
```

`unknown`은 증상이고, 진짜 문제는 head입니다 — head를 고칩니다
(`Result.Ok(v) |> ...`). 이 에러는 `rlc --check-types`와 에디터 양쪽에서
원본 위치로 보고됩니다 ([`errors.md`](./errors.md#타입-에러-tsc)).

### 7.4 구조 규칙

| 규칙 | 내용 |
|------|------|
| 삼항 `? :` | head/step 최상위에서 금지 — 괄호로 감쌉니다: `(c ? a : b) \|> f`, `x \|> (c ? f : g)`. 위반은 컴파일 에러 |
| 괄호 없는 화살표 | step 최상위에서 금지 — `x \|> (n => n + 1)`. 위반은 컴파일 에러 |
| `?.` 시작 스텝 | 미지원 (`x \|> ?.m()` — 컴파일 에러) |
| 빈 스텝 | `x \|>;`, `x \|> \|> f` — 컴파일 에러 |
| head/step 내부의 `try` 문 | match와 같은 사유로 금지 ([§5.4](#54-사용-위치-제약)) |

구조 파싱에 실패한 `|>`는 통과되지 않고 `파일:행:열`과 함께 에러로 보고됩니다
(`|>`가 남은 생성물은 유효한 TS가 아니므로 — [`errors.md`](./errors.md)).
match 스크루티니·암 본문, 템플릿 보간, `try` 문의 식 안에서는 자유롭게 쓸 수
있습니다: `const a = try readCfg() |> normalize;`.

### 7.5 함수 합성: `flow`

head 자리에 값 대신 **`flow`** 를 쓰면 같은 스텝 체인이 값을 흘려보내는 대신
**함수를 합성해 새 함수를 만듭니다**. 파이프가 "지금 이 값을 흘려보낸다"라면
flow는 "값이 오면 이렇게 흘려보낼 함수를 만든다"입니다.

```rl
const label = flow |> half |> Option.mapP(x => x + 1) |> Option.unwrapOrP(0) |> .toFixed(1);
label(4);          // "3.0" — 파이프와 같은 체인, 값만 나중에
```

- **`flow`는 문맥 키워드입니다.** 파이프라인 head가 정확히 식별자 `flow`
  하나일 때만 합성이고, 그 밖의 `flow`는 평범한 TypeScript 식별자입니다
  (`import { flow } from "fp-ts/function"`도 그대로 통과합니다). `flow`라는
  **변수**를 파이프에 흘리려면 괄호로 감쌉니다: `(flow) |> f`.
- 스텝 규칙은 파이프와 같습니다([§7.4](#74-구조-규칙)). 다만 **첫 스텝은
  메서드 스텝이 될 수 없습니다** — 합성 함수의 입력이 아직 없으므로 체인을
  걸 값이 없습니다 (컴파일 에러). 두 번째 스텝부터는 자유롭게 씁니다.
- 스텝이 하나뿐인 `flow |> f`는 `f` 그 자체입니다.
- 합성 자체는 아무것도 실행하지 않습니다. 스텝 식은 합성 시점에 한 번
  평가되고(커링 콤비네이터 호출 등), 체인은 합성 함수를 **호출할 때**
  좌→우로 실행됩니다.

#### 컴파일 결과

합성이 있는 파일에는 2인자 합성 헬퍼가 파일 끝에 한 번 방출되고, 스텝은
그 헬퍼 호출로 중첩됩니다. 메서드 스텝은 헬퍼의 인자 위치에 놓이는 화살표가
됩니다(그 자리에서 파라미터 타입이 문맥으로 정해지므로 주석이 필요 없습니다).

```ts
const label = $rl_fl($rl_fl($rl_fl((half), (Option.mapP(x => x + 1))), (Option.unwrapOrP(0))), (($rl_v) => ($rl_v).toFixed(1)));
function $rl_fl<A extends unknown[], B, C>(f: (...a: A) => B, g: (b: B) => C): (...a: A) => C { return (...a: A) => g(f(...a)); }
```

- 이항 합성의 중첩이므로 **단계 수 제한이 없습니다**(라이브러리 `flow(f, g, h)`
  가 겪는 오버로드 arity 한계가 없습니다).
- 첫 스텝의 파라미터가 그대로 합성 함수의 파라미터입니다 — 다인자 함수도
  다인자로 남습니다: `flow |> add |> double` → `(a: number, b: number) => number`.

#### 입력 타입은 첫 스텝이 정합니다

파이프는 head 값에서 타입이 출발하지만([§7.3](#73-컴파일-결과)), 합성에는 값이
없으므로 **첫 스텝의 파라미터 타입이 입력 타입**이 됩니다. 따라서 첫 스텝은
타입이 이미 정해진 함수여야 합니다 — 제네릭 함수나 커링 콤비네이터를 첫
스텝으로 쓰면 타입 인자를 추론할 근거가 없어 `unknown`으로 떨어집니다
(TypeScript가 고차 위치의 제네릭을 추론하지 못하는 한계로, 라이브러리 `flow`
도 같습니다). 타입 인자를 직접 주면 됩니다:

```rl
const f = flow |> wrap |> .length;              // wrap이 제네릭이면 unknown 붕괴
const g = flow |> wrap<number> |> .length;      // 타입 인자를 명시
const h = flow |> Option.mapP((x: number) => x + 1) |> Option.unwrapOrP(0);
```

두 번째 스텝부터는 앞 스텝의 반환 타입이 문맥이 되므로 파이프와 똑같이 —
커링 콤비네이터의 화살표 인자까지 — 주석 없이 추론됩니다.

---

## 8. `result` 계산 블록

`Result`를 돌려주는 연산을 여러 단계 잇는 코드는 콤비네이터로 쓰면 단계마다
콜백이 한 겹씩 깊어지고, 앞 단계의 값을 계속 안쪽으로 넘겨야 합니다.
`result { ... }`는 같은 계산을 **평탄한 문장 나열**로 씁니다.

```rl
const data = result {
  const user <- getUser(id);
  const company <- getCompany(user.companyId);
  const permission <- getPermission(user, company);
  { user, company, permission }
};
```

`<-`는 **Result 바인딩**입니다. 오른쪽 식이 `Ok`면 그 값을 왼쪽 이름에 묶고 다음
문장으로 갑니다. `Err`면 나머지 문장을 실행하지 않고 그 `Err`가 블록 전체의
값이 됩니다.

### 8.1 문법

```
result-블록   ::= "result" "{" 문장* 값-식 "}"
문장          ::= result-바인딩 | 임의의 TypeScript·rl 문장
result-바인딩 ::= ("const" | "let" | "var") 바인딩 "<-" 식 ";"
값-식         ::= 식                     // 블록의 마지막, 세미콜론 없이
```

| 규칙 | 내용 |
|------|------|
| 바인딩 최소 1개 | Result 바인딩이 하나도 없는 `result { ... }`는 rl 구문이 아닙니다 — 그대로 통과합니다 ([§8.4](#84-구조-규칙)) |
| `<-` | 두 바이트를 **붙여** 씁니다. `a < -b`(비교)와는 선언 키워드 뒤라는 점으로 구분됩니다 |
| 바인딩 자리 | `try` 선언 형태와 같습니다 — 이름, 구조 분해, 타입 주석 모두 됩니다: `const n: number <- parse(raw);`, `const { x, y } <- point();` |
| 세미콜론 | 바인딩은 `;`로 끝납니다. **마지막 값 식에는 `;`를 붙이지 않습니다** |
| 사이 문장 | 바인딩 사이에는 평범한 TypeScript·rl 문장을 자유롭게 씁니다 |

### 8.2 의미

`result { ... }`는 **표현식**입니다. 값은 이렇게 정해집니다.

- 모든 바인딩이 `Ok`면 → 마지막 값 식을 `Ok(...)`로 감싼 값.
- 어떤 바인딩이 `Err`면 → 그 `Err` 그대로. 이후 문장(다음 바인딩의 식 포함)은
  실행되지 않습니다.

```rl
const name = result {
  const user <- getUser(id);
  const profile <- getProfile(user);
  profile.name                       // Result<string, ...>의 성공값
};
```

**타입은 전부 tsc가 추론합니다.** rlc는 `getUser`의 타입을 보지 않습니다 —
`<-`라는 구문 자체가 Result 바인딩이라는 표시이고, rlc는 tsc가 정확히 추론할 수
있는 구조로 낮출 뿐입니다([§8.3](#83-컴파일-결과)). 그래서 에러 타입도 자연히
합쳐집니다: `Result<User, UserError>`와 `Result<Company, CompanyError>`를 잇는
블록의 타입은 `Result<T, UserError | CompanyError>`에 그대로 대입됩니다(둘 중
하나를 빠뜨린 주석은 tsc가 그 주석 위치에서 잡습니다).

`|>`와는 푸는 문제가 다릅니다. 파이프라인은 **값 하나**를 연달아 변환할 때,
`result` 블록은 **여러 Result 연산**이 앞 단계의 값을 계속 참조할 때 씁니다.
둘은 섞어 씁니다.

```rl
const data = result {
  const user <- getUser(id);
  const name = user.name |> .trim() |> .toLowerCase();
  const company <- getCompany(name);
  { name, company }
};
```

### 8.3 컴파일 결과

블록은 **평범한 문장들의 IIFE**가 됩니다. 바인딩마다 식을 한 번 평가해 임시
변수에 담고, `Ok`가 아니면 블록에서 즉시 `return` 하고(= `Err` 전파), 아니면
값을 꺼내 바인딩합니다 — `try` 문과 같은 모양이며([§5.3](#53-컴파일-결과)),
탈출 범위가 함수가 아니라 블록이라는 점만 다릅니다.

```rl
const data = result {
  const user <- getUser(id);
  const label = user.name.trim();
  const company <- getCompany(user.companyId);
  { user, company, label }
};
```

```ts
const data = ((() => {
  const $rl_r0 = (getUser(id)); if ($rl_r0.kind !== "Ok") return $rl_r0; const user = $rl_r0.value;
  const label = user.name.trim();
  const $rl_r1 = (getCompany(user.companyId)); if ($rl_r1.kind !== "Ok") return $rl_r1; const company = $rl_r1.value;
  return { kind: "Ok" as const, value: ({ user, company, label }
) }; })());
```

- 타입 트릭도 헬퍼도 없습니다. tsc는 각 `if`에서 임시 변수를 좁히므로 바인딩은
  성공값 타입이 되고, 블록의 타입은 **반환된 `Err`들과 마지막 `Ok`의 유니언**이
  됩니다 — 그래서 에러 타입이 저절로 합쳐집니다.
- 임시 변수는 파일 단위로 번호가 붙습니다(`$rl_r0`, `$rl_r1`, ...) — 블록이
  중첩돼도 이름이 겹치지 않습니다.
- 블록 안에 `await`가 있으면 `(await (async () => { ... })())`로 방출됩니다
  (match와 같은 규칙 — [§3.5](#35-await와-async)).

### 8.4 구조 규칙

`result`는 **문맥 키워드**입니다. TypeScript에서 `result`는 평범한 식별자이고,
`result` 다음 줄에 블록 문이 오는 코드도 유효하므로(ASI) 키워드만으로는 판별할
수 없습니다. 판별하는 것은 **바인딩**입니다.

| 상황 | 결과 |
|------|------|
| 블록에 Result 바인딩(`const x <- 식;`)이 하나 이상 | rl `result` 블록 |
| 바인딩이 없는 `result { ... }`, `const result = ...`, `class result { }`, `obj.result` | 통과 (평범한 TypeScript) |
| 블록 안 최상위의 `const x = a < -b;` | 통과 — 초기화 `=`가 먼저 오므로 바인딩이 아닙니다 |
| 블록 안 최상위의 `let x: Foo<-1>;` | 통과 — `<-` 뒤에 짝 없는 `>`가 남으므로(제네릭 타입 인자) 바인딩이 아닙니다. 같은 이유로 `<-` 뒤 식의 최상위에 짝 없는 `>`를 두려면 괄호로 감쌉니다 |
| 바인딩이 있지만 완전하게 파싱되지 않음 (`;` 누락, 마지막 값 식 없음, `<-` 뒤 식 없음) | 위치를 담은 rl 에러 ([`errors.md`](./errors.md)) |

선언 키워드 뒤의 `<-`는 유효한 TypeScript일 수 없으므로(선언자에는 초기화
`=`가 필요합니다) 이 판별은 통과 계약을 깨지 않습니다. 같은 이유로, 바인딩이
있는데 파싱에 실패한 블록은 통과시킬 수 없어 **에러**가 됩니다.

그 밖의 위치·문장 규칙:

- 블록은 표현식이므로 **어디에나** 놓입니다 — 변수 초기화, 인자, 화살표 본문,
  match 암, 템플릿 보간, 파이프라인 head(`result { ... } |> Result.mapP(f)`).
- 블록 안의 `return`은 **블록에서** 빠져나옵니다(둘러싼 함수가 아니라).
  그래서 블록 안에서는 `try` 문과 let-else를 쓸 수 없습니다 — 둘 다 둘러싼
  함수에서 나가는 `return`으로 컴파일되기 때문입니다(위치를 담은 rl 에러).
  `if let`은 자체 완결 블록으로 컴파일되므로 자유롭게 씁니다.
- 마지막 값 식이 이미 `Result`라면 한 겹 더 감싸집니다(`Result<Result<...>>`).
  그럴 때는 마지막 줄도 바인딩으로 쓰고 값을 돌려주면 됩니다.
- 중첩된 `result` 블록은 안쪽부터 각각 하나의 표현식입니다.

---

## 9. 모듈: `.rl` import 지정자 재작성

`.rl` 파일은 다른 `.rl`을 상대 경로 그대로 import합니다. 컴파일러가 방출 시
지정자를 소비 측이 해석할 수 있는 형태로 바꿉니다.

```rl
import { CalcError } from "./error.rl";   // parser.rl
```

```ts
import { CalcError } from "./error.js";   // parser.ts (기본 --rewrite-imports js)
```

### 9.1 재작성 대상

**정적 import 선언과 re-export**의 지정자 중 **`./`·`../`로 시작하고 `.rl`로
끝나는 것**만 바뀝니다. 절, 따옴표 스타일, 공백, import attributes는 바이트
그대로 유지됩니다.

| 재작성됨 | 통과 |
|----------|------|
| `import def from "./a.rl"` | 상대 경로가 아닌 것: `"pkg.rl"`, `"@scope/p/x.rl"`, `"/abs/x.rl"` |
| `import def, { n as a } from "./b.rl"` | `.rl`로 끝나지 않는 것: `"./x.js"`, `"./x"`, `"pkg"` |
| `import * as ns from "./c.rl"` | 동적 `import("./x.rl")`, `import.meta` |
| `import type { T } from "./d.rl"` | `import x = require("./x.rl")` |
| `import "./side-effect.rl"` | 문자열·주석·템플릿 안의 import처럼 보이는 텍스트 |
| `export { x } from "./e.rl"`, `export * from`, `export * as g from`, `export type { U } from` | 정적 import 절로 완전하게 파싱되지 않는 후보 |

### 9.2 방출 형태 (`--rewrite-imports`)

올바른 형태는 소비 측 `moduleResolution`에 달렸으므로 플래그(라이브러리에서는
`Options { rewrite_imports }`)로 고릅니다.

| 모드 | `"./error.rl"` → | 용도 |
|------|------------------|------|
| `js` (기본) | `"./error.js"` | `nodenext`(Node ESM은 확장자 필수)와 `bundler`(tsc가 `.js`→`.ts` 대응) 모두 동작 |
| `ts` | `"./error.ts"` | 아래 두 옵션을 켠 프로젝트 (TypeScript 5.7+) |
| `off` | `"./error.rl"` | 재작성 끔 — 번들러 플러그인이 직접 해석할 때 |

`ts` 모드는 소비 측에 다음이 필요합니다. `allowImportingTsExtensions`만 켜면
emit이 막힙니다 (`TS5096`).

```jsonc
{ "compilerOptions": {
    "allowImportingTsExtensions": true,
    "rewriteRelativeImportExtensions": true } }
```

이 모드에서 확장자는 층마다 한 번 바뀝니다 — `.rl` →(rlc)→ `.ts` →(tsc)→ `.js`.

### 9.3 선언 수집과 프로젝트 단위 소진성

`rlc`로 컴파일하면 각 파일의 **직접(1-홉) 상대 경로 `.rl` import**를 따라가
참조 파일의 **exported rl enum(이름 + 태그 집합)만** 뽑아 소진성 검사에
넣습니다.

```
rlc: parser.rl:3:28: match on enum Token (imported from "./token.rl")
     is not exhaustive: missing "Eof" (add the missing arms or a final `_` arm)
```

| 규칙 | 내용 |
|------|------|
| 수집 범위 | import 절의 이름만 — `{ Token }`은 `Token`, `{ Token as Tok }`은 `Tok`, `* as ns`는 모든 exported enum을 `ns.<이름>`으로. `import type`도 동일 |
| 수집 제외 | side-effect import와 re-export (로컬 스코프에 아무것도 들이지 않음) |
| 섀도잉 | 로컬 선언 > 임포트 > 내장 |
| 깊이 | **1-홉** — 참조 파일의 re-export 체인은 따라가지 않습니다. 순환 import는 재귀가 없어 문제되지 않습니다 |
| 해석 실패 | 조용히 건너뜁니다 — 모듈 해석은 tsc의 책임(`TS2307`)이고, 알 수 없는 enum은 검사되지 않을 뿐입니다 |

라이브러리로 쓸 때는 `rl_imports`와 `exported_enums`로 모아
`Options::extern_enums`로 넘기면 `compile`이 동일하게 검사합니다.

---

## 10. 바인딩 수식자 `val`

`val`은 **바인딩 하나의 변경 권한(mutation capability)** 을 제한하는 수식자입니다.
`val`이 붙은 바인딩에서 시작하는 접근 경로로는 값을 바꿀 수 없고, rlc가 이를
컴파일 시점에 검사합니다. 수식자가 없는 바인딩은 지금까지와 똑같은 TypeScript
의미 그대로 — 언제나 변경 가능 — 이므로 `mut` 같은 반대 키워드는 없습니다.

```
수식자 없음 = 변경 가능 (기존 TypeScript)
val         = 이 바인딩을 통한 변경 금지
```

### 10.1 문법

`val`은 선언 키워드 앞과 매개변수 앞, 두 자리에 옵니다.

```rl
val const user = { name: "Kim", tags: ["dev"] };
val let state = { count: 0 };

function read(val user: User) { return user.name; }
const inspect = (val user: User) => user.name;

for (val const item of items) { log(item.id); }
try { work(); } catch (val error: unknown) { log(error); }
```

- **선언**: `val` 바로 뒤에 `const`/`let`/`var`가 **같은 줄에** 와야 합니다.
  선언이 도입하는 이름 전부(구조 분해 포함)가 `val` 바인딩이 됩니다.
- **매개변수**: `val`은 매개변수 목록 항목의 맨 앞(`(` 또는 `,` 직후, TypeScript
  매개변수 수식자 `public`/`private`/`protected`/`readonly`/`override` 뒤)에
  오고, 그 뒤에는 같은 줄의 바인딩(식별자·`{ }`·`[ ]`)이 와야 합니다.
  함수 선언·함수 식·화살표 함수·메서드·`catch` 절 모두 같은 규칙입니다.
- 이 두 형태는 유효한 TypeScript에 존재할 수 없으므로 통과 계약([§1](#1-기본-원칙))을
  깨지 않습니다. 그 밖의 모든 `val`은 **평범한 식별자**입니다 — `const val = 1;`,
  `o.val`, `f(val, x)`, `(val as User)`, `for (val of xs)`, 그리고 줄바꿈으로
  분리된 `val\nconst x = 1;`(ASI)까지 전부 그대로 통과합니다.

### 10.2 의미

`val`은 **접근 경로의 읽기 전용화**이지 객체의 불변화가 아닙니다. 같은 객체를
가리키는 다른 바인딩이 있으면 그쪽 경로로는 여전히 변경할 수 있습니다.

```rl
let original = { count: 0 };
val const view = original;

view.count++;      // 에러 — val 바인딩을 통한 변경
original.count++;  // OK — 이 바인딩에는 제한이 없음
```

`const`/`let`과 `val`은 서로 다른 축입니다. `const`/`let`은 **바인딩 자체를 다른
값으로 바꿀 수 있는가**를, `val`은 **바인딩에서 시작하는 경로로 내부 값을 바꿀 수
있는가**를 정합니다.

| 선언 | 재할당 | 내부 변경 |
|------|--------|-----------|
| `const x` | ✗ | ✓ |
| `let x` | ✓ | ✓ |
| `val const x` | ✗ | ✗ |
| `val let x` | ✓ | ✗ |

```rl
val let state = { count: 0 };
state.count++;                              // 에러
state = { ...state, count: state.count + 1 };  // OK — let이므로 교체는 가능
```

### 10.3 변경으로 간주하는 문법

루트가 `val` 바인딩이면 경로의 깊이와 상관없이 아래가 전부 에러입니다.

| 형태 | 예 |
|------|-----|
| 대입 | `x.a = v`, `x.a += v`, `x.a **= v`, `x.a ??= v`, `x.a >>= v`, `x[i] = v` |
| 증감 | `x.a++`, `x.a--`, `++x.a`, `--x.a`, `x[i]++` |
| `delete` | `delete x.a`, `delete x[i]` |

```rl
val const state = { user: { profile: { name: "Kim" } } };
state.user.profile.name = "Lee";  // 에러 — 경로의 루트가 val
```

**메서드 호출은 기본 경로에서 검사하지 않습니다.** `x.set(k, v)`가 값을 바꾸는지는
`x`가 무엇인가에 달렸고, 그건 TypeScript 타입에 대한 사실입니다. rlc는 이름으로
추측하지 않습니다 — `set`/`add`/`push`라는 이름의 사용자 정의 메서드는 아무것도
바꾸지 않을 수 있기 때문입니다. built-in에 대한 변경 메서드 호출은 타입 정보가
있는 `rlc --check-types`/`--types`에서 검사합니다 ([§10.4](#104-built-in-변경-메서드---types)).

바인딩 자체를 다른 값으로 바꾸는 `x = v`는 `val`의 검사 대상이 **아닙니다**
(`const`면 tsc가, `let`이면 아무도 막지 않습니다 — [§10.2](#102-의미)의 표).
읽기·비교·전개(`x.a === 1`, `{ ...x }`)도 당연히 허용됩니다.

### 10.4 built-in 변경 메서드 (`--types`)

`rlc --check-types`/`--types`는 낮춘 모듈을 실제 TypeScript 프로젝트에 넣으므로,
`val` 경로로 호출된 메서드의 **심볼**을 조회할 수 있습니다. 이름이 아래 표에
있고 그 심볼의 선언이 **전부 TypeScript 자신의 lib**일 때만 에러입니다 —
심볼의 소속은 컴파일러의 답이고, built-in의 어떤 메서드를 변경 연산으로 볼지는
rl의 정책(아래 표)입니다. 두 판정 모두 호출을 수집한 뒤에 내려지므로, 표에
없는 이름이 오탐을 만들 수는 없습니다 — 표의 누락은 미탐으로만 남습니다.

```
rlc: src/main.rl:2:1: cannot call mutating method `set` through val binding `map`
     (the binding is declared with `val`, so every access path from it is
     read-only)
```

메시지는 built-in의 *이름*을 대지 않습니다: 컴파일러가 답한 것은 "이 메서드는
TypeScript 자신의 것"이고, 어느 인터페이스가 선언했는지가 아닙니다.

| built-in | 변경 메서드 |
|----------|-------------|
| `Array` | `push` `pop` `shift` `unshift` `splice` `sort` `reverse` `fill` `copyWithin` |
| `Map` | `set` `delete` `clear` |
| `Set` | `add` `delete` `clear` |
| `WeakMap` | `set` `delete` |
| `WeakSet` | `add` `delete` |
| TypedArray (`Int8Array` … `BigUint64Array`) | `set` `sort` `reverse` `fill` `copyWithin` |

판정 근거는 **수신자의 선언**이지 이름도 반환 타입도 아닙니다 (`Map#set`은 자기
자신을 반환합니다). 그래서 다음은 통과합니다.

```rl
class Query {
  set(key: string): Query { return new Query(); }
}

val const query = new Query();
query.set("name");   // OK — Query#set은 built-in 변경 메서드가 아님

val const map = new Map<string, number>();
map.set("a", 1);     // 에러 — Map#set
```

리터럴 소진성([§3.9](#39-리터럴-유니언-소진성---types))과 같은 원칙으로
보수적입니다: 수신자를 확정할 수 없으면(`any`, 타입 파라미터, 해석되지 않는
import, 방출 매핑이 끊긴 자리) **검사하지 않습니다**. 유니언 수신자는 모든 후보가
built-in 변경 메서드일 때만 에러입니다(`Map | Set`의 `delete`).

### 10.5 함수 경계

`val` 바인딩은 **`val`이 아닌 매개변수로 넘길 수 없습니다.** 함수가 그 인자를
변경할 수 있기 때문입니다.

```
val 인자   → val 매개변수      OK
           → 일반 매개변수     에러
일반 인자  → val 매개변수      OK
           → 일반 매개변수     OK
```

```rl
function read(val user: User) { log(user.name); }
function update(user: User) { user.name = "Lee"; }

function process(val user: User) {
  read(user);    // OK
  update(user);  // 에러 — update는 user를 변경할 수 있음
}
```

검사 대상은 **같은 파일에서 이름으로 선언된 함수**입니다: `function f(...)`,
`const f = (...) => ...`, `const f = function (...) ...`. 인자가 `x`·`x.y.z`
형태의 접근 경로일 때만 판정하고, 계산된 인자(`f(g(x))`, `f({ ...x })`)는
검사하지 않습니다.

어느 선언을 부른 것인지는 경로마다 다르게 답합니다. 기본 경로
(`rlc`/`--check`)는 호출을 **이름**으로 선언과 대응시키므로, 같은 이름이
서로 다른 시그니처로 두 번 선언되면 그 이름은 검사에서 제외됩니다.
`--check-types`/`--types`는 호출의 callee **심볼**을 조회해 실제로 가리키는
선언과 짝지으므로, 이름이 겹쳐도(섀도잉·블록 스코프 재선언) 각 호출은 자기
선언의 시그니처로 검사됩니다. 하나의 심볼이 서로 다른 시그니처의 선언
여럿을 가지면(오버로드) 그 callee는 검사하지 않고, import된 함수처럼 같은
파일의 선언과 심볼이 일치하지 않는 callee도 검사하지 않습니다.

### 10.6 스코프와 섀도잉

`val` 여부는 **바인딩**의 성질이므로 렉시컬 스코프를 따릅니다. 안쪽 스코프의
같은 이름 선언은 바깥 `val`을 가립니다.

```rl
val const x = { a: 1 };
{
  const x = { a: 2 };
  x.a = 3;   // OK — 이 x는 val이 아님
}
x.a = 4;     // 에러 — 바깥 x는 여전히 val
```

매개변수는 함수 본문 스코프에, `for` 머리의 선언은 반복문 스코프에, `catch`
바인딩은 catch 블록에 속합니다.

### 10.7 컴파일 결과

`val`은 **컴파일 시점 전용**입니다. 방출된 TypeScript에는 키워드도, 런타임
헬퍼도, `readonly` 같은 타입 변환도 남지 않습니다 — 키워드와 그 뒤의 공백만
사라집니다.

```rl
val const user = getUser();
function read(val user: User) { return user.name; }
```

```ts
const user = getUser();
function read(user: User) { return user.name; }
```

### 10.8 검사 범위

rlc가 추적할 수 있는 것만 검사합니다. 아래는 **의도적으로** 검사하지 않습니다.

| 항목 | 이유 |
|------|------|
| 임의의 외부 함수가 내부에서 하는 변경 | 이펙트 시스템·전역 이펙트 추론을 만들지 않습니다. 시그니처를 알 수 있는 같은 파일 함수만 [§10.5](#105-함수-경계)로 검사합니다 |
| 기본 경로(`rlc`/`--check`)의 메서드 호출 | 수신자의 타입 없이는 변경 여부를 알 수 없습니다. 이름으로 추측하지 않고 `--check-types`/`--types`에서만 판정합니다 ([§10.4](#104-built-in-변경-메서드---types)) |
| built-in이 아닌 타입의 메서드 | 메서드 본문을 분석하지 않습니다. 사용자 정의 API가 내부에서 무엇을 바꾸는지는 검사 대상이 아닙니다 |
| 메서드 호출로 넘기는 인자(`obj.m(x)`) | 메서드의 매개변수 선언을 이름으로 해석하지 않습니다 |
| 별칭을 통한 우회 (`const alias = valBinding; alias.a = 1`) | 소유권·borrow checker를 만들지 않습니다. `val`은 바인딩 하나의 권한 제한입니다 |
| 괄호·단언을 거친 경로 (`(x as any).a = 1`) | 경로의 루트가 식별자일 때만 판정합니다 |
| 객체 자체의 깊은 불변성 | `Object.freeze`·`Proxy` 같은 런타임 강제를 넣지 않습니다 |
| match 패턴 바인딩의 `val` (`Ok(val user)`) | 미지원 — 패턴 문법이 아니므로 그 match는 rl 구문으로 파싱되지 않습니다 |

---

## 11. 제한사항

| 항목 | 내용 |
|------|------|
| 소스맵 | 생성하지 않습니다. 생성된 `.ts`와 원본 행이 대체로 대응하지만 보장되지 않습니다 |
| 패턴 | 태그 패턴(or-패턴·가드·중첩 패턴 포함)과 `_`뿐. 리터럴 패턴은 의도적으로 미지원. `_ if ...`는 rl 구문이 아닙니다 |
| 중첩 패턴 | match·`if let` 전용 — let-else 바인딩은 별칭만. or-패턴과 조합 불가. 소진성은 안쪽까지 검사하지만, 안쪽 enum을 정할 수 없으면(선언 타입도 패턴도 지목 못 하면) 그 자리는 `_`만 커버로 인정 ([§3.6](#36-소진성-검사)) |
| 도달 불가 암 | 무가드 암이 이미 덮은 태그의 반복(중복 암)만 에러입니다. 중첩까지 따져 죽은 암을 찾는 계산은 하지만 보고하지 않습니다 — rl에는 경고 계층이 없어 에러로 만들면 지금 컴파일되는 프로그램이 깨집니다 |
| 튜플 match | 튜플-패턴 사이의 or(`(A, B) \| (C, D)`)는 미지원 — 원소 수준 or로 씁니다: `(A, B \| D)`. 스크루티니 분리는 구조적이라 최상위 비교 연산자(`a < b, c > d`)는 제네릭 인자로 오인될 수 있습니다 — 괄호로 감쌉니다 |
| 이름 해석 | 오타로 보이는 이름만 보고합니다 — 오타가 아닌 틀린 태그·필드는 검사되지 않습니다. 임포트한 enum은 필드 검사 없음 ([§3.10](#310-패턴-이름-해석)) |
| 소진성 수집 | 직접(1-홉) 상대 경로 `.rl` import만. re-export 체인·패키지 경로의 enum과 손으로 쓴 유니언은 검사되지 않습니다 ([§3.6](#36-소진성-검사), [§9.3](#93-선언-수집과-프로젝트-단위-소진성)) |
| import 재작성 | 정적 상대 경로 `.rl` 지정자만. 참조 파일의 존재는 검사하지 않습니다 ([§9](#9-모듈-rl-import-지정자-재작성)) |
| `try` | `;` 필수, 식은 `(`/`<`로 시작 불가, match 내부·템플릿 보간·모듈 최상위 불가. `Option` 전파 미지원 |
| `let-else` | 패턴 괄호와 `;` 필수, else는 발산 키워드로 끝나야 함. or-패턴·가드·중첩 패턴, `= try 식 else` 조합 미지원 |
| `if let` | else는 블록 또는 if-let만(일반 `else if (조건)` 불가 — else 블록 안에 쓰기). or-패턴·가드 미지원. 표현식 위치 불가. `= 식` 최상위의 괄호 없는 블록 화살표는 괄호 필수 |
| 표현식 암 | 객체 리터럴은 괄호 필수: `Tag => ({ a: 1 })` |
| 스크루티니 | 괄호 필수: `match (x) { ... }` |
| `\|>` head 판별 | 구조 추적입니다. 삼항·화살표는 괄호 필수([§7.4](#74-구조-규칙)), 이항 `in`/`instanceof`·복합 시프트 대입(`>>=`) 옆이나 세미콜론 없는 코드의 문장 경계에서는 head를 짧거나 길게 잡을 수 있습니다 — head를 괄호로 감싸면 항상 명확합니다 |
| `\|> ?.m()` | `?.` 시작 스텝 미지원 ([§7.4](#74-구조-규칙)) |
| `flow` | 첫 스텝이 입력 타입을 정합니다 — 제네릭 함수·커링 콤비네이터를 첫 스텝으로 쓰면 타입 인자를 명시해야 합니다. 첫 스텝은 메서드 스텝 불가. 입력 타입 주석 문법(`flow<T>`)은 없습니다 ([§7.5](#75-함수-합성-flow)) |
| `result` 블록 | 바인딩이 하나 이상 필요하고 마지막은 세미콜론 없는 값 식이어야 합니다. 바인딩은 `Result` 전용(`Option`·`Promise` do-표기법 없음), `<-`는 블록 안 선언에서만. `<-` 뒤 식의 최상위 `>`는 제네릭 타입 인자와 구분되지 않아 괄호가 필요합니다. 블록 안의 `return`은 블록에서 빠져나가며 `try`·let-else는 쓸 수 없습니다 ([§8.4](#84-구조-규칙)) |
| async 감지 | 토큰 단위 — 중첩 함수 안의 `await`도 async 방출을 유발하므로, 그런 match를 async가 아닌 곳에 두면 생성물이 문법 에러가 됩니다 |
| `val` | 같은 줄 규칙(`val const`·`val <바인딩>`)과 접근 경로 루트가 식별자인 경우만. 메서드 호출은 기본 경로에서 판정하지 않습니다 — built-in 변경 메서드는 `--check-types`/`--types`에서만 ([§10.4](#104-built-in-변경-메서드---types)). 외부 함수의 변경, 별칭 우회는 검사하지 않고 match 패턴 바인딩에는 쓸 수 없습니다 ([§10.8](#108-검사-범위)) |
| `.tsx` | 미지원 (제네릭 화살표 함수 출력이 JSX와 충돌) |
| 식별자 | rl 구문 안에서는 ASCII만 |
| `--no-verify` | 필드 타입 오류가 컴파일 시점에 잡히지 않고 tsc 단계에서 드러납니다 |
