//! The TypeScript 7 backend, end to end (`rlc --check-types` / `--types`).
//!
//! These tests need a **built** typescript-go tree — the `tsgo` binary and
//! the JS API client from the same build (see
//! `docs/tasks/TASK-073-typescript-native-backend.md`). They skip silently
//! when one is not there, exactly as the `tsc`/`node` tests do; the guard
//! mirrors the compiler's own resolution rules so a skip means "no
//! toolchain", never "the check quietly did nothing".

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

const BIN_IN_TREE: &str = "built/local/tsgo";
const API_IN_TREE: &str = "_packages/native-preview/dist/api/sync/api.js";

/// What rlc will find, mirroring its own resolution rules.
#[derive(Clone)]
enum Toolchain {
    /// A built typescript-go checkout, named through `RLC_TSGO_ROOT`.
    Tree(PathBuf),
    /// An installed package, which rlc finds on its own.
    Installed,
}

fn toolchain() -> Option<Toolchain> {
    // rlc's own order: a directly named client wins over any checkout, so
    // the guard has to agree or a test will run against a compiler the
    // guard did not vet.
    if let Some(api) = std::env::var_os("RLC_TSGO_API").filter(|v| !v.is_empty()) {
        return Path::new(&api).exists().then_some(Toolchain::Installed);
    }
    let root = match std::env::var_os("RLC_TSGO_ROOT") {
        Some(root) if !root.is_empty() => PathBuf::from(root),
        _ => PathBuf::from("../typescript-go"),
    };
    (root.join(BIN_IN_TREE).exists() && root.join(API_IN_TREE).exists())
        .then_some(Toolchain::Tree(root))
}

/// Any resolvable compiler — enough to check.
macro_rules! require_tsgo {
    () => {
        match toolchain() {
            Some(toolchain) => toolchain,
            None => return,
        }
    };
}

/// A compiler that can also emit declarations. The released 7.0 client
/// cannot (its `Program` has no `getDeclarationEmit`), so these need a
/// built checkout until a release catches up.
macro_rules! require_emit {
    () => {
        match toolchain() {
            Some(Toolchain::Tree(root)) => Toolchain::Tree(root),
            _ => return,
        }
    };
}

static DIR_SEQ: AtomicUsize = AtomicUsize::new(0);

fn tmpdir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rl-native-{}-{}",
        std::process::id(),
        DIR_SEQ.fetch_add(1, Ordering::SeqCst)
    ));
    fs::create_dir_all(dir.join("src")).unwrap();
    dir
}

fn write(dir: &Path, name: &str, text: &str) {
    fs::write(dir.join(name), text).unwrap();
}

/// A project whose `tsconfig.json` globs `src` — the lowered `.rl` modules
/// have to enter the program through the user's own configuration.
fn project(files: &[(&str, &str)]) -> PathBuf {
    let dir = tmpdir();
    write(
        &dir,
        "tsconfig.json",
        r#"{
  "compilerOptions": {
    "target": "es2022",
    "module": "preserve",
    "moduleResolution": "bundler",
    "strict": true,
    "skipLibCheck": true,
    "noEmit": true
  },
  "include": ["src"]
}
"#,
    );
    for (name, text) in files {
        write(&dir, name, text);
    }
    dir
}

/// Runs rlc in `dir` with the toolchain the guard resolved.
fn run(dir: &Path, toolchain: &Toolchain, args: &[&str]) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rlc"));
    command.args(args).current_dir(dir);
    match toolchain {
        Toolchain::Tree(root) => {
            command.env("RLC_TSGO_ROOT", root);
        }
        // Nothing to set: rlc finds the installed package itself.
        Toolchain::Installed => {}
    }
    command.output().expect("rlc runs")
}

