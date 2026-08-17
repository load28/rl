//! Structural parsing of rl `match` expressions.
//!
//! Purely structural: anything that does not fully parse as an rl match (a
//! method named `match`, `String.prototype.match` calls, ...) returns `None`
//! and passes through verbatim. rl-level errors — duplicate arms, a
//! misplaced wildcard, non-exhaustiveness — are the semantic phase's job.
//! The scrutinee and every arm body are recursively parsed sub-programs.

use super::cursor::Cursor;
use super::is_reserved;
use crate::ast::{
    Arm, Binding, GuardExpr, MatchExpr, Pattern, Span, TagPattern, TupleArm, TupleMatchExpr,
    TuplePattern,
};
use crate::lexer::TokenKind;

/// What [`parse_match`] found: a single match or a tuple match. The arms
/// decide — a tuple match needs every arm to be a parenthesized tuple
/// pattern (or a final bare `_`) *and* the scrutinee to split at top-level
/// commas, so `match (a, b) { Tag => ... }` keeps meaning a single match
/// over a comma expression, exactly as before tuple matches existed.
pub(super) enum ParsedMatch {
    Single(MatchExpr),
    Tuple(TupleMatchExpr),
}

/// `cur` is positioned just past the `match` keyword (`kw_span`). On
/// success returns the advanced cursor, the byte just past the closing
/// brace, and the parsed expression.
pub(super) fn parse_match<'t>(
    mut cur: Cursor<'t>,
    kw_span: Span,
) -> Option<(Cursor<'t>, usize, ParsedMatch)> {
    if !cur.at_punct(b'(') {
        return None;
    }
    let open = cur.idx;
    let close = cur.find_close()?;
    let scrutinee_span = Span {
        start: cur.tokens[open].span.start + 1,
        end: cur.tokens[close].span.start,
    };
    if cur.parser.src[scrutinee_span.start..scrutinee_span.end]
        .trim()
        .is_empty()
    {
        return None;
    }
    cur.idx = close + 1;

    if !cur.at_punct(b'{') {
        return None;
    }
    let body_open = cur.idx;
    let body_close = cur.find_close()?;
    let byte_end = cur.tokens[body_close].span.end;
    let arms_cur = cur.sub(body_open + 1, body_close, cur.tokens[body_close].span.start);

    // Tuple attempt first (arm-driven): every arm a tuple pattern (or a
    // bare `_`) and a scrutinee splitting into two or more parts.
    if let Some(parts) = split_scrutinees(&cur, open, close)
        && let Some(arms) = parse_tuple_arms(arms_cur)
        && !arms.is_empty()
    {
        let scrutinees = parts
            .iter()
            .map(|&(from, to)| {
                let span = Span {
                    start: cur.tokens[from].span.start,
                    end: cur.tokens[to - 1].span.end,
                };
                let program = cur
                    .parser
                    .parse_tokens(&cur.tokens[from..to], span.start, span.end);
                (span, program)
            })
            .collect();
        cur.idx = body_close + 1;
        return Some((
            cur,
            byte_end,
            ParsedMatch::Tuple(TupleMatchExpr {
                keyword_off: kw_span.start,
                scrutinees,
                arms,
            }),
        ));
    }

    let arms = match parse_arms(arms_cur) {
        Some(arms) if !arms.is_empty() => arms,
        _ => return None,
    };

    let scrutinee = cur.parser.parse_tokens(
        &cur.tokens[open + 1..close],
        scrutinee_span.start,
        scrutinee_span.end,
    );
    cur.idx = body_close + 1;
    Some((
        cur,
        byte_end,
        ParsedMatch::Single(MatchExpr {
            keyword_off: kw_span.start,
            scrutinee_span,
            scrutinee,
            arms,
        }),
    ))
}

/// Splits the scrutinee token range `(open..close)` at top-level commas
/// into `(from, to)` token ranges. `None` unless there are two or more
/// non-empty parts. `<`/`>` count as brackets so a generic call's type
/// arguments don't split (a comparison next to a top-level comma is not a
/// meaningful tuple scrutinee — tags are matched by `kind`).
fn split_scrutinees(cur: &Cursor, open: usize, close: usize) -> Option<Vec<(usize, usize)>> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut from = open + 1;
    for k in open + 1..close {
        match cur.tokens[k].kind {
            TokenKind::Punct(b'(' | b'[' | b'{' | b'<') => depth += 1,
            TokenKind::Punct(b')' | b']' | b'}' | b'>') => depth = depth.saturating_sub(1),
            TokenKind::Punct(b',') if depth == 0 => {
                if k == from {
                    return None; // empty part
                }
                parts.push((from, k));
                from = k + 1;
            }
            _ => {}
        }
    }
    if from >= close {
        return None; // trailing comma leaves an empty last part
    }
    parts.push((from, close));
    if parts.len() < 2 {
        return None;
    }
    Some(parts)
}

