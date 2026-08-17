//! Structural parsing of static import/re-export module specifiers.
//!
//! Only the specifier *string* of a static `import` declaration or an
//! `export ... from` re-export is lifted, and only when it is a relative
//! path ending in `.rl`; the rest of the statement stays a verbatim byte
//! range. Dynamic `import(...)`, `import.meta`, and TypeScript
//! import-assignment (`import x = require(...)`) never match, and — like
//! every rl construct — a clause that deviates from the expected token
//! shape aborts the attempt, leaving the statement untouched.

use super::cursor::Cursor;
use super::is_reserved;
use crate::ast::Span;

use crate::lexer::TokenKind;

/// `cur` is positioned just past an `import` or `export` keyword (`kw`).
/// Returns the advanced cursor and the span of the statement's module
/// specifier string (including quotes) when the clause fully parses as a
/// static import/re-export *and* the specifier is a relative `.rl` path.
pub(super) fn parse_rl_import<'t>(mut cur: Cursor<'t>, kw: &str) -> Option<(Cursor<'t>, Span)> {
    let first = cur.peek()?;

    if kw == "import" {
        match first.kind {
            // `import "spec";` — side-effect import, the specifier is right here.
            TokenKind::Str => {
                let spec = rl_spec_span(&cur, first.span)?;
                cur.bump();
                return Some((cur, spec));
            }
            // `import(...)` / `import.meta` — not a static declaration.
            TokenKind::Punct(b'(' | b'.') | TokenKind::OptChain => return None,
            _ => {}
        }
        clause_then_spec(cur)
    } else {
        // A re-export starts with `{`, `*`, or `type` followed by `{`/`*`;
        // anything else (`const`, `default`, `enum`, ...) is not one.
        match first.kind {
            TokenKind::Punct(b'{' | b'*') => clause_then_spec(cur),
            TokenKind::Ident if cur.text(first) == "type" => {
                match cur.tokens.get(cur.idx + 1).map(|t| &t.kind) {
                    Some(TokenKind::Punct(b'{' | b'*')) => {
                        cur.bump();
                        clause_then_spec(cur)
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }
}

/// Consumes an import/re-export clause token by token until `from`, then
/// expects the specifier string. Clauses are only identifiers (bindings,
/// contextual `type`/`as`), `{ ... }` lists, `*`, and `,` — any other
/// token (a reserved word, `=`, `;`, ...) means this is not a static
/// import clause.
fn clause_then_spec(mut cur: Cursor<'_>) -> Option<(Cursor<'_>, Span)> {
    loop {
        let t = cur.peek()?;
        match t.kind {
            TokenKind::Punct(b'{') => {
                let close = cur.find_close()?;
                cur.idx = close + 1;
            }
            TokenKind::Punct(b'*' | b',') => {
                cur.bump();
            }
            TokenKind::Ident => {
                let word = cur.text(t);
                if word == "from" {
                    cur.bump();
                    let spec_tok = cur.peek()?;
                    if !matches!(spec_tok.kind, TokenKind::Str) {
                        return None;
                    }
                    let spec = rl_spec_span(&cur, spec_tok.span)?;
                    cur.bump();
                    return Some((cur, spec));
                }
                if is_reserved(word) {
                    return None;
                }
                cur.bump();
            }
            _ => return None,
        }
    }
}

/// `span` is a lexed string token; returns it back if its content is a
/// relative path ending in `.rl`.
fn rl_spec_span(cur: &Cursor, span: Span) -> Option<Span> {
    let src = cur.parser.bytes;
    let quote = src[span.start];
    // The lexer tolerates unterminated strings (stopping at a newline or
    // EOF) — require a real closing quote.
    if span.end < span.start + 2 || src[span.end - 1] != quote {
        return None;
    }
    let spec = &src[span.start + 1..span.end - 1];
    let relative = spec.starts_with(b"./") || spec.starts_with(b"../");
    if relative && spec.ends_with(b".rl") {
        Some(span)
    } else {
        None
    }
}
