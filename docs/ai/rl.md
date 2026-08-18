# rl — Guide for AI Assistants

This project uses **rl**, an ultra-light language that compiles to TypeScript.
This document is everything an AI assistant needs to read and write `.rl` files
correctly. It is self-contained; the normative references live in the rl
repository under `docs/reference/`.

## What rl is

rl adds exactly **six constructs** to TypeScript:

1. Rust-style `enum` (tagged unions)
2. `match` expressions (with or-patterns, guards, tuple match, nested patterns)
3. `try` statements (Rust's `?` — error propagation for `Result`)
4. `let-else` statements (extract or diverge)
5. `if let` statements (conditional extraction)
6. the pipeline operator `|>`

Two contracts govern everything:

- **Every valid TypeScript file is a valid `.rl` file.** Write normal
  TypeScript everywhere; use rl constructs only where they help. The compiler
  (`rlc`) transforms only text that parses *completely* as an rl construct and
  passes everything else through byte-for-byte.
- **The emitted code is plain TypeScript** — `kind`-tagged object unions,
  `switch`/`if` chains, no runtime library, no type tricks. rl-level errors
  (non-exhaustive match, duplicate cases) are reported by `rlc` with
  `file:line:col`; type errors in your ordinary TS code are still tsc's job.

**Critical consequence of pass-through:** if you slightly mis-write an rl
construct (missing semicolon, reserved word as a tag, unparenthesized ternary),
it is usually **not an rl error** — the text passes through unchanged and tsc
later fails on the raw `match (...)` / `try ...` in the output. If generated
output contains rl syntax verbatim, your rl syntax didn't fully parse. The two
exceptions are `if let` and `|>`: they cannot occur in valid TS, so a parse
failure there is reported by `rlc` with a location instead of passing through.

Identifiers inside rl constructs (tags, fields, bindings) must be ASCII
(`[A-Za-z_$][A-Za-z0-9_$]*`), and TS reserved words (`new`, `default`, `if`,
`in`, `of`, `static`, ...) cannot be used as tags/fields/bindings — a construct
containing one silently passes through. `.tsx` files are not supported.

## 1. `enum` — tagged unions

```rl
export enum Shape {
  Circle(radius: number),              // payload case → constructor function
  Rect(width: number, height: number),
  Point,                               // unit case → singleton value
}
enum Status { Active(), Inactive }     // Active() = zero-arg constructor
enum Tree<T> { Leaf(value: T), Node(left: Tree<T>, right: Tree<T>) }
```

Compiles to a type alias **and** a constructor object of the same name:

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

Usage: `Shape.Circle(1)`, `Shape.Point` (no parens — it's a value),
`Status.Active()` (parens — empty-paren case is a function). The discriminant
field is always `kind`. Plain objects like `{ kind: "Circle", radius: 1 }` are
assignable to the type; `match` works on **any** union discriminated by a
`kind` string field, not just rl-made values.

**An `enum` is an rl enum only if** at least one case has payload parens
(including empty `()`) **or** the declaration has generics. Everything else —
`enum Color { Red, Green }`, `const enum`, `declare enum` — is a TypeScript
enum and passes through untouched.

## 2. `match` — expression, exhaustive

```rl
const area = match (shape) {                 // scrutinee parens are mandatory
  Circle(radius) => Math.PI * radius ** 2,   // bind by FIELD NAME
  Rect(width: w, height) => w * height,      // rename with `field: alias`
  Point => 0,
};
```

Rules that matter most:

- `match` is an **expression** (compiles to an IIFE); use it on the right of
  `=`, in `return`, inside template `${...}`, etc.
- **Bindings are by field name, never by position.** `Rect(width, height)`
  matches declared field names; bind a subset in any order; rename via
  `field: alias`.
- Arm bodies: expression (`Tag => expr`) or block (`Tag => { ... return v; }`).
  An object-literal body needs parens: `Tag => ({ a: 1 })`.
- `_` (wildcard) arm must be **last**.
- **No literal patterns.** `match (n) { 1 => ..., "a" => ... }` is not rl —
  patterns are tags and `_` only. Use a plain `switch`/`if` for literals.
- or-patterns: `KeyEsc | KeyTab => close()`. All alternatives must bind the
  same (field, name) set. Separator is `|`, never `||`.
- Guards: `Some(v) if v > 0 => v`. Tag matches but guard false → falls through
  to the next arm. Guarded arms may repeat a tag.
- Nested patterns: `Ok(value: Some(v)) => v`. Inner unit cases need parens
  (`field: None()`); without parens `field: name` is an alias. Nested patterns
  cannot be combined with or-patterns; inner mismatch falls through.

**Exhaustiveness:** a match without `_` is checked against the enum's cases —
missing cases are a **compile error**. The enum is resolved as: local
declaration > enum imported via a *direct* relative `.rl` import > built-in
`Option`/`Result`. Two traps:

- **Guarded arms and nested-pattern arms never count as covering a case.**
  `Some(v) if v > 0 => v` alone leaves both `Some` and `None` uncovered — add
  an unguarded arm or `_`.
- A match with `_` is not checked at all; a match on an unknown union compiles
  with only a runtime `default` guard (throws on unexpected `kind`).

**Tuple match** — multiple scrutinees, product exhaustiveness:

```rl
const speed = match (conn, mode) {
  (Online(latency), Auto) if latency < 50 => 10,
  (Online, Auto)          => 5,
  (Online, Manual(level)) => level,
  (Offline, _)            => 0,          // `_` covers that position
};
```

Every arm must be a tuple pattern (or a final bare `_` covering everything),
element count must equal scrutinee count, and missing **combinations** are
compile errors. Or between whole tuples (`(A, B) | (C, D)`) is not supported —
use element-level or: `(A, B | D)`.

`await` is allowed in scrutinee, guards, and bodies (the IIFE becomes async and
is awaited). Detection is token-level, so an `await` inside a nested callback
also triggers async emission — avoid such matches in non-async contexts.

## 3. `try` — error propagation (Rust's `?`)

```rl
import { Result } from "@rl/std";

function readPort(cfg: string): Result<number, string> {
  const parsed = try parseNum(cfg);   // Err → return it from readPort now
  try validateRange(parsed);          // propagate-only form
  const data = try await fetchIt();   // await works
  return Result.Ok(parsed);
}
```

- **Statement position in a function body only**, and the **semicolon is
  mandatory** (without it, the line passes through and tsc fails).
- Works on `Result` only (`Ok` unwraps to `.value`, `Err` returns from the
  enclosing function). `Option` is not supported — convert with
  `Option.okOr(o, err)` first.
- The enclosing function's return type must be a `Result` compatible with the
  expression's `Err` type; there is no automatic error conversion.
- **Not allowed**: inside a match (scrutinee or arm body), inside template
  interpolation, inside another `try` expression, or at module top level —
  extract a helper function instead. These are compile errors.
- The expression must not start with `(` or `<`: write `try f(x);`, not
  `try (f(x));`.

## 4. `let-else` — extract or diverge

```rl
function greet(id: number): string {
  const Some(value: user) = findUser(id) else { return "who?"; };
  return `hello, ${user}`;
}
```

- Pattern parens **and** trailing semicolon are mandatory (else: passthrough).
- The `else` block must **diverge**, checked syntactically: its last top-level
  statement must start with `return`, `throw`, `break`, or `continue`.
  `if (c) return a; else return b;` at the end is rejected — restructure.
- One tag pattern only — no or-patterns, guards, or nested patterns, and no
  `= try expr else` combination. Position restrictions are the same as `try`.

## 5. `if let` — conditional extraction

```rl
if let Some(value: user) = findUser(id) {
  greet(user);
} else if let Some(value: cached) = cache.get(id) {
  greet(cached);
} else {
  prompt();
}
```

- Statement position only (any statement position, including match block-arm
  bodies), never in expression position.
- Pattern parens mandatory; nested patterns allowed
  (`if let Ok(value: Some(value: v)) = r { ... }`); no or-patterns or guards.
- `else` takes a block or another `if let` only. A plain `else if (cond)`
  cannot be chained directly — put it inside the else block.
- A malformed `if let` is a **located compile error**, not passthrough.

## 6. Pipeline `|>`

```rl
import { Option } from "@rl/std";

const label = half(4)
  |> Option.mapP(x => x + 1)     // apply step: f(x) — data-last `*P` combinators
  |> Option.unwrapOrP(0)
  |> .toFixed(1);                // method step: (value).toFixed(1)
```

- `x |> f` means `f(x)`; a step starting with `.` is a postfix chain on the
  piped value (`x |> .trim().split(",")`).
- Multi-arg functions: use the std `*P` curried variants or a parenthesized
  arrow: `x |> (n => add(n, 2))`.
- **Parenthesize ternaries and arrows** at head/step top level:
  `(c ? a : b) |> f`, `x |> (n => n + 1)` — violations are compile errors.
- No `?.`-starting steps; no empty steps; no `try` statements inside
  head/steps (but a pipeline inside a try expression is fine:
  `const a = try readCfg() |> normalize;`).
- A malformed `|>` is a **located compile error**, not passthrough. When the
  head boundary is ambiguous (no-semicolon style, `in`/`instanceof`),
  parenthesize the head.

## Standard library `@rl/std`

```rl
import { Option, Result } from "@rl/std";
```

`Option<T>` = `Some(value: T) | None`, `Result<T, E>` = `Ok(value: T) |
Err(error: E)`. Payload field names: **`value`** for `Some`/`Ok`, **`error`**
for `Err` — so match arms are `Some(value)`, `Ok(value)`, `Err(error)`, or
aliased: `Some(value: v)`.

Both are also **built-in enums**: a `_`-less match on `Some`/`None` or
`Ok`/`Err` tags is exhaustiveness-checked even without the import. The
built-ins provide checking only, not values — always import (or declare) to
construct.

Combinators are data-first static functions (no method chaining), with
data-last curried `*P` variants for pipelines:

- `Option`: `map andThen orElse filter unwrapOr unwrapOrElse expect okOr
  fromNullable toNullable isSome isNone zip flatten transpose collect`
  (+ `mapP andThenP orElseP filterP unwrapOrP unwrapOrElseP expectP okOrP`)
- `Result`: `map mapErr andThen orElse unwrapOr unwrapOrElse expect ok err
  fromThrowable fromPromise isOk isErr flatten transpose collect`
  (+ `mapP mapErrP andThenP orElseP unwrapOrP unwrapOrElseP expectP`)

Bridges to idiomatic use: `Option.fromNullable(x)` wraps `T | null |
undefined`; `Result.fromThrowable(() => JSON.parse(s))` captures throws;
`Result.fromPromise(p)` captures rejections; `Result.collect(list)` /
`Option.collect(list)` turn arrays inside-out.

## Modules

- Import other `.rl` files by relative path **with the `.rl` extension**:
  `import { Token } from "./token.rl";`. On emit the specifier is rewritten to
  `./token.js` (default; works with `nodenext` and `bundler` resolution).
  `--rewrite-imports ts|off` changes the form (`ts` needs
  `allowImportingTsExtensions` + `rewriteRelativeImportExtensions`).
- Exhaustiveness sees exported enums from **direct (1-hop)** relative `.rl`
  imports (named, aliased, or `* as ns`). Re-export chains and package-path
  enums are not collected — matches on those compile unchecked.
- Dynamic `import()` specifiers are not rewritten.

## Building a project

```sh
npm install --save-dev rl-lang     # prebuilt rlc binary, run as `npx rlc`
```

```jsonc
// package.json — standalone (tsc) setup
{ "scripts": {
    "build": "rlc -o build src && tsc",
    "types": "rlc --types src",              // editor/tsc declarations
    "check": "rlc --check src && tsc --noEmit" } }
```

```jsonc
// tsconfig.json — lets tsc/editors resolve "./x.rl" and "@rl/std"
{ "compilerOptions": {
    "rootDirs": ["./src", "./.rl-types"],
    "paths": { "@rl/std": ["./.rl-types/rl.d.ts"] } } }
```

- `rlc <dir>` compiles `.rl` → `.ts` and passes hand-written `.ts` through;
  use `-o <dir>` for a separate output tree (in-place overwrite of inputs is
  refused). If any input imports `@rl/std`, the std module is materialized
  into the output tree automatically.
- `rlc --types src` generates the `.rl-types/` declaration sidecar (add it to
  `.gitignore`; needs `typescript` 5/6 in the project — TS 7 has no JS API).
  Re-run (or keep `rlc --types -w src` running) after changing `.rl` files.
- `rlc -w` watches; it also recompiles importers of a changed file so
  cross-file exhaustiveness stays correct.
- With a bundler, use `unplugin-rl` (`import rl from "unplugin-rl/vite"`) so
  the bundler reads `.rl` directly; types still come from `rlc --types`.
- Generated `.ts` files start with an `// @generated` banner — never edit
  compiler output; edit the `.rl` source.

## Reading errors

- `rlc: file.rl:LINE:COL: message` — an rl-level error at a 1-based source
  position. Notable: `match on enum X is not exhaustive: missing "Y"` (add
  arms or `_`), `duplicate arm`, `or-pattern alternatives must bind the same
  fields`, `the else block must end with a return/throw/break/continue`,
  `` `try` cannot be used inside a match expression ... `` (extract a helper).
- tsc errors pointing at generated output that still contains literal
  `match`/`try` text → your rl construct silently passed through; re-check
  syntax (semicolons, parens, reserved words).
- `generated TypeScript failed to parse` → the pass-through source was itself
  invalid TS, or an rlc bug.

## Checklist before you finish

1. Scrutinee parens: `match (x) { ... }`; `_` last; object arms in parens.
2. Bindings use declared **field names** (or `field: alias`), never positions.
3. No literal patterns in match — tags and `_` only.
4. Every `_`-less match covers all cases; guards/nested arms don't count.
5. `try ...;` and `const ... = ... else { ... };` end with semicolons; else
   blocks end with a diverging statement.
6. No `try`/`let-else` inside match bodies, template interpolations, or at
   module top level.
7. Pipelines: parenthesize ternaries/arrows; use `*P` combinators.
8. Relative imports between `.rl` files keep the `.rl` extension; `@rl/std`
   for `Option`/`Result` (fields `value`/`error`).
9. Run `npx rlc --check src` (rl-level) and `tsc --noEmit` (type-level);
   regenerate types with `npx rlc --types src` after enum changes.
10. Never hand-edit generated `.ts`; `.rl-types/` and the output tree are
    build artifacts.
