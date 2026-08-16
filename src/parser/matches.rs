//! Structural parsing of rl `match` expressions.
//!
//! Purely structural: anything that does not fully parse as an rl match (a
//! method named `match`, `String.prototype.match` calls, ...) returns `None`
//! and passes through verbatim. rl-level errors — duplicate arms, a
//! misplaced wildcard, non-exhaustiveness — are the semantic phase's job.
//! The scrutinee and every arm body are recursively parsed sub-programs.

use super::{Parser, is_reserved};
use crate::ast::{Arm, Binding, MatchExpr, Pattern, Span, TagPattern};
use crate::scanner::*;

/// `j` is just past the `match` keyword. On success returns the index just
/// past the closing brace and the parsed expression.
pub(super) fn parse_match(p: &Parser, j: usize, end: usize) -> Option<(usize, MatchExpr)> {
    let src = p.bytes;
    let k = skip_ws_comments(src, j, end);
    if at(src, k, end) != Some(b'(') {
        return None;
    }
    let close_paren = find_matching(src, k, end)?;
    let scrutinee_span = Span {
        start: k + 1,
        end: close_paren,
    };
    if p.src[scrutinee_span.start..scrutinee_span.end]
        .trim()
        .is_empty()
    {
        return None;
    }

    let b = skip_ws_comments(src, close_paren + 1, end);
    if at(src, b, end) != Some(b'{') {
        return None;
    }
    let close_brace = find_matching(src, b, end)?;

    let arms = match parse_arms(p, b + 1, close_brace) {
        Some(arms) if !arms.is_empty() => arms,
        _ => return None,
    };

    Some((
        close_brace + 1,
        MatchExpr {
            keyword_off: j.saturating_sub("match".len()),
            scrutinee_span,
            scrutinee: p.parse_range(scrutinee_span.start, scrutinee_span.end),
            arms,
        },
    ))
}

fn parse_arms(p: &Parser, start: usize, end: usize) -> Option<Vec<Arm>> {
    let src = p.bytes;
    let mut arms = Vec::new();
    let mut i = start;
    loop {
        i = skip_ws_comments(src, i, end);
        if i >= end {
            break;
        }

        // pattern
        let pattern_off = i;
        let pattern;
        if src[i] == b'_' && !matches!(at(src, i + 1, end), Some(b) if is_ident_char(b)) {
            pattern = Pattern::Wildcard;
            i += 1;
        } else if is_ident_start(src[i]) {
            let (j, first) = parse_tag_pattern(p, i, end)?;
            let mut alts = vec![first];
            i = skip_ws_comments(src, j, end);
            // `|`-separated alternatives; `||` is never an alternative
            // separator, so it fails the parse and passes through.
            while at(src, i, end) == Some(b'|') && at(src, i + 1, end) != Some(b'|') {
                let k = skip_ws_comments(src, i + 1, end);
                if k >= end || !is_ident_start(src[k]) {
                    return None;
                }
                let (m, alt) = parse_tag_pattern(p, k, end)?;
                alts.push(alt);
                i = skip_ws_comments(src, m, end);
            }
            pattern = Pattern::Tags(alts);
        } else {
            return None;
        }

        i = skip_ws_comments(src, i, end);
        if !(at(src, i, end) == Some(b'=') && at(src, i + 1, end) == Some(b'>')) {
            return None;
        }
        i = skip_ws_comments(src, i + 2, end);

        // body: `{ ... }` block or a single expression
        let body_span;
        let mut block = false;
        if at(src, i, end) == Some(b'{') {
            let close = find_matching(src, i, end)?;
            body_span = Span {
                start: i + 1,
                end: close,
            };
            block = true;
            i = close + 1;
        } else {
            let body_start = i;
            i = scan_expr_end(src, i, end);
            body_span = Span {
                start: body_start,
                end: i,
            };
            if p.src[body_span.start..body_span.end].trim().is_empty() {
                return None;
            }
        }

        arms.push(Arm {
            pattern,
            pattern_off,
            body_span,
            body: p.parse_range(body_span.start, body_span.end),
            block,
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
    Some(arms)
}

/// Parses one `Tag` / `Tag(bindings...)` alternative starting at the
/// identifier at `i`. Returns the index just past the alternative.
fn parse_tag_pattern(p: &Parser, i: usize, end: usize) -> Option<(usize, TagPattern)> {
    let src = p.bytes;
    let tag_off = i;
    let j = ident_end(src, i, end);
    let tag = &p.src[i..j];
    if is_reserved(tag) {
        return None;
    }
    let mut k = skip_ws_comments(src, j, end);
    let mut bindings = None;
    if at(src, k, end) == Some(b'(') {
        let close = find_matching(src, k, end)?;
        bindings = Some(parse_bindings(p, k + 1, close)?);
        k = close + 1;
    }
    Some((
        k,
        TagPattern {
            tag: tag.to_string(),
            tag_off,
            bindings,
        },
    ))
}

/// Parses `a, b: alias, ...` between the parens of a pattern. None on failure.
fn parse_bindings(p: &Parser, start: usize, end: usize) -> Option<Vec<Binding>> {
    let src = p.bytes;
    let mut bindings = Vec::new();
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

        let mut alias = None;
        if at(src, i, end) == Some(b':') {
            i = skip_ws_comments(src, i + 1, end);
            if i >= end || !is_ident_start(src[i]) {
                return None;
            }
            let m = ident_end(src, i, end);
            let alias_name = &p.src[i..m];
            if is_reserved(alias_name) {
                return None;
            }
            alias = Some(alias_name.to_string());
            i = skip_ws_comments(src, m, end);
        }
        bindings.push(Binding {
            name: name.to_string(),
            alias,
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
    Some(bindings)
}

/// Scans an arm's expression body until a top-level `,` or closing bracket.
fn scan_expr_end(src: &[u8], mut i: usize, end: usize) -> usize {
    let mut depth = 0usize;
    while i < end {
        let c = src[i];
        if c == b'/' && at(src, i + 1, end) == Some(b'/') {
            i = line_end(src, i, end);
            continue;
        }
        if c == b'/' && at(src, i + 1, end) == Some(b'*') {
            i = match find_subslice(src, b"*/", i + 2, end) {
                Some(e) => e + 2,
                None => end,
            };
            continue;
        }
        if c == b'"' || c == b'\'' {
            i = scan_string(src, i, end);
            continue;
        }
        if c == b'`' {
            i = skip_template(src, i, end);
            continue;
        }
        match c {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                if depth == 0 {
                    return i;
                }
                depth -= 1;
            }
            b',' if depth == 0 => return i,
            _ => {}
        }
        i += 1;
    }
    i
}
