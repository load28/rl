# rl 언어 레퍼런스

rl 언어의 문법과 동작을 정의하는 문서입니다. 빠르게 감을 잡으려면
[README](../../README.md)를 보세요. CLI는 [`cli.md`](./cli.md), 에러 메시지는
[`errors.md`](./errors.md), 표준 라이브러리 API는 [`std.md`](./std.md)에
있습니다. 컴파일러 내부가 어떻게 동작하는지는
사용에 필요하지 않으므로 여기서 다루지 않습니다 — 궁금하다면
[`docs/design/`](../design/)을 보세요.

## 목차

1. [기본 원칙](#1-기본-원칙)
2. [`enum` 선언](#2-enum-선언)
3. [`match` 표현식](#3-match-표현식)
4. [표준 라이브러리와 내장 enum](#4-표준-라이브러리와-내장-enum)
5. [에러 전파: `try` 문](#5-에러-전파-try-문)
6. [예약어](#6-예약어)
7. [제한사항](#7-제한사항)

---

## 1. 기본 원칙

`.rl` 파일은 TypeScript 파일에 딱 세 구문 — rl `enum` 선언, `match` 표현식,
`try` 문 — 을 더한 것입니다.

> **모든 유효한 TypeScript 파일은 그대로 유효한 `.rl` 파일이며, 자기 자신으로
> 컴파일됩니다.**

컴파일러는 rl 구문으로 **완전하게 파싱되는** 부분만 변환하고, 나머지는 전부
원문 그대로 통과시킵니다. 그래서 기존 TypeScript 코드는 안전합니다:

- 문자열·주석·정규식·템플릿 리터럴 텍스트 안의 `enum`/`match`는 건드리지
  않습니다.
- `str.match(...)` 같은 메서드 호출, TypeScript 자체의 모든 `enum` 형태는
  변환되지 않습니다.
- 템플릿 리터럴의 `${ ... }` 보간 내부에서는 rl 구문을 쓸 수 있습니다.

rl 구문 안의 식별자(케이스 태그, 필드명, 바인딩)는 ASCII 식별자
(`[A-Za-z_$][A-Za-z0-9_$]*`)만 지원합니다. 그 밖의 위치(문자열, 주석, 통과
영역의 코드)에서는 유니코드를 자유롭게 쓸 수 있습니다.

모든 컴파일 에러는 원본 `.rl` 소스 기준 `파일:행:열`(1-기반)로 보고됩니다.
전체 목록은 [`errors.md`](./errors.md) 참조.

---

## 2. `enum` 선언

### 2.1 문법

```
rl-enum      ::= "export"? "enum" 식별자 제네릭? "{" 케이스-목록 "}"
케이스-목록  ::= 케이스 ("," 케이스)* ","?
케이스       ::= 태그                         // 유닛 케이스
               | 태그 "(" 필드-목록? ")"      // 페이로드 케이스 (빈 괄호 허용)
필드-목록    ::= 필드 ("," 필드)* ","?
필드         ::= 이름 "?"? ":" 타입
```

- **태그·이름**은 예약어([§6](#6-예약어))가 아닌 ASCII 식별자입니다.
- **제네릭**은 제약·기본값(`<T extends string, U = number>`)과
  `const`/`in`/`out` 한정자를 포함해 그대로 지원됩니다.
- **타입** 자리에는 함수 타입, 제네릭 타입, 객체 타입 등 임의의 TypeScript
  타입 표기를 쓸 수 있습니다.
- 각 자리의 공백·주석·후행 콤마는 자유롭게 허용됩니다.

### 2.2 rl enum과 TS enum의 구분

`enum` 선언은 다음 **둘 중 하나 이상**을 만족할 때만 rl enum으로 변환됩니다:

1. 케이스에 페이로드 괄호 `(...)`가 하나라도 있다 — 빈 괄호 `Tag()`도 포함.
2. 선언에 제네릭이 있다 — `enum Option<T> { ... }`.

그 외의 모든 `enum`(그리고 `const enum` / `declare enum`)은 순수 TypeScript
enum으로 **그대로 통과**합니다. 유효한 TypeScript enum이 rl enum으로 오인되는
경우는 없습니다.

**유닛 케이스만 필요한데 rl 의미론(태그드 유니언 + match)을 원하면** 한
케이스에 빈 괄호를 붙여 표시합니다:

```rl
enum Color { Red, Green, Blue }        // TS enum — 그대로 통과
enum Status { Active(), Inactive }     // rl enum — Active는 유닛 케이스와 동일하게 동작
```

### 2.3 컴파일 결과

하나의 rl enum은 **같은 이름의 타입 별칭과 생성자 객체** 두 선언으로
컴파일됩니다. `export`가 있으면 둘 다 export됩니다.

입력:

```rl
export enum Shape {
  Circle(radius: number),
  Rect(width: number, height: number),
  Point,
}
```

출력:

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

- **판별 필드는 항상 `kind`**이며 값은 케이스 태그 문자열입니다.
- 필드가 있는 케이스는 생성자 함수가 됩니다: `Shape.Circle(1)`.
- 필드가 없는 케이스(괄호 없음 또는 빈 괄호)는 싱글턴 값이 됩니다:
  `Shape.Point`.
- 제네릭 enum의 생성자는 제네릭을 그대로 이어받습니다:

```ts
export type Option<T> =
  | { kind: "Some"; value: T }
  | { kind: "None" };
export const Option = {
  Some: <T>(value: T): Option<T> => ({ kind: "Some", value }),
  None: { kind: "None" } as const,
};
```

제네릭 유닛 케이스(`Option.None`)는 모든 `Option<T>`에 대입 가능합니다.

### 2.4 컴파일 시점 검사

rl enum에는 다음 검사가 적용됩니다 (전부 rlc 컴파일 에러):

- **중복 케이스** — 같은 태그가 두 번 나오면 에러.
- **필드 타입 검증** — 타입 표기가 TypeScript 타입 문법으로 파싱되지 않으면
  에러. `--no-verify`(라이브러리에서는 `Options { verify: false }`)로 끌 수
  있습니다.

---

## 3. `match` 표현식

### 3.1 문법

```
match-식   ::= "match" "(" 식 ")" "{" 암-목록 "}"
암-목록    ::= 암 ("," 암)* ","?
암         ::= 패턴 "=>" 본문
패턴       ::= 태그                        // 유닛 패턴
             | 태그 "(" 바인딩-목록? ")"   // 필드 바인딩
             | "_"                         // 와일드카드 — 반드시 마지막 암
바인딩-목록 ::= 바인딩 ("," 바인딩)* ","?
바인딩     ::= 필드명                      // 같은 이름으로 바인딩
             | 필드명 ":" 별칭             // 이름 바꿔 바인딩
본문       ::= 식                          // 표현식 본문
             | "{" 문장* "}"               // 블록 본문
```

- **스크루티니 괄호는 필수**이고 내용이 비어 있으면 안 됩니다.
- `str.match(...)`처럼 `.` 뒤에 오는 `match`는 rl match가 아닙니다 — 기존 TS
  코드는 안전합니다.

### 3.2 의미

`match`는 **표현식**입니다. 스크루티니를 **한 번만** 평가해 그 값의 `kind`
필드로 분기하고, 선택된 암의 값으로 평가됩니다. rl `enum`이 만든 값뿐 아니라
**`kind` 문자열 필드를 가진 모든 태그드 유니언**에 쓸 수 있습니다.

바인딩은 Rust와 달리 **위치가 아닌 이름 기준**입니다. `Tag(field)`는 페이로드
객체에서 `field` 프로퍼티를 구조 분해하는 것이므로 선언된 필드명과 일치해야
하며, 일부 필드만 바인딩해도 되고 순서도 무관합니다. `Tag(field: alias)`는
`alias`라는 이름으로 바인딩합니다.

암 검사 규칙 (rlc 컴파일 에러):

- 같은 태그의 암이 두 번 나오면 **중복 암** 에러.
- `_` 암은 반드시 마지막이어야 합니다.

### 3.3 본문 형태

- **표현식 본문**: `Tag => 식`. 객체 리터럴을 바로 돌려주려면 화살표 함수처럼
  괄호가 필요합니다: `Tag => ({ a: 1 })`.
- **블록 본문**: `Tag => { ... }`. 값을 내려면 `return`을 사용합니다.
  `return` 없이 끝나면 그 암의 값은 `undefined`입니다.

본문 안에서는 **match 중첩**, rl `enum` 선언, 템플릿 보간 내 사용이 모두
지원됩니다.

### 3.4 컴파일 결과

`match`는 `kind`를 판별하는 `switch` 기반 즉시 실행 함수로 컴파일됩니다.

입력:

```rl
const area = match (shape) {
  Circle(radius) => Math.PI * radius * radius,
  Rect(width: w, height) => w * height,
  Point => 0,
};
```

출력 (형태):

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

`_` 암은 `default` 분기가 됩니다. `_` 암이 없으면 위와 같은 **런타임 가드**
`default`가 들어갑니다 — 타입 시스템을 우회해 들어온 값(외부 입력 등)에 대해
즉시 throw하는 fail-fast입니다.

### 3.5 `await`와 async

스크루티니나 암 본문에 `await`가 있으면 async 함수로 방출되고 전체가
`await`되므로, async 함수 안에서 `match` 암에 `await`를 자연스럽게 쓸 수
있습니다:

```rl
const data = match (source) {
  Url(href) => await fetch(href).then((r) => r.text()),
  Inline(text) => text,
};
```

주의: 감지는 토큰 단위이므로 중첩된 함수 안에만 `await`가 있어도 async로
방출됩니다 ([§7](#7-제한사항)).

### 3.6 소진성 검사

`_` 없는 match는 **같은 파일에 선언된 rl enum**(그리고 내장 `Option`/`Result`
— [§4.2](#42-내장-enum과-소진성-검사))과 대조해 소진성을 검사하고, 빠진
케이스가 있으면 컴파일 에러로 보고합니다 (선언 순서는 무관합니다):

```
$ rlc shapes.rl
rlc: shapes.rl:12:25: match on enum Shape is not exhaustive: missing "Rect"
     (add the missing arms or a final `_` arm)
```

- `_` 암이 있는 match는 정의상 소진적이므로 검사하지 않습니다.
- 검사는 **파일 단위**입니다. 다른 파일에서 import한 enum이나 손으로 쓴
  태그드 유니언에 대한 match는 검사 없이 컴파일되며, 런타임 가드([§3.4](#34-컴파일-결과))만
  남습니다 — 단, 내장 `Option`/`Result`([§4.2](#42-내장-enum과-소진성-검사))는
  예외입니다. 프로젝트 단위 검사는 로드맵입니다.

---

## 4. 표준 라이브러리와 내장 enum

### 4.1 표준 라이브러리 모듈

rl은 런타임을 주입하지 않습니다. 대신 `Option<T>`/`Result<T, E>`와 함수형
콤비네이터(`map`, `andThen`, `unwrapOr`, ...)가 담긴 **순수 TypeScript 모듈
하나**를 컴파일러가 제공합니다:

```sh
rlc --emit-std src/rl.ts     # 표준 라이브러리 모듈 생성
```

생성된 모듈은 일반 TypeScript 파일이므로 평범하게 import해서 씁니다
(import 문은 통과 영역이라 컴파일러가 건드리지 않습니다):

```rl
import { Option, Result } from "./rl.js";

const half = (n: number): Option<number> =>
  n % 2 === 0 ? Option.Some(n / 2) : Option.None;

const msg = match (half(4)) {
  Some(value) => `half=${value}`,
  None => "odd",
};
```

모듈 안의 `Option`/`Result` 선언은 같은 이름의 rl enum이 컴파일된 결과와
정확히 같은 형태(`kind` 태그드 유니언 + 생성자 const)이므로 `match`가 그대로
동작합니다. 전체 API(콤비네이터 목록)는 [`std.md`](./std.md) 참조.

### 4.2 내장 enum과 소진성 검사

`Option`(케이스 `Some`/`None`)과 `Result`(케이스 `Ok`/`Err`)는 **내장
enum**입니다: 파일에 선언이 없어도 소진성 검사([§3.6](#36-소진성-검사))의
대상이 됩니다.

```rl
const f = (o: Option<number>) =>
  match (o) { Some(value) => value };
// rlc: file.rl:2:3: match on built-in enum Option is not exhaustive:
//      missing "None" (add the missing arms or a final `_` arm)
```

- 파일에 **같은 이름의 rl enum을 직접 선언하면 그 선언이 내장을 대체**합니다
  (섀도잉) — `Option`/`Result`를 직접 선언하던 기존 코드는 의미가 바뀌지
  않습니다.
- 내장 enum은 **선언을 만들어주지 않습니다**. 값과 타입은 표준 라이브러리
  모듈에서 import하거나 직접 선언해야 합니다 — 내장은 소진성 검사에만
  관여합니다.
- 주의: 손으로 쓴 유니언이 `Some`/`None`/`Ok`/`Err` 태그의 일부만 쓰는 경우,
  `_` 없는 match는 내장 enum 기준으로 검사에 걸릴 수 있습니다. 의도적으로
  일부 태그만 다루려면 마지막에 `_` 암을 두세요.

---

## 5. 에러 전파: `try` 문

### 5.1 문법

```
try-문   ::= "try" 식 ";"                                    // 전파만
           | ("const" | "let" | "var") 바인딩 "=" "try" 식 ";" // 값 바인딩
바인딩   ::= 식별자 | 구조 분해 패턴      // 타입 주석 허용
```

- **세미콜론이 필수**입니다 — `;`가 없으면 rl 구문으로 인식되지 않고 원문
  통과합니다.
- 식은 `(`나 `<`로 시작할 수 없습니다 — 인터페이스의 `try(x);` /
  `try<T>(x);` 멤버 시그니처와 구분할 수 없기 때문입니다. `try (식);` 대신
  `try 식;`으로 씁니다.
- TypeScript의 `try { ... } catch` 블록, `obj.try(...)` 같은 멤버 이름
  `try`는 전부 그대로 통과합니다 — 기존 TS 코드는 안전합니다.

### 5.2 의미

Rust의 `?` 연산자에 해당합니다. 식은 `Result`([§4](#4-표준-라이브러리와-내장-enum))
여야 하고, `Ok`면 값을 (선언 형태라면 바인딩에) 풀고, `Err`면 그 값을
**둘러싼 함수에서 즉시 `return`**합니다:

```rl
function readPort(cfg: string): Result<number, string> {
  const parsed = try parseNum(cfg);   // Err면 여기서 바로 return
  try validateRange(parsed);          // 값이 필요 없으면 전파만
  return Result.Ok(parsed);
}
```

### 5.3 컴파일 결과

IIFE 없이 둘러싼 함수 스코프에 문장으로 방출됩니다 (한 줄):

```ts
const $rl_t0 = (parseNum(cfg)); if ($rl_t0.kind !== "Ok") return $rl_t0; const parsed = $rl_t0.value;
```

- 임시 변수 이름은 파일 단위로 유일합니다 (`$rl_t0`, `$rl_t1`, ...).
- IIFE가 없으므로 식 안의 `await`가 그대로 동작합니다:
  `const data = try await fetchData();`.
- 검사는 구조적(`kind !== "Ok"`)입니다. `Result`가 아닌 값에 쓰면 생성물에서
  tsc 타입 에러가 됩니다. `Option` 전파는 지원하지 않습니다 —
  `Option.okOr`로 `Result`로 바꿔서 씁니다.
- 함수의 반환 타입은 식의 `Err` 타입과 호환되는 `Result`여야 합니다
  (Rust의 `From` 같은 에러 타입 자동 변환은 없습니다).

### 5.4 사용 위치 제약

`try`는 **함수 본문의 문장 위치**에서만 씁니다:

- **match 표현식 내부(스크루티니·암 본문)·템플릿 보간·다른 try의 식
  내부에서는 컴파일 에러**입니다 — 그 위치의 `return`은 둘러싼 함수가 아니라
  match의 switch IIFE 등에서 반환되어 의미가 달라지기 때문입니다. 해당
  로직을 헬퍼 함수로 추출하면 됩니다.
- **모듈 최상위(함수 밖)**에서는 쓸 수 없습니다 — 생성물의 최상위 `return`이
  유효한 TypeScript가 아니어서 출력 자가 검사에서 실패합니다.

---

## 6. 예약어

다음 단어는 enum 이름, 케이스 태그, 필드명, match 패턴 태그, 바인딩 이름,
별칭이 될 수 없습니다. 이 단어가 들어간 구문은 rl 구문으로 해석되지 않고 원문
통과합니다 (에러가 아님).

```
async await break case catch class const continue debugger default delete
do else enum export extends false finally for function if import in
instanceof let new null of return static super switch this throw true try
typeof var void while with yield
```

---

## 7. 제한사항

- **소스맵 미생성.** 생성된 `.ts`와 원본 `.rl`의 행이 대체로 대응하지만
  보장되지 않습니다.
- **패턴은 케이스 태그와 `_`뿐.** 리터럴 패턴, 중첩 패턴, `A | B` 패턴,
  가드는 의도적으로 지원하지 않습니다 (최소 기능 유지).
- **소진성 검사는 같은 파일의 rl enum과 내장 `Option`/`Result`에 대해서만**
  동작합니다 (§3.6, §4.2).
- **`try`는 `;` 필수, 식은 `(`/`<`로 시작 불가**이며, match 내부·템플릿
  보간·모듈 최상위에서는 쓸 수 없습니다 (§5). `Option` 전파는 지원하지
  않습니다.
- **표현식 암의 객체 리터럴은 괄호 필수**: `Tag => ({ a: 1 })`.
- **스크루티니 괄호 필수**: `match (x) { ... }`.
- **async 감지는 토큰 단위**: 중첩 함수 안에만 `await`가 있어도 async로
  방출되므로, 그런 match를 async가 아닌 컨텍스트에 두면 생성물이 문법 에러가
  됩니다 (§3.5).
- **`.tsx` 미지원**: 제네릭 화살표 함수 출력(`<T>(...) => ...`)이 JSX 문법과
  충돌할 수 있습니다.
- **rl 구문 안의 식별자는 ASCII만** 지원합니다 (§1).
- **`--no-verify` 사용 시** 필드 타입 오류가 컴파일 시점에 잡히지 않고
  생성물에 그대로 전파되어 tsc 단계에서 드러납니다.
