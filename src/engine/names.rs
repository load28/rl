//! rl's own names — the semantic surface the checker cannot be asked about.
//!
//! Three of rl's name spaces exist only in `.rl` source: an **enum name**,
//! a **case tag**, and a **payload field name**. None survives lowering in
//! a form TypeScript can be pointed at — an enum declaration is synthesized
//! text with no mapping back, a tag becomes a string literal, a field a
//! destructuring key. So the answers TypeScript gives for every other
//! identifier (hover, go-to-definition) are simply absent here, and rl has
//! to give them itself.
//!
//! This module is that answer, and it follows the same layering the rest of
//! the engine does:
//!
//! - It is **parse-only**: source in, symbol out. No toolchain, no project,
//!   no service session — so it answers in a buffer mid-edit and in a
//!   workspace with no TypeScript installed, exactly like semantic tokens.
//! - It answers **only where the checker cannot be asked**. A use site like
//!   `Shape.Circle(1)` or a type annotation `const s: Shape` lowers to
//!   ordinary TypeScript that the checker knows better than rl does, so
//!   this module says nothing about those positions and lets the service
//!   answer. (The editor's previous implementation claimed them, which is
//!   how a local variable that happened to share a case's name came to
//!   hover as an enum case.)
//!
//! Where the declarations come from is the analysis' table, so an imported
//! enum is found under the name the import gave it, and a local declaration
//! shadows it — one resolution rule, not a second one written here.

use std::path::{Path, PathBuf};

use crate::analysis::{DeclaredEnum, NameKind, Origin};
use crate::{CaseSymbol, EnumSymbol, MatchConstructor, PayloadField};

use super::language::{Location, Position, Range};

/// What an rl name names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RlSymbolKind {
    /// An enum, at its declaration.
    Enum,
    /// One case of an enum.
    Case,
    /// One payload field of a case.
    Field,
}

/// An rl name the editor asked about, resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RlSymbol {
    /// What the name names.
    pub kind: RlSymbolKind,
    /// The identifier's range in the `.rl` source — what an editor
    /// highlights.
    pub range: Range,
    /// The name as written.
    pub name: String,
    /// The enum it belongs to (itself, for an enum).
    pub enum_name: String,
    /// The declaration rendered in rl syntax — the hover's first line.
    pub signature: String,
    /// One sentence about what it is and where it came from.
    pub detail: String,
    /// Where it is declared, when that is a place the editor can open.
    pub definition: Option<Location>,
}

/// The rl name at `position`, or `None` when the position is not on one.
///
/// `path` is the file the source belongs to; it is used to resolve
/// relative `.rl` imports (from disk — an unsaved imported file is seen as
/// last saved) and to name the file a definition lives in.
pub fn rl_symbol_at(path: &Path, source: &str, position: Position) -> Option<RlSymbol> {
    let offset = super::language::source_byte(source, position);
    let locals = crate::enum_symbols(source);
    let analyses = super::language::analyses_for(path, source);

    // 1. Inside an enum declaration: the name, a case tag, a field name.
    //    These bytes have no counterpart in the output at all.
    if let Some(found) = declaration_at(path, source, &locals, offset) {
        return Some(found);
    }

    // 2. In a pattern: a case tag or a payload field name, resolved by the
    //    analysis against the same declaration table sema uses.
    let resolved = analyses
        .resolved
        .iter()
        .find(|r| r.start <= offset && offset <= r.end)?;
    let declared = analyses
        .declarations
        .iter()
        .find(|d| d.name == resolved.enum_name)?;
    let (kind, signature, detail, definition) = match resolved.kind {
        NameKind::Case => {
            let constructor = declared
                .constructors
                .iter()
                .find(|c| c.tag == resolved.name)?;
            (
                RlSymbolKind::Case,
                case_signature(&declared.name, constructor),
                case_detail(declared, constructor),
                case_definition(path, source, &locals, declared, &resolved.name),
            )
        }
        NameKind::Field => {
            let tag = resolved.tag.as_deref()?;
            let constructor = declared.constructors.iter().find(|c| c.tag == tag)?;
            let field = constructor
                .fields
                .as_deref()
                .unwrap_or_default()
                .iter()
                .find(|f| f.name == resolved.name)?;
            (
                RlSymbolKind::Field,
                field_signature(field),
                format!(
                    "payload field of `{}.{tag}` — the pattern binds it by name",
                    declared.name
                ),
                field_definition(path, source, &locals, declared, tag, &resolved.name),
            )
        }
    };
    Some(RlSymbol {
        kind,
        range: super::language::span_range(source, resolved.start, resolved.end),
        name: resolved.name.clone(),
        enum_name: declared.name.clone(),
        signature,
        detail,
        definition,
    })
}

