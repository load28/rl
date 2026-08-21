//! Contract tests for `emit_mapped` (TASK-050): the tooling emission that
//! feeds the language server's virtual TypeScript documents.
//!
//! The load-bearing invariant is that every mapping points at bytes that
//! are *identical* in source and output — the language server relies on it
//! to translate offsets in both directions without ever re-parsing.

use rlc::{EmitMapping, ImportRewrite, Options, compile, emit_mapped};

/// Every mapping's chunk must read the same in source and output, chunks
/// must be in-bounds, and must not overlap in either coordinate space.
fn assert_mapping_invariants(src: &str, m: &rlc::MappedEmit) {
    let mut by_src = m.mappings.clone();
    by_src.sort_by_key(|e| e.src);
    let mut by_out = m.mappings.clone();
    by_out.sort_by_key(|e| e.out);
    for e in &m.mappings {
        assert!(e.len > 0, "empty mapping {e:?}");
        assert!(e.src + e.len <= src.len(), "src out of bounds: {e:?}");
        assert!(e.out + e.len <= m.code.len(), "out out of bounds: {e:?}");
        assert_eq!(
            &src[e.src..e.src + e.len],
            &m.code[e.out..e.out + e.len],
            "mapped chunk differs between source and output: {e:?}"
        );
    }
    for w in by_src.windows(2) {
        assert!(
            w[0].src + w[0].len <= w[1].src,
            "overlapping source mappings: {:?} / {:?}",
            w[0],
            w[1]
        );
    }
    for w in by_out.windows(2) {
        assert!(
            w[0].out + w[0].len <= w[1].out,
            "overlapping output mappings: {:?} / {:?}",
            w[0],
            w[1]
        );
    }
}

/// The output byte range a source byte falls in, if it was copied verbatim.
fn map_offset(m: &rlc::MappedEmit, src_offset: usize) -> Option<usize> {
    m.mappings
        .iter()
        .find(|e| src_offset >= e.src && src_offset < e.src + e.len)
        .map(|e| e.out + (src_offset - e.src))
}

#[test]
fn passthrough_maps_identity() {
    let src = "const n: number = 1;\nconsole.log(n);\n";
    let m = emit_mapped(src);
    assert_eq!(m.code, src);
    assert_eq!(
        m.mappings,
        [EmitMapping {
            src: 0,
            out: 0,
            len: src.len()
        }]
    );
}

#[test]
fn match_scrutinee_and_arm_bodies_are_mapped() {
    let src = r#"enum Shape { Circle(radius: number), Point }
const shape = Shape.Point;
const r = match (shape) {
  Circle(radius) => radius * 2,
  Point => 0,
};
"#;
    let m = emit_mapped(src);
    assert_mapping_invariants(src, &m);

    // The scrutinee identifier survives at a mapped position.
    let scrut = src.find("match (shape)").unwrap() + "match (".len();
    let out = map_offset(&m, scrut).expect("scrutinee is mapped");
    assert_eq!(&m.code[out..out + "shape".len()], "shape");

    // So does an arm body expression.
    let body = src.find("radius * 2").unwrap();
    let out = map_offset(&m, body).expect("arm body is mapped");
    assert_eq!(&m.code[out..out + "radius * 2".len()], "radius * 2");
}

