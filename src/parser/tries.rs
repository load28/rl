//! Structural parsing of rl `try` statements (Rust-style error propagation).
//!
//! Two forms, both terminated by a top-level `;`:
//!
//! - `try <expr>;` — propagate, discarding the `Ok` value
//! - `const|let|var <binding> = try <expr>;` — propagate or bind the value
//!
//! Contract safety: `try` is a reserved word, so in valid TypeScript it can
//! only start a `try { ... } catch` statement or appear as a member name
//! (`obj.try(...)`, `{ try: 1 }`, `class A { try = 5 }`, interface method
//! signatures). An expression-position `try` is never valid TypeScript, so
//! lifting `= try <expr>;` can never mis-transform a valid file, and the bare
//! form structurally excludes every valid member shape: dotted `try` is
//! rejected by the caller, `try {` is rejected here, and the expression
//! scanner aborts on anything an expression cannot start with or contain at
//! top level (`:` without a ternary `?`, `=`, a bare `{`, closers, `,`).
//! Anything rejected passes through verbatim, as always.

use super::cursor::{Cursor, dotted_at, skip_braced_construct};
use crate::ast::{Span, TryStmt};
use crate::lexer::{Token, TokenKind};

/// `cur` is positioned just past the `try` keyword (`kw_span`) of a bare
/// `try <expr>;` statement. On success returns the advanced cursor, the
/// byte just past the `;`, and the parsed statement.
pub(super) fn parse_try_stmt<'t>(
    cur: Cursor<'t>,
    kw_span: Span,
) -> Option<(Cursor<'t>, usize, TryStmt)> {
    let (cur, byte_end, expr_span, expr_tokens) = parse_try_tail(cur)?;
    let expr = cur
        .parser
        .parse_tokens(expr_tokens, expr_span.start, expr_span.end);
    Some((
        cur,
        byte_end,
        TryStmt {
            keyword_off: kw_span.start,
            decl: None,
            expr,
        },
    ))
}

/// `cur` is positioned just past a `const`/`let`/`var` keyword
/// (`kw_span`). Parses `<binding> = try <expr>;`; on success returns the
/// advanced cursor, the byte just past the `;`, and the parsed statement.
pub(super) fn parse_try_decl<'t>(
    mut cur: Cursor<'t>,
    kw_span: Span,
) -> Option<(Cursor<'t>, usize, TryStmt)> {
    let binding_start = cur.stop_byte_at(cur.idx);
    let (eq_idx, eq_byte) = binding_end(&cur)?;
    let binding = cur.parser.src[binding_start..eq_byte].trim();
    if binding.is_empty() {
        return None;
    }
    // A real binding starts with the variable name or a destructuring
    // pattern. A leading reserved word means the scan ran across some other
    // construct (e.g. `const enum E { ... }` with a `= try` further down).
    match cur.tokens[cur.idx].kind {
        TokenKind::Ident => {
            if super::is_reserved(cur.text(&cur.tokens[cur.idx])) {
                return None;
            }
        }
        TokenKind::Punct(b'{' | b'[') => {}
        _ => return None,
    }
    cur.idx = eq_idx + 1;

    match cur.peek() {
        Some(t) if matches!(t.kind, TokenKind::Ident) && cur.text(t) == "try" => {
            cur.bump();
        }
        _ => return None,
    }

    let (cur, byte_end, expr_span, expr_tokens) = parse_try_tail(cur)?;
    let expr = cur
        .parser
        .parse_tokens(expr_tokens, expr_span.start, expr_span.end);
    Some((
        cur,
        byte_end,
        TryStmt {
            keyword_off: kw_span.start,
            decl: Some((
                cur.parser.src[kw_span.start..kw_span.end].to_string(),
                binding.to_string(),
            )),
            expr,
        },
    ))
}

/// Parses `<expr>;` with the cursor just past a `try` keyword. Returns the
/// advanced cursor, the byte just past the `;`, and the expression's span
/// and tokens.
fn parse_try_tail<'t>(mut cur: Cursor<'t>) -> Option<(Cursor<'t>, usize, Span, &'t [Token])> {
    let first = cur.peek()?;
    if !is_expr_start(first) {
        return None; // includes `try {` blocks and member-signature shapes
    }
    let (semi_idx, semi_byte) = stmt_expr_end(&cur)?;
    let span = Span {
        start: first.span.start,
        end: semi_byte,
    };
    if cur.parser.src[span.start..span.end].trim().is_empty() {
        return None;
    }
    let expr_tokens = &cur.tokens[cur.idx..semi_idx];
    cur.idx = semi_idx + 1;
    Some((cur, semi_byte + 1, span, expr_tokens))
}

