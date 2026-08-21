# rl — AI context

rl = TypeScript + 7 constructs + 1 binding modifier; `rlc` compiles `.rl` → plain TS. Write normal TS everywhere; rl syntax only for: enum (tagged union), match, try, let-else, if let, `|>` (+ `flow` composition), `result` block, `val` (mutation-free binding).

CONTRACTS:
- Every valid TS file is a valid `.rl` file. rlc transforms only text parsing COMPLETELY as an rl construct; all else passes through byte-for-byte.
- Output is plain TS (`kind`-tagged unions, switch/if chains), no runtime lib, no type tricks. rl-level errors: `rlc: file:line:col: msg` (the position is the construct's start; diagnostics also carry the end, which is what an editor underlines — `try <expr>`, `match (scrutinee)`). Type errors in pass-through code: tsc's job.
- Type errors that tsc raises *inside code rlc generated* (e.g. `try` on a non-Result, `match` on a plain TS enum) are restated by rlc in rl's words OVER THE CONSTRUCT (`try <expr>`, `match (scrutinee)`), with the original in parentheses: `` `try` needs a `Result` — this expression is not one (ts2339: ...) ``. Only a whitelist of (construct, TS code) pairs is restated; anything else passes through with `(in code rlc generated for this construct)`.
- TRAP: a slightly-wrong rl construct (missing `;`, reserved-word tag, unparenthesized ternary) is NOT an rl error — it passes through, then the output self-check (or tsc) fails on the raw `match`/`try` text. rlc reports that AT THE KEYWORD in the `.rl`: `` `match` here did not parse as an rl `match`, so it was passed through as TypeScript ... `` → your syntax didn't fully parse. Exceptions: malformed `if let` and `|>` DO error with location (impossible in valid TS).
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
- Literal patterns: string/number/boolean literals match the scrutinee VALUE (`===`), e.g. `match (dir) { "north" => "N", _ => "?" }`. NEVER mix tag and literal patterns in one match (compile error); `_` works in both. See "literal match" below.
- or-pattern: `A | B => body` (never `||`); all alternatives must bind same (field,name) set.
- guard: `Some(v) if v > 0 => v`; guard false → falls to next arm; guarded arms may repeat a tag; re-matching a tag already covered by an unguarded arm = duplicate-arm error. A dead arm the duplicate rule misses (nested pattern or tuple combination already covered) is NOT an error — it compiles, and the editor dims it (engine `rlHints`).
- nested: `Ok(value: Some(v)) => v`; inner UNIT case needs parens `field: None()` (`field: name` = alias); no combining with or-patterns; same binding name twice in a pattern = error (alias one); inner mismatch falls through.
- Name resolution: pattern tags and field names are checked against the declaration — but ONLY when rlc can name what you meant (case-insensitive or near-miss; a transposition counts as one edit), because tag patterns also match hand-written `kind` unions whose tags are in no table. `Circel(r)` → `enum Shape has no case \`Circel\` — did you mean \`Circle\`?`; `Circle(radiuz)` → `case \`Circle\` has no field \`radiuz\` — did you mean \`radius\`?`. Same rule in let-else / `if let` (single-tag sites need a ONE-edit match to report the tag; fields are checked once the tag resolves) and in nested patterns (resolved against the outer field's declared type). A wrong-but-not-typo name is NOT reported (needs types). A reported typo suppresses that match's exhaustiveness error.
- Exhaustiveness: match without `_` is checked; missing case = compile error. Enum resolution: local decl > direct (1-hop) relative-`.rl`-import > built-in Option/Result. GUARDED arms NEVER count as covering (add an unguarded arm or `_`); NESTED patterns DO — the check descends into payloads, so `Ok(value: Some(v))` + `Ok(value: None())` + `Err(e)` is exhaustive, and a hole is reported as a PATTERN you can paste back (`missing "Ok(value: None)"`). Inner position's enum comes from the field's declared type, else from the patterns written there (so generic `T` payloads still work); if neither names an enum, only `_` covers that position. With `_`: unchecked. Unknown union: compiles unchecked, runtime default throws on unexpected kind.
- await allowed in scrutinee/guards/bodies → async IIFE, awaited. Detection is token-level: await inside a nested callback also triggers async — avoid in non-async contexts.

Literal match (`switch ($rl_m)` instead of `$rl_m.kind`):
```rl
match (code) { 200 | 201 => "success", 404 => "not found", _ => "other" }
match (flag) { true => "yes", false => "no" }
```
- Literals: string (`"a"`/`'a'`), number (`404`, `-1`, `0xff`, `1_000`, `1.5e2`, `10n`), `true`/`false`. No bindings. or-pattern alternatives must all be the SAME kind (`"a" | 1` = error). Guards allowed, same rules as tags.
- Duplicates compared BY VALUE: `200` and `0xc8` are the same arm → duplicate-arm error. `1n` ≠ `1`.
- NOT allowed inside tuple patterns (v1) — tuple elements are tag patterns or `_`.
- Exhaustiveness: the DEFAULT compile path does NOT check it (rlc has no TS types) — `_`-less literal match just gets a runtime `throw` guard. `rlc --check-types`/`--types` DO check it via the TypeScript checker, but only when the scrutinee type is a finite literal union (`"a" | "b"`, `1 | 2`, `boolean`, `typeof arr[number]`); `string`/`number`/`unknown`/`any`/`T`/`"a" | string` are never diagnosed. Reported at the `.rl` `match` keyword.

Tuple match (product exhaustiveness — missing COMBINATIONS are errors):
```rl
match (conn, mode) { (Online(latency), Auto) if latency < 50 => 10, (Online, _) => 5, (Offline, _) => 0 }
```
Every arm = tuple pattern (or final bare `_` covering all); element count = scrutinee count; no `(A,B)|(C,D)` — use element-level or `(A, B|D)`; parenthesize scrutinees containing top-level `<`/`>` comparisons.
- Exhaustiveness is the product of the positions. `rlc --check-types` asks the checker for each position's alphabet, so narrowed types count, and reports combinations unquoted: `match is not exhaustive: missing (North, Slow)`. A position no arm tags stays `_`.

## try (Rust `?`)

```rl
const parsed = try parseNum(cfg);   // in fn returning Result: Err → returned from fn now
try validateRange(parsed);          // propagate-only; `try await f();` ok
```
- Statement position in a function body ONLY; trailing `;` MANDATORY (else passthrough).
- Result only (Ok unwraps `.value`; Err returned from enclosing fn). Option unsupported → `Option.okOr(o, err)` first.
- Enclosing fn return type must be Result compatible with expr's Err type; no auto conversion.
- UNANNOTATED fn: tsc infers the union of the return paths, so several `try`s with different Err types give `Ok<T> | Err<E1> | Err<E2>` = `Result<T, E1 | E2>`. rlc never collects/unions error types — leave inference to tsc.
- FORBIDDEN (compile error): inside match (scrutinee/arm), template interpolation, another try, module top level → extract helper fn.
- Expr can't start with `(` or `<`: `try f(x);` not `try (f(x));`.

## let-else

```rl
const Some(value: user) = findUser(id) else { return "who?"; };
```
- Pattern parens AND trailing `;` mandatory (else passthrough).
- else block must diverge SYNTACTICALLY: last top-level stmt starts with return/throw/break/continue (`if (c) return a; else return b;` ending rejected — restructure). Statement boundaries are top-level `;` and a block statement's `}`; an object literal's / arrow body's `}` is not one, so `else { return { kind: "Err", error: e }; };` counts as diverging.
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
- `flow` head = compose FUNCTIONS instead of piping a value: `const label = flow |> half |> Option.mapP(x => x + 1) |> .toFixed(1);` then `label(4)`. Same step rules; nothing runs until the composed fn is called.
- `flow` is contextual — only a head that is exactly `flow`; a `flow` VARIABLE pipes when parenthesized (`(flow) |> f`). `flow |> f` (one step) = `f`.
- flow's FIRST step fixes the input type and cannot be a method step (compile error). Generic/curried first step → `unknown`; give type args (`flow |> wrap<number> |> ...`, `flow |> Option.mapP((x: number) => x + 1) |> ...`). Later steps infer from the previous step.

## result block

```rl
const data = result {
  const user <- getUser(id);              // Ok → bind value; Err → whole block IS that Err
  const name = user.name |> .trim();      // ordinary TS/rl statements between bindings
  const company <- getCompany(user.companyId);
  { user, company, name }                 // LAST expr, NO `;` — wrapped in Ok
};
```
- Flat replacement for nested `Result.andThen(r, user => ... )` callbacks; every earlier binding stays in scope.
- `result` is contextual: block is claimed ONLY if it has ≥1 `<-` binding, else plain identifier + block statement (passthrough). Write `<-` with no space.
- Binding = `const|let|var <name|destructuring|: type> <- expr;` — `;` MANDATORY on bindings, FORBIDDEN on the final value expr (else located compile error). A top-level `>` in the bound expr needs parens (generic-type-argument ambiguity). Forgetting the keyword (`y <- g();`) is a located error (`` `result` binding is missing its declaration keyword ``) wherever the text cannot be TS — which is any claimed block; for an actual `y < -g()` comparison, put a space between `<` and `-`.
- Result only (no Option/Promise do-notation, no `<-` outside a result block).
- Block is an EXPRESSION (compiles to an IIFE of early returns): usable anywhere, incl. pipeline head. `await` inside → async IIFE, awaited.
- Error types UNION automatically: bindings of `Result<_, E1>` + `Result<_, E2>` → block assignable to `Result<T, E1 | E2>`. rlc infers NO types; tsc narrows each step.
- `return` inside the block returns from the BLOCK. So `try`/let-else are FORBIDDEN inside (located error) — use `<-`. `if let` is fine.
- Final expr already a Result → nested `Result<Result<...>>`; bind it with `<-` instead.

## @rl/std

```rl
import { Option, Result } from "@rl/std";
```
- `Option<T>` = `Some(value: T) | None`; `Result<T, E>` = `Ok<T> | Err<E>` (`Ok<T>` = `{kind:"Ok";value:T}`, `Err<E>` = `{kind:"Err";error:E}`, both exported as TYPES). Field names: `value` (Some/Ok), `error` (Err) → arms `Some(value)`, `Ok(value)`, `Err(error)`, alias `Some(value: v)`.
- Constructors take only their own variant's type: `Result.Ok(1)` → `Ok<number>`, `Result.Err("bad")` → `Err<string>` (NOT `Result.Ok<number, string>(1)` — one type arg each). Both fit any `Result<T, E>` slot; annotate the variable/return type when you need the full Result. Combinators take/return `Result<T, E>`.
- `andThen`/`andThenP` UNION the error types: `Result<T, E>` + `(T) => Result<U, F>` → `Result<U, E | F>`, so a pipeline of steps that each fail differently ends up with every error type (also works on the scattered `Ok<T> | Err<E1> | Err<E2>` a `try`/`result` value infers as; `ErrorOf<R>` is exported for reading the error side out). `map`/`mapP` add no failure → error type unchanged.
- `andThenP` reads its input type off the function you pass: named function → nothing to write; inline arrow → ANNOTATE the parameter (`Result.andThenP((u: User) => f(u))`), else it is `unknown`.
- Both are BUILT-IN enums: `_`-less match on their tags is exhaustiveness-checked even without import. Built-ins give checking only — import (or declare) to construct values.
- Combinators = data-first static fns; `*P` = data-last curried for pipelines.
  - Option: map andThen orElse filter unwrapOr unwrapOrElse expect okOr fromNullable toNullable isSome isNone zip flatten transpose collect (+P: map andThen orElse filter unwrapOr unwrapOrElse expect okOr)
  - Result: map mapErr andThen orElse unwrapOr unwrapOrElse expect ok err fromThrowable fromPromise isOk isErr flatten transpose collect (+P: map mapErr andThen orElse unwrapOr unwrapOrElse expect)
- Bridges: `Option.fromNullable(x)` (T|null|undefined), `Result.fromThrowable(() => JSON.parse(s))`, `Result.fromPromise(p)`, `Result.collect(arr)` / `Option.collect(arr)`.

## val

```rl
val const config = load();          // binding + every path from it is read-only
val let state = { count: 0 };       // still rebindable: state = {...state}
function read(val user: User) {}    // param the function cannot mutate
const f = (val u: U) => u.name;     // arrows, methods, catch (val e), for (val const x of xs)
```
- No modifier = plain TS = mutable. There is no `mut`.
- ERRORS on a val-rooted path, at ANY depth (`s.a.b.name = v`): `x.a = v` (all compound forms), `x[i] = v`, `x.a++`/`++x.a`, `delete x.a`.
- Method calls are NOT judged by name: `query.set("k")` on a user-defined `set` is fine. `rlc --check-types`/`--types` (only) report a call they resolve to a built-in mutator — Array push/pop/shift/unshift/splice/sort/reverse/fill/copyWithin, Map set/delete/clear, Set add/delete/clear, WeakMap set/delete, WeakSet add/delete, TypedArray set/sort/reverse/fill/copyWithin — so `val const items: number[] = []; items.push(1)` fails under `--check-types` and passes plain `rlc`. Unresolvable receiver (`any`, type param) = not reported. The VSCode extension shows these while editing (it runs the same mode over the buffer), so you do not have to save and run the CLI to see them.
- NOT an error: `x = v` (that is const/let's axis), reads, comparisons, spread `{...x}`.
- Call check: a val binding may only be passed to a `val` parameter of a same-file named function (`function f`, `const f = (...) =>`, `const f = function`). Plain path args only.
- val is per-BINDING, not per-object: `val const view = original;` still lets `original.x = 1`. Inner declarations shadow an outer val.
- Compile-time only: keyword (and its trailing spaces) erased, no runtime, no `readonly`.
- SYNTAX rule: `val` must sit on the same line as `const|let|var` or as the parameter binding it modifies; anywhere else `val` is an ordinary identifier and passes through. Not usable in match patterns (`Ok(val u)` → the match won't parse).

## Modules

- Import `.rl` files by relative path WITH extension: `import { Token } from "./token.rl";` → rewritten on emit to `./token.js` (default; `--rewrite-imports ts|off`; `ts` needs tsconfig `allowImportingTsExtensions` + `rewriteRelativeImportExtensions`).
- Exhaustiveness sees exported enums from DIRECT (1-hop) relative `.rl` imports (named/aliased/`* as ns`); re-export chains & package paths NOT collected → those matches compile unchecked.
- Dynamic `import()` specifiers not rewritten.

## Install

- `npm i -D rl-lang typescript@7` → prebuilt `rlc` binary (linux-x64/arm64, darwin-x64/arm64, win32-x64), run via `npx rlc`. `--check-types`/`--types` drive TypeScript 7's own compiler; writing sidecars (`--types`) additionally needs declaration emit, which the released package does not expose yet — point `RLC_TSGO_ROOT` at a built typescript-go for that.
- Other platforms / no npm: `cargo install --git https://github.com/load28/rl`; to keep using the npm launcher, set env `RLC_BINARY=/path/to/rlc`.
- Update: `npm i -D rl-lang@latest` (binary follows package version); verify `npx rlc -v`; then re-run `npx rlc --types src` and rebuild.
- Editor: VSCode extension in the rl repo `editors/vscode` (highlighting, rl + type diagnostics — including the typed-only ones: `val` built-in mutators and typed exhaustiveness — completion incl. std combinators, signature help, go-to-def). Everything TypeScript answers comes from the compiler's own language server (`tsgo --lsp`); the extension bundles no TypeScript, so install `typescript@7` in the project (or point `RLC_TSGO_ROOT` at a built typescript-go) or those features go quiet.

## Setup

New project: `npm init -y && npm i -D rl-lang typescript`; sources in `src/**/*.rl` (hand-written `.ts` alongside is fine); gitignore `.rl-types/` and the out dir.
```jsonc
// package.json
"scripts": { "build": "rlc -o build src && tsc", "types": "rlc --types src", "check": "rlc --check-types src" }
// tsconfig.json — resolve "./x.rl" and "@rl/std":
"compilerOptions": { "rootDirs": ["./src", "./.rl-types"], "paths": { "@rl/std": ["./.rl-types/rl.d.ts"] } }
```
Bundler alternative: `unplugin-rl` (`import rl from "unplugin-rl/vite"`, also `/rollup` `/webpack` `/esbuild`) — bundler reads `.rl` directly, no rlc build step; types still via `rlc --types`.

## Workflow

- Edit loop: change `.rl` → `npx rlc --check src` (fast rl-level, no TypeScript) → `npx rlc --check-types src` (types, exhaustiveness by narrowed type, `val`). Keep `npx rlc --types -w src` running so editor/tsc resolve `./x.rl` + `@rl/std`; if not watching, re-run `--types` after enum changes.
- Build: `npm run build` (rlc emits TS tree then tsc) or bundler build. CI: `rlc --check src && tsc --noEmit` + tests.
- `rlc <dir>`: `.rl`→`.ts`, hand-written `.ts` passthrough; `-o <dir>` separate tree (in-place overwrite refused); `@rl/std` auto-materialized when imported. `rlc -w` watches and also recompiles importers of changed files (cross-file exhaustiveness). Files compile in parallel (one per core) with identical output/diagnostics either way; `-j <n>` sets the count, `-j 1` = sequential.
- Emitted `.ts` starts with `// @generated` — NEVER edit output or `.rl-types/`; edit the `.rl` source.
- Offline docs: `npx rlc help` lists topics; `npx rlc help <topic>` (e.g. `match`, `try`, `install`) prints that section of this guide; `npx rlc help all` prints it whole. `npx rlc -h` = CLI options.

## Errors

- `rlc: file:line:col: msg` — e.g. `match on enum X is not exhaustive: missing "Y"` (add arms or `_`), `duplicate arm`, `or-pattern alternatives must bind the same names — <which binding differs>`, `cannot mix tag patterns and literal patterns`, else-block-must-diverge, try-position-restriction (extract helper), `cannot mutate through val binding `x``, `cannot pass val binding `x` to mutable parameter `p` of `f``.
- `rlc --check-types` also prints `match on literal union is not exhaustive: missing "..."` for `_`-less literal matches over finite literal unions, and ``cannot call mutating method `set` through val binding `m` `` for val paths it proves land on a built-in mutator. Its enum exhaustiveness message has no enum name (`match is not exhaustive`) — the answer comes from the narrowed type, not from a declaration table, which is also why it is the more accurate of the two.
- Type errors come from TypeScript, reported at the **`.rl` source** position — `rlc --check-types` prints `rlc: src/x.rl:12:31: ts(2339): <message>` (exit 1; `--types` still writes its sidecars), and the VSCode extension shows them inline (`source: ts`, `rl.typeDiagnostics` to disable). Applies inside `match` arms and `|>` pipelines too.
- A `|>` step's combinator inferring `unknown` (e.g. `Result.mapP((n) => n)` with `n: unknown`) means the pipeline **head** has no usable type — it is not a `Result`, or the head is an unannotated parameter. Fix the head, not the step.
- tsc errors on output containing literal `match`/`try` → silent passthrough; recheck semicolons/parens/reserved words.
- `generated TypeScript failed to parse` → pass-through source was invalid TS, or rlc bug.

## Checklist

`val` only in front of `const|let|var` or a parameter, same line; match parens + `_` last + object arms `({...})`; bind by field name not position; literal patterns are values (never mixed with tags, none in tuple elements); `_`-less match covers all (guards/nested don't count); `try`/`let-else` need `;` and diverging else, never inside match/`${}`/top-level; pipelines parenthesize ternaries/arrows, use `*P`; `result` blocks need ≥1 `<-` binding, `;` on bindings and none on the final expr; relative imports keep `.rl`; verify with `npx rlc --check-types src`, re-run `npx rlc --types src` after enum changes; never edit generated `.ts`.