#[test]
fn construct_corpus_upholds_mapping_invariants() {
    // One of everything the emitter rewrites: enum, guarded match with
    // or-patterns and nested patterns, tuple match, try, let-else, if let,
    // pipeline, template interpolation, .rl import.
    let src = r#"import { Token } from "./token.rl";
import { Option, Result } from "@rl/std";
enum Shape { Circle(radius: number), Rect(w: number, h: number), Point }

const area = (s: Shape): number =>
  match (s) {
    Circle(radius) if radius > 0 => Math.PI * radius * radius,
    Circle(radius) => 0,
    Rect(w, h) => w * h,
    Point => 0,
  };

function classify(a: Shape, b: Shape): string {
  return match (a, b) {
    (Circle(radius), Point) => `c${radius}`,
    (_, _) => "other",
  };
}

function run(r: Result<number, string>): Result<number, string> {
  try const n = r;
  const doubled = n |> ((x) => x * 2) |> .toString();
  const Circle(radius) = getShape() else { return r; };
  if let Circle(radius: rr) = getShape() {
    console.log(rr);
  } else {
    console.log("no");
  }
  return r;
}

declare function getShape(): Shape;
"#;
    let m = emit_mapped(src);
    assert_mapping_invariants(src, &m);

    // Expressions inside every construct stay reachable through the map.
    for needle in [
        "Math.PI * radius * radius",
        "w * h",
        "getShape()",
        "console.log(rr)",
        "(x) => x * 2",
    ] {
        let at = src.find(needle).unwrap();
        let out = map_offset(&m, at).unwrap_or_else(|| panic!("expected a mapping for {needle:?}"));
        assert_eq!(&m.code[out..out + needle.len()], needle);
    }
}

#[test]
fn emit_is_infallible_on_rl_level_errors() {
    // A non-exhaustive match is a compile() error but must still emit for
    // the editor: diagnostics stay `--check`'s job.
    let src = "enum E { A(x: number), B }\nconst v = match (E.A(1)) { A(x) => x };\n";
    assert!(compile(src, &Options::default()).is_err());
    let m = emit_mapped(src);
    assert!(m.code.contains("switch ($rl_m.kind)"));
    assert_mapping_invariants(src, &m);
}

#[test]
fn emitted_code_matches_compile_with_imports_off() {
    // For a semantically valid file, the tooling emission is byte-identical
    // to the real compile with the same import mode (no verification drift).
    let src = r#"enum Shape { Circle(radius: number), Point }
const r = match (Shape.Circle(2)) {
  Circle(radius) => radius,
  Point => 0,
};
"#;
    let options = Options {
        rewrite_imports: ImportRewrite::Off,
        ..Options::default()
    };
    assert_eq!(emit_mapped(src).code, compile(src, &options).unwrap());
}

#[test]
fn rl_import_specifiers_stay_untouched() {
    let src = "import { Token } from \"./token.rl\";\nconsole.log(Token);\n";
    let m = emit_mapped(src);
    assert_eq!(m.code, src);
    assert_mapping_invariants(src, &m);
}

#[test]
fn val_modifier_is_dropped_and_the_rest_keeps_mapping() {
    // The editor serves the emitted text as a virtual TypeScript document,
    // so the erased `val` must leave the surrounding bytes mapped exactly.
    let src = "val const user = load();\nuser.name;\n";
    let m = emit_mapped(src);
    assert_eq!(m.code, "const user = load();\nuser.name;\n");
    assert_mapping_invariants(src, &m);
    // `user` in the declaration: source column 11 → output column 7
    let at = src.find("user").unwrap();
    assert_eq!(map_offset(&m, at), Some(m.code.find("user").unwrap()));
    // a parameter modifier, mid-line
    let src = "function read(val user: User) { return user.name; }\n";
    let m = emit_mapped(src);
    assert_eq!(m.code, "function read(user: User) { return user.name; }\n");
    assert_mapping_invariants(src, &m);
}

#[test]
fn result_block_bindings_are_mapped_to_emitted_declarations() {
    let src = r#"import { Result, Ok } from "@rl/std";
function load(): Result<number, string> { return Ok(1); }
const total = result {
  const first <- load();
  let { a, b }: { a: number; b: number } <- load2();
  first + a + b
};
"#;
    let m = emit_mapped(src);
    assert_mapping_invariants(src, &m);

    // A plain binding name reaches the emitted declaration.
    let first = src.find("const first <-").unwrap() + "const ".len();
    let out = map_offset(&m, first).expect("binding name is mapped");
    assert_eq!(&m.code[out..out + "first".len()], "first");

    // So does a destructuring pattern with a type annotation — the whole
    // trimmed span, annotation included, is copied from the source.
    let pattern = "{ a, b }: { a: number; b: number }";
    let at = src.find(pattern).unwrap();
    let out = map_offset(&m, at).expect("destructuring binding is mapped");
    assert_eq!(&m.code[out..out + pattern.len()], pattern);
}

