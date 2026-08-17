# rl 언어 레퍼런스

rl 문법과 의미의 규범 문서입니다. 구현이 이 문서와 어긋나면 버그입니다.
CLI는 [`cli.md`](./cli.md), 에러 메시지는 [`errors.md`](./errors.md), 표준
라이브러리 API는 [`std.md`](./std.md)를 보세요.

1. [기본 원칙](#1-기본-원칙) · 2. [`enum`](#2-enum-선언) ·
3. [`match`](#3-match-표현식) · 4. [표준 라이브러리](#4-표준-라이브러리와-내장-enum) ·
5. [`try`](#5-에러-전파-try-문) · 6. [`let-else`·`if let`](#6-값-추출-let-else-문) ·
7. [`|>`](#7-파이프라인-연산자-) · 8. [모듈](#8-모듈-rl-import-지정자-재작성) ·
9. [제한사항](#9-제한사항)

---

## 1. 기본 원칙

`.rl` 파일은 TypeScript에 여섯 구문 — `enum` 선언, `match` 표현식, `try` 문,
let-else 문, `if let` 문, 파이프라인 연산자 `|>` — 을 더한 것입니다.

> **모든 유효한 TypeScript 파일은 그대로 유효한 `.rl` 파일이며, 자기 자신으로
> 컴파일됩니다.** 유일한 예외는 상대 경로 `.rl` import 지정자의 재작성입니다
> ([§8](#8-모듈-rl-import-지정자-재작성)).

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
가드       ::= "if" 식                     // 태그 패턴에만
패턴       ::= 태그-패턴 ("|" 태그-패턴)*  // or-패턴
             | "_"                         // 반드시 마지막 암
태그-패턴  ::= 태그 | 태그 "(" 바인딩-목록? ")"
바인딩     ::= 필드명 | 필드명 ":" 별칭 | 필드명 ":" 태그-패턴   // 중첩 패턴
본문       ::= 식 | "{" 문장* "}"
```

스크루티니 괄호는 필수이고 비어 있을 수 없습니다.

### 3.2 의미

`match`는 **표현식**입니다. 스크루티니를 한 번만 평가해 `kind` 필드로 분기하고
선택된 암의 값으로 평가됩니다. rl enum이 만든 값뿐 아니라 **`kind` 문자열
필드를 가진 모든 태그드 유니언**에 쓸 수 있습니다.

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
| 중첩 패턴이 있는 match | 같은 사유로 if-체인 방출 — 경로 조건을 잇고(`$rl_m.kind === "Ok" && $rl_m.value.kind === "Some"`) 각 단계의 바인딩을 그 경로에서 구조 분해(`const { value: v } = $rl_m.value;`). tsc의 제어 흐름 분석이 경로를 좁히므로 타입 트릭 없음 |
| 블록 본문이 있는 match | 전체가 라벨 블록 `$rl_b: { ... }`로 감싸이고 블록 본문은 `break $rl_b`로 끝남 |

### 3.5 `await`와 async

스크루티니·가드·암 본문에 `await`가 있으면 async 함수로 방출되고 전체가
`await`됩니다. 감지는 토큰 단위이므로 중첩 함수 안의 `await`도 async를
유발합니다 ([§9](#9-제한사항)).

### 3.6 소진성 검사

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
   ([§8.3](#83-선언-수집과-프로젝트-단위-소진성))
3. 내장 `Option`/`Result` ([§4.2](#42-내장-enum과-소진성-검사))

- or-패턴 암은 모든 대안 태그를 커버한 것으로 인정됩니다.
- **가드 암은 케이스를 커버하지 못합니다** (조건이 거짓일 수 있으므로).
  `Some(v) if v > 0 => v` 하나만 있는 match는 `Some`·`None` 둘 다 빠진 것으로
  보고됩니다.
- **중첩 패턴 암도 케이스를 커버하지 못합니다** (내부 태그가 다를 수
  있으므로). `Ok(value: Some(v))`와 `Ok(value: None())`로 내부 공간을 다
  덮어도 v1 검사는 이를 확인하지 않습니다 — 무중첩 `Ok` 암이나 `_` 암을
  두세요.
- `_` 암이 있는 match는 검사하지 않습니다.
- 어느 출처에도 없는 태그의 match는 검사 없이 컴파일되고 런타임 가드만
  남습니다.

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
| 원소 | 태그 패턴(or-패턴·바인딩 포함) 또는 `_`. 원소 수는 스크루티니 수와 일치해야 함 (불일치는 컴파일 에러) |
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

모듈 안의 선언은 같은 이름의 rl enum을 컴파일한 결과와 **바이트 단위로 같은
형태**이므로 `match`가 그대로 동작합니다.

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
- 검사는 구조적(`kind !== "Ok"`)입니다. `Result`가 아닌 값에 쓰면 생성물에서
  tsc 에러가 됩니다. `Option` 전파는 지원하지 않습니다 — `Option.okOr`로
  바꾸세요.
- 함수 반환 타입은 식의 `Err` 타입과 호환되는 `Result`여야 합니다
  (Rust의 `From` 같은 자동 변환 없음).

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

---

## 8. 모듈: `.rl` import 지정자 재작성

`.rl` 파일은 다른 `.rl`을 상대 경로 그대로 import합니다. 컴파일러가 방출 시
지정자를 소비 측이 해석할 수 있는 형태로 바꿉니다.

```rl
import { CalcError } from "./error.rl";   // parser.rl
```

```ts
import { CalcError } from "./error.js";   // parser.ts (기본 --rewrite-imports js)
```

### 8.1 재작성 대상

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

### 8.2 방출 형태 (`--rewrite-imports`)

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

### 8.3 선언 수집과 프로젝트 단위 소진성

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

## 9. 제한사항

| 항목 | 내용 |
|------|------|
| 소스맵 | 생성하지 않습니다. 생성된 `.ts`와 원본 행이 대체로 대응하지만 보장되지 않습니다 |
| 패턴 | 태그 패턴(or-패턴·가드·중첩 패턴 포함)과 `_`뿐. 리터럴 패턴은 의도적으로 미지원. `_ if ...`는 rl 구문이 아닙니다 |
| 중첩 패턴 | match 전용 — let-else 바인딩은 별칭만. or-패턴과 조합 불가. 소진성 커버 불인정(가드와 동일) |
| 튜플 match | 튜플-패턴 사이의 or(`(A, B) \| (C, D)`)는 미지원 — 원소 수준 or로 씁니다: `(A, B \| D)`. 스크루티니 분리는 구조적이라 최상위 비교 연산자(`a < b, c > d`)는 제네릭 인자로 오인될 수 있습니다 — 괄호로 감쌉니다 |
| 소진성 수집 | 직접(1-홉) 상대 경로 `.rl` import만. re-export 체인·패키지 경로의 enum과 손으로 쓴 유니언은 검사되지 않습니다 ([§3.6](#36-소진성-검사), [§8.3](#83-선언-수집과-프로젝트-단위-소진성)) |
| import 재작성 | 정적 상대 경로 `.rl` 지정자만. 참조 파일의 존재는 검사하지 않습니다 ([§8](#8-모듈-rl-import-지정자-재작성)) |
| `try` | `;` 필수, 식은 `(`/`<`로 시작 불가, match 내부·템플릿 보간·모듈 최상위 불가. `Option` 전파 미지원 |
| `let-else` | 패턴 괄호와 `;` 필수, else는 발산 키워드로 끝나야 함. or-패턴·가드·중첩 패턴, `= try 식 else` 조합 미지원 |
| `if let` | else는 블록 또는 if-let만(일반 `else if (조건)` 불가 — else 블록 안에 쓰기). or-패턴·가드 미지원. 표현식 위치 불가. `= 식` 최상위의 괄호 없는 블록 화살표는 괄호 필수 |
| 표현식 암 | 객체 리터럴은 괄호 필수: `Tag => ({ a: 1 })` |
| 스크루티니 | 괄호 필수: `match (x) { ... }` |
| `\|>` head 판별 | 구조 추적입니다. 삼항·화살표는 괄호 필수([§7.4](#74-구조-규칙)), 이항 `in`/`instanceof`·복합 시프트 대입(`>>=`) 옆이나 세미콜론 없는 코드의 문장 경계에서는 head를 짧거나 길게 잡을 수 있습니다 — head를 괄호로 감싸면 항상 명확합니다 |
| `\|> ?.m()` | `?.` 시작 스텝 미지원 ([§7.4](#74-구조-규칙)) |
| async 감지 | 토큰 단위 — 중첩 함수 안의 `await`도 async 방출을 유발하므로, 그런 match를 async가 아닌 곳에 두면 생성물이 문법 에러가 됩니다 |
| `.tsx` | 미지원 (제네릭 화살표 함수 출력이 JSX와 충돌) |
| 식별자 | rl 구문 안에서는 ASCII만 |
| `--no-verify` | 필드 타입 오류가 컴파일 시점에 잡히지 않고 tsc 단계에서 드러납니다 |
