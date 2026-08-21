# rl

[Website](https://load28.github.io/rl/) · [English](./README.md) · [한국어](./README.ko.md)

rl is a small language that adds expressive data and control-flow features to TypeScript, then compiles back to plain TypeScript.

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

Every valid TypeScript file is also a valid `.rl` file. rl only transforms syntax it owns, reports language errors such as non-exhaustive matches itself, and emits readable TypeScript without a runtime dependency.

## Install and use

Install the prebuilt compiler from npm. Rust is not required on supported platforms.

```sh
npm install --save-dev rl-lang typescript
```

Compile a file or source tree, or check it without writing output:

```sh
npx rlc -o build src
npx rlc --check src
npx rlc --check-types src
```

`rlc` emits TypeScript. Continue with your existing TypeScript compiler or bundler to produce JavaScript. For direct `.rl` imports, use [`unplugin-rl`](./integrations/unplugin) with Vite, Rollup, webpack, Rspack, esbuild, or Farm.

Prebuilt binaries are available for Linux x64/arm64, macOS x64/arm64, and Windows x64. On another platform, build from source:

```sh
cargo install --git https://github.com/load28/rl
```

Run `rlc --help` for compiler options or `rlc help <topic>` for the built-in language guide.

## The language at a glance

- Model data with Rust-style `enum` declarations and unpack it with exhaustive `match` expressions, including guards, tuples, literals, or-patterns, and nested patterns.
- Work with `Option` and `Result` through `try`, `let-else`, `if let`, and `result` blocks. Import the standard types from `@rl/std`.
- Build value and function pipelines with `|>` and `flow`.
- Mark bindings and parameters with `val` when mutation through them must be rejected.

The rest of the file is ordinary TypeScript. Existing TypeScript types, modules, tooling, and runtime behavior remain the foundation.

## Develop rl

The compiler is a Rust crate with a small public API and an `rlc` CLI. Rust 1.88 or newer is required. Node.js and TypeScript are needed for the full integration suite.

```sh
git clone https://github.com/load28/rl.git
cd rl
cargo build
cargo test
```

Before submitting a change, run the repository gates:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

For local compiler, TypeScript, and VS Code extension setup, run `./scripts/setup --tsgo-npm`. Contribution workflow and project rules are in [CONTRIBUTING.md](./CONTRIBUTING.md).

The compiler can also be embedded as a Rust library:

```rust
use rlc::{compile, Options};

let typescript = compile(source, &Options::default())?;
```

Architecture records live in [`docs/design`](./docs/design).

## License

[MIT](./LICENSE)