fn parse_arms(mut cur: Cursor) -> Option<Vec<Arm>> {
    let mut arms = Vec::new();
    while let Some(first) = cur.peek() {
        let pattern_off = first.span.start;

        // pattern
        let pattern = match first.kind {
            TokenKind::Ident if cur.text(first) == "_" => {
                cur.bump();
                Pattern::Wildcard
            }
            TokenKind::Ident => Pattern::Tags(parse_tag_alternatives(&mut cur)?),
            _ => return None,
        };

        // Only tag patterns take a guard — `_ if` never parses, so it
        // passes through.
        let allow_guard = matches!(pattern, Pattern::Tags(_));
        let (guard, body_span, body, block) = parse_arm_tail(&mut cur, allow_guard)?;

        arms.push(Arm {
            pattern,
            pattern_off,
            guard,
            body_span,
            body,
            block,
        });

        if cur.peek().is_none() {
            break;
        }
        cur.eat_punct(b',')?;
    }
    Some(arms)
}

/// Parses tuple arms: `(elem, elem, ...) (if guard)? => body` with an
/// optional final bare `_` arm. `None` unless *every* arm has that shape —
/// the caller then falls back to single-match arms.
fn parse_tuple_arms(mut cur: Cursor) -> Option<Vec<TupleArm>> {
    let mut arms = Vec::new();
    while let Some(first) = cur.peek() {
        let pattern_off = first.span.start;

        let pattern = match first.kind {
            TokenKind::Ident if cur.text(first) == "_" => {
                cur.bump();
                TuplePattern::Wildcard
            }
            TokenKind::Punct(b'(') => {
                let open = cur.idx;
                let close = cur.find_close()?;
                let elems =
                    parse_tuple_elems(cur.sub(open + 1, close, cur.tokens[close].span.start))?;
                if elems.len() < 2 {
                    return None; // `(P)` is not a tuple pattern
                }
                cur.idx = close + 1;
                TuplePattern::Elems(elems)
            }
            _ => return None,
        };

        let allow_guard = matches!(pattern, TuplePattern::Elems(_));
        let (guard, body_span, body, block) = parse_arm_tail(&mut cur, allow_guard)?;

        arms.push(TupleArm {
            pattern_off,
            pattern,
            guard,
            body_span,
            body,
            block,
        });

        if cur.peek().is_none() {
            break;
        }
        cur.eat_punct(b',')?;
    }
    Some(arms)
}

/// Parses the comma-separated element patterns between a tuple pattern's
/// parens: each element a tag pattern (or-alternatives included) or `_`.
fn parse_tuple_elems(mut cur: Cursor) -> Option<Vec<Pattern>> {
    let mut elems = Vec::new();
    loop {
        let first = cur.peek()?;
        let pat = match first.kind {
            TokenKind::Ident if cur.text(first) == "_" => {
                cur.bump();
                Pattern::Wildcard
            }
            TokenKind::Ident => Pattern::Tags(parse_tag_alternatives(&mut cur)?),
            _ => return None,
        };
        elems.push(pat);
        if cur.peek().is_none() {
            break;
        }
        cur.eat_punct(b',')?;
        if cur.peek().is_none() {
            break; // trailing comma
        }
    }
    Some(elems)
}

/// Parses `Tag (| Tag)*` alternatives starting at the identifier under the
/// cursor. `||` lexes as a single OrOr token, so it can never be an
/// alternative separator — the candidate then fails the parse.
fn parse_tag_alternatives(cur: &mut Cursor) -> Option<Vec<TagPattern>> {
    let mut alts = vec![parse_tag_pattern(cur)?];
    while cur.at_punct(b'|') {
        cur.bump();
        alts.push(parse_tag_pattern(cur)?);
    }
    Some(alts)
}

