//! CLI contract tests: `rlc help` — the embedded language & workflow
//! reference (docs/ai/rl.md served by topic) — and `--jobs`, whose whole
//! contract is that parallelism changes nothing an observer can see.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

fn rlc(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rlc"))
        .args(args)
        .output()
        .expect("failed to run rlc")
}

static DIR_SEQ: AtomicUsize = AtomicUsize::new(0);

fn tmpdir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rl-cli-{}-{}",
        std::process::id(),
        DIR_SEQ.fetch_add(1, Ordering::SeqCst)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A small project: one shared module every other file imports (the shape
/// that exercises the imported-declaration cache), plus a file that fails
/// to compile so diagnostics are part of what must stay ordered.
fn write_project(dir: &Path, files: usize) {
    fs::write(
        dir.join("shared.rl"),
        "export enum Token { Num(value: number), Word(text: string), Eof }\n",
    )
    .unwrap();
    for n in 0..files {
        fs::write(
            dir.join(format!("m{n}.rl")),
            format!(
                "import {{ Token }} from \"./shared.rl\";\n\
                 export const n{n} = {n};\n\
                 export function name{n}(t: Token): string {{\n\
                 \x20 return match (t) {{ Num(value) => `${{value}}`, Word(text) => text, Eof => \"\" }};\n\
                 }}\n"
            ),
        )
        .unwrap();
    }
    // one non-exhaustive match: its error must appear in the same place
    // however many threads ran
    fs::write(
        dir.join("bad.rl"),
        "import { Token } from \"./shared.rl\";\n\
         export const broken = (t: Token) => match (t) { Eof => 0 };\n",
    )
    .unwrap();
}

/// What one `rlc` run leaves behind: the files it wrote (name → content,
/// sorted), its diagnostics, and whether it succeeded.
type RunResult = (Vec<(String, String)>, String, bool);

#[test]
fn jobs_does_not_change_outputs_or_diagnostics() {
    let src = tmpdir();
    write_project(&src, 12);

    let mut baseline: Option<RunResult> = None;
    for jobs in ["1", "2", "3", "8"] {
        let out = tmpdir();
        let result = rlc(&[
            "-j",
            jobs,
            "-o",
            out.to_str().unwrap(),
            src.to_str().unwrap(),
        ]);
        let mut written: Vec<(String, String)> = fs::read_dir(&out)
            .unwrap()
            .map(|e| {
                let path = e.unwrap().path();
                (
                    path.file_name().unwrap().to_string_lossy().into_owned(),
                    fs::read_to_string(&path).unwrap(),
                )
            })
            .collect();
        written.sort();
        let stderr = String::from_utf8(result.stderr)
            .unwrap()
            .replace(out.to_str().unwrap(), "<out>");
        let observed = (written, stderr, result.status.success());
        match &baseline {
            None => baseline = Some(observed),
            Some(expected) => assert_eq!(*expected, observed, "-j {jobs} diverged"),
        }
    }
    // the run really did compile something, and really did report the error
    let (written, stderr, success) = baseline.unwrap();
    assert!(!success, "the non-exhaustive match should fail the run");
    assert!(written.iter().any(|(name, _)| name == "m0.ts"));
    assert!(stderr.contains("not exhaustive"), "{stderr}");
}

#[test]
fn jobs_rejects_zero_and_garbage() {
    for value in ["0", "many", "-1"] {
        let out = rlc(&["-j", value, "--check", "examples"]);
        assert!(!out.status.success(), "--jobs {value} should be rejected");
        let stderr = String::from_utf8(out.stderr).unwrap();
        assert!(
            stderr.contains("--jobs expects a positive number"),
            "{stderr}"
        );
    }
    let out = rlc(&["--jobs"]);
    assert!(!out.status.success());
    assert!(
        String::from_utf8(out.stderr)
            .unwrap()
            .contains("--jobs requires a value")
    );
}

#[test]
fn help_lists_every_topic() {
    let out = rlc(&["help"]);
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    for topic in [
        "overview",
        "enum",
        "match",
        "try",
        "let-else",
        "if-let",
        "pipe",
        "std",
        "modules",
        "install",
        "setup",
        "workflow",
        "errors",
        "checklist",
    ] {
        assert!(stdout.contains(topic), "topic list missing {topic}");
        let out = rlc(&["help", topic]);
        assert!(out.status.success(), "rlc help {topic} failed");
        assert!(!out.stdout.is_empty(), "rlc help {topic} printed nothing");
    }
}

#[test]
fn help_topic_prints_only_its_section() {
    let out = rlc(&["help", "match"]);
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.starts_with("## match"));
    assert!(stdout.contains("or-pattern"));
    assert!(!stdout.contains("\n## try"), "leaked into the next section");
}

#[test]
fn help_resolves_aliases_case_insensitively() {
    let out = rlc(&["help", "Pipeline"]);
    assert!(out.status.success());
    assert!(String::from_utf8(out.stdout).unwrap().starts_with("## |>"));
}

