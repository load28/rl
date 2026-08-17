//! Structural parsing of rl let-else statements (Rust-style refutable
//! binding):
//!
//! ```text
//! const|let|var Tag(bindings...) = <expr> else { ... };
//! ```
//!
//! Contract safety: in valid TypeScript a `const`/`let`/`var` keyword is
//! always followed by a binding identifier or a destructuring pattern —
//! never by `<ident>(`. The pattern's mandatory parens therefore decide
//! construct-hood immediately after the keyword, before anything else is
//! scanned. Reserved tags reject TypeScript-only forms (`const enum`), and
//! a method *named* `const`/`let`/`var` (`{ const(x) { ... } }`) is
//! followed by `(`, not an identifier, so it never gets here. Anything
//! that deviates passes through verbatim, as always.
//!
//! The "else block must diverge" rule is *computed* here (a bool on the AST
//! node) but *enforced* by [`crate::sema`] — the parser stays infallible.

use super::cursor::{Cursor, dotted_at, skip_match_shape};
use crate::ast::LetElseStmt;
use crate::lexer::TokenKind;

/// `cur` is positioned just past a `const`/`let`/`var` keyword
/// (`kw_span`). Parses `Tag(bindings...) = <expr> else { ... };`; on
/// success returns the advanced cursor, the byte just past the `;`, and
/// the parsed statement.
pub(super) fn parse_let_else<'t>(
    mut cur: Cursor<'t>,
    kw_span: crate::ast::Span,
) -> Option<(Cursor<'t>, usize, LetElseStmt)> {
    // pattern: `Tag(bindings...)`
    let (tag, _) = cur.eat_ident()?;
    if super::is_reserved(tag) {
        return None; // `const enum E { ... }` and friends
    }
    if !cur.at_punct(b'(') {
        return None;
    }
    let open = cur.idx;
    let close = cur.find_close()?;
    let bindings = super::matches::parse_bindings(
        cur.sub(open + 1, close, cur.tokens[close].span.start),
        false, // let-else bindings stay alias-only (no nested patterns)
    )?;
    cur.idx = close + 1;

    // `=` (but not `==` / `=>`; `=>` lexes as a fused Arrow token)
    let eq = cur.eat_punct(b'=')?;
    if matches!(cur.peek(), Some(t) if t.span.start == eq.end
        && matches!(cur.parser.bytes[t.span.start], b'=' | b'>'))
    {
        return None;
    }

    // `<expr> else`
    let expr_from = cur.idx;
    let expr_start = cur.stop_byte_at(cur.idx);
    let (expr_end, else_idx) = expr_until_else(&cur)?;
    if cur.parser.src[expr_start..expr_end].trim().is_empty() {
        return None;
    }
    let else_off = cur.tokens[else_idx].span.start;
    cur.idx = else_idx + 1;

    // `{ ... };`
    if !cur.at_punct(b'{') {
        return None;
    }
    let body_open = cur.idx;
    let body_close = cur.find_close()?;
    cur.idx = body_close + 1;
    let semi = cur.eat_punct(b';')?;

    let body_range = (
        cur.tokens[body_open].span.start + 1,
        cur.tokens[body_close].span.start,
    );
    let body_tokens = &cur.tokens[body_open + 1..body_close];
    Some((
        cur,
        semi.end,
        LetElseStmt {
            keyword_off: kw_span.start,
            kw: cur.parser.src[kw_span.start..kw_span.end].to_string(),
            tag: tag.to_string(),
            bindings,
            expr: cur
                .parser
                .parse_tokens(&cur.tokens[expr_from..else_idx], expr_start, expr_end),
            else_body: cur
                .parser
                .parse_tokens(body_tokens, body_range.0, body_range.1),
            else_off,
            diverges: block_diverges(cur.parser, body_tokens),
        },
    ))
}

/// Scans the bound expression from `cur.idx` until a top-level undotted
/// `else`, returning `(expression end byte, else token index)`. The same
/// aborts as the try statement's expression scanner apply — anything that
/// cannot appear at the top level of an expression (a bare `{`, a closer,
/// `,`, `=` except the fused `=>`, `:` without a pending `?`, `;`, an
/// undotted statement-only keyword) fails the parse so the text passes
/// through.
fn expr_until_else(cur: &Cursor) -> Option<(usize, usize)> {
    let mut depth = 0usize;
    let mut ternaries = 0usize;
    let mut expr_end = cur.stop_byte_at(cur.idx);
    let mut k = cur.idx;
    while k < cur.tokens.len() {
        let t = &cur.tokens[k];
        if let TokenKind::Ident = t.kind {
            if depth == 0 && !dotted_at(cur.tokens, cur.idx, k) {
                let word = cur.text(t);
                if word == "else" {
                    return if ternaries == 0 {
                        Some((expr_end, k))
                    } else {
                        None
                    };
                }
                if super::tries::STMT_ONLY_WORDS.contains(&word) {
                    return None;
                }
                // Skip a whole `match ( ... ) { ... }` shape so the bare-`{`
                // abort below doesn't reject it (the recursive parse of the
                // expression decides whether it really is an rl match).
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
                TokenKind::Punct(b';' | b'{' | b'}' | b')' | b']' | b',' | b'=') => return None,
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

/// True when the block's last top-level statement starts with `return`,
/// `throw`, `break`, or `continue` — the syntactic stand-in for Rust's
/// "the else block must diverge" rule (rlc does no type analysis, so e.g.
/// an `if`/`else` where both branches return is *not* recognized; end the
/// block with one of the four keywords instead).
fn block_diverges(parser: &super::Parser, tokens: &[crate::lexer::Token]) -> bool {
    let mut last = 0usize;
    let mut depth = 0usize;
    for (k, t) in tokens.iter().enumerate() {
        match t.kind {
            TokenKind::Punct(b'(' | b'[' | b'{') => depth += 1,
            TokenKind::Punct(b')' | b']') => depth = depth.saturating_sub(1),
            TokenKind::Punct(b'}') => {
                depth = depth.saturating_sub(1);
                if depth == 0 && k + 1 < tokens.len() {
                    // end of a block statement (if/for/function body, ...)
                    last = k + 1;
                }
            }
            TokenKind::Punct(b';') if depth == 0 && k + 1 < tokens.len() => {
                last = k + 1;
            }
            _ => {}
        }
    }
    match tokens.get(last) {
        Some(t) if matches!(t.kind, TokenKind::Ident) => matches!(
            &parser.src[t.span.start..t.span.end],
            "return" | "throw" | "break" | "continue"
        ),
        _ => false,
    }
}
