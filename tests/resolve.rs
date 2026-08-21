//! Name-resolution tests: declaration collection, shadowing, identity, and
//! the fidelity of the conversion into the analysis surface (the analysis
//! consumes this resolver — Phase 3, TASK-123·129).

use rlc::ExternEnum;
use rlc::hir::{self, FileId};
use rlc::resolve::{self, DefKind, Namespace, Res, Resolution, UseKind};

fn resolved(source: &str, externs: &[ExternEnum]) -> (hir::HirFile, Resolution) {
    let mut hir = hir::lower_source(FileId(0), source);
    let decls: Vec<resolve::ExternDecl> = externs.iter().map(Into::into).collect();
    let resolution = resolve::resolve_file(&mut hir, &decls);
    (hir, resolution)
}

fn span_text<'s>(source: &'s str, hir: &hir::HirFile, node: hir::NodeId) -> &'s str {
    let span = hir.source_map.node_span(node).expect("node has a span");
    &source[span.start..span.end]
}

#[test]
fn an_enum_defines_a_type_and_a_constructor_value() {
    let src = "export enum Shape { Circle(radius: number), Point }\n";
    let (hir, resolution) = resolved(src, &[]);
    let type_def = resolution
        .lookup(Namespace::Type, "Shape")
        .expect("type def");
    let value_def = resolution
        .lookup(Namespace::Value, "Shape")
        .expect("value def");
    assert_ne!(type_def, value_def, "two definitions, one per namespace");
    let DefKind::EnumValue { enum_def } = resolution.defs[value_def].kind else {
        panic!("the value-namespace def is the constructor object");
    };
    assert_eq!(enum_def, type_def);
    // The definition's span is the declared name.
    let span = hir
        .source_map
        .def_span(type_def)
        .expect("local def has a span");
    assert_eq!(&src[span.start..span.end], "Shape");
}

#[test]
fn same_spelling_is_not_same_identity() {
    // Two enums both declare `Empty`. The two uses resolve to *different*
    // variants — identity is the (enum, index) pair, not the string.
    let src = "enum A { Empty, Ok(x: number) }\n\
        enum B { Empty, Fail(code: number) }\n\
        const a = match (v) { Ok(x) => x, Empty => 0 };\n\
        const b = match (w) { Fail(code) => code, Empty => 0 };\n";
    let (hir, resolution) = resolved(src, &[]);
    let empties: Vec<resolve::VariantRef> = hir
        .sites
        .iter()
        .flat_map(|(_, site)| &site.arms)
        .filter_map(|arm| match &hir.patterns[arm.pattern] {
            hir::Pat::Constructor { path, .. } if path.name == "Empty" => {
                match resolution.uses.get(&path.node) {
                    Some(Res::Variant(v)) => Some(*v),
                    other => panic!("Empty did not resolve to a variant: {other:?}"),
                }
            }
            _ => None,
        })
        .collect();
    assert_eq!(empties.len(), 2);
    assert_ne!(empties[0].enum_def, empties[1].enum_def);
    assert_eq!(resolution.defs[empties[0].enum_def].name, "A");
    assert_eq!(resolution.defs[empties[1].enum_def].name, "B");
}

#[test]
fn locals_shadow_imports_shadow_builtins() {
    // A local `Option` replaces the built-in; an imported `Token` exists;
    // an imported `Result` would shadow the built-in the same way.
    let src = "enum Option { Nothing, Just(v: number) }\n\
        const a = match (o) { Just(v) => v, Nothing => 0 };\n";
    let externs = [ExternEnum {
        name: "Token".to_string(),
        tags: vec!["Num".to_string(), "Eof".to_string()],
        from: Some("./token.rl".to_string()),
    }];
    let (hir, resolution) = resolved(src, &externs);
    let option = resolution.lookup(Namespace::Type, "Option").unwrap();
    let DefKind::Enum(data) = &resolution.defs[option].kind else {
        panic!()
    };
    assert!(matches!(data.origin, resolve::DeclOrigin::Local(_)));
    assert_eq!(
        data.variants.len(),
        2,
        "the local declaration, not the built-in"
    );
    // Result is still the built-in.
    let result = resolution.lookup(Namespace::Type, "Result").unwrap();
    let DefKind::Enum(data) = &resolution.defs[result].kind else {
        panic!()
    };
    assert!(matches!(data.origin, resolve::DeclOrigin::Builtin));
    // Token came in from the import, tags only.
    let token = resolution.lookup(Namespace::Type, "Token").unwrap();
    let DefKind::Enum(data) = &resolution.defs[token].kind else {
        panic!()
    };
    assert_eq!(
        data.origin,
        resolve::DeclOrigin::Imported {
            from: Some("./token.rl".to_string())
        }
    );
    // The match resolved against the *local* Option.
    let site = hir.sites.iter().next().unwrap().1;
    let subjects = &resolution.sites[&hir.sites.iter().next().unwrap().0].subjects;
    assert_eq!(subjects[0], Some(option));
    let _ = site;
}