/// Runs `rlc --check-types src` in `dir`, returning its diagnostics.
fn check(dir: &Path, toolchain: &Toolchain) -> String {
    let out = run(dir, toolchain, &["--check-types", "src"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("no TypeScript compiler found"),
        "the toolchain guard passed but rlc disagreed: {stderr}"
    );
    // Diagnostics are diagnostics: stderr, in rlc's own form. stdout is
    // reserved for the modes that pipe.
    assert!(
        out.stdout.is_empty(),
        "a checking mode wrote to stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    stderr.into_owned()
}

#[test]
fn watching_re_checks_against_the_compiler_it_already_started() {
    let root = require_tsgo!();
    let dir = project(&[(
        "src/color.rl",
        "export enum Color { Red(), Green() }\n\
         export function name(c: Color): string {\n\
         \x20 return match (c) { Red => \"red\", Green => \"green\" };\n\
         }\n",
    )]);

    let mut command = Command::new(env!("CARGO_BIN_EXE_rlc"));
    command
        .args(["--check-types", "src", "-w"])
        .current_dir(&dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    if let Toolchain::Tree(root) = &root {
        command.env("RLC_TSGO_ROOT", root);
    }
    let mut child = command.spawn().expect("rlc runs");

    // Let the first pass finish, then add a case with no arm: the watch has
    // to see the edit through the compiler it is already holding.
    std::thread::sleep(std::time::Duration::from_secs(6));
    write(
        &dir,
        "src/color.rl",
        "export enum Color { Red(), Green(), Blue() }\n\
         export function name(c: Color): string {\n\
         \x20 return match (c) { Red => \"red\", Green => \"green\" };\n\
         }\n",
    );
    std::thread::sleep(std::time::Duration::from_secs(5));
    let _ = child.kill();
    let out = child.wait_with_output().expect("rlc exits");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("missing \"Blue\""),
        "the second pass saw the edit: {stdout}{stderr}"
    );
    assert_eq!(
        stderr.matches("— watching").count(),
        2,
        "one pass at startup and one for the edit: {stderr}"
    );
}

#[test]
fn a_hand_written_ts_file_imports_an_rl_file_by_the_specifier_it_writes() {
    let root = require_tsgo!();
    // `"./shape.rl"` is what a user writes, and it needs no configuration:
    // the lowered module is served at `shape.rl.ts`, which is what ordinary
    // TypeScript resolution finds for that specifier. The project's
    // tsconfig here sets no rl-specific option at all.
    let dir = project(&[
        (
            "src/shape.rl",
            "export enum Shape { Circle(radius: number), Point }\n",
        ),
        (
            "src/use.ts",
            "import { Shape } from \"./shape.rl\";\n\
             export const s: Shape = Shape.Point;\n\
             export const bad: number = Shape.Point;\n",
        ),
    ]);
    let out = check(&dir, &root);
    // The import resolved — the only error is the deliberate one, reported
    // in the hand-written file at TypeScript's own coordinates.
    assert!(
        out.contains("src/use.ts: ts(2322)"),
        "the .ts file's own error, in one project with the .rl: {out}"
    );
    assert!(
        !out.contains("2307") && !out.contains("Cannot find module"),
        "and nothing failed to resolve: {out}"
    );
}

#[test]
fn naming_one_file_still_compiles_against_the_whole_project() {
    let root = require_emit!();
    let dir = project(&[
        (
            "src/token.rl",
            "export enum Token { Num(value: number), Eof }\n",
        ),
        (
            "src/parse.rl",
            "import { Token } from \"./token.rl\";\n\
             export function width(t: Token): number {\n\
             \x20 return match (t) { Num(value) => value, Eof => 0 };\n\
             }\n",
        ),
    ]);
    let out_dir = dir.join("out");
    let out = run(
        &dir,
        &root,
        &["--types", "src/parse.rl", "-o", out_dir.to_str().unwrap()],
    );
    // `./token.rl` was never named, but it is part of the project, so it is
    // part of the graph — otherwise this would be TS2307.
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    assert!(
        out_dir.join("parse.rl.d.ts").is_file(),
        "the named input is written"
    );
    assert!(
        !out_dir.join("token.rl.d.ts").exists(),
        "what was not named is in the graph, not in the output"
    );
}

#[test]
fn a_declaration_carries_a_map_back_to_the_rl_source() {
    let root = require_emit!();
    let dir = project(&[(
        "src/token.rl",
        "export enum Token { Num(value: number), Eof }\n\
         export function width(t: Token): number {\n\
         \x20 return match (t) { Num(value) => value, Eof => 0 };\n\
         }\n",
    )]);
    let out_dir = dir.join("out");
    let out = run(
        &dir,
        &root,
        &["--types", "src", "-o", out_dir.to_str().unwrap()],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    // The sidecar takes the name a `"./token.rl"` specifier resolves to
    // when no compiler is running — which is what makes it a sidecar.
    let declarations = fs::read_to_string(out_dir.join("token.rl.d.ts")).expect("the sidecar");
    assert!(
        declarations.contains("//# sourceMappingURL=token.rl.d.ts.map"),
        "and points at its map: {declarations}"
    );
    let map = fs::read_to_string(out_dir.join("token.rl.d.ts.map")).expect("the map");
    assert!(
        map.contains("token.rl\"") && map.contains("\"mappings\""),
        "whose sources is the .rl file, so go-to-definition lands there: {map}"
    );
}

#[test]
fn declarations_are_emitted_by_the_compiler_itself() {
    let root = require_emit!();
    let dir = project(&[(
        "src/shape.rl",
        "export enum Shape { Circle(radius: number), Point }\n\
         export function area(s: Shape): number {\n\
         \x20 return match (s) { Circle(radius) => radius, Point => 0 };\n\
         }\n",
    )]);
    let out_dir = dir.join("out");
    let out = run(
        &dir,
        &root,
        &["--types", "src", "-o", out_dir.to_str().unwrap()],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let declaration = fs::read_to_string(out_dir.join("shape.rl.d.ts")).expect("a .d.ts");
    // rlc writes no declaration syntax of its own: this is what the compiler
    // emits for the module rlc lowered, exactly as for a hand-written one.
    assert!(
        declaration.contains("kind: \"Circle\"") && declaration.contains("radius: number"),
        "the enum's union type: {declaration}"
    );
    assert!(
        declaration.contains("export declare function area(s: Shape): number;"),
        "the function's signature: {declaration}"
    );
}

#[test]
fn the_standard_library_enters_the_graph_as_a_module_of_the_project() {
    let root = require_emit!();
    let dir = project(&[(
        "src/parse.rl",
        "import { Result } from \"@rl/std\";\n\
         export function parse(text: string): Result<number, string> {\n\
         \x20 const n = Number(text);\n\
         \x20 return Number.isNaN(n) ? Result.Err(\"not a number\") : Result.Ok(n);\n\
         }\n",
    )]);
    let out_dir = dir.join("out");
    let out = run(
        &dir,
        &root,
        &["--types", "src", "-o", out_dir.to_str().unwrap()],
    );
    assert!(
        out.status.success(),
        "@rl/std has to resolve, and its types have to check: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    // The library is a module of the project, resolved by ordinary node
    // resolution — so the specifier stays bare, in the source and in the
    // declaration alike, and no `paths` entry is involved in this compile.
    let declaration = fs::read_to_string(out_dir.join("parse.rl.d.ts")).expect("a .d.ts");
    assert!(
        declaration.contains("from \"@rl/std\""),
        "the declaration keeps the specifier the user wrote: {declaration}"
    );
}

#[test]
fn a_diagnostic_on_generated_code_still_names_the_construct_it_came_from() {
    let root = require_tsgo!();
    // A plain TypeScript enum is not an rl enum, so matching on one lowers
    // to a `.kind` switch over a value that has no `kind`. The error is
    // real; what matters here is that it is reported in the `.rl` file, at
    // the construct rlc generated the code for, and labelled as generated.
    let dir = project(&[(
        "src/ts_enum.rl",
        "export enum Plain { A, B }\n\
         export function f(p: Plain): number {\n\
         \x20 return match (p) { A => 1 };\n\
         }\n",
    )]);
    let out = check(&dir, &root);
    assert!(
        out.contains("src/ts_enum.rl:3:") && out.contains("ts(2339)"),
        "reported in the .rl file: {out}"
    );
    assert!(
        out.contains("(in code rlc generated for this construct)"),
        "and labelled as generated rather than as the user's own line: {out}"
    );
}

#[test]
fn a_ts_file_and_an_rl_file_share_one_project_graph() {
    let root = require_tsgo!();
    let dir = project(&[
        (
            "src/user.ts",
            "export type State = \"idle\" | \"loading\" | \"done\";\n",
        ),
        (
            "src/state.rl",
            "import type { State } from \"./user\";\n\
             export function render(state: State): number {\n\
             \x20 return match (state) { \"idle\" => 0, \"loading\" => 1, \"done\" => 2 };\n\
             }\n",
        ),
    ]);
    // The type comes from the `.ts` file; the match is exhaustive over it.
    assert_eq!(check(&dir, &root), "");
}

#[test]
fn literal_exhaustiveness_uses_the_narrowed_type_at_the_match() {
    let root = require_tsgo!();
    let dir = project(&[
        (
            "src/user.ts",
            "export type State = \"idle\" | \"loading\" | \"done\";\n",
        ),
        (
            "src/state.rl",
            "import type { State } from \"./user\";\n\
             export function render(state: State): number {\n\
             \x20 if (state !== \"idle\") {\n\
             \x20   return match (state) { \"loading\" => 1 };\n\
             \x20 }\n\
             \x20 return 0;\n\
             }\n",
        ),
    ]);
    let out = check(&dir, &root);
    assert!(
        out.contains("missing \"done\""),
        "the narrowed type still allows \"done\": {out}"
    );
    assert!(
        !out.contains("idle"),
        "the guard removed \"idle\" before the match: {out}"
    );
}

#[test]
fn enum_exhaustiveness_uses_the_narrowed_type_at_the_match() {
    let root = require_tsgo!();
    let dir = project(&[(
        "src/shape.rl",
        "export enum Shape { Circle(radius: number), Square(side: number), Point }\n\
         export function area(s: Shape): number {\n\
         \x20 if (s.kind !== \"Point\") {\n\
         \x20   return match (s) { Circle(radius) => radius };\n\
         \x20 }\n\
         \x20 return 0;\n\
         }\n",
    )]);
    let out = check(&dir, &root);
    assert!(
        out.contains("missing \"Square\""),
        "the narrowed type still allows Square: {out}"
    );
    assert!(
        !out.contains("Point"),
        "the guard removed Point before the match: {out}"
    );
}

#[test]
fn val_holds_on_a_parameter_and_across_a_function_boundary() {
    let root = require_tsgo!();
    let dir = project(&[(
        "src/pass.rl",
        "interface User { name: string; tags: string[] }\n         function update(user: User) { user.name = \"Lee\"; }\n         export function process(val user: User) {\n         \x20 user.name = \"Lee\";\n         \x20 user.tags.push(\"x\");\n         \x20 update(user);\n         }\n",
    )]);
    // `val` has two syntactic homes and three rules; a mode that checks
    // only declarations, or only mutation paths, silently passes code the
    // rl-level check rejects.
    let out = check(&dir, &root);
    assert!(
        out.contains("cannot mutate through val binding `user`"),
        "a val parameter is a val binding: {out}"
    );
    assert!(
        out.contains("mutating method `push` through val binding `user`"),
        "and its access paths are read-only too: {out}"
    );
    assert!(
        out.contains("cannot pass val binding `user` to mutable parameter `user` of `update`"),
        "and it cannot be handed to a parameter that is not `val`: {out}"
    );
}

#[test]
fn exhaustiveness_holds_when_the_scrutinee_is_not_a_name() {
    let root = require_tsgo!();
    let dir = project(&[(
        "src/shape.rl",
        "export enum Shape { Circle(radius: number), Rect(w: number, h: number) }\n         declare function getShape(): Shape;\n         type State = \"idle\" | \"loading\" | \"done\";\n         declare function getState(): State;\n         export const area = match (getShape()) { Circle(radius) => radius };\n         export const label = match (getState()) { \"idle\" => 0, \"loading\" => 1 };\n",
    )]);
    // The question is asked about the temporary the match binds, not about
    // the scrutinee's text: at `getShape` the checker answers "a function",
    // which has no cases and no literals, and both questions came back
    // silent when that was where they were asked.
    let out = check(&dir, &root);
    assert!(
        out.contains("missing \"Rect\""),
        "a call scrutinee still has an enum type: {out}"
    );
    assert!(
        out.contains("missing \"done\""),
        "a call scrutinee still has a literal union type: {out}"
    );
}

#[test]
fn an_enum_from_another_module_needs_no_declaration_collecting() {
    let root = require_tsgo!();
    let dir = project(&[
        (
            "src/token.rl",
            "export enum Token { Num(value: number), Eof }\n",
        ),
        (
            "src/parse.rl",
            "import { Token } from \"./token.rl\";\n\
             export function width(t: Token): number {\n\
             \x20 return match (t) { Num(value) => value };\n\
             }\n",
        ),
    ]);
    let out = check(&dir, &root);
    assert!(
        out.contains("missing \"Eof\""),
        "the enum's cases come from the imported module's own type: {out}"
    );
}

#[test]
fn val_mutation_is_decided_by_the_method_the_call_resolves_to() {
    let root = require_tsgo!();
    let dir = project(&[
        (
            "src/store.ts",
            "export class Store {\n  set(key: string, value: string): void {}\n}\n",
        ),
        (
            "src/use.rl",
            "import { Store } from \"./store\";\n\
             export function go(): void {\n\
             \x20 val const map = new Map<string, number>();\n\
             \x20 map.set(\"a\", 1);\n\
             \x20 val const store = new Store();\n\
             \x20 store.set(\"a\", \"b\");\n\
             }\n",
        ),
    ]);
    let out = check(&dir, &root);
    assert!(
        out.contains("mutating method `set` through val binding `map`"),
        "Map#set is declared in TypeScript's own lib: {out}"
    );
    assert!(
        !out.contains("val binding `store`"),
        "Store#set only shares the name: {out}"
    );
}

#[test]
fn a_shadowing_binding_is_a_different_binding() {
    let root = require_tsgo!();
    let dir = project(&[(
        "src/shadow.rl",
        "export function go(): void {\n\
         \x20 val const items = new Map<string, number>();\n\
         \x20 {\n\
         \x20   const items = new Map<string, number>();\n\
         \x20   items.set(\"inner\", 1);\n\
         \x20 }\n\
         }\n",
    )]);
    assert_eq!(
        check(&dir, &root),
        "",
        "the inner `items` is an ordinary binding that shares a name"
    );
}

#[test]
fn a_direct_mutation_through_a_val_binding_is_reported() {
    let root = require_tsgo!();
    let dir = project(&[(
        "src/direct.rl",
        "export function go(): void {\n\
         \x20 val const user = { name: \"a\", count: 0 };\n\
         \x20 user.name = \"b\";\n\
         }\n",
    )]);
    let out = check(&dir, &root);
    assert!(
        out.contains("cannot mutate through val binding `user`"),
        "an assignment mutates on syntax alone: {out}"
    );
}

#[test]
fn a_mutation_through_an_unmarked_binding_is_left_alone() {
    let root = require_tsgo!();
    let dir = project(&[(
        "src/plain.rl",
        "export function go(): void {\n\
         \x20 const items: number[] = [];\n\
         \x20 items.push(1);\n\
         \x20 const user = { name: \"a\" };\n\
         \x20 user.name = \"b\";\n\
         }\n",
    )]);
    assert_eq!(check(&dir, &root), "");
}

#[test]
fn an_any_receiver_is_never_called_a_mutation() {
    let root = require_tsgo!();
    let dir = project(&[(
        "src/any.rl",
        "export function go(x: any): void {\n\
         \x20 val const y = x;\n\
         \x20 y.set(\"a\", 1);\n\
         \x20 y.push(1);\n\
         }\n",
    )]);
    assert_eq!(check(&dir, &root), "");
}

#[test]
fn a_call_is_checked_against_the_declaration_it_resolves_to() {
    // Two functions share a name; which one a call names is the callee
    // symbol's answer, not the name's. The outer call reaches the
    // top-level declaration (mutable parameter — an error); the inner
    // call reaches the block's val-parameter arrow (fine). The
    // name-keyed model had to skip both as ambiguous.
    let root = require_tsgo!();
    let dir = project(&[(
        "src/who.rl",
        "type U = { name: string };\n\
         export function go(): void {\n\
         \x20 val const user: U = { name: \"a\" };\n\
         \x20 handle(user);\n\
         \x20 {\n\
         \x20   const handle = (val u: U): void => {};\n\
         \x20   handle(user);\n\
         \x20 }\n\
         }\n\
         function handle(u: U): void { u.name = \"b\"; }\n",
    )]);
    let out = check(&dir, &root);
    assert_eq!(
        out.lines()
            .filter(|l| l.contains("cannot pass val binding `user`"))
            .count(),
        1,
        "only the call that names the mutable-parameter declaration: {out}"
    );
    assert!(
        out.contains("src/who.rl:4:10") && out.contains("mutable parameter `u` of `handle`"),
        "reported at the outer call's argument: {out}"
    );
}

#[test]
fn an_answer_past_the_pipe_buffer_still_arrives() {
    // A few hundred diagnostics make the host's one-line answer larger
    // than a pipe buffer (64 KB on Linux). The host must flush the whole
    // line synchronously before it turns around to wait for the next
    // request — an async write that queued the tail past the buffer
    // deadlocked the session: the host blocked reading, the compiler
    // blocked waiting for the rest of the answer.
    let root = require_tsgo!();
    let mut source = String::new();
    for i in 0..400 {
        source.push_str(&format!("export const a{i}: number = \"x{i}\";\n"));
    }
    let dir = project(&[("src/big.rl", source.as_str())]);
    let out = check(&dir, &root);
    assert_eq!(
        out.lines().filter(|l| l.contains("ts(2322)")).count(),
        400,
        "every diagnostic of a >64 KB answer arrives: {out}"
    );
}

#[test]
fn a_non_mutating_builtin_method_is_not_a_mutation() {
    // Collection asks about every method call through a `val` path; the
    // verdict is two halves — the checker's (a built-in's method) and rl's
    // policy (one of the mutating ones). A built-in read fails the second,
    // so widening collection must never widen what is reported.
    let root = require_tsgo!();
    let dir = project(&[(
        "src/read.rl",
        "export function go(): void {\n\
         \x20 val const m = new Map<string, number>();\n\
         \x20 m.get(\"a\");\n\
         \x20 m.has(\"a\");\n\
         \x20 val const items: number[] = [];\n\
         \x20 items.at(0);\n\
         \x20 items.includes(1);\n\
         }\n",
    )]);
    assert_eq!(
        check(&dir, &root),
        "",
        "a built-in method outside rl's mutator policy reads, it does not mutate"
    );
}

#[test]
fn batched_answers_land_on_their_own_questions() {
    // One ask carries every module's questions; the host groups them by
    // module for the checker's batch endpoints and scatters the answers
    // back by index. Each diagnostic must land on its own file and line,
    // whichever module its group ran under.
    let root = require_tsgo!();
    let dir = project(&[
        (
            "src/a.rl",
            "declare const x: \"a\" | \"b\";\n\
             export const va = match (x) { \"a\" => 1 };\n\
             export function fa(): void {\n\
             \x20 val const ua = { n: 0 };\n\
             \x20 ua.n = 1;\n\
             }\n",
        ),
        (
            "src/b.rl",
            "declare const y: \"c\" | \"d\";\n\
             export const vb = match (y) { \"c\" => 1 };\n\
             export function fb(): void {\n\
             \x20 val const ub = { m: 0 };\n\
             \x20 ub.m = 1;\n\
             }\n",
        ),
    ]);
    let out = check(&dir, &root);
    for (file, line) in [
        ("src/a.rl:2:", "missing \"b\""),
        ("src/b.rl:2:", "missing \"d\""),
        ("src/a.rl:5:3", "cannot mutate through val binding `ua`"),
        ("src/b.rl:5:3", "cannot mutate through val binding `ub`"),
    ] {
        let landed = out.lines().any(|l| l.contains(file) && l.contains(line));
        assert!(landed, "expected {line} at {file}: {out}");
    }
}

#[test]
fn a_type_error_is_reported_at_its_position_in_the_rl_source() {
    let root = require_tsgo!();
    let dir = project(&[(
        "src/bad.rl",
        // A multi-byte prefix: TypeScript counts UTF-16 code units and the
        // `.rl` position is a byte offset, so the two have to be converted.
        "export function go(): void {\n  const 한글: string = 1;\n}\n",
    )]);
    let out = check(&dir, &root);
    assert!(
        out.starts_with("src/bad.rl:2:9: ts(2322):") || out.contains("bad.rl:2:9: ts(2322):"),
        "the diagnostic belongs at the declaration in the .rl file: {out}"
    );
}
