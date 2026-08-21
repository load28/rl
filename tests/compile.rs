//! Emitted-code and error-reporting tests for the rl → TypeScript transform.

use rlc::{Options, compile};

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
    assert!(
        e.message.contains("invalid type for field `f`"),
        "{}",
        e.message
    );
    assert_eq!(e.line, 2);
    assert_eq!(e.col, 8); // points at the start of the type annotation
}

#[test]
fn enum_invalid_field_type_passes_without_verify() {
    // Without swc validation the construct still parses; the broken type is
    // carried into the output (where tsc would catch it).
    let opts = Options {
        verify: false,
        ..Options::default()
    };
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
    assert!(out.contains(
        "case \"Circle\": { const { radius } = $rl_m; return (3.14 * radius * radius); }"
    ));
    assert!(out.contains("case \"Point\": { return (0); }"));
    // The output is plain TypeScript: a runtime guard, no type-level tricks.
    assert!(out.contains(
        "default: { throw new Error(\"rl match: unexpected case \" + JSON.stringify($rl_m)); }"
    ));
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
    let out = ok(
        "async function f(x: T) { return match (x) { A(url) => await fetch(url), _ => null }; }",
    );
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
/* or-patterns                                                         */
/* ------------------------------------------------------------------ */

#[test]
fn or_pattern_emits_fallthrough_cases() {
    let out = ok(r#"
enum Key { Enter(), Escape, Tab, Char(ch: string) }
const action = match (key) {
  Enter => "submit",
  Escape | Tab => "cancel",
  Char(ch) => "type:" + ch,
};
"#);
    assert!(
        out.contains("case \"Escape\": case \"Tab\": { return (\"cancel\"); }"),
        "{out}"
    );
}

#[test]
fn or_pattern_with_identical_bindings_shares_destructuring() {
    let out = ok("const r = match (x) { A(v) | B(v) => v, _ => 0 };");
    assert!(
        out.contains("case \"A\": case \"B\": { const { v } = $rl_m; return (v); }"),
        "{out}"
    );
}

#[test]
fn or_pattern_binding_order_is_insensitive() {
    let out = ok("const r = match (p) { A(x, y) | B(y, x) => x + y, _ => 0 };");
    assert!(
        out.contains("case \"A\": case \"B\": { const { x, y } = $rl_m;"),
        "{out}"
    );
}

#[test]
fn or_pattern_counts_for_exhaustiveness() {
    ok(r#"
enum Dir { North(), South, East, West }
const f = (d: Dir) => match (d) {
  North | South => 1,
  East | West => 2,
};
"#);
    let e = err(r#"
enum Dir { North(), South, East, West }
const f = (d: Dir) => match (d) {
  North | South => 1,
  East => 2,
};
"#);
    assert!(e.message.contains("missing \"West\""), "{}", e.message);
}

#[test]
fn or_pattern_duplicate_tag_is_error() {
    // duplicate inside one arm
    let e = err("const r = match (x) { A | A => 1, _ => 0 };");
    assert!(e.message.contains("duplicate arm \"A\""), "{}", e.message);
    // duplicate across arms
    let e = err("const r = match (x) { A | B => 1, B => 2, _ => 0 };");
    assert!(e.message.contains("duplicate arm \"B\""), "{}", e.message);
}

#[test]
fn or_pattern_binding_mismatch_is_error() {
    let e = err("const r = match (x) { A(v) | B(w) => v, _ => 0 };");
    assert!(
        e.message
            .contains("or-pattern alternatives must bind the same names"),
        "{}",
        e.message
    );
    // ... and the message names the binding that differs.
    assert!(
        e.message
            .contains("`v` is bound in `A(...)` but not in `B(...)`"),
        "{}",
        e.message
    );
    assert_eq!((e.line, e.col), (1, 30)); // points at the offending alternative

    // an alias changes the bound name, so it must match too
    let e = err("const r = match (x) { A(v) | B(v: w) => w, _ => 0 };");
    assert!(
        e.message
            .contains("`v` is bound in `A(...)` but not in `B(...)`"),
        "{}",
        e.message
    );

    // a binding-free alternative cannot pair with a binding one
    let e = err("const r = match (x) { A | B(v) => 1, _ => 0 };");
    assert!(
        e.message
            .contains("`v` is bound in `B(...)` but not in `A(...)`"),
        "{}",
        e.message
    );

    // an arity mismatch names the extra binding
    let e = err("const r = match (x) { A(v) | B(v, w) => v, _ => 0 };");
    assert!(
        e.message
            .contains("`w` is bound in `B(...)` but not in `A(...)`"),
        "{}",
        e.message
    );

    // a wildcard-looking `_` is a binding like any other
    let e = err("const r = match (x) { A(v) | B(_) => v, _ => 0 };");
    assert!(
        e.message
            .contains("`v` is bound in `A(...)` but not in `B(...)`"),
        "{}",
        e.message
    );

    // same names, different fields: the pairing is named
    let e = err("const r = match (x) { A(v) | B(w: v) => v, _ => 0 };");
    assert!(
        e.message
            .contains("`v` is bound from field `v` in `A(...)` but from field `w` in `B(...)`"),
        "{}",
        e.message
    );
}

#[test]
fn or_pattern_double_pipe_is_not_rl_syntax() {
    // `A || B` is not an or-pattern; the candidate fails to parse and the
    // (invalid-TS) text passes through to the output self-check.
    let e = err("const r = match (x) { A || B => 1 };");
    // Reported where the text that failed is — the `match` that stayed
    // verbatim — not at a position in the generated module.
    assert!(
        e.message
            .contains("`match` here did not parse as an rl `match`"),
        "{}",
        e.message
    );
    assert_eq!((e.line, e.col), (1, 11));
    assert_eq!((e.end_line, e.end_col), (1, 16));
}

/* ------------------------------------------------------------------ */
/* guards                                                              */
/* ------------------------------------------------------------------ */

#[test]
fn guarded_match_compiles_to_if_chain() {
    let out = ok(r#"
enum Score { Graded(points: number), Pending }
const grade = match (s) {
  Graded(points) if points >= 90 => "A",
  Graded(points) => "F",
  Pending => "-",
};
"#);
    assert!(!out.contains("switch ("), "{out}");
    assert!(
        out.contains(
            "if ($rl_m.kind === \"Graded\") { const { points } = $rl_m; if ((points >= 90)) return (\"A\"); }"
        ),
        "{out}"
    );
    assert!(
        out.contains(
            "if ($rl_m.kind === \"Graded\") { const { points } = $rl_m; return (\"F\"); }"
        ),
        "{out}"
    );
    // the same fail-fast runtime guard as the switch emission
    assert!(
        out.contains("throw new Error(\"rl match: unexpected case \" + JSON.stringify($rl_m));"),
        "{out}"
    );
}

#[test]
fn guard_free_match_still_emits_switch() {
    let out = ok("const r = match (x) { A => 1, _ => 0 };");
    assert!(out.contains("switch ($rl_m.kind)"), "{out}");
    assert!(!out.contains("$rl_b"), "{out}");
}

#[test]
fn repeated_guarded_tags_are_allowed() {
    let out =
        ok("const r = match (x) { A(v) if v > 9 => 2, A(v) if v > 0 => 1, A => 0, _ => -1 };");
    assert_eq!(out.matches("$rl_m.kind === \"A\"").count(), 3, "{out}");
}

#[test]
fn guard_after_unguarded_same_tag_is_duplicate() {
    // the unguarded A already covers the tag, so the guarded arm is unreachable
    let e = err("const r = match (x) { A => 1, A if c => 2, _ => 0 };");
    assert!(e.message.contains("duplicate arm \"A\""), "{}", e.message);
}

#[test]
fn guarded_arms_do_not_satisfy_exhaustiveness() {
    let e = err(
        "const f = (o: Option<number>) => match (o) { Some(value) if value > 0 => value, None => 0 };",
    );
    assert!(
        e.message
            .contains("match on built-in enum Option is not exhaustive: missing \"Some\""),
        "{}",
        e.message
    );
}

#[test]
fn fully_guarded_match_is_not_exhaustive() {
    // guarded tags still identify the enum — they just cover nothing
    let e =
        err("const f = (o: Option<number>) => match (o) { Some(value) if value > 0 => value };");
    assert!(e.message.contains("\"None\""), "{}", e.message);
    assert!(e.message.contains("\"Some\""), "{}", e.message);
}

#[test]
fn guard_with_or_pattern_emits_combined_condition() {
    let out = ok("const r = match (x) { A(v) | B(v) if v > 0 => v, _ => 0 };");
    assert!(
        out.contains(
            "if ($rl_m.kind === \"A\" || $rl_m.kind === \"B\") { const { v } = $rl_m; if ((v > 0)) return (v); }"
        ),
        "{out}"
    );
}

#[test]
fn guarded_block_body_uses_labeled_break() {
    let out = ok("const r = match (x) { A(v) if v > 0 => { log(v); }, _ => 0 };");
    assert!(out.contains("$rl_b: {"), "{out}");
    assert!(out.contains("break $rl_b;"), "{out}");
}

#[test]
fn await_in_guard_makes_match_async() {
    let out = ok(
        "async function f(x: T) { return match (x) { A(u) if await allowed(u) => 1, _ => 0 }; }",
    );
    assert!(out.contains("(await (async () => {"), "{out}");
}

#[test]
fn wildcard_with_guard_is_not_rl_syntax() {
    // `_ if ...` does not parse as an rl match; the (invalid-TS) text passes
    // through and the output self-check reports it.
    let e = err("const r = match (x) { A => 1, _ if c => 0 };");
    assert!(
        e.message
            .contains("`match` here did not parse as an rl `match`"),
        "{}",
        e.message
    );
    assert_eq!((e.line, e.col), (1, 11));
}

#[test]
fn try_inside_a_function_inside_a_guard_is_allowed() {
    // Rust's `?` inside a closure: the emitted `return` exits the arrow
    // the user wrote, not the match's IIFE — placement is a flow fact.
    let out = ok(
        "const r = match (x) {\n  A(v) if run(() => { try g(); return true; }) => v,\n  _ => 0,\n};\n",
    );
    assert!(
        out.contains("if ($rl_t0.kind !== \"Ok\") return $rl_t0;"),
        "{out}"
    );
}

/* ------------------------------------------------------------------ */
/* exhaustiveness — an rlc error, not a tsc error                      */
/* ------------------------------------------------------------------ */

#[test]
fn non_exhaustive_match_is_an_rlc_error_with_position() {
    let e = err(
        r#"enum Shape { Circle(radius: number), Rect(w: number, h: number), Point }
const f = (s: Shape) => match (s) {
  Circle(radius) => radius,
  Point => 0,
};
"#,
    );
    assert!(
        e.message
            .contains("match on enum Shape is not exhaustive: missing \"Rect\""),
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
fn match_on_builtin_option_is_exhaustiveness_checked() {
    // Option/Result are built-in enums: checked without a local declaration.
    let e = err("const f = (o: Option<number>) => match (o) { Some(value) => value };\n");
    assert!(
        e.message
            .contains("match on built-in enum Option is not exhaustive: missing \"None\""),
        "{}",
        e.message
    );
}

#[test]
fn match_on_builtin_result_is_exhaustiveness_checked() {
    let e = err("const f = (r: Result<number, string>) => match (r) { Err(error) => error };\n");
    assert!(
        e.message
            .contains("match on built-in enum Result is not exhaustive: missing \"Ok\""),
        "{}",
        e.message
    );
}

#[test]
fn full_match_on_builtin_enums_compiles() {
    let out = ok(r#"
const f = (o: Option<number>) => match (o) { Some(value) => value, None => 0 };
const g = (r: Result<number, string>) => match (r) { Ok(value) => value, Err(error) => error.length };
"#);
    assert!(out.contains("case \"Some\""));
    assert!(out.contains("case \"Err\""));
}

#[test]
fn wildcard_exempts_builtin_exhaustiveness() {
    ok("const f = (o: Option<number>) => match (o) { Some(value) => value, _ => 0 };\n");
}

#[test]
fn local_enum_shadows_builtin() {
    // A file-local rl enum named Option replaces the built-in for this file.
    let e =
        err("enum Option { Some(), Stale }\nconst f = (o: Option) => match (o) { Some => 1 };\n");
    assert!(
        e.message
            .contains("match on enum Option is not exhaustive: missing \"Stale\""),
        "{}",
        e.message
    );
    assert!(!e.message.contains("built-in"), "{}", e.message);
}

#[test]
fn a_candidate_the_arms_satisfy_makes_the_match_exhaustive() {
    // Two enums contain every arm tag. The arms cover `Small` completely,
    // so nothing is missing — the check names an enum only when *no*
    // candidate is satisfied, and then the one left fewest cases.
    ok(
        "enum Big { A(s: string), B, C }\nenum Small { A(s: string), B }\nconst f = (v: Small) => match (v) { A(s) => s, B => \"b\" };\n",
    );

    let e = err(
        "enum Big { A(s: string), B, C, D }\nenum Small { A(s: string), B, C }\nconst f = (v: Small) => match (v) { A(s) => s, B => \"b\" };\n",
    );
    assert!(
        e.message
            .contains("match on enum Small is not exhaustive: missing \"C\""),
        "{}",
        e.message
    );
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
/* try — Rust-style error propagation                                  */
/* ------------------------------------------------------------------ */

#[test]
fn try_decl_emits_early_return_and_bind() {
    let out = ok("function f(): X {\n  const n = try g();\n  return h(n);\n}\n");
    assert!(
        out.contains(
            "const $rl_t0 = (g()); if ($rl_t0.kind !== \"Ok\") return $rl_t0; const n = $rl_t0.value;"
        ),
        "{out}"
    );
}

#[test]
fn try_bare_statement_emits_early_return_only() {
    let out = ok("function f(): X {\n  try g();\n  return h();\n}\n");
    assert!(
        out.contains("const $rl_t0 = (g()); if ($rl_t0.kind !== \"Ok\") return $rl_t0;"),
        "{out}"
    );
    assert!(!out.contains("$rl_t0.value"), "{out}");
}

#[test]
fn try_temporaries_are_unique_and_keep_declaration_keyword() {
    let out = ok(
        "function f(): X {\n  let a: number = try g();\n  var b = try h(a);\n  return k(b);\n}\n",
    );
    assert!(out.contains("let a: number = $rl_t0.value;"), "{out}");
    assert!(out.contains("var b = $rl_t1.value;"), "{out}");
}

#[test]
fn try_destructuring_binding_is_kept_verbatim() {
    let out = ok("function f(): X {\n  const { a, b } = try g();\n  return a + b;\n}\n");
    assert!(out.contains("const { a, b } = $rl_t0.value;"), "{out}");
}

#[test]
fn try_expression_may_contain_a_match() {
    let out = ok(
        "function f(): X {\n  const x = try match (m) { Ok(value) => wrap(value), Err(error) => rewrap(error) };\n  return x;\n}\n",
    );
    assert!(out.contains("const $rl_t0 = ("), "{out}");
    assert!(out.contains("switch ($rl_m.kind)"), "{out}");
}

#[test]
fn try_without_semicolon_is_not_recognized() {
    // No terminating `;` → not rl syntax; the (invalid-TS) source passes
    // through and the output self-check reports it.
    let e = err("function f(): X {\n  const n = try g()\n  return h(n);\n}\n");
    // The `try` that did not parse is the thing to look at, and the
    // message says why the output no longer parses.
    assert!(
        e.message
            .contains("`try` here did not parse as an rl `try`"),
        "{}",
        e.message
    );
    assert_eq!((e.line, e.col), (2, 13));
    assert_eq!((e.end_line, e.end_col), (2, 16));
}

#[test]
fn try_inside_match_arm_is_an_error() {
    // Directly in an arm's statement stream the emitted `return` would
    // exit the switch IIFE — the match would *evaluate to* the `Err`
    // instead of propagating it.
    let e = err(
        "const x = match (r) {\n  Ok(value) => { const y = try f(value); return y; },\n  Err(error) => fallback(error),\n};\n",
    );
    assert!(
        e.message.contains("`try` cannot be used here"),
        "{}",
        e.message
    );
    // The propagation, not the declaration it is written in.
    assert_eq!((e.line, e.col), (2, 28));
    assert_eq!((e.end_line, e.end_col), (2, 40));
}

#[test]
fn try_at_module_top_level_is_an_error() {
    // The lowering's `return` would have no function to exit — before the
    // flow answer this fell through to the output self-check's "invalid
    // TypeScript or an rlc bug" backstop.
    let e = err("function f(): void {}\ntry g();\n");
    assert!(
        e.message.contains("`try` must be inside a function"),
        "{}",
        e.message
    );
    assert_eq!((e.line, e.col), (2, 1));

    let e = err("const x = try g();\n");
    assert!(
        e.message.contains("`try` must be inside a function"),
        "{}",
        e.message
    );

    // A namespace body is not a function body either.
    let e = err("namespace N {\n  try g();\n}\n");
    assert!(
        e.message.contains("`try` must be inside a function"),
        "{}",
        e.message
    );
}

#[test]
fn try_inside_a_function_inside_a_scrutinee_is_allowed() {
    let out = ok(
        "const x = match (run(() => { try g(); return h(); })) {\n  Ok(value) => value,\n  Err(error) => 0,\n};\n",
    );
    assert!(
        out.contains("if ($rl_t0.kind !== \"Ok\") return $rl_t0;"),
        "{out}"
    );
}

#[test]
fn try_inside_a_function_inside_an_arm_body_is_allowed() {
    let out = ok(
        "const x = match (r) {\n  Ok(value) => { const f = () => { try g(value); return 1; }; return f(); },\n  Err(error) => 0,\n};\n",
    );
    assert!(
        out.contains("if ($rl_t0.kind !== \"Ok\") return $rl_t0;"),
        "{out}"
    );
}

#[test]
fn try_inside_a_function_inside_a_template_interpolation_is_allowed() {
    let out = ok("const s = `${run(() => { try g(); return h(); })}`;\n");
    assert!(
        out.contains("if ($rl_t0.kind !== \"Ok\") return $rl_t0;"),
        "{out}"
    );
}

/* ------------------------------------------------------------------ */
/* let-else — Rust-style refutable binding                             */
/* ------------------------------------------------------------------ */

#[test]
fn let_else_emits_guard_and_bind() {
    let out = ok(
        "function f(): number {\n  const Some(value) = find() else { return 0; };\n  return value;\n}\n",
    );
    assert!(
        out.contains(
            "const $rl_t0 = (find()); if ($rl_t0.kind !== \"Some\") { return 0; } const { value } = $rl_t0;"
        ),
        "{out}"
    );
}

#[test]
fn let_else_binding_alias_and_keyword() {
    let out = ok(
        "function f(): string {\n  let Some(value: user) = find() else { throw new Error(\"none\"); };\n  return user;\n}\n",
    );
    assert!(out.contains("let { value: user } = $rl_t0;"), "{out}");
}

#[test]
fn let_else_empty_bindings_checks_only() {
    let out =
        ok("function f(): number {\n  const Ok() = check() else { return -1; };\n  return 1;\n}\n");
    assert!(
        out.contains("if ($rl_t0.kind !== \"Ok\") { return -1; }"),
        "{out}"
    );
    assert!(!out.contains("} = $rl_t0;"), "{out}");
}

#[test]
fn let_else_shares_try_temp_counter() {
    let out = ok(
        "function f(): X {\n  const n = try g();\n  const Some(v) = h(n) else { return fallback(); };\n  return wrap(v);\n}\n",
    );
    assert!(out.contains("if ($rl_t0.kind !== \"Ok\")"), "{out}");
    assert!(
        out.contains("const $rl_t1 = (h(n)); if ($rl_t1.kind !== \"Some\")"),
        "{out}"
    );
}

#[test]
fn let_else_diverges_via_throw_and_continue() {
    ok(
        "function f(): number {\n  const Some(v) = find() else { throw new Error(\"no\"); };\n  return v;\n}\n",
    );
    ok(
        "function f(): number {\n  for (const x of xs) {\n    const Some(v) = find(x) else { continue; };\n    use(v);\n  }\n  return 0;\n}\n",
    );
}

#[test]
fn let_else_expression_may_be_a_match() {
    let out = ok(
        "function f(): number {\n  const Some(v) = match (x) { A => some(1), _ => none() } else { return 0; };\n  return v;\n}\n",
    );
    assert!(out.contains("if ($rl_t0.kind !== \"Some\")"), "{out}");
    assert!(out.contains("switch ($rl_m.kind)"), "{out}");
}

#[test]
fn let_else_diverges_when_the_return_value_is_an_object_literal() {
    // The `}` of an object literal ends no statement, so `return { ... };`
    // is still a `return` — the shape every `Result`-returning function
    // writes.
    let out = ok(
        "function f(): number {\n  const Some(v) = find() else { return { kind: \"Err\", error: \"no\" }; };\n  return v;\n}\n",
    );
    assert!(
        out.contains("{ return { kind: \"Err\", error: \"no\" }; }"),
        "{out}"
    );
    // Same with a statement in front of it, and with an rl construct as
    // the returned value.
    ok(
        "function f(): number {\n  const Some(v) = find() else { log(\"x\"); return { k: 1 }; };\n  return v;\n}\n",
    );
    ok(
        "function f(): number {\n  const Some(v) = find() else { return match (o) { A => 1, _ => 0 }; };\n  return v;\n}\n",
    );
    ok(
        "function f(): number {\n  const Some(v) = find() else { throw { code: 1 }; };\n  return v;\n}\n",
    );
}

#[test]
fn let_else_divergence_still_sees_block_statements() {
    // A *block* statement's `}` does end a statement, so the diverging
    // keyword after one is found — that is the half the object-literal fix
    // must not break.
    ok(
        "function f(): number {\n  const Some(v) = find() else { if (c) { log(\"x\"); } return 0; };\n  return v;\n}\n",
    );
    ok(
        "function f(): number {\n  const Some(v) = find() else { try { log(\"x\"); } catch (e) { log(\"y\"); } return 0; };\n  return v;\n}\n",
    );
    ok(
        "function f(): number {\n  const Some(v) = find() else { for (const x of xs) { log(x); } return 0; };\n  return v;\n}\n",
    );
    ok(
        "function f(): number {\n  const Some(v) = find() else { function g() { return 1; } return g(); };\n  return v;\n}\n",
    );
    // An arrow body and a declaration's initializer close nothing, and the
    // `;` after them is what starts the next statement.
    ok(
        "function f(): number {\n  const Some(v) = find() else { const g = () => { return 1; }; return g(); };\n  return v;\n}\n",
    );
    ok(
        "function f(): number {\n  const Some(v) = find() else { const o = { n: 1 }; return o.n; };\n  return v;\n}\n",
    );
}

#[test]
fn let_else_non_diverging_else_ending_in_a_brace_is_still_an_error() {
    // The flow answer must not turn every trailing `}` into a divergence:
    // an `if` without an `else` can fall through, a loop can run zero
    // times, an object literal is no statement at all.
    for body in [
        "const o = { n: 1 };",
        "if (c) { return 1; }",
        "for (const x of xs) { log(x); }",
    ] {
        let e = err(&format!(
            "function f(): number {{\n  const Some(v) = find() else {{ {body} }};\n  return v;\n}}\n"
        ));
        assert!(e.message.contains("must diverge"), "{body}: {}", e.message);
    }
}

#[test]
fn let_else_divergence_is_a_flow_answer_not_a_last_keyword_check() {
    // The CFG (TASK-125) accepts what the last-keyword heuristic wrongly
    // rejected: both-branch if/else, a diverging bare block, and code
    // after a `return` (unreachable, not a hole).
    for body in [
        "if (c) { return 1; } else { return 2; }",
        "if (c) { return 1; } else if (d) { throw e; } else { return 2; }",
        "if (c) return 1; else return 2;",
        "{ return 1; }",
        "return 0; log(\"never\");",
    ] {
        ok(&format!(
            "function f(): number {{\n  const Some(v) = find() else {{ {body} }};\n  return v;\n}}\n"
        ));
    }
}

#[test]
fn let_else_non_diverging_else_is_error() {
    let e =
        err("function f(): number {\n  const Some(v) = find() else { log(); };\n  return v;\n}\n");
    assert!(e.message.contains("must diverge"), "{}", e.message);
    assert_eq!((e.line, e.col), (2, 26)); // points at the `else` keyword
}

#[test]
fn let_else_empty_else_block_is_error() {
    let e = err("function f(): number {\n  const Some(v) = find() else { };\n  return v;\n}\n");
    assert!(e.message.contains("must diverge"), "{}", e.message);
}

#[test]
fn let_else_inside_match_arm_is_error() {
    let e = err(
        "const x = match (r) {\n  Ok(value) => { const Some(v) = h(value) else { return 0; }; return v; },\n  _ => 0,\n};\n",
    );
    assert!(
        e.message.contains("let-else cannot be used here"),
        "{}",
        e.message
    );
}

#[test]
fn let_else_inside_a_function_inside_an_arm_body_is_allowed() {
    // The `else`'s `return` exits the arrow the user wrote — the same
    // flow fact that places `try`.
    let out = ok(
        "const x = match (r) {\n  Ok(value) => { const f = () => { const Some(v) = h(value) else { return 0; }; return v; }; return f(); },\n  _ => 0,\n};\n",
    );
    assert!(out.contains("if ($rl_t0.kind !== \"Some\")"), "{out}");
}

#[test]
fn let_else_without_semicolon_is_not_recognized() {
    // No terminating `;` → not rl syntax; the (invalid-TS) source passes
    // through and the output self-check reports it.
    let e = err(
        "function f(): number {\n  const Some(v) = find() else { return 0; }\n  return v;\n}\n",
    );
    assert!(
        e.message.contains("generated TypeScript failed to parse"),
        "{}",
        e.message
    );
}

#[test]
fn let_else_requires_parens_on_the_pattern() {
    // `const Point = e else { ... };` (no parens) is not rl syntax — the
    // invalid-TS text passes through to the output self-check.
    let e =
        err("function f(): number {\n  const Point = find() else { return 0; };\n  return 1;\n}\n");
    assert!(
        e.message.contains("generated TypeScript failed to parse"),
        "{}",
        e.message
    );
}

/* ------------------------------------------------------------------ */
/* swc output verification                                             */
/* ------------------------------------------------------------------ */

#[test]
fn verify_rejects_invalid_passthrough_typescript() {
    let e = err("const = 5;\n");
    assert!(
        e.message.contains("generated TypeScript failed to parse"),
        "{}",
        e.message
    );
}

#[test]
fn no_verify_passes_invalid_typescript_through() {
    let opts = Options {
        verify: false,
        ..Options::default()
    };
    let out = compile("const = 5;\n", &opts).unwrap();
    assert_eq!(out, "const = 5;\n");
}

#[test]
fn filename_appears_in_error_display() {
    let opts = Options {
        filename: Some("demo.rl"),
        ..Options::default()
    };
    let e = compile("const r = match (x) { A => 1, A => 2 };", &opts).expect_err("expected error");
    assert_eq!(e.to_string(), "demo.rl:1:31: match: duplicate arm \"A\"");
}

/* ------------------------------------------------------------------ */
/* import specifier rewriting                                          */
/* ------------------------------------------------------------------ */

#[test]
fn relative_rl_import_is_rewritten_to_js_by_default() {
    let out = ok("import { CalcError } from \"./error.rl\";\n");
    assert_eq!(out, "import { CalcError } from \"./error.js\";\n");
}

#[test]
fn rewrite_covers_all_static_import_forms() {
    let out = ok(r#"
import def from "./a.rl";
import def2, { named as alias } from "./b.rl";
import * as ns from "./c.rl";
import type { T } from "./d.rl";
import "./side.rl";
export { x, y as z } from "./e.rl";
export * from "./f.rl";
export * as g from "./g.rl";
export type { U } from "./h.rl";
"#);
    for stem in ["a", "b", "c", "d", "side", "e", "f", "g", "h"] {
        assert!(out.contains(&format!("\"./{stem}.js\"")), "{out}");
        assert!(!out.contains(&format!("\"./{stem}.rl\"")), "{out}");
    }
}

#[test]
fn rewrite_keeps_quote_style_and_parent_paths() {
    let out = ok("import a from './x.rl';\nimport b from \"../up/y.rl\";\n");
    assert_eq!(
        out,
        "import a from './x.js';\nimport b from \"../up/y.js\";\n"
    );
}

#[test]
fn the_std_specifier_is_left_alone_by_default() {
    // A bundler plugin resolves `@rl/std` itself, so the untouched
    // specifier is the right default.
    let src = "import { Option, Result } from \"@rl/std\";\n";
    assert_eq!(ok(src), src);
}

#[test]
fn the_std_specifier_is_rewritten_when_the_caller_places_the_module() {
    let opts = Options {
        std_import: Some("../rl.js"),
        ..Options::default()
    };
    let out = compile("import { Option } from '@rl/std';\n", &opts).unwrap();
    // The quote style survives; only the specifier's text changes.
    assert_eq!(out, "import { Option } from '../rl.js';\n");
}

#[test]
fn the_std_specifier_is_not_a_project_module() {
    // It has no file to follow, so it is not part of the module graph the
    // CLI walks for declarations.
    assert!(rlc::rl_imports("import { Option } from \"@rl/std\";\n").is_empty());
    assert!(rlc::imports_std("export { Result } from \"@rl/std\";\n"));
    assert!(!rlc::imports_std("import { Option } from \"./rl.js\";\n"));
}

#[test]
fn ts_mode_points_at_the_emitted_file() {
    // With `allowImportingTsExtensions` + `rewriteRelativeImportExtensions`,
    // tsc accepts `.ts` specifiers and rewrites them to `.js` on emit — so
    // rlc only has to name the file it actually produces.
    let opts = Options {
        rewrite_imports: rlc::ImportRewrite::Ts,
        ..Options::default()
    };
    let out = compile("import { E } from \"./error.rl\";\n", &opts).unwrap();
    assert_eq!(out, "import { E } from \"./error.ts\";\n");
}

#[test]
fn ts_mode_preserves_the_quote_style_and_path() {
    let opts = Options {
        rewrite_imports: rlc::ImportRewrite::Ts,
        ..Options::default()
    };
    let out = compile(
        "import a from './x.rl';\nexport * from \"../up/y.rl\";\n",
        &opts,
    )
    .unwrap();
    assert_eq!(
        out,
        "import a from './x.ts';\nexport * from \"../up/y.ts\";\n"
    );
}

#[test]
fn off_mode_leaves_the_specifier_untouched() {
    let opts = Options {
        rewrite_imports: rlc::ImportRewrite::Off,
        ..Options::default()
    };
    let src = "import { E } from \"./error.rl\";\n";
    assert_eq!(compile(src, &opts).unwrap(), src);
}

#[test]
fn non_relative_rl_specifiers_are_untouched() {
    // Only relative paths are rewritten — package-like and absolute
    // specifiers keep their bytes.
    let src = "import a from \"pkg.rl\";\nimport b from \"/abs/x.rl\";\nimport c from \"@scope/p/x.rl\";\n";
    assert_eq!(ok(src), src);
}

#[test]
fn dynamic_import_and_import_meta_are_untouched() {
    let src = "const m = import(\"./x.rl\");\nconst u = import.meta.url;\n";
    assert_eq!(ok(src), src);
}

#[test]
fn import_assignment_is_untouched() {
    // TS import-assignment is not a static import declaration.
    let src = "import fs = require(\"./legacy.rl\");\n";
    assert_eq!(ok(src), src);
}

#[test]
fn rewrite_composes_with_rl_constructs_in_the_same_file() {
    let out = ok(r#"
import { CalcError } from "./error.rl";
enum Shape { Circle(radius: number), Point }
const area = match (Shape.Point) {
  Circle(radius) => radius,
  Point => 0,
};
"#);
    assert!(out.contains("\"./error.js\""), "{out}");
    assert!(out.contains("switch ($rl_m.kind)"), "{out}");
}

/* ------------------------------------------------------------------ */
/* project-wide exhaustiveness (extern enums)                          */
/* ------------------------------------------------------------------ */

fn token_extern() -> rlc::ExternEnum {
    rlc::ExternEnum {
        name: "Token".to_string(),
        tags: vec!["Num".to_string(), "Ident".to_string(), "Eof".to_string()],
        from: Some("./token.rl".to_string()),
    }
}

#[test]
fn extern_enum_makes_match_checked() {
    let externs = [token_extern()];
    let opts = Options {
        extern_enums: &externs,
        ..Options::default()
    };
    let e = compile(
        "const s = match (t) {\n  Num(value) => value,\n  Ident(name) => 0,\n};\n",
        &opts,
    )
    .expect_err("expected non-exhaustive error");
    assert!(
        e.message
            .contains("match on enum Token (imported from \"./token.rl\") is not exhaustive"),
        "{}",
        e.message
    );
    assert!(e.message.contains("missing \"Eof\""), "{}", e.message);
    assert_eq!((e.line, e.col), (1, 11));
}

#[test]
fn extern_enum_full_coverage_compiles() {
    let externs = [token_extern()];
    let opts = Options {
        extern_enums: &externs,
        ..Options::default()
    };
    let out = compile(
        "const s = match (t) { Num(value) => value, Ident(name) => 0, Eof => -1 };\n",
        &opts,
    )
    .unwrap();
    assert!(out.contains("switch ($rl_m.kind)"));
}

#[test]
fn local_enum_shadows_extern_of_same_name() {
    // The local Token has only two cases; the extern one must not resurrect
    // a third. Full local coverage compiles.
    let externs = [token_extern()];
    let opts = Options {
        extern_enums: &externs,
        ..Options::default()
    };
    let out = compile(
        "enum Token { Num(value: number), Ident(name: string) }\nconst s = match (t) { Num(value) => value, Ident(name) => 0 };\n",
        &opts,
    )
    .unwrap();
    assert!(out.contains("switch ($rl_m.kind)"));
}

#[test]
fn extern_enum_shadows_builtin_of_same_name() {
    // An imported `Option` with an extra case replaces the built-in: the
    // two-case match that satisfies the built-in must now be an error.
    let externs = [rlc::ExternEnum {
        name: "Option".to_string(),
        tags: vec!["Some".to_string(), "None".to_string(), "Maybe".to_string()],
        from: Some("./opt.rl".to_string()),
    }];
    let opts = Options {
        extern_enums: &externs,
        ..Options::default()
    };
    let e = compile(
        "const s = match (o) { Some(value) => value, None => 0 };\n",
        &opts,
    )
    .expect_err("expected non-exhaustive error");
    assert!(e.message.contains("missing \"Maybe\""), "{}", e.message);
}

#[test]
fn extern_enums_do_not_affect_unrelated_matches() {
    // Tags that belong to no known enum stay unchecked (runtime guard only).
    let externs = [token_extern()];
    let opts = Options {
        extern_enums: &externs,
        ..Options::default()
    };
    let out = compile("const s = match (x) { Foo(a) => a, Bar => 0 };\n", &opts).unwrap();
    assert!(out.contains("switch ($rl_m.kind)"));
}

/* ------------------------------------------------------------------ */
/* declaration collection API                                          */
/* ------------------------------------------------------------------ */

#[test]
fn exported_enums_returns_exported_rl_enums_only() {
    let decls = rlc::exported_enums(
        "export enum Token { Num(value: number), Eof }\nenum Private { A(), B }\nexport enum Color { Red, Green }\n",
    );
    // Color is a plain TS enum (no payload, no generics) — not an rl enum.
    assert_eq!(decls.len(), 1);
    assert_eq!(decls[0].name, "Token");
    assert_eq!(decls[0].tags, vec!["Num".to_string(), "Eof".to_string()]);
    assert_eq!(decls[0].from, None);
}

#[test]
fn rl_imports_reports_specifiers_and_names() {
    use rlc::RlImportNames;
    let imports = rlc::rl_imports(
        r#"
import { Token, Kind as K, type T } from "./a.rl";
import * as ns from "../b.rl";
import "./side.rl";
export { X } from "./re.rl";
import { skip } from "./not-rl.ts";
"#,
    );
    assert_eq!(imports.len(), 4);
    assert_eq!(imports[0].specifier, "./a.rl");
    assert_eq!(
        imports[0].names,
        RlImportNames::Named(vec![
            ("Token".to_string(), None),
            ("Kind".to_string(), Some("K".to_string())),
            ("T".to_string(), None),
        ])
    );
    assert_eq!(imports[1].specifier, "../b.rl");
    assert_eq!(imports[1].names, RlImportNames::Namespace("ns".to_string()));
    assert_eq!(imports[2].names, RlImportNames::None);
    assert_eq!(imports[3].specifier, "./re.rl");
    assert_eq!(imports[3].names, RlImportNames::None);
}

#[test]
fn scan_module_answers_both_questions_in_one_pass() {
    // The single-fact helpers are defined in terms of the scan, so the two
    // views must never disagree — that equivalence is what lets the CLI
    // parse each input once.
    for source in [
        "import { Option } from \"@rl/std\";\nimport { T } from \"./t.rl\";\n",
        "import { T } from \"./t.rl\";\n",
        "export { Result } from \"@rl/std\";\n",
        "const match = 1;\n",
        "",
    ] {
        let scan = rlc::scan_module(source);
        assert_eq!(scan.imports, rlc::rl_imports(source), "{source:?}");
        assert_eq!(scan.imports_std, rlc::imports_std(source), "{source:?}");
    }

    let scan =
        rlc::scan_module("import { Option } from \"@rl/std\";\nimport * as ns from \"../b.rl\";\n");
    assert!(scan.imports_std);
    assert_eq!(scan.imports.len(), 1);
    assert_eq!(scan.imports[0].specifier, "../b.rl");
}

/* ------------------------------------------------------------------ */
/* symbol API                                                          */
/* ------------------------------------------------------------------ */

#[test]
fn enum_symbols_carries_positions_and_field_shapes() {
    let src =
        "export enum Token {\n  Num(value: number),\n  Empty(),\n  Eof,\n}\nenum Local { A() }\n";
    let syms = rlc::enum_symbols(src);
    assert_eq!(syms.len(), 2);

    let token = &syms[0];
    assert_eq!(token.name, "Token");
    assert!(token.exported);
    assert_eq!(rlc::line_col(src, token.offset), (1, 13));
    assert_eq!(token.cases.len(), 3);
    assert_eq!(rlc::line_col(src, token.cases[0].offset), (2, 3));
    let fields = token.cases[0].fields.as_ref().unwrap();
    assert_eq!(fields[0].name, "value");
    assert_eq!(fields[0].ty, "number");
    assert!(!fields[0].optional);
    // `Empty()` has an empty field list; `Eof` has none at all.
    assert_eq!(token.cases[1].fields.as_deref(), Some(&[][..]));
    assert_eq!(token.cases[2].fields, None);

    assert_eq!(syms[1].name, "Local");
    assert!(!syms[1].exported);
}

/* ------------------------------------------------------------------ */
/* pipeline                                                            */
/* ------------------------------------------------------------------ */

#[test]
fn pipeline_emits_nested_apply_helper_calls() {
    let out = ok("const y = half(4) |> double |> label;\n");
    assert!(
        out.contains("const y = $rl_ap($rl_ap((half(4)), (double)), (label));"),
        "{out}"
    );
    assert!(out.contains("function $rl_ap<A, B>(v: A, f: (v: A) => B): B { return f(v); }"));
}

#[test]
fn pipeline_method_step_chains_postfix() {
    let out = ok("const t = s |> .trim() |> .split(\",\") |> f;\n");
    assert!(
        out.contains("const t = $rl_ap((((s)).trim()).split(\",\"), (f));"),
        "{out}"
    );
}

#[test]
fn pipeline_helper_is_emitted_once_per_file() {
    let out = ok("const a = x |> f;\nconst b = y |> g;\n");
    assert_eq!(out.matches("function $rl_ap").count(), 1, "{out}");
    // and appended at the end so original lines keep their positions
    assert!(out.trim_end().ends_with("{ return f(v); }"), "{out}");
}

#[test]
fn file_without_pipeline_gets_no_helper() {
    let out = ok("const a = f(x);\n");
    assert!(!out.contains("$rl_ap"), "{out}");
}

#[test]
fn pipeline_head_reclaims_a_lifted_template() {
    // The template token is lifted as a segment before the `|>` is seen —
    // the claim must rewind it into the head sub-program.
    let out = ok("const a = `v=${n}` |> f;\n");
    assert!(out.contains("const a = $rl_ap((`v=${n}`), (f));"), "{out}");
}

#[test]
fn pipeline_head_reclaims_a_lifted_match() {
    let out =
        ok("enum E { A(v: number), B }\nconst a = match (e) { A(v) => v, B => 0, } |> double;\n");
    assert!(out.contains("const a = $rl_ap((("), "{out}");
    assert!(out.contains("switch ($rl_m.kind)"), "{out}");
    assert!(out.contains(")())), (double));"), "{out}");
}

#[test]
fn pipeline_head_is_the_whole_call_not_the_inner_argument() {
    // Bracket tracking must restore the enclosing expression's start:
    // the head of `a(b) |> g` is `a(b)`, not `b`.
    let out = ok("const y = f(a(b) |> g);\n");
    assert!(out.contains("const y = f($rl_ap((a(b)), (g)));"), "{out}");
}

#[test]
fn pipeline_inside_match_scrutinee_arm_and_template() {
    let out = ok(
        "enum E { A(v: number), B }\nconst r = match (x |> norm) {\n  A(v) => v |> double,\n  B => 0,\n};\nconst t = `n=${x |> f}`;\n",
    );
    assert!(
        out.contains("const $rl_m = ($rl_ap((x), (norm)));"),
        "{out}"
    );
    assert!(out.contains("return ($rl_ap((v), (double)));"), "{out}");
    assert!(out.contains("`n=${$rl_ap((x), (f))}`"), "{out}");
}

#[test]
fn pipeline_composes_with_try() {
    let out = ok(
        "function f(): Result<number, string> {\n  const a = try readCfg() |> norm;\n  return Result.Ok(a);\n}\n",
    );
    assert!(
        out.contains("const $rl_t0 = ($rl_ap((readCfg()), (norm)));"),
        "{out}"
    );
}

#[test]
fn pipeline_await_in_head_needs_no_async_wrapper() {
    let out = ok("async function f(p: Promise<string>) {\n  return await p |> norm;\n}\n");
    assert!(out.contains("return $rl_ap((await p), (norm));"), "{out}");
    assert!(!out.contains("async () =>"), "{out}");
}

#[test]
fn unparenthesized_ternary_next_to_pipeline_is_an_error() {
    let e = err("const a = c ? x : y |> f;\n");
    assert!(e.message.contains("parenthesize"), "{}", e.message);
    assert_eq!((e.line, e.col), (1, 21));
}

#[test]
fn parenthesized_ternary_head_compiles() {
    let out = ok("const a = (c ? x : y) |> f;\n");
    assert!(out.contains("$rl_ap(((c ? x : y)), (f))"), "{out}");
}

#[test]
fn unparenthesized_arrow_step_is_an_error() {
    let e = err("const a = x |> n => n + 1;\n");
    assert!(e.message.contains("parenthesize"), "{}", e.message);
}

#[test]
fn empty_or_dangling_step_is_an_error() {
    let e = err("const a = x |>;\n");
    assert!(e.message.contains("could not be parsed"), "{}", e.message);
    let e = err("const a = x |> |> f;\n");
    assert!(e.message.contains("could not be parsed"), "{}", e.message);
}

#[test]
fn optional_chain_step_is_an_error() {
    let e = err("const a = x |> ?.trim();\n");
    assert!(e.message.contains("could not be parsed"), "{}", e.message);
}

#[test]
fn try_inside_a_function_inside_a_pipeline_step_is_allowed() {
    let out = ok("const a = x |> (n => { const b = try f(n); return b; });\n");
    assert!(
        out.contains("if ($rl_t0.kind !== \"Ok\") return $rl_t0;"),
        "{out}"
    );
}

/* ------------------------------------------------------------------ */
/* flow (function composition)                                         */
/* ------------------------------------------------------------------ */

#[test]
fn flow_emits_nested_composition_helper_calls() {
    let out = ok("const f = flow |> parse |> double |> label;\n");
    assert!(
        out.contains("const f = $rl_fl($rl_fl((parse), (double)), (label));"),
        "{out}"
    );
    assert!(out.contains(
        "function $rl_fl<A extends unknown[], B, C>(f: (...a: A) => B, g: (b: B) => C): \
         (...a: A) => C { return (...a: A) => g(f(...a)); }"
    ));
    // a composition is not a value pipeline — no apply helper
    assert!(!out.contains("$rl_ap"), "{out}");
}

#[test]
fn flow_method_step_becomes_a_contextually_typed_arrow() {
    let out = ok("const f = flow |> parse |> .toFixed(1);\n");
    assert!(
        out.contains("const f = $rl_fl((parse), (($rl_v) => ($rl_v).toFixed(1)));"),
        "{out}"
    );
}

#[test]
fn flow_with_a_single_step_is_that_step_and_needs_no_helper() {
    let out = ok("const f = flow |> parse;\n");
    assert!(out.contains("const f = (parse);"), "{out}");
    assert!(!out.contains("$rl_fl"), "{out}");
}

#[test]
fn flow_helper_is_emitted_once_per_file() {
    let out = ok("const a = flow |> f |> g;\nconst b = flow |> h |> i;\n");
    assert_eq!(out.matches("function $rl_fl").count(), 1, "{out}");
    assert!(out.trim_end().ends_with("g(f(...a)); }"), "{out}");
}

#[test]
fn file_without_flow_gets_no_composition_helper() {
    let out = ok("const a = x |> f;\n");
    assert!(!out.contains("$rl_fl"), "{out}");
}

#[test]
fn flow_is_a_contextual_keyword_only_at_a_pipeline_head() {
    // a `flow` variable still pipes when parenthesized, and a dotted or
    // called head is an ordinary value head
    let out = ok("const a = (flow) |> f;\nconst b = o.flow |> f;\nconst c = flow() |> f;\n");
    assert!(out.contains("const a = $rl_ap(((flow)), (f));"), "{out}");
    assert!(out.contains("const b = $rl_ap((o.flow), (f));"), "{out}");
    assert!(out.contains("const c = $rl_ap((flow()), (f));"), "{out}");
    assert!(!out.contains("$rl_fl"), "{out}");
}

#[test]
fn flow_composes_inside_expressions() {
    let out = ok("const a = xs.map(flow |> parse |> double);\nconst b = `${flow |> f |> g}`;\n");
    assert!(
        out.contains("const a = xs.map($rl_fl((parse), (double)));"),
        "{out}"
    );
    assert!(out.contains("const b = `${$rl_fl((f), (g))}`;"), "{out}");
}

#[test]
fn flow_step_can_be_a_parenthesized_arrow() {
    let out = ok("const f = flow |> parse |> (n => n + 1);\n");
    assert!(
        out.contains("const f = $rl_fl((parse), ((n => n + 1)));"),
        "{out}"
    );
}

#[test]
fn flow_first_step_cannot_be_a_method_step() {
    let e = err("const f = flow |> .trim() |> lower;\n");
    assert!(
        e.message.contains("the first step cannot be a method step"),
        "{}",
        e.message
    );
    assert_eq!((e.line, e.col), (1, 19));
}

#[test]
fn flow_without_a_step_is_an_error() {
    let e = err("const f = flow |>;\n");
    assert!(e.message.contains("could not be parsed"), "{}", e.message);
}

/* ------------------------------------------------------------------ */
/* tuple match                                                         */
/* ------------------------------------------------------------------ */

#[test]
fn tuple_match_emits_joint_if_chain() {
    let out = ok(r#"
enum Dir { North(), South }
enum Speed { Fast(), Slow }
const step = match (dir, speed) {
  (North, Fast) => 2,
  (North, Slow) => 1,
  (South, _) => -1,
};
"#);
    assert!(out.contains("const $rl_m0 = (dir);"), "{out}");
    assert!(out.contains("const $rl_m1 = (speed);"), "{out}");
    assert!(
        out.contains("if ($rl_m0.kind === \"North\" && $rl_m1.kind === \"Fast\") { return (2); }"),
        "{out}"
    );
    assert!(
        out.contains("if ($rl_m0.kind === \"South\") { return (-1); }"),
        "{out}"
    );
    assert!(out.contains("JSON.stringify([$rl_m0, $rl_m1])"), "{out}");
}

#[test]
fn tuple_match_binds_fields_from_each_position() {
    let out = ok(r#"
const r = match (a, b) {
  (Some(value: x), Some(value: y)) => x + y,
  _ => 0,
};
"#);
    assert!(
        out.contains(
            "{ const { value: x } = $rl_m0; const { value: y } = $rl_m1; return (x + y); }"
        ),
        "{out}"
    );
}

#[test]
fn comma_expression_scrutinee_is_still_a_single_match() {
    // No tuple pattern in the arms → the comma is a comma expression,
    // exactly as before tuple matches existed.
    let out = ok(
        "const r = match ((a, b)) { A => 1, _ => 0 };\nconst s = match (a, b) { A => 1, _ => 0 };\n",
    );
    assert!(out.contains("const $rl_m = ((a, b));"), "{out}");
    assert!(out.contains("const $rl_m = (a, b);"), "{out}");
    assert!(!out.contains("$rl_m0"), "{out}");
}

#[test]
fn tuple_match_product_exhaustiveness_reports_missing_combination() {
    let e = err(r#"
enum Dir { North(), South }
enum Speed { Fast(), Slow }
const step = match (d, s) {
  (North, Fast) => 2,
  (North, Slow) => 1,
  (South, Fast) => -1,
};
"#);
    assert!(
        e.message
            .contains("match on (Dir, Speed) is not exhaustive: missing (South, Slow)"),
        "{}",
        e.message
    );
    assert_eq!((e.line, e.col), (4, 14));
}

#[test]
fn tuple_match_wildcard_element_and_or_pattern_cover_the_product() {
    let out = ok(r#"
enum Dir { North(), South, East, West }
enum Speed { Fast(), Slow }
const step = match (d, s) {
  (North | South, _) => 1,
  (East, Fast | Slow) => 2,
  (West, _) => 3,
};
"#);
    assert!(out.contains("$rl_m0"), "{out}");
    assert!(
        out.contains(
            "if (($rl_m0.kind === \"North\" || $rl_m0.kind === \"South\")) { return (1); }"
        ),
        "{out}"
    );
}

#[test]
fn tuple_match_guarded_arm_covers_nothing() {
    let e = err(r#"
enum Coin { Heads(), Tails }
const r = match (a, b) {
  (Heads, Heads) if lucky() => 1,
  (Heads, Heads) => 2,
  (Heads, Tails) => 3,
  (Tails, Heads) => 4,
};
"#);
    assert!(
        e.message.contains("missing (Tails, Tails)"),
        "{}",
        e.message
    );
}

#[test]
fn tuple_match_bare_wildcard_arm_skips_the_check_and_must_be_last() {
    let out = ok(r#"
enum Coin { Heads(), Tails }
const r = match (a, b) {
  (Heads, Heads) => 1,
  _ => 0,
};
"#);
    assert!(out.contains("return (0);"), "{out}");

    let e = err("const r = match (a, b) {\n  _ => 0,\n  (A, B) => 1,\n};\n");
    assert!(e.message.contains("must be the last arm"), "{}", e.message);
}

#[test]
fn tuple_match_arity_mismatch_is_an_error() {
    let e = err("const r = match (a, b) {\n  (A, B, C) => 1,\n  _ => 0,\n};\n");
    assert!(
        e.message
            .contains("tuple pattern has 3 elements but the match has 2 scrutinees"),
        "{}",
        e.message
    );
    assert_eq!((e.line, e.col), (2, 3));
}

#[test]
fn tuple_match_duplicate_binding_across_elements_is_an_error() {
    let e =
        err("const r = match (a, b) {\n  (Some(value), Some(value)) => value,\n  _ => 0,\n};\n");
    assert!(
        e.message.contains("binding `value` is used more than once"),
        "{}",
        e.message
    );
}

#[test]
fn tuple_match_or_alternatives_must_bind_the_same_fields_per_element() {
    let e = err("const r = match (a, b) {\n  (Some(value) | None, _) => 1,\n  _ => 0,\n};\n");
    assert!(
        e.message
            .contains("or-pattern alternatives must bind the same names"),
        "{}",
        e.message
    );
    assert!(
        e.message
            .contains("`value` is bound in `Some(...)` but not in `None(...)`"),
        "{}",
        e.message
    );
}

#[test]
fn tuple_match_over_builtin_enums() {
    let e = err(
        "const r = match (o, r2) {\n  (Some(value), Ok(value: v)) => value + v,\n  (None, _) => 0,\n};\n",
    );
    assert!(
        e.message
            .contains("match on (Option, Result) is not exhaustive: missing (Some, Err)"),
        "{}",
        e.message
    );
}

#[test]
fn tuple_match_block_bodies_and_guards_use_the_label() {
    let out = ok(r#"
enum Coin { Heads(), Tails }
const r = match (a, b) {
  (Heads, Tails) if go() => { log(); return 1; },
  _ => 0,
};
"#);
    assert!(out.contains("$rl_b: {"), "{out}");
    assert!(out.contains("if ((go()))"), "{out}");
    assert!(out.contains("break $rl_b;"), "{out}");
}

#[test]
fn tuple_match_await_in_scrutinee_makes_it_async() {
    let out = ok(
        "async function f() {\n  return match (await a, b) {\n    (X, Y) => 1,\n    _ => 0,\n  };\n}\n",
    );
    assert!(out.contains("(await (async () => {"), "{out}");
}

#[test]
fn tuple_match_three_positions() {
    let e = err(r#"
enum B { T(), F }
const r = match (x, y, z) {
  (T, _, _) => 1,
  (F, T, _) => 2,
  (F, F, T) => 3,
};
"#);
    assert!(e.message.contains("missing (F, F, F)"), "{}", e.message);
}

/* ------------------------------------------------------------------ */
/* nested patterns                                                     */
/* ------------------------------------------------------------------ */

#[test]
fn nested_pattern_emits_path_conditions_and_binds() {
    let out = ok(r#"
const n = match (r) {
  Ok(value: Some(value: v)) => v,
  Ok(value: None()) => 0,
  _ => -1,
};
"#);
    assert!(
        out.contains("if ($rl_m.kind === \"Ok\" && $rl_m.value.kind === \"Some\") { const { value: v } = $rl_m.value; return (v); }"),
        "{out}"
    );
    assert!(
        out.contains("if ($rl_m.kind === \"Ok\" && $rl_m.value.kind === \"None\") { return (0); }"),
        "{out}"
    );
    // nested patterns force the if-chain form
    assert!(!out.contains("switch ($rl_m.kind)"), "{out}");
}

#[test]
fn nested_pattern_two_levels_deep() {
    let out = ok(r#"
const n = match (r) {
  Ok(value: Some(value: Pair(a, b))) => a + b,
  _ => 0,
};
"#);
    assert!(
        out.contains("$rl_m.kind === \"Ok\" && $rl_m.value.kind === \"Some\" && $rl_m.value.value.kind === \"Pair\""),
        "{out}"
    );
    assert!(out.contains("const { a, b } = $rl_m.value.value;"), "{out}");
}

#[test]
fn nested_pattern_mixes_plain_bindings_at_each_level() {
    let out = ok(r#"
const n = match (r) {
  Both(left, right: Some(value)) => left + value,
  _ => 0,
};
"#);
    assert!(
        out.contains(
            "{ const { left } = $rl_m; const { value } = $rl_m.right; return (left + value); }"
        ),
        "{out}"
    );
}

#[test]
fn plain_alias_is_still_an_alias_not_a_nested_pattern() {
    // `value: v` (no parens) binds; only `value: Tag(...)` nests. A match
    // without nested patterns keeps the switch form.
    let out = ok("const n = match (o) { Some(value: None) => None, _ => 0 };");
    assert!(out.contains("const { value: None } = $rl_m;"), "{out}");
    assert!(out.contains("switch ($rl_m.kind)"), "{out}");
}

#[test]
fn a_nested_pattern_covers_exactly_what_it_matches() {
    // The exhaustiveness recursion descends into the payload, so the
    // witness names the *value* that is missing, not just its outer case.
    let e = err(r#"
const n = match (r) {
  Ok(value: Some(value: v)) => v,
  Err(error) => 0,
};
"#);
    assert!(
        e.message.contains(
            "match on built-in enum Result is not exhaustive: missing \"Ok(value: None())\""
        ),
        "{}",
        e.message
    );
}

#[test]
fn nested_pattern_arm_may_repeat_a_tag() {
    // Two Ok arms with different inner patterns are not duplicates —
    // exactly like two guarded arms of one tag.
    let out = ok(r#"
const n = match (r) {
  Ok(value: Some(value: v)) => v,
  Ok(value) => 0,
  Err(error) => -1,
};
"#);
    assert!(out.contains("$rl_m.value.kind === \"Some\""), "{out}");
}

#[test]
fn plain_arm_before_nested_arm_is_a_duplicate() {
    let e = err("const n = match (r) { Ok(value) => 1, Ok(value: Some(value: v)) => v, _ => 0 };");
    assert!(e.message.contains("duplicate arm \"Ok\""), "{}", e.message);
}

#[test]
fn nested_pattern_inside_or_pattern_is_an_error() {
    let e = err("const n = match (r) { Ok(value: Some(v)) | Err(error) => 1, _ => 0 };");
    assert!(
        e.message
            .contains("nested patterns cannot be combined with or-patterns"),
        "{}",
        e.message
    );
}

#[test]
fn duplicate_binding_within_one_pattern_is_an_error() {
    let e = err("const n = match (r) { Ok(value: Some(value), error: value) => value, _ => 0 };");
    assert!(
        e.message.contains("binding `value` is used more than once"),
        "{}",
        e.message
    );
}

#[test]
fn nested_pattern_in_tuple_match_elements() {
    let out = ok(r#"
const n = match (a, b) {
  (Ok(value: Some(value: x)), Ok(value: Some(value: y))) => x + y,
  _ => 0,
};
"#);
    assert!(
        out.contains("$rl_m0.kind === \"Ok\" && $rl_m0.value.kind === \"Some\" && $rl_m1.kind === \"Ok\" && $rl_m1.value.kind === \"Some\""),
        "{out}"
    );
    assert!(
        out.contains("const { value: x } = $rl_m0.value; const { value: y } = $rl_m1.value;"),
        "{out}"
    );
}

#[test]
fn nested_pattern_with_guard() {
    let out = ok(r#"
const n = match (r) {
  Ok(value: Some(value: v)) if v > 0 => v,
  _ => 0,
};
"#);
    assert!(out.contains("if ((v > 0)) return (v);"), "{out}");
}

#[test]
fn let_else_does_not_take_nested_patterns() {
    // `const Some(value: Ok(v)) = ...` is not rl let-else syntax; the
    // candidate passes through and (being invalid TS) fails the output
    // self-check — same as any malformed candidate.
    let e = err("function f() {\n  const Some(value: Ok(v)) = g() else { return; };\n}\n");
    assert!(
        e.message.contains("generated TypeScript failed to parse"),
        "{}",
        e.message
    );
}

/* ------------------------------------------------------------------ */
/* if let                                                              */
/* ------------------------------------------------------------------ */

#[test]
fn if_let_emits_a_self_contained_block() {
    let out =
        ok("function f() {\n  if let Some(value: user) = find() {\n    greet(user);\n  }\n}\n");
    assert!(
        out.contains("{ const $rl_t0 = (find()); if ($rl_t0.kind === \"Some\") { const { value: user } = $rl_t0; greet(user); } }"),
        "{out}"
    );
}

#[test]
fn if_let_else_block_and_chaining() {
    let out = ok(r#"
function f() {
  if let Some(value) = a() {
    use1(value);
  } else if let Ok(value: v) = b() {
    use2(v);
  } else {
    fallback();
  }
}
"#);
    assert!(
        out.contains("} else { const $rl_t1 = (b()); if ($rl_t1.kind === \"Ok\")"),
        "{out}"
    );
    assert!(out.contains("else { fallback(); } } }"), "{out}");
}

#[test]
fn if_let_takes_nested_patterns() {
    let out = ok("function f(r: Res) {\n  if let Ok(value: Some(value: v)) = r { use(v); }\n}\n");
    assert!(
        out.contains("if ($rl_t0.kind === \"Ok\" && $rl_t0.value.kind === \"Some\") { const { value: v } = $rl_t0.value; use(v); }"),
        "{out}"
    );
}

#[test]
fn if_let_shares_the_temp_counter_with_try() {
    let out = ok(
        "function f(): Result<number, string> {\n  const a = try g();\n  if let Some(value) = h(a) { use(value); }\n  return Result.Ok(a);\n}\n",
    );
    assert!(out.contains("$rl_t0"), "{out}");
    assert!(out.contains("const $rl_t1 = (h(a));"), "{out}");
}

#[test]
fn if_let_allowed_in_statement_contexts() {
    // A match arm's block body and a let-else else block are statement
    // positions — if let works there.
    let out = ok(r#"
function f(x: X, o: O) {
  const r = match (x) {
    A => {
      if let Some(value) = o { return value; }
      return 0;
    },
    _ => 1,
  };
  return r;
}
"#);
    assert!(out.contains("if ($rl_t0.kind === \"Some\")"), "{out}");
}

#[test]
fn if_let_in_expression_position_is_an_error() {
    let e = err("const s = `${if let Some(value) = o { 1 }}`;\n");
    assert!(
        e.message
            .contains("`if let` cannot be used in expression position"),
        "{}",
        e.message
    );
}

#[test]
fn malformed_if_let_is_an_error_with_position() {
    // `if let` cannot be passed through (never valid TS), so a candidate
    // that fails to parse is reported instead of failing the self-check.
    let e = err("function f() {\n  if let Some = o { g(); }\n}\n");
    assert!(
        e.message.contains("`if let` could not be parsed here"),
        "{}",
        e.message
    );
    assert_eq!((e.line, e.col), (2, 3));

    let e = err("function f() {\n  if let Some(v) = o { g(); } else if (x) { h(); }\n}\n");
    assert!(
        e.message.contains("`if let` could not be parsed here"),
        "{}",
        e.message
    );
}

#[test]
fn if_let_duplicate_binding_is_an_error() {
    let e = err("function f() {\n  if let Both(a: v, b: v) = o { g(v); }\n}\n");
    assert!(
        e.message.contains("binding `v` is used more than once"),
        "{}",
        e.message
    );
}

#[test]
fn plain_if_statements_pass_through() {
    let src = "if (x) { a(); } else if (y) { b(); } else { c(); }\nconst z = cond ? 1 : 2;\n";
    assert_eq!(ok(src), src);
}

/* ------------------------------------------------------------------ */
/* result computation block                                            */
/* ------------------------------------------------------------------ */

#[test]
fn result_block_emits_an_iife_of_early_returns() {
    let out = ok(r#"
const data = result {
  const user <- getUser(id);
  const company <- getCompany(user.companyId);
  { user, company }
};
"#);
    assert!(out.contains("const data = ((() => {"), "{out}");
    assert!(
        out.contains(
            "const $rl_r0 = (getUser(id)); if ($rl_r0.kind !== \"Ok\") return $rl_r0; \
             const user = $rl_r0.value;"
        ),
        "{out}"
    );
    assert!(
        out.contains(
            "const $rl_r1 = (getCompany(user.companyId)); if ($rl_r1.kind !== \"Ok\") \
             return $rl_r1; const company = $rl_r1.value;"
        ),
        "{out}"
    );
    assert!(
        out.contains("return { kind: \"Ok\" as const, value: ({ user, company }"),
        "{out}"
    );
    assert!(out.contains(") }; })())"), "{out}");
}

#[test]
fn result_block_keeps_ordinary_statements_and_nested_constructs() {
    let out = ok(r#"
const data = result {
  const user <- getUser(id);
  // normalize before the next step
  const name = user.name |> .trim() |> .toLowerCase();
  const company <- getCompany(name);
  { name, company }
};
"#);
    assert!(out.contains("// normalize before the next step"), "{out}");
    assert!(
        out.contains("const name = (((user.name)).trim()).toLowerCase();"),
        "{out}"
    );
}

#[test]
fn result_block_binding_may_be_let_var_annotated_or_destructured() {
    let out = ok(r#"
const a = result {
  let n: number <- parse(raw);
  var { x, y } <- point();
  const [first] <- items();
  n + x + y + first
};
"#);
    assert!(out.contains("let n: number = $rl_r0.value;"), "{out}");
    assert!(out.contains("var { x, y } = $rl_r1.value;"), "{out}");
    assert!(out.contains("const [first] = $rl_r2.value;"), "{out}");
}

#[test]
fn result_block_with_await_becomes_an_async_iife() {
    let out = ok(r#"
const data = async () => result {
  const user <- await getUser(id);
  user.name
};
"#);
    assert!(out.contains("(await (async () => {"), "{out}");
    assert!(out.contains("const $rl_r0 = (await getUser(id));"), "{out}");
}

#[test]
fn result_blocks_nest_and_number_their_temporaries_uniquely() {
    let out = ok(r#"
const a = result {
  const outer <- result {
    const inner <- f();
    inner + 1
  };
  outer
};
"#);
    assert!(out.contains("$rl_r0"), "{out}");
    assert!(out.contains("$rl_r1"), "{out}");
    assert!(!out.contains("$rl_r2"), "{out}");
}

#[test]
fn result_block_can_be_a_pipeline_head() {
    let out = ok("const a = result {\n  const x <- f();\n  x\n} |> Result.mapP(double);\n");
    assert!(out.contains("$rl_ap((((() => {"), "{out}");
    assert!(out.contains(", (Result.mapP(double)))"), "{out}");
}

#[test]
fn result_block_value_may_be_a_match_expression() {
    let out = ok(r#"
const a = result {
  const r <- f();
  match (r) {
    Ok(value) => value,
    Err(error) => error,
  }
};
"#);
    assert!(
        out.contains("return { kind: \"Ok\" as const, value: (((() => {"),
        "{out}"
    );
    assert!(out.contains("switch ($rl_m.kind)"), "{out}");
}

#[test]
fn try_and_let_else_are_rejected_inside_a_result_block() {
    let e = err("const a = result {\n  const x <- f();\n  const y = try g();\n  x + y\n};\n");
    assert!(
        e.message.contains("`try` cannot be used here"),
        "{}",
        e.message
    );
    assert!(e.message.contains("`<-` binding"), "{}", e.message);

    let e = err(
        "const a = result {\n  const x <- f();\n  const Some(v) = o else { return 0; };\n  v\n};\n",
    );
    assert!(
        e.message.contains("let-else cannot be used here"),
        "{}",
        e.message
    );
}

#[test]
fn if_let_is_allowed_inside_a_result_block() {
    let out = ok(r#"
const a = result {
  const x <- f();
  let label = "none";
  if let Some(value) = o { label = value; }
  { x, label }
};
"#);
    assert!(out.contains("if ($rl_t1.kind === \"Some\")"), "{out}");
}

#[test]
fn result_block_without_a_trailing_expression_is_an_error() {
    let e = err("const a = result {\n  const x <- f();\n};\n");
    assert!(
        e.message
            .contains("`result` block could not be parsed here"),
        "{}",
        e.message
    );
    assert_eq!((e.line, e.col), (1, 11));
}

#[test]
fn a_binding_without_a_declaration_keyword_is_a_located_error() {
    // The block is claimed by its other binding, so the file is not
    // TypeScript and rlc can say where the mistake is — instead of
    // emitting the comparison `b < -readNum()` and saying nothing.
    let e = err("const a = result {\n  const x <- f();\n  y <- g();\n  x + y\n};\n");
    assert!(
        e.message
            .contains("`result` binding is missing its declaration keyword"),
        "{}",
        e.message
    );
    assert_eq!((e.line, e.col), (3, 3));

    // No binding is claimed at all, but `result {` in expression position
    // is not TypeScript either — so this is reported here rather than by
    // the output self-check, at generated-code coordinates.
    let e = err("const a = result {\n  y <- g();\n  y\n};\n");
    assert!(
        e.message
            .contains("`result` binding is missing its declaration keyword"),
        "{}",
        e.message
    );
    assert_eq!((e.line, e.col), (2, 3));
}

#[test]
fn result_binding_without_a_semicolon_is_an_error() {
    // The binding is rl syntax whether or not the `;` is there, so this is
    // a located rl error rather than a failed output self-check.
    let e = err("const a = result {\n  const x <- f()\n  x\n};\n");
    assert!(
        e.message
            .contains("`result` block could not be parsed here"),
        "{}",
        e.message
    );

    let e = err("const a = result {\n  const x <- f()\n  const y <- g();\n  y\n};\n");
    assert!(
        e.message
            .contains("`result` block could not be parsed here"),
        "{}",
        e.message
    );
}

#[test]
fn result_binding_without_an_expression_is_an_error() {
    let e = err("const a = result {\n  const x <- ;\n  x\n};\n");
    assert!(
        e.message
            .contains("`result` block could not be parsed here"),
        "{}",
        e.message
    );
    let e = err("const a = result {\n  const <- f();\n  x\n};\n");
    assert!(
        e.message
            .contains("`result` block could not be parsed here"),
        "{}",
        e.message
    );
}

#[test]
fn result_block_is_an_expression_in_every_nested_position() {
    // Its own braces are skipped whole by the `try`/let-else/`if let`
    // expression scanners, so a block can sit inside them too.
    let out = ok(r#"
const s = `${result { const x <- f(); x }}`;
const t = g(result { const y <- h(); y + 1 }, 2);
function outer() {
  const w = try result { const q <- m(); q };
  return w;
}
"#);
    assert!(out.contains("const s = `${((() => {"), "{out}");
    assert!(out.contains("const t = g(((() => {"), "{out}");
    assert!(
        out.contains("const $rl_t2 = (((() => {") && out.contains("const w = $rl_t2.value;"),
        "{out}"
    );
}

/* ------------------------------------------------------------------ */
/* literal match patterns                                             */
/* ------------------------------------------------------------------ */

#[test]
fn literal_string_match_switches_on_the_scrutinee_itself() {
    let out = ok(r#"
const label = match (dir) {
  "north" => "N",
  "south" => "S",
  _ => "?",
};
"#);
    assert!(out.contains("const $rl_m = (dir);"));
    assert!(out.contains("switch ($rl_m) {"));
    assert!(!out.contains("$rl_m.kind"));
    assert!(out.contains(r#"case "north": { return ("N"); }"#));
    assert!(out.contains(r#"case "south": { return ("S"); }"#));
    assert!(out.contains(r#"default: { return ("?"); }"#));
}

#[test]
fn literal_number_match_emits_number_cases() {
    let out = ok(r#"
const message = match (status) {
  200 => "ok",
  404 => "not found",
  500 => "error",
  _ => "unknown",
};
"#);
    assert!(out.contains("switch ($rl_m) {"));
    assert!(out.contains(r#"case 200: { return ("ok"); }"#));
    assert!(out.contains(r#"case 404: { return ("not found"); }"#));
    assert!(out.contains(r#"case 500: { return ("error"); }"#));
}

#[test]
fn literal_boolean_match_emits_true_and_false_cases() {
    let out = ok("const v = match (flag) { true => 1, false => 0 };");
    assert!(out.contains("switch ($rl_m) {"));
    assert!(out.contains("case true: { return (1); }"));
    assert!(out.contains("case false: { return (0); }"));
}

#[test]
fn literal_or_pattern_shares_one_body_via_fallthrough() {
    let out = ok(r#"
const kind = match (code) {
  200 | 201 | 204 => "success",
  400 | 404 => "client error",
  _ => "unknown",
};
"#);
    assert!(out.contains(r#"case 200: case 201: case 204: { return ("success"); }"#));
    assert!(out.contains(r#"case 400: case 404: { return ("client error"); }"#));
    // one body per arm, never duplicated per alternative
    assert_eq!(out.matches(r#"return ("success")"#).count(), 1);
}

#[test]
fn literal_match_keeps_the_number_spelling_of_the_source() {
    let out = ok("const v = match (x) { 0xff => 1, 1_000 => 2, 1.5e2 => 3, -1 => 4, _ => 0 };");
    assert!(out.contains("case 0xff:"));
    assert!(out.contains("case 1_000:"));
    assert!(out.contains("case 1.5e2:"));
    assert!(out.contains("case -1:"));
}

#[test]
fn literal_match_without_a_wildcard_gets_a_runtime_guard() {
    let out = ok(r#"const label = match (dir) { "a" => 1, "b" => 2 };"#);
    assert!(out.contains(
        r#"default: { throw new Error("rl match: unexpected literal " + JSON.stringify($rl_m)); }"#
    ));
}

#[test]
fn literal_match_evaluates_the_scrutinee_once() {
    let out = ok(r#"const v = match (getValue()) { "a" => foo(), _ => bar() };"#);
    assert_eq!(out.matches("getValue()").count(), 1);
    assert!(out.contains("const $rl_m = (getValue());"));
}

#[test]
fn literal_match_block_bodies_break_out_of_the_switch() {
    let out = ok(r#"const v = match (s) { "a" => { return 1; }, _ => 0 };"#);
    assert!(out.contains(r#"case "a": { return 1;"#));
    assert!(out.contains("break; }"));
}

#[test]
fn literal_match_with_a_guard_becomes_an_if_chain() {
    let out = ok("const v = match (code) { 200 if ok => 1, 200 => 2, _ => 3 };");
    assert!(!out.contains("switch ("));
    assert!(out.contains("if ($rl_m === 200) { if ((ok)) return (1); }"));
    assert!(out.contains("if ($rl_m === 200) { return (2); }"));
}

#[test]
fn literal_or_pattern_if_chain_tests_each_alternative() {
    let out = ok(r#"const v = match (s) { "a" | "b" if ok => 1, _ => 2 };"#);
    assert!(out.contains(r#"if ($rl_m === "a" || $rl_m === "b")"#));
}

#[test]
fn literal_match_without_a_wildcard_has_no_if_chain_case_guard() {
    let out = ok("const v = match (code) { 200 if ok => 1, 404 => 2 };");
    assert!(
        out.contains(
            r#"throw new Error("rl match: unexpected literal " + JSON.stringify($rl_m));"#
        )
    );
}

#[test]
fn literal_duplicate_arm_is_error() {
    let e = err(r#"const v = match (x) { "a" => 1, "a" => 2 };"#);
    assert!(e.message.contains(r#"duplicate arm "a""#), "{}", e.message);
    assert_eq!((e.line, e.col), (1, 33));
}

#[test]
fn literal_duplicate_across_or_alternatives_is_error() {
    let e = err(r#"const v = match (x) { "a" | "b" => 1, "b" | "c" => 2 };"#);
    assert!(e.message.contains(r#"duplicate arm "b""#), "{}", e.message);
    assert_eq!((e.line, e.col), (1, 39));
}

#[test]
fn literal_duplicate_compares_values_not_spellings() {
    // `200`, `0xc8` and `2e2` are one `switch` case — `===` says so.
    let e = err("const v = match (x) { 200 => 1, 0xc8 => 2 };");
    assert!(e.message.contains("duplicate arm 200"), "{}", e.message);
    let e = err(r#"const v = match (x) { "a" => 1, '\x61' => 2 };"#);
    assert!(e.message.contains(r#"duplicate arm "a""#), "{}", e.message);
    let e = err("const v = match (x) { true => 1, true => 2 };");
    assert!(e.message.contains("duplicate arm true"), "{}", e.message);
}

#[test]
fn literal_duplicate_is_allowed_between_guarded_arms() {
    // A guard may be false, so a guarded arm covers nothing — the same rule
    // tag patterns follow.
    let out = ok("const v = match (x) { 1 if a => 1, 1 if b => 2, 1 => 3, _ => 4 };");
    assert_eq!(out.matches("$rl_m === 1").count(), 3);
}

#[test]
fn literal_and_tag_patterns_cannot_be_mixed() {
    let e = err(r#"const v = match (x) { Some(v) => v, "none" => 0 };"#);
    assert!(
        e.message
            .contains("cannot mix tag patterns and literal patterns"),
        "{}",
        e.message
    );
    assert_eq!((e.line, e.col), (1, 37));
}

#[test]
fn literal_and_tag_patterns_cannot_be_mixed_in_either_order() {
    let e = err(r#"const v = match (x) { "none" => 0, Some(v) => v };"#);
    assert!(
        e.message
            .contains("cannot mix tag patterns and literal patterns"),
        "{}",
        e.message
    );
}

#[test]
fn literal_or_pattern_alternatives_must_share_a_kind() {
    let e = err(r#"const v = match (x) { "a" | 1 => 1, _ => 2 };"#);
    assert!(e.message.contains("same kind of literal"), "{}", e.message);
    assert_eq!((e.line, e.col), (1, 29));
}

#[test]
fn literal_match_wildcard_must_be_last() {
    let e = err(r#"const v = match (x) { _ => 0, "a" => 1 };"#);
    assert!(e.message.contains("must be the last arm"), "{}", e.message);
    assert_eq!((e.line, e.col), (1, 23));
}

#[test]
fn literal_match_is_not_checked_against_enums() {
    // A literal match carries no tags, so the tag exhaustiveness pass must
    // not adopt it — `Option`/`Result` are always in the candidate table.
    let out = ok(r#"const v = match (x) { "Some" => 1, "None" => 2 };"#);
    assert!(out.contains("switch ($rl_m) {"));
}

#[test]
fn literal_patterns_nest_inside_arm_bodies() {
    let out = ok(r#"
const v = match (a) {
  "x" => match (b) { 1 => "one", _ => "other" },
  _ => "none",
};
"#);
    assert_eq!(out.matches("switch ($rl_m) {").count(), 2);
}

#[test]
fn literal_match_with_await_becomes_an_async_iife() {
    let out = ok(r#"async function f() { return match (s) { "a" => await g(), _ => null }; }"#);
    assert!(out.contains("(await (async () => {"));
}

#[test]
fn tuple_patterns_do_not_accept_literals() {
    // v1 keeps literals out of tuple positions (design §18): the arms fail
    // the tuple parse, and — with no `=>`-terminated single-match reading
    // either — the whole construct passes through untouched. (The text is
    // not valid TypeScript, so the output self-check is skipped here.)
    let opts = Options {
        verify: false,
        ..Options::default()
    };
    let src = r#"const v = match (a, b) { ("x", 1) => 1, _ => 0 };"#;
    assert_eq!(compile(src, &opts).unwrap(), src);
}

#[test]
fn a_block_of_literals_is_not_a_match() {
    // A call to a function named `match` followed by a block statement:
    // the arms have no `=>`, so the candidate is not claimed and the bytes
    // pass through.
    let src = "match (x)\n{ 1 }\n";
    assert_eq!(ok(src), src);
}

/* ------------------------------------------------------------------ */
/* val — binding modifier                                             */
/* ------------------------------------------------------------------ */

#[test]
fn plain_typescript_bindings_stay_mutable() {
    // The default is unchanged TypeScript semantics: without `val` every
    // mutation is legal and the bytes pass through.
    let src = "const x = { a: 1 };\nx.a = 2;\nlet y = { a: 1 };\ny.a = 2;\ny = { a: 3 };\n";
    assert_eq!(ok(src), src);
    let src = "function f(x: X) {\n  x.value = 1;\n}\n";
    assert_eq!(ok(src), src);
}

#[test]
fn val_declaration_modifier_is_erased_from_the_output() {
    assert_eq!(
        ok("val const user = getUser();\n"),
        "const user = getUser();\n"
    );
    assert_eq!(
        ok("val let state = getState();\n"),
        "let state = getState();\n"
    );
    assert_eq!(
        ok("export val const cfg = load();\n"),
        "export const cfg = load();\n"
    );
    // the keyword's trailing spaces go with it; a comment stays
    assert_eq!(ok("val\tconst a = 1;\n"), "const a = 1;\n");
    assert_eq!(ok("val /*c*/ const a = 1;\n"), "/*c*/ const a = 1;\n");
}

#[test]
fn val_parameter_modifier_is_erased_from_the_output() {
    assert_eq!(
        ok("function read(val user: User) {\n  return user.name;\n}\n"),
        "function read(user: User) {\n  return user.name;\n}\n",
    );
    assert_eq!(
        ok("const read = (val user: User) => user.name;\n"),
        "const read = (user: User) => user.name;\n",
    );
    assert_eq!(
        ok("function pick(a: A, val b: B, val { c }: C) {}\n"),
        "function pick(a: A, b: B, { c }: C) {}\n",
    );
}

#[test]
fn val_const_forbids_property_assignment() {
    let e = err("val const x = { a: 1 };\nx.a = 2;\n");
    assert_eq!((e.line, e.col), (2, 1));
    assert!(e.message.contains("cannot mutate through val binding `x`"));
}

#[test]
fn val_const_forbids_mutation_at_any_depth() {
    let e = err("val const x = { nested: { a: 1 } };\nx.nested.a = 2;\n");
    assert_eq!((e.line, e.col), (2, 1));
    let e = err("val const s = { u: { p: { n: 0 } } };\ns.u.p.n += 1;\n");
    assert_eq!((e.line, e.col), (2, 1));
    assert!(e.message.contains("val binding `s`"));
}

#[test]
fn val_let_forbids_mutation_but_allows_rebinding() {
    let e = err("val let state = { count: 0 };\nstate.count++;\n");
    assert_eq!((e.line, e.col), (2, 1));
    assert!(e.message.contains("`state`"));
    let src = "val let state = { count: 0 };\nstate = { ...state, count: state.count + 1 };\n";
    assert_eq!(ok(src), src.replacen("val ", "", 1));
}

#[test]
fn val_forbids_every_mutating_operator() {
    for stmt in [
        "x.a = 1;",
        "x.a += 1;",
        "x.a -= 1;",
        "x.a *= 1;",
        "x.a /= 1;",
        "x.a **= 2;",
        "x.a ||= 1;",
        "x.a &&= 1;",
        "x.a ??= 1;",
        "x.a >>= 1;",
        "x.a >>>= 1;",
        "x.a++;",
        "x.a--;",
        "++x.a;",
        "--x.a;",
        "delete x.a;",
        "x[0] = 1;",
        "x[0] += 1;",
        "x[0]++;",
        "delete x[0];",
        "x!.a = 1;",
        "x?.a.b = 1;",
    ] {
        let src = format!("val const x = load();\n{stmt}\n");
        let e = compile(&src, &Options::default())
            .err()
            .unwrap_or_else(|| panic!("expected `{stmt}` to be rejected"));
        assert!(
            e.message.contains("val binding `x`"),
            "{stmt}: {}",
            e.message
        );
    }
}

#[test]
fn val_leaves_reads_and_comparisons_alone() {
    // Nothing here mutates `x`, and none of these operators may be
    // mistaken for an assignment.
    let src = "val const x = load();\nconst r = [x.a == 1, x.a === 1, x.a != 1, x.a >= 1, x.a <= 1, x.a && 1, x.a || 1, x.a ?? 1, x.a + 1, x.a > 1];\nconst y = x.a;\nconst z = { ...x };\n";
    assert_eq!(ok(src), src.replacen("val ", "", 1));
}

#[test]
fn val_binding_replacement_is_not_a_val_error() {
    // Whether the binding itself can be replaced is `const`/`let`'s
    // question, not `val`'s — tsc reports the `const` case.
    let src = "val let a = 1;\na = 2;\nval const b = 1;\nb = 2;\n";
    assert_eq!(ok(src), src.replace("val ", ""));
}

#[test]
fn val_parameter_is_read_only_inside_the_function() {
    let e = err("function read(val user: User) {\n  user.name = \"Lee\";\n}\n");
    assert_eq!((e.line, e.col), (2, 3));
    let e = err("const read = (val user: User) => user.name = \"Lee\";\n");
    assert!(e.message.contains("val binding `user`"));
    // a parameter without the modifier keeps TypeScript's semantics
    let src = "function update(user: User) {\n  user.name = \"Lee\";\n}\n";
    assert_eq!(ok(src), src);
}

#[test]
fn val_parameter_positions_beyond_plain_identifiers() {
    let e = err("function foo(val { user }: Ctx) {\n  user.name = \"x\";\n}\n");
    assert!(e.message.contains("val binding `user`"));
    let e = err("for (val const item of items) {\n  item.a = 1;\n}\n");
    assert!(e.message.contains("val binding `item`"));
    let e = err("try {\n  f();\n} catch (val error: any) {\n  error.code = 1;\n}\n");
    assert!(e.message.contains("val binding `error`"));
    let e = err("class B {\n  constructor(private val inner: I) {\n    inner.a = 1;\n  }\n}\n");
    assert!(e.message.contains("val binding `inner`"));
}

#[test]
fn val_argument_may_only_reach_a_val_parameter() {
    let src = "\
function read(val user: User) { log(user.name); }
function update(user: User) { user.name = \"Lee\"; }
function process(val user: User) {
  read(user);
}
";
    assert_eq!(ok(src), src.replace("val ", ""));

    let e = err("\
function read(val user: User) { log(user.name); }
function update(user: User) { user.name = \"Lee\"; }
function process(val user: User) {
  update(user);
}
");
    assert_eq!((e.line, e.col), (4, 10));
    assert!(
        e.message
            .contains("cannot pass val binding `user` to mutable parameter `user` of `update`"),
        "{}",
        e.message,
    );
}

#[test]
fn a_mutable_argument_may_reach_any_parameter() {
    let src = "\
function read(val user: User) { log(user.name); }
function update(user: User) { user.name = \"Lee\"; }
function process(user: User) {
  read(user);
  update(user);
}
";
    assert_eq!(ok(src), src.replace("val ", ""));
}

#[test]
fn val_capability_flows_through_arrow_declarations() {
    let e = err("\
const update = (user: User) => { user.name = \"x\"; };
function process(val user: User) {
  update(user);
}
");
    assert!(e.message.contains("mutable parameter `user` of `update`"));
}

#[test]
fn val_is_an_access_path_restriction_not_object_immutability() {
    // An alias keeps its own capability: the `val` binding cannot mutate,
    // the original binding still can.
    let src = "let original = { count: 0 };\nval const view = original;\noriginal.count++;\n";
    assert_eq!(ok(src), src.replacen("val ", "", 1));
    let e = err("let original = { count: 0 };\nval const view = original;\nview.count++;\n");
    assert_eq!((e.line, e.col), (3, 1));
    assert!(e.message.contains("val binding `view`"));
}

#[test]
fn an_inner_declaration_shadows_an_outer_val() {
    let src = "val const x = { a: 1 };\n{\n  const x = { a: 2 };\n  x.a = 3;\n}\n";
    assert_eq!(ok(src), src.replacen("val ", "", 1));
    let src = "val const cfg = { a: 1 };\nfunction f(cfg: C) {\n  cfg.a = 2;\n}\n";
    assert_eq!(ok(src), src.replacen("val ", "", 1));
    // ... and the outer binding is still `val` after the inner scope ends
    let e = err("val const x = { a: 1 };\n{\n  const x = { a: 2 };\n  x.a = 3;\n}\nx.a = 4;\n");
    assert_eq!((e.line, e.col), (6, 1));
}

#[test]
fn val_never_calls_a_method_a_mutation_from_its_name() {
    // Whether `q.set(k)` mutates depends on what `q` is, and `compile` has
    // no types: a user-defined `set`/`add`/`push` must not be rejected on
    // its name alone (TASK-071). The typed path decides — see
    // `val_probes_collect_every_method_call_for_the_verdict` below and the
    // `--types` tests in tests/cli.rs.
    for src in [
        "class Query {\n  set(key: string): Query {\n    return new Query();\n  }\n}\nval const query = new Query();\nquery.set(\"name\");\n",
        "class Collection {\n  add(v: number): Collection {\n    return new Collection();\n  }\n}\nval const collection = new Collection();\ncollection.add(1);\n",
        // ... and neither are the built-in shapes, without types to prove it
        "val const items: number[] = [];\nitems.push(1);\n",
        "val const m = new Map<string, number>();\nm.set(\"a\", 1);\n",
        "val const s = { u: { p: { tags: [] as string[] } } };\ns.u.p.tags.push(\"rl\");\n",
        // reading methods were never in question
        "val const items: number[] = [];\nconst n = items.map((v) => v).filter(Boolean).length;\n",
    ] {
        assert_eq!(ok(src), src.replacen("val ", "", 1), "{src}");
    }
}

#[test]
fn val_probes_collect_every_method_call_for_the_verdict() {
    // The delegated form collects method calls whatever they are called:
    // the mutator-name policy is applied at the verdict, beside the
    // checker's built-in answer, so a name outside the policy can never
    // hide a question — and never make a report on its own.
    const SRC: &str = "\
val const d = mk();
d.setHours(1);
d.at(0);
d.count = 2;
";
    let probes = rlc::val_probes(SRC);
    let seen: Vec<(&str, Option<&str>)> = probes
        .mutations
        .iter()
        .map(|m| (m.name.as_str(), m.method.as_ref().map(|(n, _)| n.as_str())))
        .collect();
    assert_eq!(
        seen,
        [("d", Some("setHours")), ("d", Some("at")), ("d", None),]
    );
    // The policy half of the verdict, stated as the library's own answer.
    assert!(rlc::is_builtin_mutator_name("push"));
    assert!(!rlc::is_builtin_mutator_name("at"));
    assert!(!rlc::is_builtin_mutator_name("get"));
}

#[test]
fn val_probes_carry_the_callee_and_the_declarations_it_may_name() {
    // The call-capability check's pairing is delegated: probes hand over
    // every declaration (as a node) and every call's callee (as a node),
    // and which call names which declaration is symbol identity — so
    // nothing is matched by name here, and an "ambiguous" name is not a
    // concept collection needs.
    const SRC: &str = "\
val const user = { name: \"a\" };
function handle(u: { name: string }): void {}
handle(user);
handle(user.name, user);
";
    let probes = rlc::val_probes(SRC);
    assert_eq!(probes.functions.len(), 1);
    let function = &probes.functions[0];
    assert_eq!(function.name, "handle");
    assert_eq!(&SRC[function.ident..function.ident + 6], "handle");
    assert_eq!(
        function.params,
        vec![rlc::ValParam {
            name: Some("u".into()),
            is_val: false,
        }]
    );
    let seen: Vec<(&str, &str, usize)> = probes
        .passes
        .iter()
        .map(|p| (p.name.as_str(), p.callee.as_str(), p.arg_index))
        .collect();
    // Every plain-path argument is collected with its position — including
    // `user.name` at index 0 and `user` at index 1 of the second call.
    assert_eq!(
        seen,
        [
            ("user", "handle", 0),
            ("user", "handle", 0),
            ("user", "handle", 1),
        ]
    );
    for pass in &probes.passes {
        assert_eq!(&SRC[pass.callee_at..pass.callee_at + 6], "handle");
    }
}

#[test]
fn a_type_argument_list_does_not_declare_a_val_binding() {
    // `<...>` is not a bracket the scanner matches, so the comma in
    // `Map<string, number>` used to look like a declarator separator and
    // made `number` a val binding — after which the `number[]` of a later
    // annotation read as a mutation (TASK-071).
    let src = "val const m = new Map<string, number>();\nval const items: number[] = [];\n";
    assert_eq!(ok(src), src.replace("val ", ""));
    // multi-declarator forms still bind every name
    let e = err("val let a, b, c;\nb.x = 1;\n");
    assert!(e.message.contains("val binding `b`"), "{}", e.message);
    let e = err("val const p = 1, q = { n: 0 };\nq.n = 2;\n");
    assert!(e.message.contains("val binding `q`"), "{}", e.message);
}

#[test]
fn val_is_checked_inside_nested_rl_constructs() {
    let e =
        err("val const cfg = { a: 1 };\nconst msg = `${(() => { cfg.a = 2; return 1; })()}`;\n");
    assert!(e.message.contains("val binding `cfg`"));
    let e = err("\
enum Shape { Circle(r: number), Point }
val const s = Shape.Circle(1);
const v = match (s) {
  Circle(r) => { s.kind = \"Point\"; return r; },
  Point => 0,
};
");
    assert!(e.message.contains("val binding `s`"));
}

#[test]
fn val_on_a_let_else_pattern_covers_the_names_it_binds() {
    let e = err("\
enum Opt { Some(value: Box), None }
function f(o: Opt) {
  val const Some(value) = o else { return; };
  value.n = 1;
}
");
    assert!(e.message.contains("val binding `value`"), "{}", e.message);
}

#[test]
fn val_capability_check_only_covers_resolvable_callees() {
    // An imported (or otherwise unknown) function has no signature rlc can
    // read, so passing a `val` binding to it is allowed — the documented
    // limit of the check (language.md §10.7).
    let src = "import { save } from \"./io.js\";\nfunction f(val user: User) {\n  save(user);\n  user.save();\n}\n";
    assert_eq!(ok(src), src.replacen("val ", "", 1));
    // A name declared twice with different signatures is ambiguous and
    // drops out of the table rather than guessing.
    let src = "\
function apply(user: User) {}
function apply(val user: User) {}
function f(val user: User) {
  apply(user);
}
";
    assert_eq!(ok(src), src.replace("val ", ""));
    // A computed argument is not an access path, so it is not checked.
    let src = "\
function update(user: User) { user.name = \"x\"; }
function f(val user: User) {
  update({ ...user });
}
";
    assert_eq!(ok(src), src.replacen("val ", "", 1));
}

#[test]
fn val_capability_check_reads_annotated_declarators() {
    let e = err("\
type Handler = (u: Box) => void;
const update: Handler = (u) => { u.n = 1; };
function f(val b: Box) {
  update(b);
}
");
    assert!(
        e.message.contains("mutable parameter `u` of `update`"),
        "{}",
        e.message
    );
}

/* ------------------------------------------------------------------ */
/* name resolution (TASK-102)                                          */
/* ------------------------------------------------------------------ */

#[test]
fn misspelled_case_in_a_match_arm_names_the_case_meant() {
    let e = err(r#"enum Shape { Circle(radius: number), Empty }
const a = match (s) {
  Circel(radius) => radius,
  Empty => 0,
};
"#);
    assert!(
        e.message
            .contains("enum Shape has no case `Circel` — did you mean `Circle`?"),
        "{}",
        e.message
    );
    // reported at the tag, not at the match
    assert_eq!((e.line, e.col), (3, 3));
}

#[test]
fn a_misspelled_case_is_reported_instead_of_the_exhaustiveness_it_breaks() {
    // The typo removes Shape from the candidate table, which used to turn
    // the exhaustiveness check off *silently* — the bug this pass fixes.
    let e = err(r#"enum Shape { Circle(radius: number), Empty }
const a = match (s) { Circel(radius) => radius, Empty => 0 };
"#);
    assert!(e.message.contains("has no case `Circel`"), "{}", e.message);
    assert!(!e.message.contains("exhaustive"), "{}", e.message);
}

#[test]
fn misspelled_case_in_let_else_and_if_let_is_reported() {
    let e = err(r#"enum Shape { Circle(radius: number), Empty }
function f(): number {
  const Circel(radius) = s else { return 0; };
  return radius;
}
"#);
    assert!(e.message.contains("has no case `Circel`"), "{}", e.message);

    let e = err(r#"enum Shape { Circle(radius: number), Empty }
if let Circel(radius) = s { log(radius); }
"#);
    assert!(e.message.contains("has no case `Circel`"), "{}", e.message);
    assert_eq!((e.line, e.col), (2, 8));
}

#[test]
fn misspelled_field_names_the_field_meant() {
    let e = err(r#"enum Shape { Circle(radius: number), Empty }
const a = match (s) { Circle(radiuz) => radiuz, Empty => 0 };
"#);
    assert!(
        e.message
            .contains("enum Shape: case `Circle` has no field `radiuz` — did you mean `radius`?"),
        "{}",
        e.message
    );
}

#[test]
fn misspelled_field_is_reported_in_let_else_too() {
    let e = err(r#"enum Shape { Circle(radius: number), Empty }
function f(): number {
  const Circle(radiuz) = s else { return 0; };
  return radiuz;
}
"#);
    assert!(e.message.contains("has no field `radiuz`"), "{}", e.message);
}

#[test]
fn misspelled_case_of_a_nested_pattern_is_resolved_through_the_field_type() {
    let e = err(r#"enum Inner { Yes(n: number), No }
enum Outer { Wrap(inner: Inner), Bare }
const a = match (o) { Wrap(inner: Yess(n)) => n, Bare => 0 };
"#);
    assert!(
        e.message
            .contains("enum Inner has no case `Yess` — did you mean `Yes`?"),
        "{}",
        e.message
    );
}

#[test]
fn a_misspelled_builtin_case_is_reported() {
    let e = err("const n = match (o) { Some(value) => value, Non => 0 };\n");
    assert!(
        e.message
            .contains("built-in enum Option has no case `Non` — did you mean `None`?"),
        "{}",
        e.message
    );
}

#[test]
fn a_misspelled_case_of_an_imported_enum_names_its_origin() {
    let externs = [token_extern()];
    let opts = Options {
        extern_enums: &externs,
        ..Options::default()
    };
    let e = compile(
        "const s = match (t) { Num(value) => value, Idnet(name) => 0, Eof => -1 };\n",
        &opts,
    )
    .expect_err("expected a resolution error");
    assert!(
        e.message.contains(
            "enum Token (imported from \"./token.rl\") has no case `Idnet` — did you mean `Ident`?"
        ),
        "{}",
        e.message
    );
}

#[test]
fn tags_of_a_hand_written_union_are_not_resolution_errors() {
    // A tag pattern matches any `kind`-tagged union (language.md §3.2), so
    // names no declaration table holds are not wrong — they are the point.
    let out = ok(
        r#"type Msg = { kind: "Ping" } | { kind: "Pong"; n: number };
const a = match (m) { Ping => 0, Pong(n) => n, _ => -1 };
"#,
    );
    assert!(out.contains("case \"Ping\""));
}

#[test]
fn a_shared_tag_name_does_not_drag_an_unrelated_union_into_an_enum() {
    // `Empty` is also a Shape case, so the analysis identifies Shape — but
    // `Full` is nobody's misspelling, so nothing is reported.
    let out = ok(r#"enum Shape { Circle(radius: number), Empty }
type Msg = { kind: "Empty" } | { kind: "Full"; n: number };
const a = match (m) { Empty => 0, Full(n) => n };
"#);
    assert!(out.contains("case \"Full\""));
}

#[test]
fn a_hand_written_payload_field_is_not_a_misspelling() {
    // The tags are exactly Option's, so the analysis reads Option's
    // declaration — but `v` is not `value` misspelled, so it stays quiet.
    let out = ok("const n = match (o) { Some(v) => v, None => 0 };\n");
    assert!(out.contains("const { v } = $rl_m"));
}

#[test]
fn a_two_edit_case_typo_needs_a_match_to_corroborate_the_enum() {
    // `Cyrcla` is two edits from `Circle`. In a match another arm names
    // the enum, so the typo is reported...
    let e = err(r#"enum Shape { Circle(radius: number), Empty }
const a = match (s) { Cyrcla(radius) => radius, Empty => 0 };
"#);
    assert!(e.message.contains("has no case `Cyrcla`"), "{}", e.message);

    // ...but a let-else has only its own tag, so two edits are not enough
    // evidence that this is Shape at all. One edit is (`Circel` above).
    let out = ok(r#"enum Shape { Circle(radius: number), Empty }
function f(): number {
  const Cyrcla(radius) = s else { return 0; };
  return radius;
}
"#);
    assert!(out.contains("\"Cyrcla\""));
}

#[test]
fn a_misspelled_case_in_a_tuple_match_position_is_reported() {
    // Payload cases make these rl enums rather than TypeScript ones.
    let e = err(r#"enum Dir { North(dx: number), South }
enum Speed { Fast(v: number), Slow }
const n = match (d, s) {
  (North(dx), Fast(v)) => dx + v,
  (Nrth(dx), Slow) => dx,
  (South, _) => 3,
};
"#);
    assert!(
        e.message
            .contains("enum Dir has no case `Nrth` — did you mean `North`?"),
        "{}",
        e.message
    );
}

/* ------------------------------------------------------------------ */
/* exhaustiveness by usefulness (TASK-103)                             */
/* ------------------------------------------------------------------ */

#[test]
fn nested_patterns_that_cover_the_payload_are_exhaustive() {
    // The old rule counted tags, so an arm with a nested pattern covered
    // nothing and this exhaustive match was rejected.
    let out = ok(r#"enum Inner { Yes(n: number), No }
enum Outer { Wrap(inner: Inner), Bare }
const a = match (o) {
  Wrap(inner: Yes(n)) => n,
  Wrap(inner: No()) => 0,
  Bare => -1,
};
"#);
    assert!(out.contains("$rl_m.inner.kind === \"Yes\""), "{out}");
}

#[test]
fn a_generic_payload_is_typed_by_the_patterns_written_in_it() {
    // `Ok`'s payload is declared `T`, which names no enum — but `Some`
    // and `None` written there name Option, exactly as arm tags name a
    // match's subject.
    let out = ok(r#"const n = match (r) {
  Ok(value: Some(value: v)) => v,
  Ok(value: None()) => 0,
  Err(error) => -1,
};
"#);
    assert!(out.contains("$rl_m.value.kind === \"Some\""), "{out}");
}

#[test]
fn a_witness_names_the_value_that_is_missing() {
    let e = err(r#"enum Inner { Yes(n: number), No }
enum Outer { Wrap(inner: Inner), Bare }
const a = match (o) { Wrap(inner: Yes(n)) => n, Bare => -1 };
"#);
    assert!(
        e.message.contains("missing \"Wrap(inner: No())\""),
        "{}",
        e.message
    );
}

#[test]
fn a_fully_guarded_match_still_names_every_case() {
    // No arm covers anything, so every constructor is a witness — the
    // column has no wildcard row to hide behind.
    let e =
        err("const f = (o: Option<number>) => match (o) { Some(value) if value > 0 => value };\n");
    assert!(
        e.message.contains("missing \"Some\", \"None\""),
        "{}",
        e.message
    );
}

#[test]
fn deeply_nested_exhaustiveness_terminates_and_answers() {
    // Three levels of payload, one hole at the bottom.
    let e = err(r#"enum A { A1(b: B), A2 }
enum B { B1(c: C), B2 }
enum C { C1(n: number), C2 }
const v = match (a) {
  A1(b: B1(c: C1(n))) => n,
  A1(b: B2()) => 2,
  A2 => 3,
};
"#);
    assert!(
        e.message.contains("missing \"A1(b: B1(c: C2()))\""),
        "{}",
        e.message
    );
}

#[test]
fn a_witness_can_be_pasted_back_as_an_arm() {
    // The message promises a pattern, not a description: whatever it names
    // must compile as the arm that covers it. A nested unit case is where
    // that promise used to break — `inner: No` *binds* the field to a name
    // called `No`, so the arm compiled and covered every `Wrap`.
    let base = r#"enum Inner { Yes(n: number), No }
enum Outer { Wrap(inner: Inner), Bare }
declare const o: Outer;
const a = match (o) {
  Wrap(inner: Yes(n)) => n,
  Bare => -1,
};
"#;
    let reported = err(base).message;
    let witnesses: Vec<&str> = reported.split('"').skip(1).step_by(2).collect();
    assert_eq!(witnesses, ["Wrap(inner: No())"], "{reported}");

    let arms: String = witnesses.iter().map(|w| format!("  {w} => 0,\n")).collect();
    let pasted = base.replace("  Bare => -1,\n", &format!("  Bare => -1,\n{arms}"));
    let out = ok(&pasted);
    // ...and it really is the No case, not a binding that swallows Wrap.
    assert!(out.contains("$rl_m.inner.kind === \"No\""), "{out}");
}

/* ------------------------------------------------------------------ */
/* diagnostic ranges (TASK-116)                                        */
/* ------------------------------------------------------------------ */

/// The source text an error's range covers — what an editor underlines.
fn covered(src: &str, e: &rlc::CompileError) -> String {
    let offset = |line: usize, col: usize| {
        src.split_inclusive('\n')
            .take(line - 1)
            .map(str::len)
            .sum::<usize>()
            + col
            - 1
    };
    src[offset(e.line, e.col)..offset(e.end_line, e.end_col)].to_string()
}

#[test]
fn a_non_exhaustive_match_covers_its_head() {
    // The head — not the word the position lands on, and not the arms
    // below it, which are the user's own code.
    let src = "enum S { A(x: number), B }\nconst v = match (s) { A(x) => x };\n";
    let e = err(src);
    assert!(e.message.contains("is not exhaustive"), "{}", e.message);
    assert_eq!((e.line, e.col), (2, 11));
    assert_eq!(covered(src, &e), "match (s)");
}

#[test]
fn a_tuple_match_covers_every_scrutinee() {
    let src = "enum S { A(x: number), B }\nenum T { C(), D }\nconst v = match (s, t) { (A(x), C) => x };\n";
    let e = err(src);
    assert!(e.message.contains("is not exhaustive"), "{}", e.message);
    assert_eq!(covered(src, &e), "match (s, t)");
}

#[test]
fn a_duplicate_arm_covers_the_tag_it_repeats() {
    let src = "enum S { A(x: number), B }\nconst v = match (s) { A(x) => x, A(x) => 0, B => 1 };\n";
    let e = err(src);
    assert!(e.message.contains("duplicate arm"), "{}", e.message);
    assert_eq!(covered(src, &e), "A");
}

#[test]
fn a_misspelled_case_covers_the_name_as_written() {
    let src = "enum Shape { Circle(r: number), Square(s: number) }\nconst v = match (s) { Circel(r) => r, Square(s) => s };\n";
    let e = err(src);
    assert!(e.message.contains("did you mean"), "{}", e.message);
    assert_eq!(covered(src, &e), "Circel");
}

#[test]
fn a_misplaced_try_covers_the_propagation() {
    let src = "const x = match (r) {\n  Ok(v) => { const y = try f(v); return y; },\n  Err(e) => 0,\n};\n";
    let e = err(src);
    assert!(e.message.contains("`try` cannot be used"), "{}", e.message);
    assert_eq!(covered(src, &e), "try f(v)");
}

#[test]
fn a_val_mutation_covers_the_binding() {
    let src = "function f() {\n  val const cfg = { a: 1 };\n  cfg.a = 2;\n}\n";
    let e = err(src);
    assert!(e.message.contains("cannot mutate"), "{}", e.message);
    assert_eq!(covered(src, &e), "cfg");
}

#[test]
fn an_error_without_a_known_extent_reports_a_position_only() {
    // No end means "the consumer decides the width" — the editor then
    // underlines the word at the position, as it always has.
    let src = "enum S { A(x: number), B }\nconst v = match (s) { A(x) => x, _ => 0, B => 1 };\n";
    let e = err(src);
    assert!(e.message.contains("must be the last arm"), "{}", e.message);
    assert_eq!(covered(src, &e), "_");
}

/* ------------------------------------------------------------------ */
/* multiple diagnostics (TASK-120)                                     */
/* ------------------------------------------------------------------ */

#[test]
fn analyze_reports_every_uncovered_match_in_source_order() {
    // TASK-117 symptom 1: the default path used to report one error per
    // run; tsc and rustc report them all.
    let src = "enum Shape { Circle(r: number), Square(s: number), Tri(a: number) }\n\
        export function f(x: Shape): number {\n  return match (x) { Circle(r) => r };\n}\n\
        export function g(x: Shape): number {\n  return match (x) { Square(s) => s };\n}\n\
        export function h(x: Shape): number {\n  return match (x) { Tri(a) => a };\n}\n";
    let diagnostics = rlc::analyze(src, &Options::default());
    assert_eq!(diagnostics.len(), 3, "{diagnostics:#?}");
    assert!(
        diagnostics
            .iter()
            .all(|d| d.code == rlc::DiagnosticCode::MatchNotExhaustive)
    );
    let starts: Vec<usize> = diagnostics.iter().map(|d| d.start.unwrap()).collect();
    let mut sorted = starts.clone();
    sorted.sort_unstable();
    assert_eq!(starts, sorted, "diagnostics arrive in source order");
    assert!(
        diagnostics[0]
            .message
            .contains("missing \"Square\", \"Tri\"")
    );
    assert!(
        diagnostics[1]
            .message
            .contains("missing \"Circle\", \"Tri\"")
    );
    assert!(
        diagnostics[2]
            .message
            .contains("missing \"Circle\", \"Square\"")
    );
}

#[test]
fn a_duplicate_arm_does_not_hide_the_files_other_diagnostics() {
    // TASK-117 symptom 3, the rl half: one recoverable error used to stop
    // the whole check.
    let src = "enum Shape { Circle(r: number), Square(s: number), Tri(a: number) }\n\
        export function f(x: Shape): number {\n\
          return match (x) { Circle(r) => r, Circle(r) => 0, Square(s) => s, Tri(a) => a };\n\
        }\n\
        export function g(x: Shape): number { return match (x) { Square(s) => s }; }\n\
        export function h(x: Shape): number { return match (x) { Tri(a) => a }; }\n";
    let diagnostics = rlc::analyze(src, &Options::default());
    let codes: Vec<_> = diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(
        codes,
        [
            rlc::DiagnosticCode::MatchDuplicateArm,
            rlc::DiagnosticCode::MatchNotExhaustive,
            rlc::DiagnosticCode::MatchNotExhaustive,
        ],
        "{diagnostics:#?}"
    );
}

#[test]
fn a_typo_suppresses_coverage_for_its_own_match_only() {
    // The recovery boundary is the match, not the file: `Circel` silences
    // f's exhaustiveness question (the typo is the cause), while g's hole
    // is still reported.
    let src = "enum Shape { Circle(r: number), Empty }\n\
        export function f(x: Shape): number {\n\
          return match (x) { Circel(r) => r, Empty => 0 };\n\
        }\n\
        export function g(x: Shape): number { return match (x) { Empty => 0 }; }\n";
    let diagnostics = rlc::analyze(src, &Options::default());
    let codes: Vec<_> = diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(
        codes,
        [
            rlc::DiagnosticCode::UnknownCase,
            rlc::DiagnosticCode::MatchNotExhaustive,
        ],
        "{diagnostics:#?}"
    );
    assert!(diagnostics[0].message.contains("did you mean `Circle`"));
    assert!(diagnostics[1].message.contains("missing \"Circle\""));
}

#[test]
fn sema_and_val_diagnostics_merge_in_source_order() {
    let src = "enum E { A(x: number), B }\n\
        val const cfg = { a: 1 };\n\
        cfg.a = 2;\n\
        const v = match (E.B) { B => 0 };\n";
    let diagnostics = rlc::analyze(src, &Options::default());
    let codes: Vec<_> = diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(
        codes,
        [
            rlc::DiagnosticCode::ValMutation,
            rlc::DiagnosticCode::MatchNotExhaustive,
        ],
        "{diagnostics:#?}"
    );
}

#[test]
fn compile_still_returns_the_first_error_in_source_order() {
    let src = "enum Shape { Circle(r: number), Square(s: number) }\n\
        export function f(x: Shape): number {\n  return match (x) { Circle(r) => r };\n}\n\
        export function g(x: Shape): number {\n  return match (x) { Square(s) => s };\n}\n";
    let e = err(src);
    assert_eq!(
        e.line, 3,
        "the first uncovered match decides compile()'s error"
    );
    assert!(e.message.contains("missing \"Square\""));
}

#[test]
fn compile_report_still_emits_under_recoverable_errors() {
    // Codegen is infallible, so a duplicate arm does not withhold the
    // lowered TypeScript — that is what lets the typed pass run and report
    // alongside the rl errors (TASK-117 symptom 3).
    let src = "enum E { A(x: number), B }\n\
        const v = match (E.A(1)) { A(x) => x, A(x) => 0, B => 1 };\n";
    let report = rlc::compile_report(src, &Options::default());
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(
        report.diagnostics[0].code,
        rlc::DiagnosticCode::MatchDuplicateArm
    );
    let emit = report.emit.expect("recoverable errors still emit");
    assert!(emit.code.contains("switch ($rl_m.kind)"));
}

#[test]
fn compile_report_withholds_emission_when_the_output_cannot_be_typescript() {
    // A stray `|>` passes through verbatim, so the output would not parse:
    // that diagnostic blocks projection.
    let src = "const x = 1 |> ;\n";
    let report = rlc::compile_report(src, &Options::default());
    assert!(report.emit.is_none());
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == rlc::DiagnosticCode::StrayPipe),
        "{:#?}",
        report.diagnostics
    );
}

#[test]
fn every_stray_construct_is_reported_not_just_the_first() {
    let src = "const x = 1 |> ;\nconst y = 2 |> ;\n";
    let diagnostics = rlc::analyze(src, &Options::default());
    let strays = diagnostics
        .iter()
        .filter(|d| d.code == rlc::DiagnosticCode::StrayPipe)
        .count();
    assert_eq!(strays, 2, "{diagnostics:#?}");
}

#[test]
fn a_mixed_match_reports_the_cause_and_suppresses_its_coverage() {
    // The mixed-pattern error is the cause; that match's own exhaustiveness
    // answer would be an effect stacked on it.
    let src = "const v = match (x) {\n  Some(v) => v,\n  1 => 0,\n};\n";
    let diagnostics = rlc::analyze(src, &Options::default());
    let codes: Vec<_> = diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(
        codes,
        [rlc::DiagnosticCode::MatchMixedPatterns],
        "{diagnostics:#?}"
    );
}

#[test]
fn diagnostic_codes_are_stable_strings() {
    assert_eq!(
        rlc::DiagnosticCode::MatchNotExhaustive.as_str(),
        "match-not-exhaustive"
    );
    assert_eq!(rlc::DiagnosticCode::ValMutation.as_str(), "val-mutation");
    assert!(rlc::DiagnosticCode::StrayPipe.blocks_projection());
    assert!(!rlc::DiagnosticCode::MatchDuplicateArm.blocks_projection());
}
