//! Emitted-code and error-reporting tests for the rl → TypeScript transform.

use rlc::{compile, Options};

fn ok(src: &str) -> String {
    compile(src, &Options::default()).expect("compile failed")
}

fn err(src: &str) -> rlc::CompileError {
    compile(src, &Options::default()).expect_err("expected a compile error")
}

/* ------------------------------------------------------------------ */
/* enum                                                                */
/* ------------------------------------------------------------------ */

#[test]
fn enum_with_payload_emits_union_type_and_constructors() {
    let out = ok(r#"
enum Shape {
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
fn unit_only_enum_is_a_plain_typescript_enum() {
    // No payload case and no generics → this is TypeScript's own enum and
    // must pass through byte for byte.
    let src = "enum Color { Red, Green, Blue }\n";
    assert_eq!(ok(src), src);
    let src = "export enum Color { Red, Green, Blue }\n";
    assert_eq!(ok(src), src);
}

#[test]
fn empty_parens_case_forces_rl_enum() {
    // A unit-only enum can opt into rl semantics by giving one case parens.
    let out = ok("enum Status { Active(), Inactive }\n");
    assert!(out.contains("type Status ="));
    assert!(out.contains("Active: (): Status => ({ kind: \"Active\" })"));
    assert!(out.contains("Inactive: { kind: \"Inactive\" } as const"));
}

#[test]
fn generics_force_rl_enum() {
    let out = ok("enum Pair<T> { First, Second }\n");
    assert!(out.contains("type Pair<T> ="));
}

#[test]
fn enum_export_prefix_on_both_declarations() {
    let out = ok("export enum Shape { Circle(radius: number), Point }");
    assert!(out.contains("export type Shape ="));
    assert!(out.contains("export const Shape = {"));
}

#[test]
fn enum_generics_flow_into_constructors() {
    let out = ok("enum Option<T> {\n  Some(value: T),\n  None,\n}\n");
    assert!(out.contains("type Option<T> ="));
    assert!(out.contains("Some: <T>(value: T): Option<T> => ({ kind: \"Some\", value })"));
}

#[test]
fn enum_duplicate_case_is_error_with_position() {
    let e = err("const a = 1;\nenum X { A(v: number), A }\n");
    assert!(e.message.contains("duplicate case \"A\""), "{}", e.message);
    assert_eq!((e.line, e.col), (2, 24));
}

#[test]
fn enum_complex_field_types() {
    let out = ok(r#"
enum Node {
  Leaf(entries: Map<string, number[]>),
  Branch(children: Array<string>, meta: { tag: string, depth: number }),
}
"#);
    assert!(out.contains("entries: Map<string, number[]>"));
    assert!(out.contains("meta: { tag: string, depth: number }"));
}

#[test]
fn enum_invalid_field_type_is_rejected_by_swc_with_position() {
    let e = err("enum X {\n  A(f: number number),\n}\n");
    assert!(e.message.contains("invalid type for field `f`"), "{}", e.message);
    assert_eq!(e.line, 2);
    assert_eq!(e.col, 8); // points at the start of the type annotation
}

#[test]
fn enum_invalid_field_type_passes_without_verify() {
    // Without swc validation the construct still parses; the broken type is
    // carried into the output (where tsc would catch it).
    let opts = Options { verify: false, ..Options::default() };
    let out = compile("enum X {\n  A(f: number number),\n}\n", &opts).unwrap();
    assert!(out.contains("f: number number"));
}

/* ------------------------------------------------------------------ */
/* match                                                               */
/* ------------------------------------------------------------------ */

#[test]
fn match_compiles_to_switch_with_runtime_guard_only() {
    let out = ok(r#"
enum Shape { Circle(radius: number), Point }
const area = match (shape) {
  Circle(radius) => 3.14 * radius * radius,
  Point => 0,
};
"#);
    assert!(out.contains("switch ($rl_m.kind)"));
    assert!(out.contains("case \"Circle\": { const { radius } = $rl_m; return (3.14 * radius * radius); }"));
    assert!(out.contains("case \"Point\": { return (0); }"));
    // The output is plain TypeScript: a runtime guard, no type-level tricks.
    assert!(out.contains("default: { throw new Error(\"rl match: unexpected case \" + JSON.stringify($rl_m)); }"));
    assert!(!out.contains("never"));
}

#[test]
fn match_wildcard_becomes_default() {
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
    let e = err("const s = `${match (x) { A => 1, A => 2 }}`;\n");
    assert!(e.message.contains("duplicate arm"), "{}", e.message);
    assert_eq!((e.line, e.col), (1, 34));
}

/* ------------------------------------------------------------------ */
/* exhaustiveness — an rlc error, not a tsc error                      */
/* ------------------------------------------------------------------ */

#[test]
fn non_exhaustive_match_is_an_rlc_error_with_position() {
    let e = err(r#"enum Shape { Circle(radius: number), Rect(w: number, h: number), Point }
const f = (s: Shape) => match (s) {
  Circle(radius) => radius,
  Point => 0,
};
"#);
    assert!(
        e.message.contains("match on enum Shape is not exhaustive: missing \"Rect\""),
        "{}",
        e.message
    );
    assert_eq!((e.line, e.col), (2, 25)); // points at the `match` keyword
}

#[test]
fn exhaustive_match_compiles() {
    let out = ok(r#"
enum Shape { Circle(radius: number), Rect(w: number, h: number), Point }
const f = (s: Shape) => match (s) {
  Circle(radius) => radius,
  Rect(w, h) => w * h,
  Point => 0,
};
"#);
    assert!(out.contains("case \"Rect\""));
}

#[test]
fn wildcard_satisfies_exhaustiveness() {
    ok(r#"
enum Shape { Circle(radius: number), Rect(w: number, h: number), Point }
const f = (s: Shape) => match (s) {
  Circle(radius) => radius,
  _ => 0,
};
"#);
}

#[test]
fn exhaustiveness_is_declaration_order_independent() {
    // match appears before the enum declaration — still checked.
    let e = err(r#"const f = (s: Shape) => match (s) {
  Circle(radius) => radius,
};
enum Shape { Circle(radius: number), Point }
"#);
    assert!(e.message.contains("missing \"Point\""), "{}", e.message);
}

#[test]
fn match_on_unknown_tags_is_not_checked() {
    // Hand-written unions / imported enums: rlc has no type info, so no
    // exhaustiveness check — the runtime guard still protects.
    let out = ok(r#"
type AppEvent = { kind: "click"; x: number } | { kind: "key"; code: string };
const f = (e: AppEvent) => match (e) {
  click(x) => x,
};
"#);
    assert!(out.contains("case \"click\""));
}

#[test]
fn missing_cases_are_all_listed() {
    let e = err(r#"enum Dir { North, South, East, West(deg: number) }
const f = (d: Dir) => match (d) { North => 1 };
"#);
    assert!(e.message.contains("\"East\""), "{}", e.message);
    assert!(e.message.contains("\"South\""), "{}", e.message);
    assert!(e.message.contains("\"West\""), "{}", e.message);
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
    let e = compile("const r = match (x) { A => 1, A => 2 };", &opts).expect_err("expected error");
    assert_eq!(e.to_string(), "demo.rl:1:31: match: duplicate arm \"A\"");
}