#[test]
fn help_all_prints_the_whole_guide() {
    let out = rlc(&["help", "all"]);
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(stdout, include_str!("../docs/ai/rl.md"));
}

#[test]
fn help_unknown_topic_fails_with_a_pointer() {
    let out = rlc(&["help", "nosuch"]);
    assert!(!out.status.success());
    assert!(out.stdout.is_empty(), "errors must not pollute stdout");
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("unknown help topic \"nosuch\""));
    assert!(stderr.contains("rlc help"));
}

#[test]
fn help_only_triggers_as_the_first_argument() {
    // `rlc --check help` must treat "help" as an input path, not a command.
    let out = rlc(&["--check", "help"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("no such file or directory"), "{stderr}");
}

/* ------------------------------------------------------------------ */
/* --check-types: what only the real checker can answer                */
/* ------------------------------------------------------------------ */

fn have(cmd: &str) -> bool {
    Command::new(cmd)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Whether rlc can resolve a TypeScript to drive. Asked by running the mode
/// itself over a trivial project: the answer is rlc's own resolution, not a
/// guess about the machine.
fn have_typescript() -> bool {
    let dir = tmpdir();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join("src/probe.rl"), "export const n: number = 1;\n").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_rlc"))
        .args(["--check-types", "src"])
        .current_dir(&dir)
        .output()
        .expect("failed to run rlc");
    out.status.success()
}

/// Runs `rlc --check-types` over a one-file project and returns rlc's
/// stderr. Nothing is written, so a released TypeScript 7 — which cannot
/// emit declarations — answers these just as well as a built one.
fn types_stderr(source: &str) -> String {
    let dir = tmpdir();
    let src = dir.join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("main.rl"), source).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_rlc"))
        .args(["--check-types", "src"])
        .current_dir(&dir)
        .output()
        .expect("failed to run rlc");
    String::from_utf8_lossy(&out.stderr).into_owned()
}

macro_rules! require_types_toolchain {
    () => {
        if !have("node") || !have_typescript() {
            eprintln!("skipping: no node, or no TypeScript for rlc to drive");
            return;
        }
    };
}

#[test]
fn types_reports_a_missing_literal_of_a_finite_union() {
    require_types_toolchain!();
    let err = types_stderr(
        "type Direction = \"north\" | \"south\";\n\
         export function short(dir: Direction) {\n\
         \x20 return match (dir) { \"north\" => \"N\" };\n\
         }\n",
    );
    assert!(
        err.contains("match on literal union is not exhaustive: missing \"south\""),
        "{err}"
    );
    // reported at the `match` keyword of the .rl source, not in the
    // generated TypeScript
    assert!(err.contains("src/main.rl:3:10:"), "{err}");
}

#[test]
fn types_is_silent_when_the_literal_match_is_exhaustive() {
    require_types_toolchain!();
    let err = types_stderr(
        "type Direction = \"north\" | \"south\";\n\
         export function short(dir: Direction) {\n\
         \x20 return match (dir) { \"north\" => \"N\", \"south\" => \"S\" };\n\
         }\n",
    );
    assert!(!err.contains("not exhaustive"), "{err}");
}

#[test]
fn types_does_not_guess_when_the_scrutinee_type_is_open() {
    require_types_toolchain!();
    // string / number / unknown / any / a type parameter / a widened union
    // are not finite literal sets — no diagnostic, by design.
    let err = types_stderr(
        "export const a = (x: string) => match (x) { \"a\" => 1, \"b\" => 2 };\n\
         export const b = (x: number) => match (x) { 1 => 1, 2 => 2 };\n\
         export const c = (x: unknown) => match (x) { \"a\" => 1 };\n\
         export const d = (x: any) => match (x) { \"a\" => 1 };\n\
         export const e = <T extends string>(x: T) => match (x) { \"a\" => 1 };\n\
         export const f = (x: \"a\" | string) => match (x) { \"a\" => 1 };\n\
         export const g = (x: string | number) => match (x) { \"a\" => 1 };\n",
    );
    assert!(!err.contains("not exhaustive"), "{err}");
}

#[test]
fn types_checks_a_union_derived_from_as_const() {
    require_types_toolchain!();
    // The kind of type rlc could never resolve on its own — the checker can.
    let err = types_stderr(
        "const values = [\"north\", \"south\"] as const;\n\
         type D = (typeof values)[number];\n\
         export const pick = (x: D) => match (x) { \"north\" => 1 };\n",
    );
    assert!(err.contains("not exhaustive: missing \"south\""), "{err}");
}

#[test]
fn types_skips_a_literal_match_with_a_wildcard() {
    require_types_toolchain!();
    let err = types_stderr(
        "type D = \"north\" | \"south\";\n\
         export const pick = (x: D) => match (x) { \"north\" => 1, _ => 0 };\n",
    );
    assert!(!err.contains("not exhaustive"), "{err}");
}

