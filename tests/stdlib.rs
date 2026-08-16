//! The standard library module (`STD_SOURCE`): it must itself honor the
//! passthrough contract (it is plain TypeScript) and stay byte-consistent
//! with what the corresponding rl enums compile to.

use rlc::{Options, STD_SOURCE, compile};

#[test]
fn std_source_is_plain_typescript_and_passes_through() {
    // Compiling the module doubles as an swc parse check (verify is on).
    let out = compile(STD_SOURCE, &Options::default()).expect("std module failed to compile");
    assert_eq!(out, STD_SOURCE);
}

#[test]
fn std_declarations_match_rl_enum_emission() {
    // The std module must declare Option/Result in exactly the shape the
    // equivalent rl enums compile to — otherwise match semantics and the
    // built-in exhaustiveness check drift apart from the real declarations.
    let emitted = compile(
        "export enum Option<T> {\n  Some(value: T),\n  None,\n}\nexport enum Result<T, E> {\n  Ok(value: T),\n  Err(error: E),\n}\n",
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