/// The symbol at `offset` when it sits inside one of this file's own enum
/// declarations — the one region that lowers to text with no mapping at
/// all, so nothing else can answer for it.
fn declaration_at(
    path: &Path,
    source: &str,
    locals: &[EnumSymbol],
    offset: usize,
) -> Option<RlSymbol> {
    for declaration in locals {
        if offset >= declaration.offset && offset <= declaration.offset + declaration.name.len() {
            let here = Location {
                path: path.to_path_buf(),
                range: super::language::span_range(
                    source,
                    declaration.offset,
                    declaration.offset + declaration.name.len(),
                ),
            };
            return Some(RlSymbol {
                kind: RlSymbolKind::Enum,
                range: here.range,
                name: declaration.name.clone(),
                enum_name: declaration.name.clone(),
                signature: enum_signature(declaration),
                detail: "rl enum — compiles to a `kind`-tagged union type and a constructor \
                         object of the same name"
                    .to_string(),
                definition: Some(here),
            });
        }
        for case in &declaration.cases {
            if offset >= case.offset && offset <= case.offset + case.tag.len() {
                return Some(symbol_of_local_case(path, source, declaration, case));
            }
            for field in case.fields.as_deref().unwrap_or_default() {
                if offset >= field.offset && offset <= field.offset + field.name.len() {
                    let range = super::language::span_range(
                        source,
                        field.offset,
                        field.offset + field.name.len(),
                    );
                    return Some(RlSymbol {
                        kind: RlSymbolKind::Field,
                        range,
                        name: field.name.clone(),
                        enum_name: declaration.name.clone(),
                        signature: format!(
                            "{}{}: {}",
                            field.name,
                            if field.optional { "?" } else { "" },
                            field.ty
                        ),
                        detail: format!(
                            "payload field of `{}.{}` — a pattern binds it by name",
                            declaration.name, case.tag
                        ),
                        definition: Some(Location {
                            path: path.to_path_buf(),
                            range,
                        }),
                    });
                }
            }
        }
    }
    None
}

fn symbol_of_local_case(
    path: &Path,
    source: &str,
    declaration: &EnumSymbol,
    case: &CaseSymbol,
) -> RlSymbol {
    let range = super::language::span_range(source, case.offset, case.offset + case.tag.len());
    let constructor = MatchConstructor {
        tag: case.tag.clone(),
        fields: case.fields.as_ref().map(|fields| {
            fields
                .iter()
                .map(|f| PayloadField {
                    name: f.name.clone(),
                    optional: f.optional,
                    ty: f.ty.clone(),
                })
                .collect()
        }),
    };
    RlSymbol {
        kind: RlSymbolKind::Case,
        range,
        name: case.tag.clone(),
        enum_name: declaration.name.clone(),
        signature: case_signature(&declaration.name, &constructor),
        detail: unit_or_payload(&constructor),
        definition: Some(Location {
            path: path.to_path_buf(),
            range,
        }),
    }
}

