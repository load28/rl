//! The standard library module (`STD_SOURCE`): it must itself honor the
//! passthrough contract (it is plain TypeScript) and stay byte-consistent
//! with the values the corresponding rl enums compile to.

use rlc::{Options, STD_SOURCE, compile};

#[test]
fn std_source_is_plain_typescript_and_passes_through() {
    // Compiling the module doubles as an swc parse check (verify is on).
    let out = compile(STD_SOURCE, &Options::default()).expect("std module failed to compile");
    assert_eq!(out, STD_SOURCE);
}

#[test]
fn std_option_matches_rl_enum_emission() {
    // The std module must declare Option in exactly the shape the equivalent
    // rl enum compiles to — otherwise match semantics and the built-in
    // exhaustiveness check drift apart from the real declarations.
    let emitted = compile(
        "export enum Option<T> {\n  Some(value: T),\n  None,\n}\n",
        &Options::default(),
    )
    .unwrap();
    for line in emitted.lines().filter(|l| !l.trim().is_empty()) {
        assert!(
            STD_SOURCE.contains(line),
            "std module drifted from rl enum emission; missing line: {line}"
        );
    }
}

#[test]
fn std_result_matches_rl_enum_value_shape() {
    // `Result`'s constructors deliberately diverge from enum emission: each
    // one is typed by the variant it actually builds (`Ok<T>` / `Err<E>`)
    // instead of the whole `Result<T, E>` (TASK-065). What must not diverge
    // is the *value* shape — the union arms and the objects the constructors
    // build — since that is what `match` and the built-in exhaustiveness
    // check compile against.
    let emitted = compile(
        "export enum Result<T, E> {\n  Ok(value: T),\n  Err(error: E),\n}\n",
        &Options::default(),
    )
    .unwrap();
    let shapes: Vec<&str> = emitted
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if let Some(arm) = line.strip_prefix("| ") {
                Some(arm.trim_end_matches(';')) // union arm: { kind: "Ok"; value: T }
            } else {
                // constructor body: { kind: "Ok", value }
                line.split_once("=> (")
                    .map(|(_, body)| body.trim_end_matches("),"))
            }
        })
        .collect();
    assert_eq!(shapes.len(), 4, "unexpected enum emission:\n{emitted}");
    for shape in shapes {
        assert!(
            STD_SOURCE.contains(shape),
            "std Result drifted from rl enum value shape; missing: {shape}"
        );
    }
}
