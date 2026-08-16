# rl

**TypeScript로 컴파일되는 초경량 전처리 언어.**
[Civet](https://civet.dev)처럼 TypeScript 위에 얹히는 언어이며, 딱 두 가지 기능만 추가합니다:
Rust 스타일의 **`variant`** (태그드 유니언) 선언과 **`match`** 표현식.

핵심 원칙: **모든 유효한 TypeScript 파일은 그대로 유효한 `.rl` 파일입니다.**
컴파일러는 `variant`/`match` 구문만 변환하고, 나머지는 바이트 단위 그대로 통과시킵니다.
`.match(...)` 메서드 호출, `match`라는 이름의 함수/메서드, 문자열·주석·정규식 안의
`match` 등은 전혀 건드리지 않습니다.

## 사용법

```sh
node bin/rlc.js file.rl        # file.ts 생성
node bin/rlc.js src/           # src/ 아래 모든 .rl 재귀 컴파일
node bin/rlc.js -p file.rl     # stdout으로 출력
node bin/rlc.js -o out/ src/   # 출력 디렉터리 지정
node bin/rlc.js --check src/   # 컴파일만 하고 쓰지 않음 (문법 검사)
```

라이브러리로도 쓸 수 있습니다:

```js
import { compile } from "rl-lang";
const { code } = compile(rlSource, { filename: "shapes.rl" });
```

## `variant` — 태그드 유니언 선언

```rl
export variant Shape {
  Circle(radius: number),
  Rect(width: number, height: number),
  Point,
}
```

위 선언은 아래 TypeScript로 컴파일됩니다. 타입과 생성자 객체가 같은 이름으로 생성되어,
Rust의 `Shape::Circle(1.0)`처럼 `Shape.Circle(1)`로 값을 만듭니다.

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

제네릭도 지원합니다:

```rl
variant Option<T> {
  Some(value: T),
  None,
}

const x = Option.Some(7);          // Option<number>
const y: Option<string> = Option.None;
```

## `match` — 패턴 매칭 표현식

```rl
const area = match (shape) {
  Circle(radius) => Math.PI * radius * radius,
  Rect(width, height) => width * height,
  Point => 0,
};
```

`match`는 **표현식**이며, `kind` 필드를 판별하는 `switch` IIFE로 컴파일됩니다.
`variant`로 선언한 타입뿐 아니라 `kind` 필드를 가진 **모든 태그드 유니언**에
그대로 사용할 수 있습니다.

### 소진성 검사 (exhaustiveness check)

와일드카드 `_` 암이 없으면 컴파일러가 `default` 분기에 `never` 검사를 삽입합니다:

```ts
default: {
  const $rl_never: never = $rl_m;
  throw new Error("rl match: unhandled variant " + JSON.stringify($rl_never));
}
```

베리언트를 하나라도 빠뜨리면 **tsc가 컴파일 타임에 에러**를 냅니다
(`Type '...' is not assignable to type 'never'`). 다른 파일에서 import한
variant에 대해서도 동일하게 동작합니다 — 검사는 tsc의 타입 내로잉이 수행하기 때문입니다.

### 문법 정리

```rl
match (expr) {
  Tag => expr,                 // 유닛 베리언트
  Tag(field) => expr,          // 필드 바인딩 — 이름은 선언된 필드명과 일치해야 함
  Tag(field: alias) => expr,   // 이름 바꿔서 바인딩
  Tag(a, b) => {               // 블록 본문 — 값을 내려면 return 사용
    const s = a + b;
    return s * 2;
  },
  _ => expr,                   // 와일드카드 — 반드시 마지막 암
}
```

- 바인딩은 **이름 기준**입니다 (Rust의 위치 기준과 다름). 필드 일부만 바인딩해도 됩니다.
- `_` 암이 있으면 `default`가 되고 `never` 검사는 생략됩니다.
- 암 본문(또는 스크루티니)에 `await`가 있으면 `(await (async () => ...)())`로
  컴파일되어 async 함수 안에서 자연스럽게 동작합니다.
- match 중첩, 템플릿 리터럴 보간(`${...}`) 내부 사용 모두 지원합니다.

## TypeScript 호환성이 지켜지는 방식

컴파일러는 전체 파싱을 하지 않습니다. 문자열·템플릿·주석·정규식을 인식하며 소스를
스캔하다가 `variant` / `match` 키워드 후보를 만나면 **해당 구문 전체가 완전하게
파싱될 때만** 변환합니다. 파싱이 하나라도 어긋나면 원문 그대로 통과시키므로,
`match`라는 이름의 클래스 메서드, `variant`라는 변수 등 기존 TS 코드는 안전합니다.

## 제한사항 (v0.1)

- 소스맵은 아직 생성하지 않습니다.
- 패턴은 베리언트 태그와 `_`만 지원합니다 (리터럴/중첩/`|` 패턴 없음 — 의도적으로 최소 기능).
- 표현식 암에서 객체 리터럴을 바로 반환하려면 화살표 함수처럼 괄호가 필요합니다:
  `Tag => ({ a: 1 })`.
- `match (x) { ... }` 형태처럼 스크루티니에 괄호가 필수입니다.
- 중첩된 비동기 함수 안에만 `await`가 있는 암도 async로 감싸질 수 있습니다
  (이 경우 바깥 컨텍스트가 async가 아니면 문법 에러로 드러납니다).
- `.tsx` 미지원 (제네릭 화살표 함수 출력이 JSX와 충돌할 수 있음).

## 개발

```sh
npm test   # node --test; tsc가 있으면 타입/런타임 테스트까지 수행
```

`examples/shapes.rl` → `examples/shapes.ts`가 전체 동작 예시입니다.