/// Where a case is declared: this file when the enum is local, the imported
/// file when the analysis found it there, and nowhere for a built-in (whose
/// declaration is the compiler's own).
fn case_definition(
    path: &Path,
    source: &str,
    locals: &[EnumSymbol],
    declared: &DeclaredEnum,
    tag: &str,
) -> Option<Location> {
    if let Some(local) = locals.iter().find(|d| d.name == declared.name) {
        let case = local.cases.iter().find(|c| c.tag == tag)?;
        return Some(Location {
            path: path.to_path_buf(),
            range: super::language::span_range(source, case.offset, case.offset + case.tag.len()),
        });
    }
    let (target, text, imported) = imported_declaration(path, declared)?;
    let case = imported.cases.iter().find(|c| c.tag == tag)?;
    Some(Location {
        path: target,
        range: super::language::span_range(&text, case.offset, case.offset + case.tag.len()),
    })
}

fn field_definition(
    path: &Path,
    source: &str,
    locals: &[EnumSymbol],
    declared: &DeclaredEnum,
    tag: &str,
    name: &str,
) -> Option<Location> {
    let find = |declaration: &EnumSymbol| -> Option<(usize, usize)> {
        let case = declaration.cases.iter().find(|c| c.tag == tag)?;
        let field = case.fields.as_deref()?.iter().find(|f| f.name == name)?;
        Some((field.offset, field.offset + field.name.len()))
    };
    if let Some(local) = locals.iter().find(|d| d.name == declared.name) {
        let (start, end) = find(local)?;
        return Some(Location {
            path: path.to_path_buf(),
            range: super::language::span_range(source, start, end),
        });
    }
    let (target, text, imported) = imported_declaration(path, declared)?;
    let (start, end) = find(&imported)?;
    Some(Location {
        path: target,
        range: super::language::span_range(&text, start, end),
    })
}

/// The file an imported enum was declared in, its text, and the declaration
/// — found the way the analysis found it, by walking this file's relative
/// `.rl` imports.
fn imported_declaration(
    path: &Path,
    declared: &DeclaredEnum,
) -> Option<(PathBuf, String, EnumSymbol)> {
    let Origin::Imported { .. } = declared.origin else {
        return None;
    };
    // A namespace import renames the enum to `ns.Name`; the declaration in
    // the other file still carries its own name.
    let own = declared
        .name
        .rsplit('.')
        .next()
        .unwrap_or(&declared.name)
        .to_string();
    let dir = path.parent().unwrap_or(Path::new("."));
    let source = std::fs::read_to_string(path).ok()?;
    for import in crate::rl_imports(&source) {
        let target = dir.join(&import.specifier).canonicalize().ok()?;
        let Ok(text) = std::fs::read_to_string(&target) else {
            continue;
        };
        if let Some(found) = crate::enum_symbols(&text)
            .into_iter()
            .find(|d| d.exported && (d.name == own || d.name == declared.name))
        {
            return Some((target, text, found));
        }
    }
    None
}

