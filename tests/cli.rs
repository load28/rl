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
