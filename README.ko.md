# rl

[English](./README.md) · [한국어](./README.ko.md)

rl은 TypeScript에 표현력 높은 데이터·제어 흐름 기능을 더하고, 다시 순수 TypeScript로 컴파일하는 작은 언어입니다.

```rl
export enum Shape {
  Circle(radius: number),
  Rectangle(width: number, height: number),
  Point,
}

export const area = (shape: Shape): number =>
  match (shape) {
    Circle(radius) => Math.PI * radius ** 2,
    Rectangle(width, height) => width * height,
    Point => 0,
  };
```

모든 유효한 TypeScript 파일은 그대로 유효한 `.rl` 파일입니다. rl은 자신이 소유한 구문만 변환하고, 소진되지 않은 match 같은 언어 오류를 직접 보고하며, 런타임 의존성 없는 읽기 쉬운 TypeScript를 방출합니다.

## 설치와 사용

npm에서 프리빌트 컴파일러를 설치합니다. 지원 플랫폼에서는 Rust가 필요하지 않습니다.

```sh
npm install --save-dev rl-lang typescript
```

파일이나 소스 트리를 컴파일하거나, 출력 없이 검사합니다.

```sh
npx rlc -o build src
npx rlc --check src
npx rlc --check-types src
```

`rlc`의 출력은 TypeScript입니다. 기존 TypeScript 컴파일러나 번들러로 JavaScript를 만드세요. `.rl` 파일을 번들러에서 직접 import하려면 Vite, Rollup, webpack, Rspack, esbuild, Farm을 지원하는 [`unplugin-rl`](./integrations/unplugin)을 사용하세요.

프리빌트 바이너리는 Linux x64/arm64, macOS x64/arm64, Windows x64를 지원합니다. 다른 플랫폼에서는 소스에서 빌드하세요.

```sh
cargo install --git https://github.com/load28/rl
```

컴파일러 옵션은 `rlc --help`, 내장 언어 가이드는 `rlc help <topic>`으로 확인할 수 있습니다.

## 언어 한눈에 보기

- Rust 스타일 `enum`으로 데이터를 모델링하고, 가드·튜플·리터럴·or-패턴·중첩 패턴을 지원하는 소진적 `match`로 값을 추출합니다.
- `@rl/std`의 `Option`과 `Result`를 `try`, `let-else`, `if let`, `result` 블록으로 다룹니다.
- `|>`와 `flow`로 값 파이프라인과 함수 파이프라인을 만듭니다.
- 변경을 허용하지 않을 바인딩과 매개변수에는 `val`을 붙입니다.

나머지는 모두 평범한 TypeScript입니다. 기존 TypeScript의 타입, 모듈, 도구, 런타임 동작을 그대로 기반으로 사용합니다.

## rl 개발하기

컴파일러는 작은 공개 API와 `rlc` CLI를 제공하는 Rust 크레이트입니다. Rust 1.88 이상이 필요합니다. 전체 통합 테스트에는 Node.js와 TypeScript도 필요합니다.

```sh
git clone https://github.com/load28/rl.git
cd rl
cargo build
cargo test
```

변경을 제출하기 전에 저장소 검증 게이트를 실행합니다.

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

로컬 컴파일러, TypeScript, VS Code 확장을 함께 설정하려면 `./scripts/setup --tsgo-npm`을 실행하세요. 기여 절차와 프로젝트 규칙은 [CONTRIBUTING.md](./CONTRIBUTING.md)에 있습니다.

컴파일러는 Rust 라이브러리로도 포함할 수 있습니다.

```rust
use rlc::{compile, Options};

let typescript = compile(source, &Options::default())?;
```

아키텍처 기록은 [`docs/design`](./docs/design)에 있습니다.

## 라이선스

[MIT](./LICENSE)
