# rl-lang

**rl** is a tiny preprocessor language that compiles to TypeScript. It adds
exactly six things on top of 100%-compatible TypeScript: Rust-style `enum`
(tagged unions), `match` expressions (with or-patterns, guards, tuple match
and nested patterns), error-propagating `try`, value-extracting `let-else`
and `if let`, and the pipeline operator `|>`.

This package installs `rlc`, the rl compiler, as a prebuilt native binary —
no Rust toolchain required.

```sh
npm install --save-dev rl-lang
```

```sh
npx rlc -o build src/     # compile a source tree to TypeScript
npx rlc --check src/      # check without writing anything
npx rlc --types src/      # editor/typecheck declarations
```

Using a bundler? [`unplugin-rl`](https://github.com/load28/rl/tree/main/integrations/unplugin)
reads `.rl` files directly in Vite, Rollup, webpack, Rspack, esbuild and
Farm, and finds this package's binary automatically.

## Supported platforms

Prebuilt binaries ship as optional dependencies; npm installs the one
matching your machine.

| Package | OS | CPU |
|---------|----|----|
| `rl-lang-linux-x64` | Linux | x64 (static musl build) |
| `rl-lang-linux-arm64` | Linux | arm64 (static musl build) |
| `rl-lang-darwin-x64` | macOS | x64 |
| `rl-lang-darwin-arm64` | macOS | arm64 |
| `rl-lang-win32-x64` | Windows | x64 |

On other platforms, build from source
(`cargo install --git https://github.com/load28/rl`) and set the
`RLC_BINARY` environment variable to the resulting binary.

## API

The package exports one helper for tools that want to spawn the compiler
directly:

```js
const { binaryPath } = require("rl-lang");
binaryPath(); // absolute path to the rlc binary for this platform
```

## Local development install

In a checkout of the [rl repository](https://github.com/load28/rl),
`./scripts/setup` stamps this directory for local installs. A project can
then use the work-in-progress compiler like any other dependency:

```sh
pnpm add -D file:/path/to/rl/npm/rl-lang
```

The launcher runs the repository's `target/release/rlc` and, when setup was
pointed at a typescript-go checkout, hands that toolchain to the spawned
compiler process only — your shell environment is never modified. Published
installs are unaffected (the stamp file is not committed or published).

## Documentation

Language reference, CLI options, standard library and error index:
<https://github.com/load28/rl#readme>.

## License

MIT
