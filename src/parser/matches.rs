//! Structural parsing of rl `match` expressions.
//!
//! Purely structural: anything that does not fully parse as an rl match (a
//! method named `match`, `String.prototype.match` calls, ...) returns `None`
//! and passes through verbatim. rl-level errors — duplicate arms, a
//! misplaced wildcard, non-exhaustiveness — are the semantic phase's job.
//! The scrutinee and every arm body are recursively parsed sub-programs.

use super::cursor::Cursor;
use super::is_reserved;
use crate::ast::{Arm, Binding, GuardExpr, MatchExpr, Pattern, Span, TagPattern};
use crate::lexer::TokenKind;

/// `cur` is positioned just past the `match` keyword (`kw_span`). On
/// success returns the advanced cursor, the byte just past the closing
/// brace, and the parsed expression.
pub(super) fn parse_match<'t>(
    mut cur: Cursor<'t>,
    kw_span: Span,
) -> Option<(Cursor<'t>, usize, MatchExpr)> {
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
    let arms =
        match parse_arms(cur.sub(body_open + 1, body_close, cur.tokens[body_close].span.start)) {
            Some(arms) if !arms.is_empty() => arms,
            _ => return None,
        };

    let byte_end = cur.tokens[body_close].span.end;
    let scrutinee = cur.parser.parse_tokens(
        &cur.tokens[open + 1..close],
        scrutinee_span.start,
        scrutinee_span.end,
    );
    cur.idx = body_close + 1;
    Some((
        cur,
        byte_end,
        MatchExpr {
            keyword_off: kw_span.start,
            scrutinee_span,
            scrutinee,
            arms,
        },
    ))
}

fn parse_arms(mut cur: Cursor) -> Option<Vec<Arm>> {
    let mut arms = Vec::new();
    loop {
        let Some(first) = cur.peek() else {
            break;
        };
        let pattern_off = first.span.start;

        // pattern
        let pattern = match first.kind {
            TokenKind::Ident if cur.text(first) == "_" => {
                cur.bump();
                Pattern::Wildcard
            }
            TokenKind::Ident => {
                let mut alts = vec![parse_tag_pattern(&mut cur)?];
                // `|`-separated alternatives; `||` lexes as a single OrOr
                // token, so it can never be an alternative separator — the
                // candidate then fails the parse and passes through.
                while cur.at_punct(b'|') {
                    cur.bump();
                    alts.push(parse_tag_pattern(&mut cur)?);
                }
                Pattern::Tags(alts)
            }
            _ => return None,
        };

        // optional guard: `if <cond>` between the pattern and `=>`. Only tag
        // patterns take a guard — `_ if` never parses, so it passes through.
        let mut guard = None;
        if matches!(pattern, Pattern::Tags(_))
            && matches!(cur.peek(), Some(t) if matches!(t.kind, TokenKind::Ident) && cur.text(t) == "if")
        {
            cur.bump();
            let g_start = cur.stop_byte_at(cur.idx);
            let (arrow_idx, g_end) = guard_end(&cur)?;
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
            let (stop_idx, stop_byte) = expr_body_end(&cur);
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

        arms.push(Arm {
            pattern,
            pattern_off,
            guard,
            body_span,
            body: cur
                .parser
                .parse_tokens(body_tokens, body_span.start, body_span.end),
            block,
        });

        if cur.peek().is_none() {
            break;
        }
        cur.eat_punct(b',')?;
    }
    Some(arms)
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
