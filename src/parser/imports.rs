//! Structural parsing of static import/re-export module specifiers.
//!
//! Only the specifier *string* of a static `import` declaration or an
//! `export ... from` re-export is lifted, and only when it is a relative
//! path ending in `.rl`; the rest of the statement stays a verbatim byte
//! range. Dynamic `import(...)`, `import.meta`, and TypeScript
//! import-assignment (`import x = require(...)`) never match, and — like
//! every rl construct — a clause that deviates from the expected token
//! shape aborts the attempt, leaving the statement untouched.
//!
//! Alongside the specifier, the clause's imported names are collected for
//! the declaration-collection API (project-wide exhaustiveness). Name
//! collection is best-effort and never changes whether a specifier is
//! lifted: an entry that doesn't parse as `[type] name [as alias]` is
//! skipped, costing only the exhaustiveness information for that binding.

use super::cursor::Cursor;
use super::is_reserved;
use crate::ast::{RlImportDecl, RlImportNames, RlSpecifier, Span};

use crate::lexer::TokenKind;

/// `cur` is positioned just past an `import` or `export` keyword (`kw`).
/// Returns the advanced cursor and the lifted import (specifier span plus
/// imported names) when the clause fully parses as a static
/// import/re-export *and* the specifier is a relative `.rl` path.
pub(super) fn parse_rl_import<'t>(
    mut cur: Cursor<'t>,
    kw: &str,
) -> Option<(Cursor<'t>, RlImportDecl)> {
    let first = cur.peek()?;

    if kw == "import" {
        match first.kind {
            // `import "spec";` — side-effect import, the specifier is right here.
            TokenKind::Str => {
                let (spec, kind) = rl_spec_span(&cur, first.span)?;
                cur.bump();
                return Some((
                    cur,
                    RlImportDecl {
                        spec,
                        kind,
                        names: RlImportNames::None,
                    },
                ));
            }
            // `import(...)` / `import.meta` — not a static declaration.
            TokenKind::Punct(b'(' | b'.') | TokenKind::OptChain => return None,
            _ => {}
        }
        clause_then_spec(cur, true)
    } else {
        // A re-export starts with `{`, `*`, or `type` followed by `{`/`*`;
        // anything else (`const`, `default`, `enum`, ...) is not one.
        // Re-exports bring nothing into local scope, so no names.
        match first.kind {
            TokenKind::Punct(b'{' | b'*') => clause_then_spec(cur, false),
            TokenKind::Ident if cur.text(first) == "type" => {
                match cur.tokens.get(cur.idx + 1).map(|t| &t.kind) {
                    Some(TokenKind::Punct(b'{' | b'*')) => {
                        cur.bump();
                        clause_then_spec(cur, false)
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
/// import clause. `local` is true for `import` declarations, whose clause
/// names enter local scope and are collected.
fn clause_then_spec(mut cur: Cursor<'_>, local: bool) -> Option<(Cursor<'_>, RlImportDecl)> {
    let mut namespace: Option<String> = None;
    let mut named: Option<Vec<(String, Option<String>)>> = None;
    loop {
        let t = cur.peek()?;
        match t.kind {
            TokenKind::Punct(b'{') => {
                let open = cur.idx;
                let close = cur.find_close()?;
                if local {
                    named.get_or_insert_default().extend(named_entries(cur.sub(
                        open + 1,
                        close,
                        cur.tokens[close].span.start,
                    )));
                }
                cur.idx = close + 1;
            }
            TokenKind::Punct(b'*') => {
                cur.bump();
                // `* as ns` — remember the namespace name for `import`.
                if local
                    && matches!(cur.peek(), Some(t) if matches!(t.kind, TokenKind::Ident) && cur.text(t) == "as")
                    && let Some(ns) = cur.tokens.get(cur.idx + 1)
                    && matches!(ns.kind, TokenKind::Ident)
                {
                    namespace = Some(cur.text(ns).to_string());
                }
            }
            TokenKind::Punct(b',') => {
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
                    let (spec, kind) = rl_spec_span(&cur, spec_tok.span)?;
                    cur.bump();
                    let names = match (namespace, named) {
                        _ if !local => RlImportNames::None,
                        (Some(ns), _) => RlImportNames::Namespace(ns),
                        (None, Some(entries)) => RlImportNames::Named(entries),
                        // only a default binding (rl enums are named exports)
                        (None, None) => RlImportNames::None,
                    };
                    return Some((cur, RlImportDecl { spec, kind, names }));
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

/// Best-effort parse of `{ [type] name [as alias], ... }` entries as
/// (exported name, alias) pairs. An entry that doesn't fit the shape is
/// skipped — never a parse failure.
fn named_entries(cur: Cursor) -> Vec<(String, Option<String>)> {
    let mut entries = Vec::new();
    // split the brace contents on top-level commas
    let mut entry: Vec<&str> = Vec::new();
    let mut flush = |entry: &mut Vec<&str>| {
        let words: Vec<&str> = match entry.first() {
            Some(&"type") if entry.len() > 1 => entry[1..].to_vec(),
            _ => entry.clone(),
        };
        match words.as_slice() {
            [name] => entries.push((name.to_string(), None)),
            [name, "as", alias] => entries.push((name.to_string(), Some(alias.to_string()))),
            _ => {} // exotic entry (string name, ...) — skip
        }
        entry.clear();
    };
    let mut clean = true; // entry contained only identifiers
    for t in cur.tokens {
        match t.kind {
            TokenKind::Punct(b',') => {
                if clean {
                    flush(&mut entry);
                } else {
                    entry.clear();
                }
                clean = true;
            }
            TokenKind::Ident => entry.push(&cur.parser.src[t.span.start..t.span.end]),
            _ => clean = false,
        }
    }
    if clean {
        flush(&mut entry);
    }
    entries
}

/// `span` is a lexed string token; returns it back if its content is a
/// relative path ending in `.rl`.
fn rl_spec_span(cur: &Cursor, span: Span) -> Option<(Span, RlSpecifier)> {
    let src = cur.parser.bytes;
    let quote = src[span.start];
    // The lexer tolerates unterminated strings (stopping at a newline or
    // EOF) — require a real closing quote.
    if span.end < span.start + 2 || src[span.end - 1] != quote {
        return None;
    }
    let spec = &src[span.start + 1..span.end - 1];
    if spec == crate::stdlib::STD_SPECIFIER.as_bytes() {
        return Some((span, RlSpecifier::Std));
    }
    let relative = spec.starts_with(b"./") || spec.starts_with(b"../");
    if relative && spec.ends_with(b".rl") {
        Some((span, RlSpecifier::Relative))
    } else {
        None
    }
}
