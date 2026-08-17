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
            .contains("or-pattern alternatives must bind the same fields"),
        "{}",
        e.message
    );
    assert_eq!((e.line, e.col), (1, 30)); // points at the offending alternative

    // an alias changes the bound name, so it must match too
    let e = err("const r = match (x) { A(v) | B(v: w) => w, _ => 0 };");
    assert!(
        e.message
            .contains("or-pattern alternatives must bind the same fields"),
        "{}",
        e.message
    );

    // a binding-free alternative cannot pair with a binding one
    let e = err("const r = match (x) { A | B(v) => 1, _ => 0 };");
    assert!(
        e.message
            .contains("or-pattern alternatives must bind the same fields"),
        "{}",
        e.message
    );
}

#[test]
fn or_pattern_double_pipe_is_not_rl_syntax() {
    // `A || B` is not an or-pattern; the candidate fails to parse and the
    // (invalid-TS) text passes through to the output self-check.
    let e = err("const r = match (x) { A || B => 1 };");
    assert!(
        e.message.contains("generated TypeScript failed to parse"),
        "{}",
        e.message
    );
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
        e.message.contains("generated TypeScript failed to parse"),
        "{}",
        e.message
    );
}

#[test]
fn try_inside_guard_is_an_error() {
    let e = err(
        "const r = match (x) {\n  A(v) if run(() => { try g(); return true; }) => v,\n  _ => 0,\n};\n",
    );
    assert!(
        e.message.contains("`try` cannot be used inside"),
        "{}",
        e.message
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
    assert!(
        e.message.contains("generated TypeScript failed to parse"),
        "{}",
        e.message
    );
}

#[test]
fn try_inside_match_arm_is_an_error() {
    let e = err(
        "const x = match (r) {\n  Ok(value) => { const y = try f(value); return y; },\n  Err(error) => fallback(error),\n};\n",
    );
    assert!(
        e.message.contains("`try` cannot be used inside"),
        "{}",
        e.message
    );
    assert_eq!((e.line, e.col), (2, 18)); // points at the `const`
}

#[test]
fn try_inside_match_scrutinee_is_an_error() {
    let e = err(
        "const x = match (run(() => { try g(); return h(); })) {\n  Ok(value) => value,\n  Err(error) => 0,\n};\n",
    );
    assert!(
        e.message.contains("`try` cannot be used inside"),
        "{}",
        e.message
    );
}

#[test]
fn try_inside_template_interpolation_is_an_error() {
    let e = err("const s = `${run(() => { try g(); return h(); })}`;\n");
    assert!(
        e.message.contains("`try` cannot be used inside"),
        "{}",
        e.message
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
fn let_else_non_diverging_else_is_error() {
    let e =
        err("function f(): number {\n  const Some(v) = find() else { log(); };\n  return v;\n}\n");
    assert!(
        e.message.contains("must end with a `return`"),
        "{}",
        e.message
    );
    assert_eq!((e.line, e.col), (2, 26)); // points at the `else` keyword
}

#[test]
fn let_else_empty_else_block_is_error() {
    let e = err("function f(): number {\n  const Some(v) = find() else { };\n  return v;\n}\n");
    assert!(
        e.message.contains("must end with a `return`"),
        "{}",
        e.message
    );
}

#[test]
fn let_else_inside_match_arm_is_error() {
    let e = err(
        "const x = match (r) {\n  Ok(value) => { const Some(v) = h(value) else { return 0; }; return v; },\n  _ => 0,\n};\n",
    );
    assert!(
        e.message.contains("let-else cannot be used inside"),
        "{}",
        e.message
    );
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
fn try_inside_a_pipeline_step_is_an_error() {
    let e = err("const a = x |> (n => { const b = try f(n); return b; });\n");
    assert!(e.message.contains("`try` cannot be used"), "{}", e.message);
}