/// Parses everything after an arm's pattern: an optional `if <cond>` guard
/// (refused when `allow_guard` is false), the `=>`, and the expression or
/// block body. Shared between single-match and tuple-match arms.
#[allow(clippy::type_complexity)]
fn parse_arm_tail(
    cur: &mut Cursor,
    allow_guard: bool,
) -> Option<(Option<GuardExpr>, Span, crate::ast::Program, bool)> {
    let mut guard = None;
    if allow_guard
        && matches!(cur.peek(), Some(t) if matches!(t.kind, TokenKind::Ident) && cur.text(t) == "if")
    {
        cur.bump();
        let g_start = cur.stop_byte_at(cur.idx);
        let (arrow_idx, g_end) = guard_end(cur)?;
        if cur.parser.src[g_start..g_end].trim().is_empty() {
            return None;
        }
        guard = Some(GuardExpr {
            span: Span {
                start: g_start,
                end: g_end,
            },
            expr: cur
                .parser
                .parse_tokens(&cur.tokens[cur.idx..arrow_idx], g_start, g_end),
        });
        cur.idx = arrow_idx;
    }

    if !matches!(cur.peek().map(|t| &t.kind), Some(TokenKind::Arrow)) {
        return None;
    }
    cur.bump();

    // body: `{ ... }` block or a single expression
    let body_span;
    let body_tokens;
    let mut block = false;
    if cur.at_punct(b'{') {
        let open = cur.idx;
        let close = cur.find_close()?;
        body_span = Span {
            start: cur.tokens[open].span.start + 1,
            end: cur.tokens[close].span.start,
        };
        body_tokens = &cur.tokens[open + 1..close];
        block = true;
        cur.idx = close + 1;
    } else {
        let body_start = cur.stop_byte_at(cur.idx);
        let (stop_idx, stop_byte) = expr_body_end(cur);
        body_span = Span {
            start: body_start,
            end: stop_byte,
        };
        if cur.parser.src[body_span.start..body_span.end]
            .trim()
            .is_empty()
        {
            return None;
        }
        body_tokens = &cur.tokens[cur.idx..stop_idx];
        cur.idx = stop_idx;
    }

    let body = cur
        .parser
        .parse_tokens(body_tokens, body_span.start, body_span.end);
    Some((guard, body_span, body, block))
}

/// Parses one `Tag` / `Tag(bindings...)` alternative starting at the
/// identifier under the cursor.
fn parse_tag_pattern(cur: &mut Cursor) -> Option<TagPattern> {
    let (tag, tag_span) = cur.eat_ident()?;
    if is_reserved(tag) {
        return None;
    }
    let mut bindings = None;
    if cur.at_punct(b'(') {
        let open = cur.idx;
        let close = cur.find_close()?;
        bindings = Some(parse_bindings(cur.sub(
            open + 1,
            close,
            cur.tokens[close].span.start,
        ))?);
        cur.idx = close + 1;
    }
    Some(TagPattern {
        tag: tag.to_string(),
        tag_off: tag_span.start,
        bindings,
    })
}

/// Parses `a, b: alias, ...` between the parens of a pattern (shared with
/// the let-else pattern). None on failure.
pub(super) fn parse_bindings(mut cur: Cursor) -> Option<Vec<Binding>> {
    let mut bindings = Vec::new();
    loop {
        if cur.peek().is_none() {
            break;
        }
        let (name, _) = cur.eat_ident()?;
        if is_reserved(name) {
            return None;
        }

        let mut alias = None;
        if cur.eat_punct(b':').is_some() {
            let (alias_name, _) = cur.eat_ident()?;
            if is_reserved(alias_name) {
                return None;
            }
            alias = Some(alias_name.to_string());
        }
        bindings.push(Binding {
            name: name.to_string(),
            alias,
        });

        if cur.peek().is_none() {
            break;
        }
        cur.eat_punct(b',')?;
    }
    Some(bindings)
}

/// Scans a guard condition from `cur.idx` until the arm's top-level `=>`,
/// returning the arrow's token index and byte offset. None on anything a
/// guard cannot contain at its top level (`,`, `;`, a closer) — the
/// candidate then passes through.
fn guard_end(cur: &Cursor) -> Option<(usize, usize)> {
    let mut depth = 0usize;
    let mut k = cur.idx;
    while k < cur.tokens.len() {
        let t = &cur.tokens[k];
        match t.kind {
            TokenKind::Arrow if depth == 0 => return Some((k, t.span.start)),
            TokenKind::Punct(b'(' | b'[' | b'{') => depth += 1,
            TokenKind::Punct(b')' | b']' | b'}') => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
            }
            TokenKind::Punct(b',' | b';') if depth == 0 => return None,
            _ => {}
        }
        k += 1;
    }
    None
}

/// Scans an arm's expression body from `cur.idx` until a top-level `,` or
/// closing bracket, returning the stopping token index and byte offset
/// (the region end when the tokens run out).
fn expr_body_end(cur: &Cursor) -> (usize, usize) {
    let mut depth = 0usize;
    let mut k = cur.idx;
    while k < cur.tokens.len() {
        let t = &cur.tokens[k];
        match t.kind {
            TokenKind::Punct(b'(' | b'[' | b'{') => depth += 1,
            TokenKind::Punct(b')' | b']' | b'}') => {
                if depth == 0 {
                    return (k, t.span.start);
                }
                depth -= 1;
            }
            TokenKind::Punct(b',') if depth == 0 => return (k, t.span.start),
            _ => {}
        }
        k += 1;
    }
    (k, cur.range_end)
}
