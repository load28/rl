//! Emitted-code and error-reporting tests for the rl → TypeScript transform.

use rlc::{compile, Options};

fn ok(src: &str) -> String {
    compile(src, &Options::default()).expect("compile failed")
}

fn err(src: &str) -> rlc::CompileError {
    compile(src, &Options::default()).expect_err("expected a compile error")
}

/* ------------------------------------------------------------------ */
/* variant                                                             */
/* ------------------------------------------------------------------ */

#[test]
fn variant_emits_union_type_and_constructors() {
    let out = ok(r#"
variant Shape {
  Circle(radius: number),
  Rect(width: number, height: number),
  Point,
}
"#);
    assert!(out.contains("type Shape ="));
    assert!(out.contains("{ kind: \"Circle\"; radius: number }"));
    assert!(out.contains("{ kind: \"Rect\"; width: number; height: number }"));
    assert!(out.contains("{ kind: \"Point\" }"));
    assert!(out.contains("const Shape = {"));
    assert!(out.contains("Circle: (radius: number): Shape => ({ kind: \"Circle\", radius })"));
    assert!(out.contains("Point: { kind: \"Point\" } as const"));
}

#[test]
fn variant_export_prefix_on_both_declarations() {
    let out = ok("export variant Color { Red, Green, Blue }");
    assert!(out.contains("export type Color ="));
    assert!(out.contains("export const Color = {"));
}

#[test]
fn variant_generics_flow_into_constructors() {
    let out = ok("variant Option<T> {\n  Some(value: T),\n  None,\n}\n");
    assert!(out.contains("type Option<T> ="));
    assert!(out.contains("Some: <T>(value: T): Option<T> => ({ kind: \"Some\", value })"));
}

#[test]
fn variant_duplicate_case_is_error_with_position() {
    let e = err("const a = 1;\nvariant X { A, A }\n");
    assert!(e.message.contains("duplicate case \"A\""), "{}", e.message);
    assert_eq!((e.line, e.col), (2, 16));
}

#[test]
fn variant_complex_field_types() {
    let out = ok(r#"
variant Node {
  Leaf(entries: Map<string, number[]>),
  Branch(children: Array<string>, meta: { tag: string, depth: number }),
}
"#);
    assert!(out.contains("entries: Map<string, number[]>"));
    assert!(out.contains("meta: { tag: string, depth: number }"));
}

#[test]
fn variant_invalid_field_type_is_rejected_by_swc_with_position() {
    let e = err("variant X {\n  A(f: number number),\n}\n");
    assert!(e.message.contains("invalid type for field `f`"), "{}", e.message);
    assert_eq!(e.line, 2);
    assert_eq!(e.col, 8); // points at the start of the type annotation
}

#[test]
fn variant_invalid_field_type_passes_without_verify() {
    // Without swc validation the construct still parses; the broken type is
    // carried into the output (where tsc would catch it).
    let opts = Options { verify: false, ..Options::default() };
    let out = compile("variant X {\n  A(f: number number),\n}\n", &opts).unwrap();
    assert!(out.contains("f: number number"));
}

/* ------------------------------------------------------------------ */
/* match                                                               */
/* ------------------------------------------------------------------ */

#[test]
fn match_compiles_to_switch_with_never_default() {
    let out = ok(r#"
const area = match (shape) {
  Circle(radius) => 3.14 * radius * radius,
  Point => 0,
};
"#);
    assert!(out.contains("switch ($rl_m.kind)"));
    assert!(out.contains("case \"Circle\": { const { radius } = $rl_m; return (3.14 * radius * radius); }"));
    assert!(out.contains("case \"Point\": { return (0); }"));
    assert!(out.contains("const $rl_never: never = $rl_m;"));
}

#[test]
fn match_wildcard_becomes_default_without_never_check() {
    let out = ok("const r = match (x) { A => 1, _ => 0 };");
    assert!(out.contains("default: { return (0); }"));
    assert!(!out.contains("never"));
}

#[test]
fn match_wildcard_must_be_last_with_position() {
    let e = err("const r = match (x) { _ => 0, A => 1 };");
    assert!(e.message.contains("must be the last arm"), "{}", e.message);
    assert_eq!((e.line, e.col), (1, 23));
}

#[test]
fn match_duplicate_arm_is_error() {
    let e = err("const r = match (x) { A => 1, A => 2 };");
    assert!(e.message.contains("duplicate arm \"A\""), "{}", e.message);
    assert_eq!((e.line, e.col), (1, 31));
}

#[test]
fn match_await_arm_produces_awaited_async_iife() {
    let out = ok("async function f(x: T) { return match (x) { A(url) => await fetch(url), _ => null }; }");
    assert!(out.contains("(await (async () => {"));
}

#[test]
fn match_nested_compiles_recursively() {
    let out = ok(r#"
const r = match (a) {
  X(inner) => match (inner) { Y => 1, _ => 2 },
  _ => 0,
};
"#);
    assert_eq!(out.matches("switch ($rl_m.kind)").count(), 2);
}

#[test]
fn match_inside_template_interpolation() {
    let out = ok("const s = `v=${match (x) { A => 1, _ => 0 }}`;");
    assert!(out.contains("switch ($rl_m.kind)"));
}

#[test]
fn match_binding_alias_and_block_body() {
    let out = ok(r#"
const r = match (m) {
  Move(x: px, y: py) => {
    const sum = px + py;
    return sum;
  },
  _ => 0,
};
"#);
    assert!(out.contains("const { x: px, y: py } = $rl_m;"));
    assert!(out.contains("break; }"));
}

#[test]
fn error_position_reported_inside_template_interpolation() {
    // The absolute-offset recursion keeps positions exact even for errors
    // nested inside template literals — the JS implementation could not.
    let e = err("const s = `${match (x) { A => 1, A => 2 }}`;\n");
    assert!(e.message.contains("duplicate arm"), "{}", e.message);
    assert_eq!((e.line, e.col), (1, 34));
}

/* ------------------------------------------------------------------ */
/* swc output verification                                             */
/* ------------------------------------------------------------------ */

#[test]
fn verify_rejects_invalid_passthrough_typescript() {
    let e = err("const = 5;\n");
    assert!(e.message.contains("generated TypeScript failed to parse"), "{}", e.message);
}

#[test]
fn no_verify_passes_invalid_typescript_through() {
    let opts = Options { verify: false, ..Options::default() };
    let out = compile("const = 5;\n", &opts).unwrap();
    assert_eq!(out, "const = 5;\n");
}

#[test]
fn filename_appears_in_error_display() {
    let opts = Options { filename: Some("demo.rl"), ..Options::default() };
    let e = compile("variant X { A, A }", &opts).expect_err("expected error");
    assert_eq!(e.to_string(), "demo.rl:1:16: variant X: duplicate case \"A\"");
}
