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

use super::Parser;
use crate::ast::LetElseStmt;
use crate::scanner::*;

/// `i..j` is a `const`/`let`/`var` keyword. Parses
/// `Tag(bindings...) = <expr> else { ... };`; on success returns the index
/// just past the `;` and the parsed statement.
pub(super) fn parse_let_else(
    p: &Parser,
    i: usize,
    j: usize,
    end: usize,
) -> Option<(usize, LetElseStmt)> {
    let src = p.bytes;

    // pattern: `Tag(bindings...)`
    let tag_start = skip_ws_comments(src, j, end);
    if tag_start >= end || !is_ident_start(src[tag_start]) {
        return None;
    }
    let tag_end = ident_end(src, tag_start, end);
    let tag = &p.src[tag_start..tag_end];
    if super::is_reserved(tag) {
        return None; // `const enum E { ... }` and friends
    }
    let open = skip_ws_comments(src, tag_end, end);
    if at(src, open, end) != Some(b'(') {
        return None;
    }
    let close = find_matching(src, open, end)?;
    let bindings = super::matches::parse_bindings(p, open + 1, close)?;

    // `=` (but not `==` / `=>`)
    let eq = skip_ws_comments(src, close + 1, end);
    if at(src, eq, end) != Some(b'=') || matches!(at(src, eq + 1, end), Some(b'=') | Some(b'>')) {
        return None;
    }

    // `<expr> else`
    let expr_start = skip_ws_comments(src, eq + 1, end);
    let (expr_end, else_off) = scan_expr_until_else(src, expr_start, end)?;
    if p.src[expr_start..expr_end].trim().is_empty() {
        return None;
    }

    // `{ ... };`
    let b = skip_ws_comments(src, else_off + "else".len(), end);
    if at(src, b, end) != Some(b'{') {
        return None;
    }
    let close_brace = find_matching(src, b, end)?;
    let semi = skip_ws_comments(src, close_brace + 1, end);
    if at(src, semi, end) != Some(b';') {
        return None;
    }

    Some((
        semi + 1,
        LetElseStmt {
            keyword_off: i,
            kw: p.src[i..j].to_string(),
            tag: tag.to_string(),
            bindings,
            expr: p.parse_range(expr_start, expr_end),
            else_body: p.parse_range(b + 1, close_brace),
            else_off,
            diverges: block_diverges(src, b + 1, close_brace),
        },
    ))
}

/// Scans the bound expression until a top-level undotted `else`, returning
/// `(expression end, offset of the else keyword)`. The same aborts as the
/// try statement's expression scanner apply — anything that cannot appear
/// at the top level of an expression (a bare `{`, a closer, `,`, `=` except
/// `=>`, `:` without a pending `?`, `;`, an undotted statement-only
/// keyword) fails the parse so the text passes through.
fn scan_expr_until_else(src: &[u8], mut i: usize, end: usize) -> Option<(usize, usize)> {
    let mut depth = 0usize;
    let mut ternaries = 0usize;
    let mut prev_sig = 0u8;
    let mut expr_end = i;
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
            expr_end = i;
            continue;
        }
        if c == b'`' {
            i = skip_template(src, i, end);
            prev_sig = b'`';
            expr_end = i;
            continue;
        }
        if c == b'=' && at(src, i + 1, end) == Some(b'>') {
            i += 2;
            prev_sig = b'>';
            expr_end = i;
            continue;
        }
        if is_ident_start(c) {
            let j = ident_end(src, i, end);
            if depth == 0 && prev_sig != b'.' {
                if &src[i..j] == b"else" {
                    return if ternaries == 0 {
                        Some((expr_end, i))
                    } else {
                        None
                    };
                }
                if super::tries::STMT_ONLY_WORDS
                    .contains(&std::str::from_utf8(&src[i..j]).unwrap_or(""))
                {
                    return None;
                }
                // Skip a whole `match ( ... ) { ... }` shape so the bare-`{`
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
                            expr_end = i;
                            continue;
                        }
                    }
                }
            }
            prev_sig = src[j - 1];
            i = j;
            expr_end = i;
            continue;
        }
        if depth == 0 {
            match c {
                b';' | b'{' | b'}' | b')' | b']' | b',' | b'=' => return None,
                b'?' => {
                    // `?.` and `??` are not ternary openers
                    if matches!(at(src, i + 1, end), Some(b'.') | Some(b'?')) {
                        i += 2;
                        prev_sig = src[i - 1];
                        expr_end = i;
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
            expr_end = i + 1;
        }
        i += 1;
    }
    None
}

/// True when the block's last top-level statement starts with `return`,
/// `throw`, `break`, or `continue` — the syntactic stand-in for Rust's
/// "the else block must diverge" rule (rlc does no type analysis, so e.g.
/// an `if`/`else` where both branches return is *not* recognized; end the
/// block with one of the four keywords instead).
fn block_diverges(src: &[u8], start: usize, end: usize) -> bool {
    let mut last = skip_ws_comments(src, start, end);
    let mut depth = 0usize;
    let mut i = last;
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
            b')' | b']' => depth = depth.saturating_sub(1),
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    // end of a block statement (if/for/function body, ...)
                    let n = skip_ws_comments(src, i + 1, end);
                    if n < end {
                        last = n;
                    }
                }
            }
            b';' if depth == 0 => {
                let n = skip_ws_comments(src, i + 1, end);
                if n < end {
                    last = n;
                }
            }
            _ => {}
        }
        i += 1;
    }
    if last >= end || !is_ident_start(src[last]) {
        return false;
    }
    let w = ident_end(src, last, end);
    matches!(&src[last..w], b"return" | b"throw" | b"break" | b"continue")
}
