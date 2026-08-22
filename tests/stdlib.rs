//! Standard-library package contracts.

use rlc::{Options, STD_OPTION_SOURCE, STD_RESULT_SOURCE, STD_TYPES_SOURCE, compile};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn std_modules_are_plain_typescript_and_pass_through() {
    for source in [STD_TYPES_SOURCE, STD_OPTION_SOURCE, STD_RESULT_SOURCE] {
        let out = compile(source, &Options::default()).expect("std module failed to compile");
        assert_eq!(out, source);
    }
}

#[test]
fn std_runtime_members_are_independent_esm_exports() {
    for (source, members) in [
        (
            STD_OPTION_SOURCE,
            &["Some", "None", "map", "andThen", "mapP", "collect"][..],
        ),
        (
            STD_RESULT_SOURCE,
            &["Ok", "Err", "map", "andThen", "mapP", "collect"][..],
        ),
    ] {
        for member in members {
            assert!(
                source.contains(&format!("export const {member}")),
                "missing independent export {member}"
            );
        }
        assert!(!source.contains("export const Option ="));
        assert!(!source.contains("export const Result ="));
    }
}

#[test]
fn std_value_shapes_match_builtin_enums() {
    for shape in [
        r#"{ kind: "Some"; value: T }"#,
        r#"{ kind: "None" }"#,
        r#"({ kind: "Some", value })"#,
        r#"{ kind: "Ok"; value: T }"#,
        r#"{ kind: "Err"; error: E }"#,
        r#"({ kind: "Ok", value })"#,
        r#"({ kind: "Err", error })"#,
    ] {
        assert!(
            STD_OPTION_SOURCE.contains(shape) || STD_RESULT_SOURCE.contains(shape),
            "standard-library value shape drifted: {shape}"
        );
    }
}

#[test]
fn std_type_entry_point_is_type_only() {
    assert!(STD_TYPES_SOURCE.contains("export type { TOption }"));
    assert!(STD_TYPES_SOURCE.contains("export type { TErr, TErrorOf, TOk, TResult }"));
    assert!(!STD_TYPES_SOURCE.contains("export const"));
}

#[test]
fn namespace_import_is_pruned_by_a_real_bundler() {
    let Some(rolldown) = rolldown_command() else {
        eprintln!("skipping bundler pruning test: rolldown is not installed");
        return;
    };
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock predates Unix epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("rl-stdlib-tree-shaking-{nonce}"));
    fs::create_dir_all(&root).expect("failed to create bundle fixture directory");
    fs::write(root.join("option.ts"), STD_OPTION_SOURCE).expect("failed to write option module");
    fs::write(root.join("result.ts"), STD_RESULT_SOURCE).expect("failed to write result module");
    fs::write(
        root.join("entry.ts"),
        "import * as Option from './option.ts';\nconsole.log(Option.Some(1));\n",
    )
    .expect("failed to write bundle entry");

    let output_path = root.join("bundle.js");
    let output = Command::new(rolldown)
        .arg(root.join("entry.ts"))
        .args(["--file", output_path.to_str().expect("non-UTF-8 temp path")])
        .arg("--minify")
        .output()
        .expect("failed to run rolldown");
    assert!(
        output.status.success(),
        "rolldown failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let bundle = fs::read_to_string(&output_path).expect("failed to read bundle");
    fs::remove_dir_all(&root).expect("failed to remove bundle fixture directory");

    assert!(
        bundle.contains("Some"),
        "used constructor was removed: {bundle}"
    );
    assert!(
        !bundle.contains("None"),
        "unused Option exports remained in the bundle: {bundle}"
    );
}

fn rolldown_command() -> Option<PathBuf> {
    let local = Path::new("website/node_modules/.bin/rolldown");
    if local.is_file() {
        return Some(local.to_path_buf());
    }
    Command::new("rolldown")
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|_| PathBuf::from("rolldown"))
}
