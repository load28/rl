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

use super::Parser;
use crate::ast::{Span, TryStmt};
use crate::scanner::*;

/// `j` is just past the `try` keyword of a bare `try <expr>;` statement.
/// On success returns the index just past the `;` and the parsed statement.
pub(super) fn parse_try_stmt(p: &Parser, j: usize, end: usize) -> Option<(usize, TryStmt)> {
    let (parsed_end, expr_span) = parse_try_tail(p, j, end)?;
    Some((
        parsed_end,
        TryStmt {
            keyword_off: j.saturating_sub("try".len()),
            decl: None,
            expr: p.parse_range(expr_span.start, expr_span.end),
        },
    ))
}

/// `i..j` is a `const`/`let`/`var` keyword. Parses
/// `<binding> = try <expr>;`; on success returns the index just past the `;`
/// and the parsed statement.
pub(super) fn parse_try_decl(
    p: &Parser,
    i: usize,
    j: usize,
    end: usize,
) -> Option<(usize, TryStmt)> {
    let src = p.bytes;
    let binding_start = skip_ws_comments(src, j, end);
    let eq = scan_binding_end(src, binding_start, end)?;
    let binding = p.src[binding_start..eq].trim();
    if binding.is_empty() {
        return None;
    }
    // A real binding starts with the variable name or a destructuring
    // pattern. A leading reserved word means the scan ran across some other
    // construct (e.g. `const enum E { ... }` with a `= try` further down).
    if is_ident_start(binding.as_bytes()[0]) {
        let name_end = ident_end(binding.as_bytes(), 0, binding.len());
        if super::is_reserved(&binding[..name_end]) {
            return None;
        }
    } else if !matches!(binding.as_bytes()[0], b'{' | b'[') {
        return None;
    }

    let k = skip_ws_comments(src, eq + 1, end);
    if k >= end || !is_ident_start(src[k]) {
        return None;
    }
    let m = ident_end(src, k, end);
    if &p.src[k..m] != "try" {
        return None;
    }

    let (parsed_end, expr_span) = parse_try_tail(p, m, end)?;
    Some((
        parsed_end,
        TryStmt {
            keyword_off: i,
            decl: Some((p.src[i..j].to_string(), binding.to_string())),
            expr: p.parse_range(expr_span.start, expr_span.end),
        },
    ))
}

/// Parses `<expr>;` after a `try` keyword ending at `j`. Returns the index
/// just past the `;` and the expression span.
fn parse_try_tail(p: &Parser, j: usize, end: usize) -> Option<(usize, Span)> {
    let src = p.bytes;
    let start = skip_ws_comments(src, j, end);
    if start >= end || !is_expr_start(src[start]) {
        return None; // includes `try {` blocks and member-signature shapes
    }
    let semi = scan_stmt_expr_end(src, start, end)?;
    let span = Span { start, end: semi };
    if p.src[span.start..span.end].trim().is_empty() {
        return None;
    }
    Some((semi + 1, span))
}

/// True for a byte that can start the expression of a `try`. Deliberately
/// excludes `(` and `<`: a member named `try` in valid TypeScript can
/// continue with a call/generic signature (`try(x);`, `try<T>(x: T);` in an
/// interface), which would be indistinguishable from a parenthesized
/// expression — so `try f(x);` works but `try (expr);` does not (documented).
/// Everything else a member declaration can continue with (`?`, `:`, `=`,
/// `{`, ...) is excluded too.
fn is_expr_start(c: u8) -> bool {
    is_ident_start(c)
        || c.is_ascii_digit()
        || matches!(
            c,
            b'"' | b'\'' | b'`' | b'[' | b'!' | b'~' | b'+' | b'-' | b'/'
        )
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

/// Scans a statement expression until a top-level `;`, returning its index.
/// Aborts (None) on anything that cannot appear at the top level of an
/// expression: a bare `{`, a closer, `,`, `=` (except `=>`), `:` without a
/// pending ternary `?`, or an undotted statement-only keyword — these are
/// member-declaration or next-statement shapes, not expressions.
fn scan_stmt_expr_end(src: &[u8], mut i: usize, end: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut ternaries = 0usize;
    let mut prev_sig = 0u8;
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
            prev_sig = c;
            continue;
        }
        if c == b'`' {
            i = skip_template(src, i, end);
            prev_sig = b'`';
            continue;
        }
        if c == b'=' && at(src, i + 1, end) == Some(b'>') {
            i += 2;
            prev_sig = b'>';
            continue;
        }
        if is_ident_start(c) {
            let j = ident_end(src, i, end);
            if depth == 0 && prev_sig != b'.' {
                if STMT_ONLY_WORDS.contains(&std::str::from_utf8(&src[i..j]).unwrap_or("")) {
                    return None;
                }
                // A match expression carries its own top-level braces; skip
                // the whole `match ( ... ) { ... }` shape so the bare-`{`
                // abort below doesn't reject it (the recursive parse of the
                // expression decides whether it really is an rl match).
                if &src[i..j] == b"match" {
                    let k = skip_ws_comments(src, j, end);
                    if at(src, k, end) == Some(b'(')
                        && let Some(close_paren) = find_matching(src, k, end)
                    {
                        let b = skip_ws_comments(src, close_paren + 1, end);
                        if at(src, b, end) == Some(b'{')
                            && let Some(close_brace) = find_matching(src, b, end)
                        {
                            prev_sig = b'}';
                            i = close_brace + 1;
                            continue;
                        }
                    }
                }
            }
            prev_sig = src[j - 1];
            i = j;
            continue;
        }
        if depth == 0 {
            match c {
                b';' => return if ternaries == 0 { Some(i) } else { None },
                b'{' | b'}' | b')' | b']' | b',' | b'=' => return None,
                b'?' => {
                    // `?.` and `??` are not ternary openers
                    if matches!(at(src, i + 1, end), Some(b'.') | Some(b'?')) {
                        i += 2;
                        prev_sig = src[i - 1];
                        continue;
                    }
                    ternaries += 1;
                }
                b':' => {
                    if ternaries == 0 {
                        return None;
                    }
                    ternaries -= 1;
                }
                _ => {}
            }
        }
        match c {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
        if !is_ws(c) {
            prev_sig = c;
        }
        i += 1;
    }
    None
}

/// Scans a declaration binding (identifier or destructuring pattern, with an
/// optional type annotation) until the top-level `=`, returning its index.
/// Aborts on a top-level `;`, `,`, or closer before any `=` is found.
fn scan_binding_end(src: &[u8], mut i: usize, end: usize) -> Option<usize> {
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
        if c == b'=' && at(src, i + 1, end) == Some(b'>') {
            i += 2;
            continue;
        }
        match c {
            b'(' | b'[' | b'{' | b'<' => depth += 1,
            b')' | b']' | b'}' => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
            }
            b'>' => depth = depth.saturating_sub(1),
            b'=' if depth == 0 => return Some(i),
            b';' | b',' if depth == 0 => return None,
            _ => {}
        }
        i += 1;
    }
    None
}
