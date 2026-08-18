# rl — AI context

rl = TypeScript + 6 constructs; `rlc` compiles `.rl` → plain TS. Write normal TS everywhere; rl syntax only for: enum (tagged union), match, try, let-else, if let, `|>`.

CONTRACTS:
- Every valid TS file is a valid `.rl` file. rlc transforms only text parsing COMPLETELY as an rl construct; all else passes through byte-for-byte.
- Output is plain TS (`kind`-tagged unions, switch/if chains), no runtime lib, no type tricks. rl-level errors: `rlc: file:line:col: msg`. Type errors in pass-through code: tsc's job.
- TRAP: a slightly-wrong rl construct (missing `;`, reserved-word tag, unparenthesized ternary) is NOT an rl error — it passes through, then tsc fails on raw `match`/`try` text in output. If output contains rl syntax verbatim → your syntax didn't fully parse. Exceptions: malformed `if let` and `|>` DO error with location (impossible in valid TS).
- Identifiers inside rl constructs: ASCII `[A-Za-z_$][A-Za-z0-9_$]*` only. TS reserved words (new, default, if, in, of, static, class, ...) can't be tags/fields/bindings — construct silently passes through. `.tsx` unsupported.

## enum

```rl
export enum Shape { Circle(radius: number), Rect(width: number, height: number), Point }
enum Status { Active(), Inactive }   // Active() = zero-arg ctor fn
enum Tree<T> { Leaf(value: T), Node(left: Tree<T>, right: Tree<T>) }
```
→ emits type alias `Shape` = union of `{ kind: "Tag"; ...fields }` + constructor object `Shape` (both exported if `export`).
- Use: `Shape.Circle(1)`; unit case is a VALUE not fn: `Shape.Point`; empty-paren case is fn: `Status.Active()`.
- Discriminant always `kind`. Plain `{ kind: "Circle", radius: 1 }` is assignable; match works on ANY `kind`-string-discriminated union.
- rl enum iff ≥1 case has payload parens (incl. empty `()`) OR declaration has generics. Otherwise (`enum Color { Red }`, `const enum`, `declare enum`) = TS enum, passthrough.
- Duplicate case tag = error.

## match

```rl
const area = match (shape) {
  Circle(radius) => Math.PI * radius ** 2,
  Rect(width: w, height) => w * height,   // bind by FIELD NAME; alias via `field: alias`
  Point => 0,
};
```
- Expression (compiles to IIFE): use after `=`, in `return`, in `${...}`. Scrutinee parens mandatory, non-empty.
- Bindings by field name, NEVER position; subset ok, any order.
- Arm body: expr, or block `{ ... return v; }` (no return → undefined). Object literal body needs parens: `Tag => ({a: 1})`.
- `_` arm must be LAST.
- NO literal patterns (`1 =>`, `"a" =>` invalid) — tags and `_` only; use switch/if for literals.
- or-pattern: `A | B => body` (never `||`); all alternatives must bind same (field,name) set.
- guard: `Some(v) if v > 0 => v`; guard false → falls to next arm; guarded arms may repeat a tag; re-matching a tag already covered by an unguarded arm = duplicate-arm error.
- nested: `Ok(value: Some(v)) => v`; inner UNIT case needs parens `field: None()` (`field: name` = alias); no combining with or-patterns; same binding name twice in a pattern = error (alias one); inner mismatch falls through.
- Exhaustiveness: match without `_` is checked; missing case = compile error. Enum resolution: local decl > direct (1-hop) relative-`.rl`-import > built-in Option/Result. GUARDED and NESTED arms NEVER count as covering — add unguarded arm or `_`. With `_`: unchecked. Unknown union: compiles unchecked, runtime default throws on unexpected kind.
- await allowed in scrutinee/guards/bodies → async IIFE, awaited. Detection is token-level: await inside a nested callback also triggers async — avoid in non-async contexts.

Tuple match (product exhaustiveness — missing COMBINATIONS are errors):
```rl
match (conn, mode) { (Online(latency), Auto) if latency < 50 => 10, (Online, _) => 5, (Offline, _) => 0 }
```
Every arm = tuple pattern (or final bare `_` covering all); element count = scrutinee count; no `(A,B)|(C,D)` — use element-level or `(A, B|D)`; parenthesize scrutinees containing top-level `<`/`>` comparisons.

## try (Rust `?`)

```rl
const parsed = try parseNum(cfg);   // in fn returning Result: Err → returned from fn now
try validateRange(parsed);          // propagate-only; `try await f();` ok
```
- Statement position in a function body ONLY; trailing `;` MANDATORY (else passthrough).
- Result only (Ok unwraps `.value`; Err returned from enclosing fn). Option unsupported → `Option.okOr(o, err)` first.
- Enclosing fn return type must be Result compatible with expr's Err type; no auto conversion.
- FORBIDDEN (compile error): inside match (scrutinee/arm), template interpolation, another try, module top level → extract helper fn.
- Expr can't start with `(` or `<`: `try f(x);` not `try (f(x));`.

## let-else

```rl
const Some(value: user) = findUser(id) else { return "who?"; };
```
- Pattern parens AND trailing `;` mandatory (else passthrough).
- else block must diverge SYNTACTICALLY: last top-level stmt starts with return/throw/break/continue (`if (c) return a; else return b;` ending rejected — restructure).
- Single tag pattern only: no or/guard/nested; no `= try expr else`. Position limits same as try.

