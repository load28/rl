# rl

**TypeScript로 컴파일되는 초경량 전처리 언어.** 컴파일러는 Rust로 작성되었습니다.
[Civet](https://civet.dev)처럼 TypeScript 위에 얹히는 언어이며, 딱 두 가지 기능만 추가합니다:
Rust 스타일의 **`enum`** (태그드 유니언) 선언과 **`match`** 표현식.

핵심 원칙 두 가지:

1. **모든 유효한 TypeScript 파일은 그대로 유효한 `.rl` 파일입니다.**
   컴파일러는 rl `enum`/`match` 구문만 변환하고, 나머지는 바이트 단위 그대로
   통과시킵니다. TypeScript 자체의 `enum`, `.match(...)` 메서드 호출, `match`라는
   이름의 함수, 문자열·주석·정규식 안의 키워드 등은 전혀 건드리지 않습니다.
2. **에러 계층이 분리되어 있습니다.** rl 수준의 에러(중복 케이스, 소진되지 않은
   match, 잘못된 필드 타입)는 전부 **rlc가 `파일:행:열`과 함께 직접 보고**합니다.
   컴파일 결과물은 타입 트릭 없는 순수한 TypeScript이며, tsc는 평범한 TS 코드를
   볼 뿐입니다.

## 문서

이 README는 튜토리얼입니다. 정확한 동작은 레퍼런스가 규정합니다:

- [`docs/reference/language.md`](./docs/reference/language.md) — **언어 레퍼런스**:
  문법, rl enum/TS enum 판별 규칙, 방출 코드 형태, 소진성 검사 알고리즘,
  예약어, 제한사항.
- [`docs/reference/cli.md`](./docs/reference/cli.md) — **CLI 레퍼런스**:
  옵션, 입출력 경로 규칙, 종료 코드.
- [`docs/reference/errors.md`](./docs/reference/errors.md) — **에러 레퍼런스**:
  모든 진단 메시지의 형식·원인·해결.
- Rust API 문서: `cargo doc --open` (`rlc::compile` / `Options` / `CompileError`).
- [`docs/design/`](./docs/design/) — 설계 결정 기록.

## 사용법

```sh
cargo install --path .   # rlc 설치 (또는 cargo build --release 후 target/release/rlc)

rlc file.rl              # file.ts 생성
rlc src/                 # src/ 아래 모든 .rl 재귀 컴파일
rlc -p file.rl           # stdout으로 출력
rlc -o out/ src/         # 출력 디렉터리 지정
rlc --check src/         # 컴파일만 하고 쓰지 않음 (문법 검사)
rlc --no-verify file.rl  # swc 검증 생략
```

Rust 라이브러리로도 쓸 수 있습니다:

```rust
use rlc::{compile, Options};

let code = compile(rl_source, &Options { filename: Some("shapes.rl"), verify: true })?;
```

## `enum` — Rust식 태그드 유니언

```rl
export enum Shape {
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
enum Option<T> {
  Some(value: T),
  None,
}

const x = Option.Some(7);          // Option<number>
const y: Option<string> = Option.None;
```

### TypeScript의 enum과 함께 쓰기

TypeScript 자체의 `enum`은 그대로 동작합니다. 두 형태의 구분 규칙:

- **케이스에 페이로드 `(...)`가 하나라도 있거나, 선언에 제네릭이 있으면 → rl enum.**
- 그 외 (유닛 멤버만 있거나, `= 값` 초기화가 있으면) → **순수 TS enum으로 그대로 통과.**

TS enum 멤버는 `Tag(...)` 형태나 제네릭을 가질 수 없으므로, 이 규칙이 유효한
TypeScript를 잘못 변환하는 일은 없습니다. `const enum` / `declare enum`도 항상
TS의 것으로 취급됩니다.

```rl
enum Color { Red, Green, Blue }        // TS enum — 그대로 통과
enum Level { Info = "INFO" }           // TS enum — 그대로 통과
enum Shape { Circle(r: number), Dot }  // rl enum — 태그드 유니언으로 변환
enum Status { Active(), Inactive }     // 유닛만 필요한데 rl 의미론을 원하면
                                       // 한 케이스에 ()를 붙여 표시
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
rl `enum`으로 선언한 타입뿐 아니라 `kind` 필드를 가진 **모든 태그드 유니언**에
그대로 사용할 수 있습니다.

### 소진성 검사 (exhaustiveness check)

**빠진 케이스는 rlc의 컴파일 에러입니다** — tsc에 위임하지 않습니다. rlc는 파일 안의
rl enum 선언을 수집해 두고, `_` 없는 match의 암들이 어떤 enum의 케이스들인지
판별한 뒤 전부 커버되는지 직접 검사합니다 (선언 순서 무관):

```
$ rlc shapes.rl
rlc: shapes.rl:12:25: match on enum Shape is not exhaustive: missing "Rect"
     (add the missing arms or a final `_` arm)
```

생성되는 코드에는 타입 트릭이 없습니다. `default` 분기는 타입 시스템을 우회한
값(외부에서 들어온 데이터 등)에 대비한 순수 런타임 가드일 뿐입니다:

```ts
default: { throw new Error("rl match: unexpected case " + JSON.stringify($rl_m)); }
```

암 태그가 이 파일의 어떤 rl enum과도 일치하지 않으면(다른 파일에서 import한 enum,
손으로 쓴 유니언) rlc는 검사를 건너뜁니다 — 런타임 가드는 그대로 유효합니다.

### 문법 정리

```rl
match (expr) {
  Tag => expr,                 // 유닛 케이스
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
- `_` 암이 있으면 `default`가 되고 소진성 검사는 생략됩니다.
- 암 본문(또는 스크루티니)에 `await`가 있으면 `(await (async () => ...)())`로
  컴파일되어 async 함수 안에서 자연스럽게 동작합니다.
- match 중첩, 템플릿 리터럴 보간(`${...}`) 내부 사용 모두 지원합니다.

## TypeScript 호환성이 지켜지는 방식

rl 구문은 유효한 TS가 아니므로 기존 TS 파서에 통째로 태울 수 없습니다. 컴파일러는
문자열·템플릿·주석·정규식을 인식하며 소스를 스캔하다가 `enum` / `match` 키워드
후보를 만나면 **해당 구문 전체가 완전하게 파싱될 때만** (enum은 위의 페이로드
규칙까지 만족할 때만) 변환합니다. 파싱이 하나라도 어긋나면 원문 그대로
통과시키므로, `match`라는 이름의 클래스 메서드, TS의 모든 enum 형태 등 기존 TS
코드는 안전합니다.

여기에 [swc](https://swc.rs)의 TypeScript 파서가 두 곳에서 검증을 맡습니다
(자세한 아키텍처는 `docs/design/`):

- **조각 검증** — rl enum 필드의 타입 표기를 파싱해, 잘못된 타입을 rl 컴파일
  시점에 정확한 `파일:행:열`과 함께 거부합니다.
- **출력 검증** — 생성된 TS 전체를 파싱하는 자가 검사. `--no-verify`로 끌 수
  있습니다 (swc가 아직 모르는 최신 TS 문법을 쓰는 코드를 위한 탈출구).

## 제한사항

- 소스맵은 아직 생성하지 않습니다.
- 패턴은 케이스 태그와 `_`만 지원합니다 (리터럴/중첩/`|` 패턴 없음 — 의도적으로 최소 기능).
- 소진성 검사는 **같은 파일에 선언된 rl enum**에 대해서만 동작합니다. import한
  enum에 대한 match는 검사 없이 컴파일되고 런타임 가드만 남습니다 (프로젝트 단위
  검사는 로드맵).
- 표현식 암에서 객체 리터럴을 바로 반환하려면 화살표 함수처럼 괄호가 필요합니다:
  `Tag => ({ a: 1 })`.
- `match (x) { ... }` 형태처럼 스크루티니에 괄호가 필수입니다.
- 중첩된 비동기 함수 안에만 `await`가 있는 암도 async로 감싸질 수 있습니다
  (이 경우 바깥 컨텍스트가 async가 아니면 문법 에러로 드러납니다).
- `.tsx` 미지원 (제네릭 화살표 함수 출력이 JSX와 충돌할 수 있음).

## 개발

```sh
cargo test                                  # tsc/node가 있으면 타입체크·런타임 통합 테스트까지 수행
cargo fmt --check                           # 포매팅 검사
cargo clippy --all-targets -- -D warnings   # 린트
```

- `src/scanner.rs` — 바이트 단위 저수준 스캔
- `src/transform/` — 메인 변환 루프(`mod.rs`) + enum 파싱·방출(`enums.rs`) +
  match 파싱·방출(`matches.rs`) + 소진성 검사
- `src/verify.rs` — swc 기반 검증
- `docs/reference/` — 언어·CLI·에러 레퍼런스 (규범 문서)
- `docs/design/rust-rewrite.md` — Rust 재작성 설계 문서
- `docs/design/enum-and-error-layers.md` — enum 키워드 통합과 에러 계층 설계
- `CLAUDE.md` — 설계 계약·검증 게이트·태스크 관리 규칙
- `docs/tasks/INDEX.md` — 모든 작업의 태스크 인덱스 (단일 진실 소스)
- `CONTRIBUTING.md` — 기여 가이드

`examples/shapes.rl` → `examples/shapes.ts`가 전체 동작 예시입니다.
