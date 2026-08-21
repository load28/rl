//! The declaration surface an editor consumes — the compiler's own answer.
//!
//! This is what retires the editor's second, regex-based implementation of
//! rl semantics (`docs/design/rust-parity-analysis.md` GAP-3): the list of
//! enums visible in a file — local, imported (aliases applied), built-in —
//! comes from [`crate::resolve`], under exactly the shadowing the compiler
//! resolves with, together with everything a completion or an outline
//! needs (case signatures, payload fields, spans for local declarations).
//! The file's `match` sites ride along for the one editing question the
//! declarations cannot answer: where a missing arm is inserted.

use std::path::Path;

use crate::hir;
use crate::resolve::{self, DeclOrigin, DefKind};

/// Everything the declaration surface answers for one file.
#[derive(Debug)]
pub struct RlDeclarations {
    /// The enums visible in the file, in resolution order (local
    /// declarations in source order, then imports, then built-ins) —
    /// shadowed names appear once, as the declaration that wins.
    pub enums: Vec<RlEnumDecl>,
    /// The file's `match` sites (nested ones included), in source order.
    pub matches: Vec<RlMatchSite>,
}

/// One visible enum.
#[derive(Debug)]
pub struct RlEnumDecl {
    /// The name this enum is known by in the file's scope.
    pub name: String,
    /// The verbatim `<...>` generic parameter list, or `""`.
    pub generics: String,
    /// Where it comes from.
    pub origin: RlEnumOrigin,
    /// The cases, in declaration order.
    pub cases: Vec<RlCaseDecl>,
}

/// Where a visible enum is declared.
#[derive(Debug, PartialEq, Eq)]
pub enum RlEnumOrigin {
    /// Declared in this file. `name_span` is the declared name;
    /// `span` runs from the name to the last declared piece — the outline
    /// range.
    Local {
        /// Byte span of the declared name.
        name_span: (usize, usize),
        /// Byte span of the declaration's names, name through last field.
        span: (usize, usize),
    },
    /// Imported; the specifier as written, when recorded.
    Imported {
        /// e.g. `./token.rl`.
        specifier: Option<String>,
    },
    /// A built-in (`Option`, `Result`).
    Builtin,
}

/// One case of a visible enum.
#[derive(Debug)]
pub struct RlCaseDecl {
    /// The tag.
    pub tag: String,
    /// Byte span of the tag, for a local declaration.
    pub span: Option<(usize, usize)>,
    /// `true` when the case is declared without parens — the constructor
    /// is a plain value, not a call.
    pub unit: bool,
    /// The payload fields (empty for a unit case, and for imported
    /// declarations that carried tags only).
    pub fields: Vec<RlFieldDecl>,
}

/// One payload field of a case.
#[derive(Debug)]
pub struct RlFieldDecl {
    /// The field name.
    pub name: String,
    /// Whether it is optional (`name?: T`).
    pub optional: bool,
    /// The verbatim declared type text.
    pub ty: String,
}

/// One `match` site: where its keyword is and where an arm is inserted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RlMatchSite {
    /// Byte offset of the `match` keyword.
    pub keyword: usize,
    /// Byte offset of the body's opening `{`.
    pub body_open: usize,
    /// Byte offset of the body's closing `}`.
    pub body_close: usize,
}

/// The declarations visible in `source`, at `path` (which is what resolves
/// its relative `.rl` imports — from disk; an editor passes the buffer's
/// text for the file itself).
pub fn rl_declarations(path: &Path, source: &str) -> RlDeclarations {
    let program = crate::parser::parse(source);
    let externs: Vec<resolve::ExternDecl> =
        super::language::externs_of(path, source, &|target| std::fs::read_to_string(target).ok())
            .iter()
            .map(Into::into)
            .collect();
    let mut hir = hir::lower_program(hir::FileId(0), &program);
    let resolution = resolve::resolve_file(&mut hir, &externs);

    let mut enums = Vec::new();
    for (id, def) in resolution.defs.iter() {
        let DefKind::Enum(data) = &def.kind else {
            continue;
        };
        // Only the winner of each name: a shadowed declaration is not
        // visible.
        if resolution.type_ns.get(&def.name) != Some(&id) {
            continue;
        }
        let cases: Vec<RlCaseDecl> = data
            .variants
            .iter()
            .map(|variant| RlCaseDecl {
                tag: variant.name.clone(),
                span: variant
                    .node
                    .and_then(|node| hir.source_map.node_span(node))
                    .map(|s| (s.start, s.end)),
                unit: variant.fields.is_none(),
                fields: variant
                    .fields
                    .as_deref()
                    .unwrap_or_default()
                    .iter()
                    .map(|field| RlFieldDecl {
                        name: field.name.clone(),
                        optional: field.optional,
                        ty: field.ty_text.clone(),
                    })
                    .collect(),
            })
            .collect();
        let origin = match &data.origin {
            DeclOrigin::Local(node) => {
                let name_span = hir
                    .source_map
                    .node_span(*node)
                    .map(|s| (s.start, s.end))
                    .unwrap_or((0, 0));
                // The outline range: the name through the last declared
                // name (a tag, or a field of the last case).
                let end = data
                    .variants
                    .iter()
                    .flat_map(|v| {
                        v.node.into_iter().chain(
                            v.fields
                                .as_deref()
                                .unwrap_or_default()
                                .iter()
                                .filter_map(|f| f.node),
                        )
                    })
                    .filter_map(|node| hir.source_map.node_span(node))
                    .map(|s| s.end)
                    .max()
                    .unwrap_or(name_span.1);
                RlEnumOrigin::Local {
                    name_span,
                    span: (name_span.0, end.max(name_span.1)),
                }
            }
            DeclOrigin::Imported { from } => RlEnumOrigin::Imported {
                specifier: from.clone(),
            },
            DeclOrigin::Builtin => RlEnumOrigin::Builtin,
        };
        enums.push(RlEnumDecl {
            name: def.name.clone(),
            generics: data.generics.clone(),
            origin,
            cases,
        });
    }

    let mut matches = Vec::new();
    collect_matches(&program, &mut matches);
    matches.sort_by_key(|m| m.keyword);

    RlDeclarations { enums, matches }
}

