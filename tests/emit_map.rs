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
    let src = r#"type Res<T, E> = { kind: "Ok"; value: T } | { kind: "Err"; error: E };
declare function load(): Res<{ id: number; name: string }, string>;
declare function read(): Res<[number, string], string>;

const user = result {
  const loaded <- load();
  const { id, name }: { id: number; name: string } <- load();
  const [count, label] <- read();
  { loaded, id, name, count, label }
};
"#;
    let m = emit_mapped(src);
    assert_mapping_invariants(src, &m);

    for needle in [
        "loaded",
        "{ id, name }: { id: number; name: string }",
        "[count, label]",
    ] {
        let at = src.find(needle).unwrap();
        let out = map_offset(&m, at).unwrap_or_else(|| panic!("expected {needle:?} to map"));
        assert_eq!(&m.code[out..out + needle.len()], needle);
        assert!(
            m.code[..out].ends_with("const "),
            "{needle:?} maps to a declaration"
        );
    }
}