/// `enum Shape { Circle(radius: number), Point }` — the declaration as the
/// user would write it, on one line.
fn enum_signature(declaration: &EnumSymbol) -> String {
    let cases: Vec<String> = declaration
        .cases
        .iter()
        .map(|case| match &case.fields {
            None => case.tag.clone(),
            Some(fields) => format!(
                "{}({})",
                case.tag,
                fields
                    .iter()
                    .map(|f| format!("{}{}: {}", f.name, if f.optional { "?" } else { "" }, f.ty))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        })
        .collect();
    format!(
        "enum {}{} {{ {} }}",
        declaration.name,
        declaration.generics,
        cases.join(", ")
    )
}

pub(super) fn case_signature(enum_name: &str, constructor: &MatchConstructor) -> String {
    match &constructor.fields {
        None => format!("{enum_name}.{}", constructor.tag),
        Some(fields) => format!(
            "{enum_name}.{}({})",
            constructor.tag,
            fields
                .iter()
                .map(field_signature)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

pub(super) fn field_signature(field: &PayloadField) -> String {
    format!(
        "{}{}: {}",
        field.name,
        if field.optional { "?" } else { "" },
        field.ty
    )
}

fn case_detail(declared: &DeclaredEnum, constructor: &MatchConstructor) -> String {
    let origin = match &declared.origin {
        Origin::Local => format!("`enum {}`", declared.name),
        Origin::Builtin => format!("built-in `enum {}`", declared.name),
        Origin::Imported { from: Some(from) } => {
            format!("`enum {}` (imported from \"{from}\")", declared.name)
        }
        Origin::Imported { from: None } => format!("imported `enum {}`", declared.name),
    };
    format!("{} of {origin}", unit_or_payload(constructor))
}

fn unit_or_payload(constructor: &MatchConstructor) -> String {
    match &constructor.fields {
        None => "unit case — compiles to a singleton value".to_string(),
        Some(_) => "payload case — compiles to a constructor function".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(source: &str, needle: &str, delta: usize) -> Position {
        let offset = source.find(needle).expect("needle") + delta;
        let before = &source[..offset];
        Position {
            line: before.matches('\n').count() as u32,
            character: (offset - before.rfind('\n').map_or(0, |n| n + 1)) as u32,
        }
    }

    fn symbol(source: &str, needle: &str, delta: usize) -> RlSymbol {
        rl_symbol_at(Path::new("/p/a.rl"), source, at(source, needle, delta))
            .unwrap_or_else(|| panic!("no rl symbol at {needle:?}+{delta}"))
    }

    const SRC: &str = "enum Shape { Circle(radius: number), Point }\n\
                       const a = match (s) { Circle(radius) => radius, Point => 0 };\n\
                       if let Circle(radius: r) = s { use(r); }\n";

    #[test]
    fn an_enum_declaration_answers_for_its_own_names() {
        let e = symbol(SRC, "enum Shape", 5);
        assert_eq!(e.kind, RlSymbolKind::Enum);
        assert_eq!(e.signature, "enum Shape { Circle(radius: number), Point }");

        let case = symbol(SRC, "Circle(radius: number)", 0);
        assert_eq!(case.kind, RlSymbolKind::Case);
        assert_eq!(case.signature, "Shape.Circle(radius: number)");

        let field = symbol(SRC, "radius: number", 0);
        assert_eq!(field.kind, RlSymbolKind::Field);
        assert_eq!(field.signature, "radius: number");
    }

    #[test]
    fn a_pattern_tag_resolves_to_its_case() {
        let case = symbol(SRC, "Circle(radius) =>", 0);
        assert_eq!(case.kind, RlSymbolKind::Case);
        assert_eq!(case.signature, "Shape.Circle(radius: number)");
        assert!(case.detail.contains("payload case"), "{}", case.detail);
        // ...and points at the declaration.
        let definition = case.definition.expect("declared in this file");
        assert_eq!(definition.range.start.line, 0);
    }

    #[test]
    fn an_if_let_pattern_answers_too() {
        // The construct with no answer at all before this module.
        let case = symbol(SRC, "Circle(radius: r)", 0);
        assert_eq!(case.signature, "Shape.Circle(radius: number)");
        let field = symbol(SRC, "radius: r)", 0);
        assert_eq!(field.kind, RlSymbolKind::Field);
        assert_eq!(field.signature, "radius: number");
    }

    #[test]
    fn a_builtin_case_is_named_as_one() {
        let src = "const n = match (o) { Some(value) => value, None => 0 };\n";
        let case = symbol(src, "Some(value)", 0);
        assert_eq!(case.enum_name, "Option");
        assert!(case.detail.contains("built-in"), "{}", case.detail);
        // The built-ins have no declaration to open.
        assert_eq!(case.definition, None);
    }

    #[test]
    fn ordinary_identifiers_are_left_to_the_checker() {
        // A binding, a value use of the constructor, a type annotation: all
        // of them lower to TypeScript the service knows better.
        let src = "enum Shape { Circle(radius: number), Point }\n\
                   const s: Shape = Shape.Circle(1);\n\
                   const Point = 2;\n";
        assert!(rl_symbol_at(Path::new("/p/a.rl"), src, at(src, "s: Shape", 0)).is_none());
        assert!(rl_symbol_at(Path::new("/p/a.rl"), src, at(src, "Shape.Circle(1)", 0)).is_none());
        assert!(rl_symbol_at(Path::new("/p/a.rl"), src, at(src, "const Point = 2", 6)).is_none());
    }
}
