//! The TypeScript 7 native backend, end to end (`rlc --native-check`).
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

/// The typescript-go tree rlc would resolve, when both halves are built.
fn toolchain_root() -> Option<PathBuf> {
    let root = match std::env::var_os("RLC_TSGO_ROOT") {
        Some(root) if !root.is_empty() => PathBuf::from(root),
        _ => PathBuf::from("../typescript-go"),
    };
    (root.join(BIN_IN_TREE).exists() && root.join(API_IN_TREE).exists()).then_some(root)
}

macro_rules! require_tsgo {
    () => {
        match toolchain_root() {
            Some(root) => root,
            None => return,
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

/// Runs `rlc --native-check src` in `dir`, returning its stdout.
fn check(dir: &Path, root: &Path) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_rlc"))
        .args(["--native-check", "src"])
        .current_dir(dir)
        .env("RLC_TSGO_ROOT", root)
        .output()
        .expect("rlc runs");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("rlc: no tsgo") && !stderr.contains("rlc: no TypeScript API"),
        "the toolchain guard passed but rlc disagreed: {stderr}"
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
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
fn a_declaration_carries_a_map_back_to_the_rl_source() {
    let root = require_tsgo!();
    let dir = project(&[(
        "src/token.rl",
        "export enum Token { Num(value: number), Eof }\n\
         export function width(t: Token): number {\n\
         \x20 return match (t) { Num(value) => value, Eof => 0 };\n\
         }\n",
    )]);
    let out_dir = dir.join("out");
    let out = Command::new(env!("CARGO_BIN_EXE_rlc"))
        .args(["--native-check", "src", "-o"])
        .arg(&out_dir)
        .current_dir(&dir)
        .env("RLC_TSGO_ROOT", root)
        .output()
        .expect("rlc runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    // The sidecar takes the name a `"./token.rl"` specifier resolves to
    // when no compiler is running — which is what makes it a sidecar.
    let declarations = fs::read_to_string(out_dir.join("src/token.rl.d.ts")).expect("the sidecar");
    assert!(
        declarations.contains("//# sourceMappingURL=token.rl.d.ts.map"),
        "and points at its map: {declarations}"
    );
    let map = fs::read_to_string(out_dir.join("src/token.rl.d.ts.map")).expect("the map");
    assert!(
        map.contains("token.rl\"") && map.contains("\"mappings\""),
        "whose sources is the .rl file, so go-to-definition lands there: {map}"
    );
}

#[test]
fn declarations_are_emitted_by_the_compiler_itself() {
    let root = require_tsgo!();
    let dir = project(&[(
        "src/shape.rl",
        "export enum Shape { Circle(radius: number), Point }\n\
         export function area(s: Shape): number {\n\
         \x20 return match (s) { Circle(radius) => radius, Point => 0 };\n\
         }\n",
    )]);
    let out_dir = dir.join("out");
    let out = Command::new(env!("CARGO_BIN_EXE_rlc"))
        .args(["--native-check", "src", "-o"])
        .arg(&out_dir)
        .current_dir(&dir)
        .env("RLC_TSGO_ROOT", root)
        .output()
        .expect("rlc runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let declaration = fs::read_to_string(out_dir.join("src/shape.rl.d.ts")).expect("a .d.ts");
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
    let root = require_tsgo!();
    let dir = project(&[(
        "src/parse.rl",
        "import { Result } from \"@rl/std\";\n\
         export function parse(text: string): Result<number, string> {\n\
         \x20 const n = Number(text);\n\
         \x20 return Number.isNaN(n) ? Result.Err(\"not a number\") : Result.Ok(n);\n\
         }\n",
    )]);
    let out_dir = dir.join("out");
    let out = Command::new(env!("CARGO_BIN_EXE_rlc"))
        .args(["--native-check", "src", "-o"])
        .arg(&out_dir)
        .current_dir(&dir)
        .env("RLC_TSGO_ROOT", root)
        .output()
        .expect("rlc runs");
    assert!(
        out.status.success(),
        "@rl/std has to resolve, and its types have to check: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    // The library is a module of the project, resolved by ordinary node
    // resolution — so the specifier stays bare, in the source and in the
    // declaration alike, and no `paths` entry is involved in this compile.
    let declaration = fs::read_to_string(out_dir.join("src/parse.rl.d.ts")).expect("a .d.ts");
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
        out.contains("`map` is a val binding"),
        "Map#set is declared in TypeScript's own lib: {out}"
    );
    assert!(
        !out.contains("`store` is a val binding"),
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