#[test]
fn types_does_not_count_a_guarded_arm_as_covering() {
    require_types_toolchain!();
    let err = types_stderr(
        "export const pick = (x: \"a\" | \"b\", ok: boolean) =>\n\
         \x20 match (x) { \"a\" if ok => 1, \"b\" => 2 };\n",
    );
    assert!(err.contains("not exhaustive: missing \"a\""), "{err}");
}

#[test]
fn types_checks_boolean_and_number_unions() {
    require_types_toolchain!();
    let err = types_stderr(
        "export const b = (x: boolean) => match (x) { true => 1 };\n\
         export const n = (x: 200 | 404) => match (x) { 200 => 1 };\n",
    );
    assert!(err.contains("not exhaustive: missing false"), "{err}");
    assert!(err.contains("not exhaustive: missing 404"), "{err}");
}

#[test]
fn types_maps_a_bad_case_literal_back_to_the_rl_source() {
    require_types_toolchain!();
    // The `case` label is copied from the source, so tsc's complaint about
    // it lands on the literal the user wrote.
    let err = types_stderr(
        "export const pick = (x: \"a\" | \"b\") => match (x) { \"a\" => 1, \"c\" => 2, \"b\" => 3 };\n",
    );
    assert!(err.contains("src/main.rl:1:61:"), "{err}");
    assert!(err.contains("is not comparable to type"), "{err}");
}

/* ------------------------------------------------------------------ */
/* --types: typed mutation for `val` (TASK-071)                        */
/* ------------------------------------------------------------------ */

#[test]
fn types_reports_a_mutating_method_of_a_built_in() {
    require_types_toolchain!();
    let err = types_stderr(
        "val const map = new Map<string, number>();\n\
         map.set(\"a\", 1);\n",
    );
    assert!(
        err.contains("cannot call mutating method `set` through val binding `map`"),
        "{err}"
    );
    // reported at the path's root in the .rl source
    assert!(err.contains("src/main.rl:2:1:"), "{err}");
}

#[test]
fn types_reports_set_add_and_array_push() {
    require_types_toolchain!();
    let err = types_stderr(
        "val const set = new Set<number>();\n\
         set.add(1);\n\
         val const items: number[] = [];\n\
         items.push(1);\n\
         val const state = { tags: [] as string[] };\n\
         state.tags.push(\"rl\");\n",
    );
    assert!(
        err.contains("mutating method `add` through val binding `set`"),
        "{err}"
    );
    assert!(
        err.contains("mutating method `push` through val binding `items`"),
        "{err}"
    );
    // ... and at any depth of the access path
    assert!(
        err.contains("mutating method `push` through val binding `state`"),
        "{err}"
    );
}

#[test]
fn types_leaves_a_user_defined_method_of_the_same_name_alone() {
    require_types_toolchain!();
    // The whole point: `set`/`add`/`push` on a user-defined type are not
    // mutations, and rlc must not guess otherwise from the name.
    let err = types_stderr(
        "class Query {\n\
         \x20 set(key: string): Query {\n\
         \x20   return new Query();\n\
         \x20 }\n\
         }\n\
         class Collection {\n\
         \x20 add(value: number): Collection {\n\
         \x20   return new Collection();\n\
         \x20 }\n\
         \x20 push(value: number): Collection {\n\
         \x20   return new Collection();\n\
         \x20 }\n\
         }\n\
         val const query = new Query();\n\
         query.set(\"name\");\n\
         val const collection = new Collection();\n\
         collection.add(1);\n\
         collection.push(2);\n",
    );
    assert!(!err.contains("mutating method"), "{err}");
}

#[test]
fn types_does_not_guess_when_the_receiver_is_unknown() {
    require_types_toolchain!();
    // No resolvable receiver — `any`, a type parameter, an unresolved
    // import — is left alone: a false positive costs more than a miss.
    let err = types_stderr(
        "declare function getSomething(): any;\n\
         val const value = getSomething();\n\
         value.set(\"x\");\n\
         export function shift<T extends { push(v: number): void }>(val box: T) {\n\
         \x20 box.push(1);\n\
         }\n",
    );
    assert!(!err.contains("mutating method"), "{err}");
}

#[test]
fn types_keeps_reporting_syntactic_mutation_without_the_checker() {
    require_types_toolchain!();
    // The syntactic half is rlc's own and fires before the host runs.
    let err = types_stderr(
        "val const user = {\n\
         \x20 profile: {\n\
         \x20   name: \"A\",\n\
         \x20 },\n\
         };\n\
         user.profile.name = \"B\";\n",
    );
    assert!(
        err.contains("cannot mutate through val binding `user`"),
        "{err}"
    );
    assert!(err.contains("src/main.rl:6:1:"), "{err}");
}
