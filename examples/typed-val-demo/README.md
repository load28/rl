# typed-val-demo

Small rl project that uses `enum`, `match`, `result`, `Result`, pipeline
syntax, and `val`.

The project is intentionally local to this repository. Build the compiler
first from the repo root:

```sh
cargo build
```

Then run the example:

```sh
cd examples/typed-val-demo
npm install
npm run build
npm start
```

Useful checks:

```sh
npm run check:rl       # rl-level checks only
npm run types:rl       # writes .rl-types for editors / tsc
npm run check:ts       # TypeScript check over generated TS
```

To see the branch's typed `val` diagnostic, copy the example diagnostic file
to a real `.rl` file and run `--types`:

```sh
cp src/diagnostics.rl.example src/diagnostics.rl
npm run types:rl
```

Expected diagnostic:

```text
cannot call mutating method `set` of built-in `Map` through val binding `byId`
```

The `Notebook#set` call in that file is deliberately allowed: it is a
user-defined method with the same name, not a method declared by TypeScript's
default library.
