//! Structural parsing of rl `enum` declarations.
//!
//! Purely structural: returns `None` for anything that is not an rl enum
//! (including every valid plain TypeScript enum), so it passes through
//! verbatim. rl-level errors — duplicate cases, bad field types — are the
//! semantic phase's job.

use super::cursor::Cursor;
use super::is_reserved;
use crate::ast::{EnumCase, EnumDecl, Field};
use crate::lexer::TokenKind;

/// `cur` is positioned just past the `enum` keyword. On success returns
/// the advanced cursor, the byte just past the closing brace, and the
/// parsed declaration.
pub(super) fn parse_enum<'t>(
    mut cur: Cursor<'t>,
    exported: bool,
) -> Option<(Cursor<'t>, usize, EnumDecl)> {
    let (name, _) = cur.eat_ident()?;
    if is_reserved(name) {
        return None;
    }

    let mut generics = "";
    if cur.at_punct(b'<') {
        let close = cur.find_close()?;
        generics = &cur.parser.src[cur.tokens[cur.idx].span.start..cur.tokens[close].span.end];
        cur.idx = close + 1;
    }

    if !cur.at_punct(b'{') {
        return None;
    }
    let open = cur.idx;
    let close = cur.find_close()?;
    let inner = cur.sub(open + 1, close, cur.tokens[close].span.start);

    let cases = match parse_enum_cases(inner) {
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

    let byte_end = cur.tokens[close].span.end;
    cur.idx = close + 1;
    Some((
        cur,
        byte_end,
        EnumDecl {
            name: name.to_string(),
            exported,
            generics: generics.to_string(),
            cases,
        },
    ))
}

fn parse_enum_cases(mut cur: Cursor) -> Option<Vec<EnumCase>> {
    let mut cases = Vec::new();
    loop {
        if cur.peek().is_none() {
            break;
        }
        let (tag, tag_span) = cur.eat_ident()?;
        if is_reserved(tag) {
            return None;
        }

        let mut fields = None;
        if cur.at_punct(b'(') {
            let open = cur.idx;
            let close = cur.find_close()?;
            fields = Some(parse_fields(cur.sub(
                open + 1,
                close,
                cur.tokens[close].span.start,
            ))?);
            cur.idx = close + 1;
        }
        cases.push(EnumCase {
            tag: tag.to_string(),
            tag_off: tag_span.start,
            fields,
        });

        if cur.peek().is_none() {
            break;
        }
        cur.eat_punct(b',')?;
    }
    Some(cases)
}

/// Parses `name: Type, name?: Type, ...`. Returns None on failure.
fn parse_fields(mut cur: Cursor) -> Option<Vec<Field>> {
    let mut fields = Vec::new();
    loop {
        if cur.peek().is_none() {
            break;
        }
        let (name, _) = cur.eat_ident()?;
        if is_reserved(name) {
            return None;
        }

        let mut optional = false;
        if cur.eat_punct(b'?').is_some() {
            optional = true;
        }
        let colon = cur.eat_punct(b':')?;

        // The annotation text runs from just past the `:` to the stopping
        // token, exactly like the byte scanner — comments inside stay part
        // of the text; only surrounding whitespace is trimmed.
        let ty_start = colon.end;
        let (stop_idx, stop_byte) = type_end(&cur);
        let raw = &cur.parser.src[ty_start..stop_byte];
        let ty = raw.trim();
        if ty.is_empty() {
            return None;
        }
        let ty_off = ty_start + (raw.len() - raw.trim_start().len());
        fields.push(Field {
            name: name.to_string(),
            optional,
            ty: ty.to_string(),
            ty_off,
        });
        cur.idx = stop_idx;

        if cur.peek().is_none() {
            break;
        }
        cur.eat_punct(b',')?;
    }
    Some(fields)
}

/// Scans a type annotation from `cur.idx` until a top-level `,` or closing
/// bracket, returning the stopping token index and the byte where the
/// annotation ends (`range_end` when the tokens run out — the enclosing
/// closer's position).
fn type_end(cur: &Cursor) -> (usize, usize) {
    let mut depth = 0usize;
    let mut k = cur.idx;
    while k < cur.tokens.len() {
        match cur.tokens[k].kind {
            TokenKind::Punct(b'(' | b'[' | b'{' | b'<') => depth += 1,
            TokenKind::Punct(b')' | b']' | b'}') => {
                if depth == 0 {
                    return (k, cur.tokens[k].span.start);
                }
                depth -= 1;
            }
            TokenKind::Punct(b'>') => depth = depth.saturating_sub(1),
            TokenKind::Punct(b',') if depth == 0 => return (k, cur.tokens[k].span.start),
            _ => {}
        }
        k += 1;
    }
    (k, cur.range_end)
}
