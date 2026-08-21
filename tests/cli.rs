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

/// [`types_stderr`], but the file's text arrives as an editor's unsaved
/// buffer: `saved` is written to disk, `buffer` goes on stdin under
/// `--overlay`, and the check is what an editor would run.
fn types_stderr_overlay(saved: &str, buffer: &str, rl_only: bool) -> String {
    use std::io::Write;
    let dir = tmpdir();
    let src = dir.join("src");
    fs::create_dir_all(&src).unwrap();
    let file = src.join("main.rl");
    fs::write(&file, saved).unwrap();

    let mut args = vec!["--check-types".to_string()];
    if rl_only {
        args.push("--rl-only".to_string());
    }
    args.push("--overlay".to_string());
    args.push(file.to_str().unwrap().to_string());
    args.push("src".to_string());

    let mut child = Command::new(env!("CARGO_BIN_EXE_rlc"))
        .args(&args)
        .current_dir(&dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to run rlc");
    child
        .stdin
        .take()
        .expect("stdin piped")
        .write_all(buffer.as_bytes())
        .unwrap();
    let out = child.wait_with_output().expect("failed to run rlc");
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

/* ------------------------------------------------------------------ */
/* --overlay / --rl-only: the editor's entry into the typed check      */
/* ------------------------------------------------------------------ */

/// Both flags only make sense while `--check-types` is reporting. Every
/// rejection names the mode that does accept them, so the message is a fix
/// rather than a complaint.
#[test]
fn overlay_and_rl_only_require_check_types() {
    let dir = tmpdir();
    let file = dir.join("a.rl");
    fs::write(&file, "export const n = 1;\n").unwrap();
    let path = file.to_str().unwrap();

    for args in [
        vec!["--overlay", path, path],
        vec!["--rl-only", path],
        vec!["--check", "--rl-only", path],
    ] {
        let out = rlc(&args);
        let err = String::from_utf8_lossy(&out.stderr).into_owned();
        assert!(!out.status.success(), "{args:?} should fail:\n{err}");
        assert!(
            err.contains("--overlay and --rl-only require --check-types"),
            "{args:?}:\n{err}"
        );
    }
}

/// `--types` writes sidecars. Unsaved text must not reach one, and a mode
/// that writes is not one that hides half of what it found.
#[test]
fn overlay_and_rl_only_are_rejected_by_the_writing_mode() {
    let dir = tmpdir();
    let file = dir.join("a.rl");
    fs::write(&file, "export const n = 1;\n").unwrap();
    let path = file.to_str().unwrap();

    for args in [
        vec!["--types", "--overlay", path, path],
        vec!["--types", "--rl-only", path],
    ] {
        let out = rlc(&args);
        let err = String::from_utf8_lossy(&out.stderr).into_owned();
        assert!(!out.status.success(), "{args:?} should fail:\n{err}");
        assert!(
            err.contains("--overlay and --rl-only work with --check-types, not --types"),
            "{args:?}:\n{err}"
        );
    }
}

/// A watch re-reads what it watches; text pinned on stdin would stay the
/// same forever, so the pair has no coherent meaning.
#[test]
fn overlay_does_not_combine_with_watch() {
    let dir = tmpdir();
    let file = dir.join("a.rl");
    fs::write(&file, "export const n = 1;\n").unwrap();
    let path = file.to_str().unwrap();

    let out = rlc(&["--check-types", "--overlay", path, "--watch", path]);
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(!out.status.success(), "{err}");
    assert!(
        err.contains("--overlay does not combine with --watch"),
        "{err}"
    );
}

/// The flag needs a value, and the path it names has to exist — it stands
/// in for a file of the project, so there has to be one.
#[test]
fn overlay_reports_a_missing_value_and_a_missing_file() {
    let out = rlc(&["--check-types", "--overlay"]);
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(!out.status.success(), "{err}");
    assert!(
        err.contains("--overlay requires the path the buffer belongs to"),
        "{err}"
    );

    let dir = tmpdir();
    let file = dir.join("a.rl");
    fs::write(&file, "export const n = 1;\n").unwrap();
    let gone = dir.join("gone.rl");
    let out = rlc(&[
        "--check-types",
        "--overlay",
        gone.to_str().unwrap(),
        file.to_str().unwrap(),
    ]);
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(!out.status.success(), "{err}");
    assert!(err.contains("--overlay"), "{err}");
    assert!(err.contains("gone.rl"), "{err}");
}

/// The overlay is what gets checked: a mutation that exists only in the
/// buffer is reported, and one that exists only on disk is not. This is the
/// whole point — an editor asks about the text it is showing.
#[test]
fn overlay_checks_the_buffer_rather_than_the_saved_file() {
    require_types_toolchain!();
    let saved = "val const saved = new Map<string, number>();\n\
                 saved.set(\"gone\", 1);\n\
                 export const n = saved.size;\n";
    let buffer = "val const edited = new Map<string, number>();\n\
                  edited.delete(\"new\");\n\
                  export const n = edited.size;\n";

    let err = types_stderr_overlay(saved, buffer, true);
    assert!(
        err.contains("cannot call mutating method `delete` through val binding `edited`"),
        "the buffer's mutation should be reported:\n{err}"
    );
    assert!(
        !err.contains("saved"),
        "the saved file's text should not be checked:\n{err}"
    );
    // The position is the buffer's, and the file is named as the user knows
    // it — not as a temporary.
    assert!(err.contains("src/main.rl:2:1:"), "{err}");
}

/// `--rl-only` drops TypeScript's layer and keeps rl's. The editor already
/// has the type errors from its own language server; it would draw them
/// twice otherwise.
#[test]
fn rl_only_keeps_the_rl_layer_and_drops_the_type_layer() {
    require_types_toolchain!();
    let source = "val const scores = new Map<string, number>();\n\
                  scores.set(\"a\", 1);\n\
                  const wrong: number = \"not a number\";\n\
                  export const n = scores.size + wrong;\n";

    let full = types_stderr_overlay(source, source, false);
    assert!(full.contains("ts(2322)"), "{full}");
    assert!(full.contains("cannot call mutating method `set`"), "{full}");

    let rl_only = types_stderr_overlay(source, source, true);
    assert!(
        !rl_only.contains("ts("),
        "no type error should survive --rl-only:\n{rl_only}"
    );
    assert!(
        rl_only.contains("cannot call mutating method `set`"),
        "{rl_only}"
    );
}

/// A `val` mutation is judged by what the receiver *is*, and the overlay
/// keeps the buffer in its own project — so a type that comes from another
/// module of the project still resolves.
#[test]
fn overlay_keeps_the_buffer_in_its_project() {
    require_types_toolchain!();
    let dir = tmpdir();
    let src = dir.join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("store.ts"),
        "export const scores = new Map<string, number>();\n\
         export class Query { set(k: string): Query { return this; } }\n",
    )
    .unwrap();
    let file = src.join("main.rl");
    fs::write(&file, "export const nothing = 0;\n").unwrap();

    let buffer = "import { scores, Query } from \"./store\";\n\
                  val const shared = scores;\n\
                  shared.set(\"a\", 1);\n\
                  val const query = new Query();\n\
                  query.set(\"b\");\n\
                  export const n = shared.size;\n";

    use std::io::Write;
    let mut child = Command::new(env!("CARGO_BIN_EXE_rlc"))
        .args([
            "--check-types",
            "--rl-only",
            "--overlay",
            file.to_str().unwrap(),
            "src",
        ])
        .current_dir(&dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to run rlc");
    child
        .stdin
        .take()
        .expect("stdin piped")
        .write_all(buffer.as_bytes())
        .unwrap();
    let out = child.wait_with_output().expect("failed to run rlc");
    let err = String::from_utf8_lossy(&out.stderr).into_owned();

    // `Map#set` through the imported binding is a built-in mutation …
    assert!(
        err.contains("cannot call mutating method `set` through val binding `shared`"),
        "{err}"
    );
    // … and `Query#set`, which only shares the name, is not.
    assert!(!err.contains("`query`"), "{err}");
}

/// `rlc --server` answers `rlSymbol` without a project or a toolchain: the
/// names it resolves exist only in `.rl` source, so nothing else can.
#[test]
fn the_server_resolves_rl_names_without_a_toolchain() {
    use std::io::Write;
    let dir = tmpdir();
    let file = dir.join("shape.rl");
    let source = "enum Shape { Circle(radius: number), Point }\n\
                  const a = match (s) { Circle(radius) => radius, Point => 0 };\n";
    fs::write(&file, source).unwrap();

    let request = serde_json::json!({
        "id": 1,
        "method": "rlSymbol",
        "params": {
            "path": file.to_string_lossy(),
            "text": source,
            // line 1, on the `Circle` of the match arm
            "position": { "line": 1, "character": 22 },
        },
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_rlc"))
        .arg("--server")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("server starts");
    writeln!(child.stdin.as_mut().unwrap(), "{request}").unwrap();
    drop(child.stdin.take());
    let out = child.wait_with_output().expect("server answers");
    let answer: serde_json::Value =
        serde_json::from_slice(String::from_utf8_lossy(&out.stdout).trim().as_bytes())
            .expect("one JSON line");
    assert_eq!(answer["result"]["kind"], "case");
    assert_eq!(
        answer["result"]["signature"],
        "Shape.Circle(radius: number)"
    );
    // ...and points at the declaration on line 0.
    assert_eq!(answer["result"]["definition"]["range"]["start"]["line"], 0);
}

/* ------------------------------------------------------------------ */
/* typed check without a backend (TASK-124)                            */
/* ------------------------------------------------------------------ */

#[test]
fn a_missing_backend_still_reports_rl_diagnostics() {
    // The TypeScript layer failing to run removes the typed facts, not
    // the pass: rl's own diagnostics are still reported in full, the
    // failure is named, and the exit code stays "could not check" (2).
    let dir = tmpdir();
    fs::write(
        dir.join("a.rl"),
        "enum E { A(x: number), B }\n\
         const v = match (E.A(1)) { A(x) => x, A(x) => 0, B => 1 };\n",
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_rlc"))
        .args(["--check-types", dir.to_str().unwrap()])
        // Point the toolchain at nothing: the backend cannot run.
        .env("RLC_TSGO_API", dir.join("nonexistent-api.js"))
        .env_remove("RLC_TSGO_ROOT")
        .output()
        .expect("failed to run rlc");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("duplicate arm"), "{stderr}");
    assert!(
        stderr.contains("only rl-level diagnostics are shown"),
        "{stderr}"
    );
    fs::remove_dir_all(&dir).ok();
}
