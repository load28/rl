//! CLI contract tests for `rlc help` — the embedded language & workflow
//! reference (docs/ai/rl.md served by topic).

use std::process::Command;

fn rlc(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rlc"))
        .args(args)
        .output()
        .expect("failed to run rlc")
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