/// Every `match` of a program, nested positions included.
fn collect_matches(program: &crate::ast::Program, out: &mut Vec<RlMatchSite>) {
    use crate::ast::{IfLetElse, ResultItem, Segment, TemplateChunk};
    for segment in &program.segments {
        match segment {
            Segment::Verbatim(_) | Segment::RlImport(_) | Segment::ValModifier(_) => {}
            Segment::Enum(_) => {}
            Segment::Match(expr) => {
                out.push(RlMatchSite {
                    keyword: expr.keyword_off,
                    body_open: expr.body_open,
                    body_close: expr.body_close,
                });
                collect_matches(&expr.scrutinee, out);
                for arm in &expr.arms {
                    if let Some(guard) = &arm.guard {
                        collect_matches(&guard.expr, out);
                    }
                    collect_matches(&arm.body, out);
                }
            }
            Segment::TupleMatch(expr) => {
                out.push(RlMatchSite {
                    keyword: expr.keyword_off,
                    body_open: expr.body_open,
                    body_close: expr.body_close,
                });
                for (_, scrutinee) in &expr.scrutinees {
                    collect_matches(scrutinee, out);
                }
                for arm in &expr.arms {
                    if let Some(guard) = &arm.guard {
                        collect_matches(&guard.expr, out);
                    }
                    collect_matches(&arm.body, out);
                }
            }
            Segment::Try(stmt) => collect_matches(&stmt.expr, out),
            Segment::LetElse(stmt) => {
                collect_matches(&stmt.expr, out);
                collect_matches(&stmt.else_body, out);
            }
            Segment::IfLet(stmt) => {
                let mut current = Some(stmt);
                while let Some(stmt) = current {
                    collect_matches(&stmt.expr, out);
                    collect_matches(&stmt.body, out);
                    match &stmt.else_part {
                        Some(IfLetElse::Block(block)) => {
                            collect_matches(block, out);
                            current = None;
                        }
                        Some(IfLetElse::IfLet(inner)) => current = Some(inner),
                        None => current = None,
                    }
                }
            }
            Segment::Pipe(pipe) => {
                if let Some(head) = &pipe.head {
                    collect_matches(head, out);
                }
                for step in &pipe.steps {
                    collect_matches(&step.body, out);
                }
            }
            Segment::ResultBlock(block) => {
                for item in &block.items {
                    match item {
                        ResultItem::Stmts(stmts) => collect_matches(stmts, out),
                        ResultItem::Bind(bind) => collect_matches(&bind.expr, out),
                    }
                }
                collect_matches(&block.value, out);
            }
            Segment::Template(template) => {
                for chunk in &template.chunks {
                    if let TemplateChunk::Interp(interp) = chunk {
                        collect_matches(interp, out);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declarations(source: &str) -> RlDeclarations {
        rl_declarations(Path::new("/nowhere/a.rl"), source)
    }

    #[test]
    fn locals_come_with_spans_imports_and_builtins_without() {
        let src = "export enum Shape { Circle(radius: number), Point }\n";
        let decls = declarations(src);
        let names: Vec<&str> = decls.enums.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Shape", "Option", "Result"]);
        let shape = &decls.enums[0];
        let RlEnumOrigin::Local { name_span, span } = &shape.origin else {
            panic!("Shape is local");
        };
        assert_eq!(&src[name_span.0..name_span.1], "Shape");
        assert!(src[span.0..span.1].contains("radius"), "outline range");
        assert!(!shape.cases[0].unit);
        assert_eq!(shape.cases[0].fields[0].ty, "number");
        assert!(shape.cases[1].unit);
        // The built-ins carry their declared type parameters.
        let option = decls.enums.iter().find(|e| e.name == "Option").unwrap();
        assert_eq!(option.generics, "<T>");
        assert_eq!(option.origin, RlEnumOrigin::Builtin);
    }

    #[test]
    fn a_local_declaration_shadows_the_builtin_once() {
        let src = "enum Option { Nothing, Just(v: number) }\n";
        let decls = declarations(src);
        let options: Vec<&RlEnumDecl> = decls.enums.iter().filter(|e| e.name == "Option").collect();
        assert_eq!(options.len(), 1, "the shadowed built-in is not listed");
        assert!(matches!(options[0].origin, RlEnumOrigin::Local { .. }));
        assert_eq!(options[0].cases[0].tag, "Nothing");
    }

    #[test]
    fn matches_carry_their_arm_insertion_point() {
        let src = "enum E { A(x: number), B }\n\
            const v = match (e) { A(x) => match (inner) { B => 0, _ => 1 }, B => 2 };\n";
        let decls = declarations(src);
        assert_eq!(decls.matches.len(), 2);
        for site in &decls.matches {
            assert_eq!(&src[site.keyword..site.keyword + 5], "match");
            assert_eq!(&src[site.body_close..site.body_close + 1], "}");
        }
        assert!(decls.matches[0].keyword < decls.matches[1].keyword);
    }
}
