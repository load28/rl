# rl

> Rust 스타일 `enum`과 `match`를 더한, TypeScript로 컴파일되는 초경량 전처리 언어

[![CI](https://github.com/load28/rl/actions/workflows/ci.yml/badge.svg)](https://github.com/load28/rl/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)
[![Rust 1.88+](https://img.shields.io/badge/rust-1.88%2B-orange.svg)](./Cargo.toml)

**rl**은 [Civet](https://civet.dev)처럼 TypeScript 위에 얹히는 언어입니다.
추가하는 것은 딱 네 가지 — 태그드 유니언 **`enum`** 선언, 패턴 매칭
**`match`** 표현식(or-패턴·가드 지원), 에러 전파 **`try`** 문, 값 추출
**`let-else`** 문. 나머지는 전부 그냥 TypeScript입니다.

```rl
// shapes.rl
export enum Shape {
  Circle(radius: number),
  Rect(width: number, height: number),
  Point,
}

export const area = (s: Shape): number =>
  match (s) {
    Circle(radius) => Math.PI * radius * radius,
    Rect(width, height) => width * height,
    Point => 0,
  };
```

```sh
$ rlc shapes.rl   # → shapes.ts
```

```ts
// shapes.ts — 타입 트릭 없는 순수 TypeScript
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

컴파일러 **rlc**는 Rust로 작성되었습니다.

## 특징

- **TypeScript 100% 호환** — 모든 유효한 TypeScript 파일은 그대로 유효한
  `.rl` 파일입니다. 컴파일러는 rl `enum`/`match` 구문만 변환하고 나머지는
  바이트 단위 그대로 통과시킵니다. TypeScript 자체의 `enum`, `.match(...)`
  메서드 호출, 문자열·주석·정규식 안의 키워드는 전혀 건드리지 않습니다.
- **태그드 유니언 `enum`** — 타입과 생성자 객체가 같은 이름으로 생성되어
  `Shape.Circle(1)`처럼 값을 만듭니다. 제네릭(`enum Option<T>`)도 지원합니다.
- **`match` 표현식** — 필드 바인딩·별칭·블록 본문·와일드카드 `_`를 지원하고,
  `kind` 필드를 가진 모든 태그드 유니언에 쓸 수 있습니다. `await`, 중첩
  match, 템플릿 리터럴 보간 내부 사용 모두 동작합니다.
- **컴파일 시점 소진성 검사** — 빠진 케이스는 tsc에 위임하지 않고 rlc가
  `파일:행:열`과 함께 직접 에러로 보고합니다. `Option`/`Result`는 내장
  enum이라 선언 없이도 검사됩니다.
- **`.rl` 간 import** — `import { E } from "./error.rl"`처럼 상대 경로로
  다른 `.rl` 파일을 그대로 가리키면, 방출 시 지정자가 `./error.js`로
  재작성되어 tsc/Node/번들러가 해석할 수 있습니다 (`--rewrite-imports`로
  형태 변경·비활성화). import한 enum도 소진성 검사를 받습니다 — rlc가
  참조된 파일의 enum 선언을 자동 수집합니다.
- **`Option`/`Result` 표준 라이브러리** — `import { Option } from "@rl/std"`
  하나면 Rust 스타일 `Option<T>`/`Result<T, E>`와 함수형 콤비네이터(`map`,
  `andThen`, `unwrapOr`, ...)를 쓸 수 있습니다. 컴파일하면 순수 TypeScript
  모듈이 출력 트리에 자동으로 실체화됩니다.
- **`try` 문으로 에러 전파** — Rust의 `?`처럼 `Err`를 즉시 리턴합니다:
  `const n = try parseNum(s);`. TypeScript의 `try/catch` 블록과 완벽히
  공존합니다 (블록 형태는 그대로 통과).
- **깨끗한 에러 계층** — rl 수준 에러(중복 케이스, 소진되지 않은 match,
  잘못된 필드 타입)는 전부 rlc의 책임. 방출되는 코드는 타입 트릭 없는 순수
  TypeScript라서 rlc가 만든 코드가 tsc 에러를 일으키지 않습니다.
- **자가 검증** — [swc](https://swc.rs) 파서로 필드 타입 표기와 최종 출력을
  검증합니다 (`--no-verify`로 생략 가능).

## 설치

```sh
git clone https://github.com/load28/rl
cd rl
cargo install --path .
```

또는 빌드만 하려면 `cargo build --release` 후 `target/release/rlc`를 사용합니다.

## 빠른 시작

```sh
rlc -o build src/        # 소스 트리(.rl + 손으로 쓴 .ts)를 완결된 TS 트리로
rlc --types src/         # 에디터·tsc용 타입 사이드카 생성 (.rl-types/)
rlc --check src/         # 컴파일만 하고 쓰지 않음 (문법·소진성 검사)
rlc -w -o build src/     # 감시 모드
rlc file.rl              # 단일 파일: file.ts 생성
```

단독(tsc)이든 번들러 플러그인([`integrations/unplugin`](./integrations/unplugin))
이든 소스는 같은 모양입니다 — 손으로 쓴 `.ts`도 `"./x.rl"`을 그대로
import하고, 타입은 어느 쪽이든 `rlc --types`가 만듭니다:

```jsonc
// package.json — 단독 (tsc)
{ "scripts": { "build": "rlc -o build src && tsc", "types": "rlc --types src" } }
// package.json — 번들러 (vite)
{ "scripts": { "build": "vite build", "types": "rlc --types src" } }
```

전체 동작 예시는 [`examples/shapes.rl`](./examples/shapes.rl) →
[`examples/shapes.ts`](./examples/shapes.ts)를 참고하세요.

### Rust 라이브러리로 사용

```rust
use rlc::{compile, Options};

let code = compile(rl_source, &Options { filename: Some("shapes.rl"), verify: true })?;
```

API 문서는 `cargo doc --open` (`rlc::compile` / `Options` / `CompileError`).

## 언어 한눈에 보기

### `enum` — Rust식 태그드 유니언

```rl
enum Option<T> {
  Some(value: T),
  None,
}

const x = Option.Some(7);          // Option<number>
const y: Option<string> = Option.None;
```

TypeScript 자체의 `enum`도 그대로 동작합니다. 구분 규칙은 단순합니다:

- 케이스에 페이로드 `(...)`가 하나라도 있거나 선언에 제네릭이 있으면 → **rl enum**
- 그 외 (유닛 멤버만 있거나 `= 값` 초기화가 있으면) → **순수 TS enum으로 통과**

```rl
enum Color { Red, Green, Blue }        // TS enum — 그대로 통과
enum Level { Info = "INFO" }           // TS enum — 그대로 통과
enum Shape { Circle(r: number), Dot }  // rl enum — 태그드 유니언으로 변환
enum Status { Active(), Inactive }     // 유닛만 있어도 ()를 붙이면 rl enum
```

TS enum 멤버는 `Tag(...)` 형태나 제네릭을 가질 수 없으므로, 이 규칙이 유효한
TypeScript를 잘못 변환하는 일은 없습니다. `const enum` / `declare enum`은 항상
TS의 것으로 취급됩니다.

### `match` — 패턴 매칭 표현식

```rl
match (expr) {
  Tag => expr,                 // 유닛 케이스
  Tag(field) => expr,          // 필드 바인딩 — 선언된 필드명 기준
  Tag(field: alias) => expr,   // 이름 바꿔서 바인딩
  Tag(a, b) => {               // 블록 본문 — 값을 내려면 return
    const s = a + b;
    return s * 2;
  },
  _ => expr,                   // 와일드카드 — 반드시 마지막 암
}
```

`match`는 **표현식**이며 `kind` 필드를 판별하는 `switch` IIFE로 컴파일됩니다.
`_` 없는 match는 같은 파일의 rl enum 선언과 대조해 소진성을 검사합니다:

```
$ rlc shapes.rl
rlc: shapes.rl:12:25: match on enum Shape is not exhaustive: missing "Rect"
     (add the missing arms or a final `_` arm)
```

### `Option` / `Result` — Rust 스타일 함수형 프로그래밍

`@rl/std`를 import하면 `Option`/`Result`와 콤비네이터를 바로 쓸 수
있습니다 — 컴파일할 때 모듈이 출력 트리에 자동으로 실체화됩니다. 두 타입은
**내장 enum**이라 match 소진성 검사도 선언 없이 동작합니다:

```rl
import { Option, Result } from "@rl/std";

function parseNum(raw: string): Result<number, string> {
  const n = Number(raw);
  return Number.isNaN(n) ? Result.Err("not a number") : Result.Ok(n);
}

const label = match (parseNum(input)) {
  Ok(value) => `n=${value}`,
  Err(error) => `error: ${error}`,   // Err 암을 빼면 rlc 컴파일 에러
};

const port = Option.unwrapOr(Option.fromNullable(config.port), 8080);
```

전체 API는 [표준 라이브러리 레퍼런스](./docs/reference/std.md) 참조.

### `try` — Rust의 `?`처럼 에러 전파

`try 식;`은 `Result`가 `Err`면 그 값을 **둘러싼 함수에서 즉시 리턴**하고,
`Ok`면 값을 풉니다. IIFE 없이 문장으로 컴파일되어 `await`와도 그대로
동작합니다:

```rl
function loadConfig(path: string): Result<Config, string> {
  const raw = try readFile(path);      // Err면 여기서 바로 return
  const parsed = try parseJson(raw);
  try validate(parsed);                // 값이 필요 없으면 전파만
  return Result.Ok(parsed);
}
```

```ts
// 컴파일 결과 (한 줄씩)
const $rl_t0 = (readFile(path)); if ($rl_t0.kind !== "Ok") return $rl_t0; const raw = $rl_t0.value;
```

TypeScript 자체의 `try { ... } catch` 블록, `obj.try()` 같은 멤버 이름은
전부 그대로 통과합니다.

문법·판별 규칙·방출 코드의 정확한 정의는
[언어 레퍼런스](./docs/reference/language.md)를 참고하세요.

## 편집기 지원

[`editors/vscode/`](./editors/vscode/)에 VSCode 확장(LSP 언어 서버)이
있습니다: 문법 하이라이팅, `rlc --check` 기반 진단, 케이스 태그·생성자
자동완성, 호버, 정의로 이동, 문서 심볼, 소진되지 않은 match 빠른 수정.
설치·개발 방법은 [확장 README](./editors/vscode/README.md) 참조.

## 문서

| 문서 | 내용 |
|------|------|
| [언어 레퍼런스](./docs/reference/language.md) | 문법, rl enum/TS enum 판별 규칙, 방출 코드, 소진성 검사, 제한사항 |
| [표준 라이브러리 레퍼런스](./docs/reference/std.md) | `Option`/`Result` 모듈 API, 값의 형태 계약 |
| [CLI 레퍼런스](./docs/reference/cli.md) | 옵션, 입출력 경로 규칙, 종료 코드 |
| [에러 레퍼런스](./docs/reference/errors.md) | 모든 진단 메시지의 형식·원인·해결 |
| [설계 문서](./docs/design/) | 아키텍처와 설계 결정 기록 |
| [CHANGELOG](./CHANGELOG.md) | 릴리스별 변경 내역 |

README는 소개용이며, 정확한 동작은 레퍼런스가 규정합니다.

## 제한사항

- 소스맵은 아직 생성하지 않습니다.
- 패턴은 케이스 태그와 `_`만 지원합니다 (리터럴/중첩/`|` 패턴 없음 —
  의도적으로 최소 기능).
- 소진성 검사는 같은 파일에 선언된 rl enum, **직접 import한 `.rl` 파일의
  exported enum**, 내장 `Option`/`Result`에 대해 동작합니다. 손으로 쓴
  유니언과 re-export 체인 너머의 enum은 검사 없이 컴파일되고 런타임 가드만
  남습니다.
- 표현식 암에서 객체 리터럴을 바로 반환하려면 화살표 함수처럼 괄호가
  필요합니다: `Tag => ({ a: 1 })`.
- `try` 문은 세미콜론이 필수이고 식이 `(`/`<`로 시작할 수 없으며, match
  내부·템플릿 보간·모듈 최상위에서는 쓸 수 없습니다.
- 스크루티니에 괄호가 필수입니다: `match (x) { ... }`.
- `.tsx`는 미지원입니다 (제네릭 화살표 함수 출력이 JSX와 충돌할 수 있음).

전체 목록은 [언어 레퍼런스의 제한사항 절](./docs/reference/language.md)을
참고하세요.

## 기여하기

```sh
cargo test                                  # tsc/node가 있으면 통합 테스트까지 수행
cargo fmt --check                           # 포매팅 검사
cargo clippy --all-targets -- -D warnings   # 린트
```

개발 환경, 작업 절차(태스크 문서 규칙), 설계 계약은
[`CONTRIBUTING.md`](./CONTRIBUTING.md)와 [`CLAUDE.md`](./CLAUDE.md)를
참고하세요. 모든 작업은 [`docs/tasks/INDEX.md`](./docs/tasks/INDEX.md)에서
태스크로 관리됩니다.

## 라이선스

[MIT](./LICENSE)