/// The byte offset of `needle` inside the first occurrence of `context`.
fn offset_in(src: &str, context: &str, needle: &str) -> usize {
    let ctx = src
        .find(context)
        .unwrap_or_else(|| panic!("{context:?} is not in the source"));
    ctx + context
        .find(needle)
        .unwrap_or_else(|| panic!("{needle:?} is not in {context:?}"))
}

/// Asserts the `needle` written inside `context` reaches the output through
/// the map, unchanged.
fn assert_mapped_in(src: &str, m: &rlc::MappedEmit, context: &str, needle: &str) {
    let at = offset_in(src, context, needle);
    let out = map_offset(m, at)
        .unwrap_or_else(|| panic!("expected a mapping for {needle:?} in {context:?}"));
    assert_eq!(&m.code[out..out + needle.len()], needle);
}

#[test]
fn try_declaration_bindings_are_mapped_to_emitted_declarations() {
    // The binding is where the editor asks what `try` produced, so it has
    // to reach `const n = $rl_t0.value;` through the map.
    let src = r#"import { Result } from "@rl/std";
declare function load(): Result<number, string>;
function run(): Result<number, string> {
  const n = try load();
  let { a, b }: { a: number; b: number } = try load();
  return { kind: "Ok", value: n + a + b };
}
"#;
    let m = emit_mapped(src);
    assert_mapping_invariants(src, &m);

    assert_mapped_in(src, &m, "n = try load", "n");
    // The whole trimmed binding, type annotation included.
    let pattern = "{ a, b }: { a: number; b: number }";
    assert_mapped_in(src, &m, pattern, pattern);
}

#[test]
fn pattern_bindings_are_mapped_to_their_destructurings() {
    // Every binding a pattern introduces — match arm, let-else, if let,
    // nested — is copied from the source into the emitted destructuring,
    // so the editor can hover it and jump to it.
    let src = r#"import { Option } from "@rl/std";
enum Shape { Circle(radius: number), Rect(w: number, h: number), Point }
declare function getShape(): Shape;
declare function boxed(): Option<Shape>;

const area = match (getShape()) {
  Circle(radius) => radius,
  Rect(w: width, h: height) => width * height,
  Point => 0,
};

function run(): void {
  const Circle(radius: letRadius) = getShape() else { return; };
  if let Rect(w: ifWidth) = getShape() {
    console.log(ifWidth, letRadius);
  }
  const nested = match (boxed()) {
    Some(value: Circle(radius: inner)) => inner,
    _ => 0,
  };
  console.log(area, nested);
}
"#;
    let m = emit_mapped(src);
    assert_mapping_invariants(src, &m);

    // A shorthand binding: the field name is the variable name.
    assert_mapped_in(src, &m, "Circle(radius) => radius", "radius");
    // An aliased one maps the field and the alias separately — the `: `
    // between them is the compiler's, and the source spacing is not.
    assert_mapped_in(src, &m, "Rect(w: width, h: height)", "width");
    assert_mapped_in(src, &m, "Rect(w: width, h: height)", "height");
    // let-else and if let, the two statement forms.
    assert_mapped_in(src, &m, "Circle(radius: letRadius)", "letRadius");
    assert_mapped_in(src, &m, "Rect(w: ifWidth)", "ifWidth");
    // A nested pattern's binding, destructured from the inner path.
    assert_mapped_in(src, &m, "Circle(radius: inner)", "inner");
}

