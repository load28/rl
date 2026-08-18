# rl match 리터럴 패턴 설계 의견

## 결론

리터럴 패턴은 추가할 만하다.

다만 `rlc`가 TypeScript 타입 시스템을 직접 흉내 내면 안 된다.
이미 존재하는 TypeScript 연동 경로를 사용해야 한다.

따라서 설계는 두 층으로 나누는 것이 좋다.

1. 기본 컴파일 경로는 리터럴 패턴을 파싱하고 TypeScript로 방출한다.
2. typed 경로는 TypeScript checker로 scrutinee 타입을 조회해서 리터럴 유니언 소진성을 검사한다.

이렇게 하면 rl의 기존 장점인 "가벼운 구조 전처리기" 성격을 유지하면서,
TypeScript 타입 정보가 있는 환경에서는 더 Rust다운 match 경험을 줄 수 있다.

## 왜 리터럴 패턴이 필요한가

현재 rl의 `match`는 이미 표현식이다.

```rl
const value = match (shape) {
  Circle(radius) => radius,
  Point => 0,
};
```

즉 "match를 값으로 쓴다"는 Rust식 모델은 이미 있다.

하지만 현재 패턴은 태그드 유니언 전용이다.

```rl
match (shape) {
  Circle(radius) => ...,
  Point => ...,
}
```

반면 TypeScript 코드에는 리터럴 유니언이 많다.

```ts
type Direction = "north" | "south" | "east" | "west";
type Size = "sm" | "md" | "lg";
type Status = 200 | 400 | 404 | 500;
```

이 값을 rl에서 자연스럽게 다루려면 아래가 가능해야 한다.

```rl
const label = match (dir) {
  "north" => "N",
  "south" => "S",
  "east" => "E",
  "west" => "W",
};
```

이건 새 `match` 기능이 아니라 기존 `match`의 패턴 공간 확장이다.

## 핵심 문제는 소진성이다

리터럴 패턴을 방출하는 것 자체는 어렵지 않다.

```rl
const label = match (dir) {
  "north" => "N",
  "south" => "S",
  _ => "?",
};
```

대략 이렇게 내리면 된다.

```ts
const label = ((() => {
  const $rl_m = dir;
  switch ($rl_m) {
    case "north": { return "N"; }
    case "south": { return "S"; }
    default: { return "?"; }
  }
})());
```

문제는 `_` 없는 match다.

```rl
type Direction = "north" | "south";

const label = match (dir) {
  "north" => "N",
  "south" => "S",
};
```

이게 exhaustive인지 알려면 `dir`의 타입이 `"north" | "south"`라는 사실을 알아야 한다.

현재 Rust 쪽 `rlc` 구조 파서만으로는 이 정보를 알 수 없다.
이 정보는 TypeScript 타입 체커 안에 있다.

## TypeScript 연동을 쓰는 게 맞다

직접 타입 추론을 구현하면 안 된다.

아래를 전부 직접 처리해야 하기 때문이다.

```ts
type Direction = "north" | "south";

const dirs = ["north", "south"] as const;
type Direction2 = typeof dirs[number];

type ApiStatus = 200 | 400 | 404 | 500;

import type { Mode } from "./mode";
```

이걸 rlc가 직접 따라가면 TypeScript의 부분 구현체가 된다.
그 순간 유지보수 비용이 급격히 커지고, TypeScript 버전 변화에도 계속 끌려간다.

이미 프로젝트에는 TypeScript 연동 경로가 있다.

- `rlc --types`
- VSCode language service
- emitted TypeScript virtual document
- TypeScript diagnostics source mapping

따라서 리터럴 유니언 소진성도 이 경로에 붙이는 게 맞다.

TypeScript compiler API는 `Program#getTypeChecker()`와 `getTypeAtLocation()`으로
AST 위치의 타입을 조회할 수 있다.

## 권장 구조

### 1. 기본 컴파일은 타입 없이 동작한다

기본 `rlc`는 지금처럼 빠르고 독립적으로 유지한다.

```rl
const label = match (dir) {
  "north" => "N",
  "south" => "S",
  _ => "?",
};
```

이 경로에서는 다음만 한다.

- 리터럴 패턴 파싱
- 중복 리터럴 검사
- `_` 위치 검사
- switch/if-chain 방출
- 런타임 fallback guard 방출

타입 기반 소진성은 하지 않는다.

### 2. typed 경로에서 소진성을 검사한다

TypeScript가 연결된 경로에서는 scrutinee 타입을 조회한다.

