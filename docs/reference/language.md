# rl 언어 레퍼런스

rl 언어의 문법과 동작을 정의하는 문서입니다. 빠르게 감을 잡으려면
[README](../../README.md)를 보세요. CLI는 [`cli.md`](./cli.md), 에러 메시지는
[`errors.md`](./errors.md)에 있습니다. 컴파일러 내부가 어떻게 동작하는지는
사용에 필요하지 않으므로 여기서 다루지 않습니다 — 궁금하다면
[`docs/design/`](../design/)을 보세요.

## 목차

1. [기본 원칙](#1-기본-원칙)
2. [`enum` 선언](#2-enum-선언)
3. [`match` 표현식](#3-match-표현식)
4. [예약어](#4-예약어)
5. [제한사항](#5-제한사항)

---

## 1. 기본 원칙

`.rl` 파일은 TypeScript 파일에 딱 두 구문 — rl `enum` 선언과 `match` 표현식 —
을 더한 것입니다.

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

- **태그·이름**은 예약어([§4](#4-예약어))가 아닌 ASCII 식별자입니다.
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
방출됩니다 ([§5](#5-제한사항)).

### 3.6 소진성 검사

`_` 없는 match는 **같은 파일에 선언된 rl enum**과 대조해 소진성을 검사하고,
빠진 케이스가 있으면 컴파일 에러로 보고합니다 (선언 순서는 무관합니다):

```
$ rlc shapes.rl
rlc: shapes.rl:12:25: match on enum Shape is not exhaustive: missing "Rect"
     (add the missing arms or a final `_` arm)
```

- `_` 암이 있는 match는 정의상 소진적이므로 검사하지 않습니다.
- 검사는 **파일 단위**입니다. 다른 파일에서 import한 enum이나 손으로 쓴
  태그드 유니언에 대한 match는 검사 없이 컴파일되며, 런타임 가드([§3.4](#34-컴파일-결과))만
  남습니다. 프로젝트 단위 검사는 로드맵입니다.

---

## 4. 예약어

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

## 5. 제한사항

- **소스맵 미생성.** 생성된 `.ts`와 원본 `.rl`의 행이 대체로 대응하지만
  보장되지 않습니다.
- **패턴은 케이스 태그와 `_`뿐.** 리터럴 패턴, 중첩 패턴, `A | B` 패턴,
  가드는 의도적으로 지원하지 않습니다 (최소 기능 유지).
- **소진성 검사는 같은 파일의 rl enum에 대해서만** 동작합니다 (§3.6).
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