#[test]
fn an_import_alias_is_the_name_in_scope() {
    // The CLI hands the resolver the alias-applied name (`T` for
    // `import { Token as T }`) — resolution happens under it.
    let externs = [ExternEnum {
        name: "T".to_string(),
        tags: vec!["Num".to_string(), "Eof".to_string()],
        from: Some("./token.rl".to_string()),
    }];
    let src = "const v = match (t) { Num => 1, Eof => 0 };\n";
    let (hir, resolution) = resolved(src, &externs);
    let site_id = hir.sites.iter().next().unwrap().0;
    let subject = resolution.sites[&site_id].subjects[0].expect("identified");
    assert_eq!(resolution.defs[subject].name, "T");
}

#[test]
fn unknown_case_suggestions_stay_in_the_identified_enums_domain() {
    // `Circel` misses in Shape; Other also has a `Circle`. The suggestion
    // must come from Shape (the identified subject), and resolution must
    // not silently wire the pattern to Other's homonym.
    let src = "enum Shape { Circle(r: number), Empty }\n\
        enum Other { Circle(r: number), Rest }\n\
        const v = match (s) { Circel(r) => r, Empty => 0 };\n";
    let (hir, resolution) = resolved(src, &[]);
    assert_eq!(resolution.unresolved.len(), 1);
    let miss = &resolution.unresolved[0];
    assert_eq!(miss.name, "Circel");
    assert_eq!(miss.suggestion, "Circle");
    assert_eq!(resolution.defs[miss.against].name, "Shape");
    assert_eq!(span_text(src, &hir, miss.node), "Circel");
}

#[test]
fn a_hand_written_union_resolves_to_silence() {
    // No declaration knows these tags: no subject, no uses, no report —
    // tag patterns match hand-written unions by design.
    let src = "const v = match (u) { Loading => 0, Ready => 1, _ => 2 };\n";
    let (hir, resolution) = resolved(src, &[]);
    let site_id = hir.sites.iter().next().unwrap().0;
    assert_eq!(resolution.sites[&site_id].subjects[0], None);
    assert!(resolution.unresolved.is_empty());
}

#[test]
fn nested_patterns_resolve_against_the_fields_declared_type() {
    let src = "enum Inner { Leaf(v: number), Nil }\n\
        enum Outer { Wrap(inner: Inner), Bare }\n\
        const v = match (o) { Wrap(inner: Leaf(n)) => n, Wrap(inner: Nil) => 0, Bare => 1 };\n";
    let (hir, resolution) = resolved(src, &[]);
    let inner_def = resolution.lookup(Namespace::Type, "Inner").unwrap();
    // Find the nested `Leaf` use and check it resolved into Inner.
    let mut found = false;
    for (_, pat) in hir.patterns.iter() {
        if let hir::Pat::Constructor { path, .. } = pat
            && path.name == "Leaf"
        {
            let Some(Res::Variant(v)) = resolution.uses.get(&path.node) else {
                panic!("Leaf did not resolve");
            };
            assert_eq!(v.enum_def, inner_def);
            found = true;
        }
    }
    assert!(found);
}

#[test]
fn a_single_pattern_site_reports_only_a_unique_one_edit_miss() {
    // `Circel` is one transposition from Shape's `Circle` and nothing
    // else is that close → reported. `Cxxcle` is not → silence.
    let src = "enum Shape { Circle(radius: number), Empty }\n\
        if let Circel(radius) = s {\n  use(radius);\n}\n\
        if let Cxxcle(radius) = s {\n  use(radius);\n}\n";
    let (_, resolution) = resolved(src, &[]);
    assert_eq!(resolution.unresolved.len(), 1);
    assert_eq!(resolution.unresolved[0].name, "Circel");
    assert_eq!(resolution.unresolved[0].kind, UseKind::Case);
}

