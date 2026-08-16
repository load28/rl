//! Structural parsing of rl `enum` declarations.
//!
//! Purely structural: returns `None` for anything that is not an rl enum
//! (including every valid plain TypeScript enum), so it passes through
//! verbatim. rl-level errors — duplicate cases, bad field types — are the
//! semantic phase's job.

use super::{Parser, is_reserved};
use crate::ast::{EnumCase, EnumDecl, Field};
use crate::scanner::*;

/// `j` is just past the `enum` keyword. On success returns the index just
/// past the closing brace and the parsed declaration.
pub(super) fn parse_enum(
    p: &Parser,
    j: usize,
    end: usize,
    exported: bool,
) -> Option<(usize, EnumDecl)> {
    let src = p.bytes;
    let mut i = skip_ws_comments(src, j, end);
    if i >= end || !is_ident_start(src[i]) {
        return None;
    }
    let k = ident_end(src, i, end);
    let name = &p.src[i..k];
    if is_reserved(name) {
        return None;
    }

    i = skip_ws_comments(src, k, end);
    let mut generics = "";
    if at(src, i, end) == Some(b'<') {
        let close = find_matching(src, i, end)?;
        generics = &p.src[i..close + 1];
        i = skip_ws_comments(src, close + 1, end);
    }
    if at(src, i, end) != Some(b'{') {
        return None;
    }
    let close_brace = find_matching(src, i, end)?;

    let cases = match parse_enum_cases(p, i + 1, close_brace) {
        Some(cases) if !cases.is_empty() => cases,
        _ => return None,
    };

    // A declaration with no payload case and no generics is a plain
    // TypeScript enum — pass it through untouched. (TS enum members can
    // never look like `Tag(...)`, and TS enums can never have generics, so
    // this rule never captures valid TypeScript.)
    let is_rl_enum = !generics.is_empty() || cases.iter().any(|c| c.fields.is_some());
    if !is_rl_enum {
        return None;
    }

    Some((
        close_brace + 1,
        EnumDecl {
            name: name.to_string(),
            exported,
            generics: generics.to_string(),
            cases,
        },
    ))
}

fn parse_enum_cases(p: &Parser, start: usize, end: usize) -> Option<Vec<EnumCase>> {
    let src = p.bytes;
    let mut cases = Vec::new();
    let mut i = start;
    loop {
        i = skip_ws_comments(src, i, end);
        if i >= end {
            break;
        }
        if !is_ident_start(src[i]) {
            return None;
        }
        let tag_off = i;
        let j = ident_end(src, i, end);
        let tag = &p.src[i..j];
        if is_reserved(tag) {
            return None;
        }
        i = skip_ws_comments(src, j, end);

        let mut fields = None;
        if at(src, i, end) == Some(b'(') {
            let close = find_matching(src, i, end)?;
            fields = Some(parse_fields(p, i + 1, close)?);
            i = close + 1;
        }
        cases.push(EnumCase {
            tag: tag.to_string(),
            tag_off,
            fields,
        });

        i = skip_ws_comments(src, i, end);
        if i >= end {
            break;
        }
        if src[i] == b',' {
            i += 1;
            continue;
        }
        return None;
    }
    Some(cases)
}

/// Parses `name: Type, name?: Type, ...`. Returns None on failure.
fn parse_fields(p: &Parser, start: usize, end: usize) -> Option<Vec<Field>> {
    let src = p.bytes;
    let mut fields = Vec::new();
    let mut i = start;
    loop {
        i = skip_ws_comments(src, i, end);
        if i >= end {
            break;
        }
        if !is_ident_start(src[i]) {
            return None;
        }
        let j = ident_end(src, i, end);
        let name = &p.src[i..j];
        if is_reserved(name) {
            return None;
        }
        i = skip_ws_comments(src, j, end);

        let mut optional = false;
        if at(src, i, end) == Some(b'?') {
            optional = true;
            i = skip_ws_comments(src, i + 1, end);
        }
        if at(src, i, end) != Some(b':') {
            return None;
        }
        i += 1;
        let ty_start = i;
        i = scan_type_end(src, i, end);
        let ty = p.src[ty_start..i].trim();
        if ty.is_empty() {
            return None;
        }
        let ty_off = ty_start + (p.src[ty_start..i].len() - p.src[ty_start..i].trim_start().len());
        fields.push(Field {
            name: name.to_string(),
            optional,
            ty: ty.to_string(),
            ty_off,
        });

        if i >= end {
            break;
        }
        if src[i] == b',' {
            i += 1;
            continue;
        }
        return None;
    }
    Some(fields)
}
