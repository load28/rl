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
    "allowImportingTsExtensions": true,
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
fn a_ts_file_and_an_rl_file_share_one_project_graph() {
    let root = require_tsgo!();
    let dir = project(&[
        (
            "src/user.ts",
            "export type State = \"idle\" | \"loading\" | \"done\";\n",
        ),
        (
            "src/state.rl",
            "import type { State } from \"./user.ts\";\n\
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
            "import type { State } from \"./user.ts\";\n\
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
            "import { Store } from \"./store.ts\";\n\
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