```rl
type Direction = "north" | "south";

const label = match (dir) {
  "north" => "N",
};
```

TypeScript checker가 `dir`의 타입을 `"north" | "south"`로 알려주면,
rlc는 arm 리터럴 집합과 비교해서 `"south"` 누락을 보고한다.

진단은 생성된 TypeScript 위치가 아니라 `.rl` 원본 위치로 매핑한다.

```txt
rlc: src/main.rl:4:15: match on literal union is not exhaustive: missing "south"
```

### 3. 타입을 모르면 보수적으로 간다

scrutinee 타입이 아래 중 하나면 typed 소진성 검사를 하지 않는다.

```ts
string
number
boolean
unknown
any
T
string | number
```

즉 유한한 리터럴 집합으로 확정되는 경우만 검사한다.

검사 가능한 예시는 아래다.

```ts
"a" | "b"
1 | 2 | 3
true | false
"sm" | "md" | "lg"
```

검사하지 않는 예시는 아래다.

```ts
string
number
boolean
T extends string
"a" | string
```

## 문법 제안

v1은 문자열, 숫자, boolean 리터럴만 받는다.

```rl
match (x) {
  "a" => 1,
  "b" => 2,
  _ => 0,
}

match (code) {
  200 => "ok",
  404 => "not found",
  _ => "error",
}

match (flag) {
  true => "yes",
  false => "no",
}
```

or-pattern도 허용할 수 있다.

```rl
match (code) {
  200 | 201 | 204 => "success",
  400 | 404 => "client error",
  500 => "server error",
  _ => "unknown",
}
```

태그 패턴과 리터럴 패턴은 한 match에서 섞지 않는 게 좋다.

```rl
// v1에서는 금지
match (x) {
  Some(value) => value,
  "none" => 0,
}
```

이유는 방출 기준이 다르기 때문이다.

- 태그 match는 `$rl_m.kind`를 본다.
- 리터럴 match는 `$rl_m` 자체를 본다.

## 소진성 정책

기본 경로에서는 `_` 없는 리터럴 match도 허용하는 편이 좋다.
대신 런타임 guard를 넣는다.

```rl
const label = match (dir) {
  "north" => "N",
  "south" => "S",
};
```

방출:

```ts
const label = ((() => {
  const $rl_m = dir;
  switch ($rl_m) {
    case "north": { return "N"; }
    case "south": { return "S"; }
    default: { throw new Error("rl match: unexpected literal " + JSON.stringify($rl_m)); }
  }
})());
```

typed 경로에서는 소진성을 검사한다.

- exhaustive면 통과한다.
- 누락이면 rl 진단을 낸다.
- 타입을 모르면 진단하지 않는다.

이 방식이 기존 태그 match와 가장 비슷하다.
현재 태그 match도 알 수 없는 enum이면 검사 없이 컴파일하고 런타임 guard를 둔다.

## 구현 관점

AST는 대략 이렇게 확장한다.

```rust
enum Pattern {
    Wildcard,
    Tags(Vec<TagPattern>),
    Literals(Vec<LiteralPattern>),
}

struct LiteralPattern {
    value: LiteralValue,
    off: usize,
}

enum LiteralValue {
    String(String),
    Number(String),
    Boolean(bool),
}
```

파서는 arm 첫 토큰을 더 받는다.

- `_`는 `Pattern::Wildcard`
- identifier는 `Pattern::Tags`
- string, number, `true`, `false`는 `Pattern::Literals`

codegen은 match 종류를 판별한다.

- 모든 arm이 tag/wildcard이면 기존 tag match
- 모든 arm이 literal/wildcard이면 literal match
- 섞이면 에러

typed 소진성은 별도 레이어에서 한다.

- emitted TS에서 scrutinee 위치를 찾는다.
- TypeScript checker로 타입을 조회한다.
- union member가 전부 string, number, boolean literal인지 확인한다.
- arm 리터럴과 비교한다.
- 누락을 `.rl` 원본 위치로 보고한다.

## 최종 의견

리터럴 패턴은 넣을 가치가 있다.

하지만 첫 구현에서 TypeScript 연동 소진성까지 한 번에 넣으면 범위가 커진다.
그래서 단계적으로 가는 게 좋다.

1. 리터럴 패턴 파싱과 방출
2. 중복 리터럴과 혼합 패턴 검사
3. VSCode와 `--types` 경로에서 TypeScript 기반 소진성 검사
4. 필요하면 `rlc --check --typed` 같은 명시적 typed check 모드 추가

이 순서가 rl의 기존 설계 계약을 가장 덜 흔든다.
