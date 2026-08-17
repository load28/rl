//! Structural parsing of rl `if let` statements:
//!
//! ```text
//! if let Tag(bindings...) = <expr> { ... }
//! if let Tag(bindings...) = <expr> { ... } else { ... }
//! if let Tag(bindings...) = <expr> { ... } else if let ... { ... }
//! ```
//!
//! Contract safety: in valid TypeScript an undotted `if` is always followed
//! by `(` — never by the reserved word `let` — so an `if let` sequence can
//! only be rl syntax. That also means a candidate that *starts* with
//! `if let` but fails to parse cannot be passed through (the output would
//! be invalid TypeScript with no position); the caller records the offset
//! and the semantic phase reports it, like a stray `|>`.
//!
//! The bound expression runs to the top-level `{` opening the then-block.
//! `match ( ... ) { ... }` shapes inside it are skipped whole, and a `{`
//! directly after `=>` (an unparenthesized block-bodied arrow) aborts —
//! parenthesize the arrow. The `else` continuation is a block or another
//! `if let`; a plain `else if (...)` is not part of the statement (v1) and
//! fails the parse.

use super::cursor::{Cursor, dotted_at, skip_match_shape};
use crate::ast::{IfLetElse, IfLetStmt, Span, TagPattern};
use crate::lexer::TokenKind;

/// `cur` is positioned just past an undotted `if` keyword (`kw_span`) whose
/// next token is `let` (the caller pre-checked). On success returns the
/// advanced cursor, the byte just past the statement, and the parsed
/// statement.
pub(super) fn parse_if_let<'t>(
    mut cur: Cursor<'t>,
    kw_span: Span,
) -> Option<(Cursor<'t>, usize, IfLetStmt)> {
    match cur.peek() {
        Some(t) if matches!(t.kind, TokenKind::Ident) && cur.text(t) == "let" => {
            cur.bump();
        }
        _ => return None,
    }

    // pattern: `Tag(bindings...)` — parens mandatory, nested allowed
    let (tag, tag_span) = cur.eat_ident()?;
    if super::is_reserved(tag) {
        return None;
    }
    if !cur.at_punct(b'(') {
        return None;
    }
    let open = cur.idx;
    let close = cur.find_close()?;
    let bindings = super::matches::parse_bindings(
        cur.sub(open + 1, close, cur.tokens[close].span.start),
        true,
    )?;
    cur.idx = close + 1;

    // `=` (but not `==` / `=>`; `=>` lexes as a fused Arrow token)
    let eq = cur.eat_punct(b'=')?;
    if matches!(cur.peek(), Some(t) if t.span.start == eq.end
        && matches!(cur.parser.bytes[t.span.start], b'=' | b'>'))
    {
        return None;
    }

    // `<expr> {` — the expression runs to the top-level then-block brace
    let expr_from = cur.idx;
    let expr_start = cur.stop_byte_at(cur.idx);
    let (expr_end, brace_idx) = expr_until_block(&cur)?;
    if cur.parser.src[expr_start..expr_end].trim().is_empty() {
        return None;
    }
    let expr = cur
        .parser
        .parse_tokens(&cur.tokens[expr_from..brace_idx], expr_start, expr_end);
    cur.idx = brace_idx;

    // then-block
    let body_open = cur.idx;
    let body_close = cur.find_close()?;
    let body_range = (
        cur.tokens[body_open].span.start + 1,
        cur.tokens[body_close].span.start,
    );
    let body = cur.parser.parse_tokens(
        &cur.tokens[body_open + 1..body_close],
        body_range.0,
        body_range.1,
    );
    let mut byte_end = cur.tokens[body_close].span.end;
    cur.idx = body_close + 1;

    // optional `else` continuation: a block or another `if let`
    let mut else_part = None;
    if matches!(cur.peek(), Some(t) if matches!(t.kind, TokenKind::Ident) && cur.text(t) == "else")
    {
        cur.bump();
        match cur.peek() {
            Some(t) if matches!(t.kind, TokenKind::Punct(b'{')) => {
                let open = cur.idx;
                let close = cur.find_close()?;
                let range = (
                    cur.tokens[open].span.start + 1,
                    cur.tokens[close].span.start,
                );
                else_part = Some(IfLetElse::Block(cur.parser.parse_tokens(
                    &cur.tokens[open + 1..close],
                    range.0,
                    range.1,
                )));
                byte_end = cur.tokens[close].span.end;
                cur.idx = close + 1;
            }
            Some(t) if matches!(t.kind, TokenKind::Ident) && cur.text(t) == "if" => {
                let if_span = t.span;
                cur.bump();
                let (next_cur, end, inner) = parse_if_let(cur, if_span)?;
                cur = next_cur;
                byte_end = end;
                else_part = Some(IfLetElse::IfLet(Box::new(inner)));
            }
            _ => return None,
        }
    }

    Some((
        cur,
        byte_end,
        IfLetStmt {
            keyword_off: kw_span.start,
            pattern: TagPattern {
                tag: tag.to_string(),
                tag_off: tag_span.start,
                bindings: Some(bindings),
            },
            expr,
            body,
            else_part,
        },
    ))
}

/// Scans the bound expression from `cur.idx` until the top-level `{` that
/// opens the then-block, returning `(expression end byte, brace token
/// index)`. Aborts on anything the expression cannot contain at its top
/// level (a closer, `;`, `,`, `=` — `=>` is fused — a `:` without a
/// pending `?`, an undotted statement-only keyword) and on a `{` directly
/// after `=>` (an unparenthesized block-bodied arrow — parenthesize it).
fn expr_until_block(cur: &Cursor) -> Option<(usize, usize)> {
    let mut depth = 0usize;
    let mut ternaries = 0usize;
    let mut expr_end = cur.stop_byte_at(cur.idx);
    let mut k = cur.idx;
    while k < cur.tokens.len() {
        let t = &cur.tokens[k];
        if let TokenKind::Ident = t.kind {
            if depth == 0 && !dotted_at(cur.tokens, cur.idx, k) {
                let word = cur.text(t);
                if super::tries::STMT_ONLY_WORDS.contains(&word) {
                    return None;
                }
                // Skip a whole `match ( ... ) { ... }` shape so its braces
                // are not mistaken for the then-block.
                if word == "match"
                    && let Some(past) = skip_match_shape(cur.tokens, k)
                {
                    expr_end = cur.tokens[past - 1].span.end;
                    k = past;
                    continue;
                }
            }
            expr_end = t.span.end;
            k += 1;
            continue;
        }
        if depth == 0 {
            match t.kind {
                TokenKind::Punct(b'{') => {
                    return if ternaries == 0
                        && !matches!(
                            k.checked_sub(1).map(|p| &cur.tokens[p].kind),
                            Some(TokenKind::Arrow)
                        ) {
                        Some((expr_end, k))
                    } else {
                        None
                    };
                }
                TokenKind::Punct(b';' | b'}' | b')' | b']' | b',' | b'=') => return None,
                TokenKind::Punct(b'?') => ternaries += 1,
                TokenKind::Punct(b':') => {
                    if ternaries == 0 {
                        return None;
                    }
                    ternaries -= 1;
                }
                _ => {}
            }
        }
        match t.kind {
            TokenKind::Punct(b'(' | b'[' | b'{') => depth += 1,
            TokenKind::Punct(b')' | b']' | b'}') => depth = depth.saturating_sub(1),
            _ => {}
        }
        expr_end = t.span.end;
        k += 1;
    }
    None
}
