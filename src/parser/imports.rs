//! Structural parsing of static import/re-export module specifiers.
//!
//! Only the specifier *string* of a static `import` declaration or an
//! `export ... from` re-export is lifted, and only when it is a relative
//! path ending in `.rl`; the rest of the statement stays a verbatim byte
//! range. Dynamic `import(...)`, `import.meta`, and TypeScript
//! import-assignment (`import x = require(...)`) never match, and — like
//! every rl construct — a clause that deviates from the expected token
//! shape aborts the attempt, leaving the statement untouched.

use crate::ast::Span;
use crate::scanner::*;

use super::{Parser, is_reserved};

/// `kw` is `import` or `export` and `kw_end` the offset just past it.
/// Returns the span of the statement's module specifier string (including
/// quotes) when the clause fully parses as a static import/re-export *and*
/// the specifier is a relative `.rl` path.
pub(super) fn parse_rl_import(p: &Parser, kw: &str, kw_end: usize, end: usize) -> Option<Span> {
    let src = p.bytes;
    let i = skip_ws_comments(src, kw_end, end);
    let c = at(src, i, end)?;

    if kw == "import" {
        // `import "spec";` — side-effect import, the specifier is right here.
        if c == b'"' || c == b'\'' {
            return rl_spec_span(src, i, end);
        }
        // `import(...)` / `import.meta` — not a static declaration.
        if c == b'(' || c == b'.' {
            return None;
        }
        clause_then_spec(p, i, end)
    } else {
        // A re-export starts with `{`, `*`, or `type` followed by `{`/`*`;
        // anything else (`const`, `default`, `enum`, ...) is not one.
        match c {
            b'{' | b'*' => clause_then_spec(p, i, end),
            _ if is_ident_start(c) => {
                let j = ident_end(src, i, end);
                if &p.src[i..j] != "type" {
                    return None;
                }
                let k = skip_ws_comments(src, j, end);
                match at(src, k, end)? {
                    b'{' | b'*' => clause_then_spec(p, k, end),
                    _ => None,
                }
            }
            _ => None,
        }
    }
}

/// Scans an import/re-export clause token by token until `from`, then
/// expects the specifier string. Clauses are only identifiers (bindings,
/// contextual `type`/`as`), `{ ... }` lists, `*`, and `,` — any other
/// token (a reserved word, `=`, `;`, ...) means this is not a static
/// import clause.
fn clause_then_spec(p: &Parser, mut i: usize, end: usize) -> Option<Span> {
    let src = p.bytes;
    loop {
        i = skip_ws_comments(src, i, end);
        let c = at(src, i, end)?;
        match c {
            b'{' => i = find_matching(src, i, end)? + 1,
            b'*' | b',' => i += 1,
            _ if is_ident_start(c) => {
                let j = ident_end(src, i, end);
                let word = &p.src[i..j];
                if word == "from" {
                    let k = skip_ws_comments(src, j, end);
                    return rl_spec_span(src, k, end);
                }
                if is_reserved(word) {
                    return None;
                }
                i = j;
            }
            _ => return None,
        }
    }
}

/// `src[i]` should start a string literal whose content is a relative path
/// ending in `.rl`; returns its span (including quotes) if so.
fn rl_spec_span(src: &[u8], i: usize, end: usize) -> Option<Span> {
    let quote = at(src, i, end)?;
    if quote != b'"' && quote != b'\'' {
        return None;
    }
    let close = scan_string(src, i, end);
    // `scan_string` stops at a newline/EOF for unterminated strings —
    // require a real closing quote.
    if close < i + 2 || src[close - 1] != quote {
        return None;
    }
    let spec = &src[i + 1..close - 1];
    let relative = spec.starts_with(b"./") || spec.starts_with(b"../");
    if relative && spec.ends_with(b".rl") {
        Some(Span {
            start: i,
            end: close,
        })
    } else {
        None
    }
}