## if let

```rl
if let Some(value: user) = findUser(id) { greet(user); }
else if let Some(value: c) = cache.get(id) { greet(c); }
else { prompt(); }
```
- Statement position only (incl. match block-arm bodies); never expression position.
- Pattern parens mandatory; nested ok (`if let Ok(value: Some(value: v)) = r {}`); no or/guards.
- else = block or another if-let ONLY; plain `else if (cond)` must go inside an else block.
- Malformed if let = located compile error (not passthrough).

## |>

```rl
const label = half(4) |> Option.mapP(x => x + 1) |> Option.unwrapOrP(0) |> .toFixed(1);
```
- `x |> f` = `f(x)`; step starting `.` = postfix chain on piped value (`x |> .trim().split(",")`).
- Multi-arg: std `*P` curried variants or parenthesized arrow `x |> (n => add(n, 2))`.
- PARENTHESIZE ternaries & arrows at head/step top level: `(c ? a : b) |> f`, `x |> (n => n+1)` — else compile error.
- No `?.`-starting step; no empty step; no try STATEMENT inside head/step (pipeline inside a try expr is fine: `const a = try readCfg() |> normalize;`).
- Malformed `|>` = located compile error. Ambiguous head (no-semicolon style, `in`/`instanceof`) → parenthesize head.

## @rl/std

```rl
import { Option, Result } from "@rl/std";
```
- `Option<T>` = `Some(value: T) | None`; `Result<T, E>` = `Ok(value: T) | Err(error: E)`. Field names: `value` (Some/Ok), `error` (Err) → arms `Some(value)`, `Ok(value)`, `Err(error)`, alias `Some(value: v)`.
- Both are BUILT-IN enums: `_`-less match on their tags is exhaustiveness-checked even without import. Built-ins give checking only — import (or declare) to construct values.
- Combinators = data-first static fns; `*P` = data-last curried for pipelines.
  - Option: map andThen orElse filter unwrapOr unwrapOrElse expect okOr fromNullable toNullable isSome isNone zip flatten transpose collect (+P: map andThen orElse filter unwrapOr unwrapOrElse expect okOr)
  - Result: map mapErr andThen orElse unwrapOr unwrapOrElse expect ok err fromThrowable fromPromise isOk isErr flatten transpose collect (+P: map mapErr andThen orElse unwrapOr unwrapOrElse expect)
- Bridges: `Option.fromNullable(x)` (T|null|undefined), `Result.fromThrowable(() => JSON.parse(s))`, `Result.fromPromise(p)`, `Result.collect(arr)` / `Option.collect(arr)`.

## Modules

- Import `.rl` files by relative path WITH extension: `import { Token } from "./token.rl";` → rewritten on emit to `./token.js` (default; `--rewrite-imports ts|off`; `ts` needs tsconfig `allowImportingTsExtensions` + `rewriteRelativeImportExtensions`).
- Exhaustiveness sees exported enums from DIRECT (1-hop) relative `.rl` imports (named/aliased/`* as ns`); re-export chains & package paths NOT collected → those matches compile unchecked.
- Dynamic `import()` specifiers not rewritten.

## Build

```sh
npm i -D rl-lang   # prebuilt rlc; run via npx
```
```jsonc
// package.json (tsc setup)
"build": "rlc -o build src && tsc", "types": "rlc --types src", "check": "rlc --check src && tsc --noEmit"
// tsconfig.json — resolve "./x.rl" and "@rl/std":
"rootDirs": ["./src", "./.rl-types"], "paths": { "@rl/std": ["./.rl-types/rl.d.ts"] }
```
- `rlc <dir>`: `.rl`→`.ts`, hand-written `.ts` passthrough; `-o <dir>` for separate tree (in-place overwrite refused). `@rl/std` auto-materialized into output when imported.
- `rlc --types src` → `.rl-types/` declaration sidecar (gitignore it; needs typescript 5/6 — TS7 has no JS API). Re-run after `.rl` changes, or keep `rlc --types -w src` running.
- `rlc -w` watches, also recompiles importers of changed files.
- Bundler: `unplugin-rl` (`import rl from "unplugin-rl/vite"`) reads `.rl` directly; types still via `rlc --types`.
- Emitted `.ts` starts with `// @generated` — NEVER edit output; edit `.rl` source.

## Errors

- `rlc: file:line:col: msg` — e.g. `match on enum X is not exhaustive: missing "Y"` (add arms or `_`), `duplicate arm`, `or-pattern alternatives must bind the same fields`, else-block-must-diverge, try-position-restriction (extract helper).
- tsc errors on output containing literal `match`/`try` → silent passthrough; recheck semicolons/parens/reserved words.
- `generated TypeScript failed to parse` → pass-through source was invalid TS, or rlc bug.

## Checklist

match parens + `_` last + object arms `({...})`; bind by field name not position; no literal patterns; `_`-less match covers all (guards/nested don't count); `try`/`let-else` need `;` and diverging else, never inside match/`${}`/top-level; pipelines parenthesize ternaries/arrows, use `*P`; relative imports keep `.rl`; verify with `npx rlc --check src` + `tsc --noEmit`, re-run `npx rlc --types src` after enum changes; never edit generated `.ts`.