#[test]
fn tuple_element_bindings_follow_the_same_rule() {
    // A tuple match destructures each scrutinee separately, so the rule is
    // per element: a single-alternative element maps its bindings, an
    // or-pattern element does not (one destructuring, several patterns).
    let src = r#"enum Dir { North(deg: number), South }
enum Speed { Fast(kmh: number), Slow(kmh: number) }
declare function dir(): Dir;
declare function speed(): Speed;
const v = match (dir(), speed()) {
  (North(deg), Fast(kmh) | Slow(kmh)) => deg + kmh,
  _ => 0,
};
"#;
    let m = emit_mapped(src);
    assert_mapping_invariants(src, &m);
    assert_mapped_in(src, &m, "(North(deg),", "deg");
    assert_eq!(
        map_offset(&m, offset_in(src, "Fast(kmh) | Slow(kmh)", "kmh")),
        None
    );
    // The body reference is mapped either way.
    assert_mapped_in(src, &m, "=> deg + kmh", "kmh");
}

#[test]
fn or_pattern_bindings_are_left_unmapped() {
    // One destructuring stands for every alternative, so it belongs to no
    // single one: claiming a source position would point the editor at an
    // arbitrary alternative (and let a rename rewrite that one alone).
    let src = r#"enum E { A(x: number), B(x: number), C }
declare function get(): E;
const v = match (get()) {
  A(x) | B(x) => x,
  C => 0,
};
"#;
    let m = emit_mapped(src);
    assert_mapping_invariants(src, &m);
    assert!(m.code.contains("const { x } = $rl_m;"));
    assert_eq!(map_offset(&m, offset_in(src, "A(x) | B(x)", "x")), None);
    // The arm body still is mapped — only the binding list is not.
    assert_mapped_in(src, &m, "=> x,", "x");
}

/* ------------------------------------------------------------------ */
/* diagnostic anchors (TASK-104)                                       */
/* ------------------------------------------------------------------ */

#[test]
fn every_construct_anchors_the_glue_it_writes() {
    let src = "function f() {\n  const a = try readNum();\n}\n";
    let m = emit_mapped(src);
    let anchor = m
        .anchors
        .iter()
        .find(|a| a.kind == rlc::AnchorKind::Try)
        .expect("the try statement is anchored");
    // The anchor spans the construct as the user wrote it — the
    // propagation, not the declaration around it — which is what a
    // diagnostic about its glue is drawn over.
    assert_eq!(&src[anchor.src..anchor.src_end], "try readNum()");
    // ...and covers the glue the construct wrote.
    assert!(m.code[anchor.out..anchor.end].contains("$rl_t0.kind !== \"Ok\""));
}

#[test]
fn anchors_nest_innermost_first() {
    let src = "function f() {\n  const a = try wrap(match (s) { A(x) => x, _ => other() });\n}\n";
    let m = emit_mapped(src);
    let kinds: Vec<rlc::AnchorKind> = m.anchors.iter().map(|a| a.kind).collect();
    let inner = kinds
        .iter()
        .position(|k| *k == rlc::AnchorKind::Match)
        .expect("match anchored");
    let outer = kinds
        .iter()
        .position(|k| *k == rlc::AnchorKind::Try)
        .expect("try anchored");
    assert!(inner < outer, "inner anchor must come first: {kinds:?}");

    // A byte inside the match resolves to the match, not the try.
    let at = m.code.find("$rl_m.kind").expect("switch glue");
    assert_eq!(
        m.anchor_at(at).map(|a| a.kind),
        Some(rlc::AnchorKind::Match)
    );
}

#[test]
fn anchors_do_not_change_the_emitted_bytes() {
    // Anchors are zero-length notes; the output must be what it always was.
    let src = r#"enum E { A(x: number), B }
function f() {
  const a = try readNum();
  const A(x) = e else { return; };
  if let B() = e { log(); }
  const r = result { const v <- readNum(); v };
  const p = x |> f |> g;
  const m = match (e) { A(x) => x, B => 0 };
}
"#;
    let mapped = emit_mapped(src);
    let compiled = compile(
        src,
        &Options {
            rewrite_imports: ImportRewrite::Off,
            verify: false,
            ..Options::default()
        },
    )
    .expect("compiles");
    assert_eq!(mapped.code, compiled);
}