/// True for a token that can start the expression of a `try`. Deliberately
/// excludes `(` and `<`: a member named `try` in valid TypeScript can
/// continue with a call/generic signature (`try(x);`, `try<T>(x: T);` in an
/// interface), which would be indistinguishable from a parenthesized
/// expression — so `try f(x);` works but `try (expr);` does not (documented).
/// Everything else a member declaration can continue with (`?`, `:`, `=`,
/// `{`, ...) is excluded too.
fn is_expr_start(t: &Token) -> bool {
    match t.kind {
        TokenKind::Ident | TokenKind::Str | TokenKind::Template(_) | TokenKind::Regex => true,
        TokenKind::Punct(c) => {
            c.is_ascii_digit() || matches!(c, b'[' | b'!' | b'~' | b'+' | b'-' | b'/')
        }
        TokenKind::Arrow
        | TokenKind::OrOr
        | TokenKind::OptChain
        | TokenKind::Coalesce
        | TokenKind::PipeOp => false,
    }
}

/// Statement-only keywords: meeting one at the top level of the expression
/// scan means we ran past the statement (e.g. a missing `;`) or into a
/// declaration — abort so the text passes through. Expression-capable
/// keywords (`new`, `typeof`, `await`, `function`, `class`, `import(...)`,
/// ...) are deliberately absent. Shared with the let-else expression
/// scanner (which treats `else` as its terminator instead).
pub(super) const STMT_ONLY_WORDS: &[&str] = &[
    "break", "case", "catch", "const", "continue", "debugger", "default", "do", "else", "enum",
    "export", "finally", "for", "if", "let", "return", "switch", "throw", "try", "var", "while",
    "with",
];

/// Scans a statement expression from `cur.idx` until a top-level `;`,
/// returning its token index and byte offset. Aborts (None) on anything
/// that cannot appear at the top level of an expression: a bare `{`, a
/// closer, `,`, `=` (`=>` is a fused token and passes), `:` without a
/// pending ternary `?`, or an undotted statement-only keyword — these are
/// member-declaration or next-statement shapes, not expressions.
fn stmt_expr_end(cur: &Cursor) -> Option<(usize, usize)> {
    let mut depth = 0usize;
    let mut ternaries = 0usize;
    let mut k = cur.idx;
    while k < cur.tokens.len() {
        let t = &cur.tokens[k];
        if let TokenKind::Ident = t.kind {
            if depth == 0 && !dotted_at(cur.tokens, cur.idx, k) {
                let word = cur.text(t);
                if STMT_ONLY_WORDS.contains(&word) {
                    return None;
                }
                // A match expression and a `result` block carry their own
                // top-level braces; skip the whole shape so the bare-`{`
                // abort below doesn't reject it (the recursive parse of the
                // expression decides whether it really is an rl construct).
                if let Some(past) = skip_braced_construct(cur.tokens, word, k) {
                    k = past;
                    continue;
                }
            }
            k += 1;
            continue;
        }
        if depth == 0 {
            match t.kind {
                TokenKind::Punct(b';') => {
                    return if ternaries == 0 {
                        Some((k, t.span.start))
                    } else {
                        None
                    };
                }
                TokenKind::Punct(b'{' | b'}' | b')' | b']' | b',' | b'=') => return None,
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
        k += 1;
    }
    None
}

/// Scans a declaration binding (identifier or destructuring pattern, with an
/// optional type annotation) from `cur.idx` until the top-level `=`,
/// returning its token index and byte offset. Aborts on a top-level `;`,
/// `,`, or closer before any `=` is found.
fn binding_end(cur: &Cursor) -> Option<(usize, usize)> {
    let mut depth = 0usize;
    let mut k = cur.idx;
    while k < cur.tokens.len() {
        let t = &cur.tokens[k];
        match t.kind {
            TokenKind::Punct(b'(' | b'[' | b'{' | b'<') => depth += 1,
            TokenKind::Punct(b')' | b']' | b'}') => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
            }
            TokenKind::Punct(b'>') => depth = depth.saturating_sub(1),
            TokenKind::Punct(b'=') if depth == 0 => return Some((k, t.span.start)),
            TokenKind::Punct(b';' | b',') if depth == 0 => return None,
            _ => {}
        }
        k += 1;
    }
    None
}