#[test]
fn resolution_matches_the_analysis_answer() {
    // The analysis consumes the resolver (Phase 3), so this no longer
    // guards two implementations against drifting — it pins the fidelity
    // of the conversion: what the resolver reports is exactly what the
    // analysis surface hands on, span for span, word for word.
    let cases = [
        // a match typo
        "enum Shape { Circle(r: number), Empty }\n\
         const v = match (s) { Circel(r) => r, Empty => 0 };\n",
        // a field typo
        "enum Shape { Circle(radius: number), Empty }\n\
         const v = match (s) { Circle(radiuz) => radiuz, Empty => 0 };\n",
        // a nested typo
        "enum Inner { Leaf(v: number), Nil }\n\
         enum Outer { Wrap(inner: Inner), Bare }\n\
         const v = match (o) { Wrap(inner: Laef(n)) => n, Bare => 1, _ => 2 };\n",
        // an if-let typo, and one too far to report
        "enum Shape { Circle(radius: number), Empty }\n\
         if let Circel(radius) = s {\n  use(radius);\n}\n\
         if let Zzz(radius) = s {\n  use(radius);\n}\n",
        // a hand-written union: silence
        "const v = match (u) { Loading => 0, Ready => 1, _ => 2 };\n",
        // clean code: silence
        "enum Shape { Circle(r: number), Empty }\n\
         const v = match (s) { Circle(r) => r, Empty => 0 };\n",
    ];
    for src in cases {
        let (hir, resolution) = resolved(src, &[]);
        let analysis = rlc::pattern_analyses(src, &[]);
        let ours: Vec<(usize, usize, &str, &str)> = resolution
            .unresolved
            .iter()
            .map(|u| {
                let span = hir.source_map.node_span(u.node).unwrap();
                (span.start, span.end, u.name.as_str(), u.suggestion.as_str())
            })
            .collect();
        let theirs: Vec<(usize, usize, &str, &str)> = analysis
            .unresolved
            .iter()
            .map(|u| (u.start, u.end, u.name.as_str(), u.suggestion.as_str()))
            .collect();
        assert_eq!(ours, theirs, "diverged on:\n{src}");
    }
}

#[test]
fn tuple_positions_identify_independently() {
    // Empty parens force the rl-enum reading (a unit-only `enum` is
    // TypeScript's own and passes through).
    let src = "enum A { X(), Y }\nenum B { P(), Q }\n\
        const v = match (a, b) { (X, P) => 0, (Y, Q) => 1 };\n";
    let (hir, resolution) = resolved(src, &[]);
    let site_id = hir
        .sites
        .iter()
        .find(|(_, s)| s.kind == hir::SiteKind::TupleMatch)
        .unwrap()
        .0;
    let subjects = &resolution.sites[&site_id].subjects;
    assert_eq!(subjects.len(), 2);
    assert_eq!(resolution.defs[subjects[0].unwrap()].name, "A");
    assert_eq!(resolution.defs[subjects[1].unwrap()].name, "B");
}

#[test]
fn an_or_pattern_site_gets_match_grade_identification() {
    // Two tags are match-grade evidence: the unique best-overlap holder
    // identifies the enum, so a typo two edits away is reported — the
    // strict one-edit licence only governs a site whose evidence is a
    // single tag.
    let src = "enum Shape { Circle(radius: number), Empty }\n\
        function f(s: Shape): number {\n\
        \x20 const Cyrcla(radius) | Empty() = s else { return 0; };\n\
        \x20 return radius;\n\
        }\n";
    let (_, resolution) = resolved(src, &[]);
    assert_eq!(resolution.unresolved.len(), 1);
    assert_eq!(resolution.unresolved[0].name, "Cyrcla");
    assert_eq!(resolution.unresolved[0].suggestion, "Circle");

    // The same typo as the only alternative stays unreported: one tag is
    // thin evidence and `Cyrcla` is two edits from `Circle`.
    let alone = "enum Shape { Circle(radius: number), Empty }\n\
        function f(s: Shape): number {\n\
        \x20 const Cyrcla(radius) = s else { return 0; };\n\
        \x20 return radius;\n\
        }\n";
    let (_, resolution) = resolved(alone, &[]);
    assert_eq!(resolution.unresolved.len(), 0);
}
