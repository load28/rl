//! Structural parsing of rl `result { ... }` computation blocks.
//!
//! ```text
//! result {
//!   const user <- getUser(id);     // Result binding
//!   const name = user.name.trim(); // ordinary TypeScript
//!   { user, name }                 // the block's value
//! }
//! ```
//!
//! Contract safety: `result` is an ordinary identifier in TypeScript, and
//! an expression statement naming it *can* be followed by a block statement
//! (`result` + newline + `{ ... }`), so the keyword alone cannot decide.
//! The **binding** decides: a block is claimed only when at least one of its
//! top-level statements is `const|let|var <binding> <- <expr>;`, and a
//! declaration keyword followed by `<-` instead of `=` is never valid
//! TypeScript (a declarator needs an initializer). Everything else — a
//! variable named `result`, `class result { ... }`, a block holding an
//! `a < -b` comparison or a `let x: Foo<-1>;` annotation — is left
//! untouched.
//!
//! That also means a block which *does* carry a binding but fails to parse
//! cannot be passed through (the output would be invalid TypeScript with no
//! position); the caller records the offset and the semantic phase reports
//! it, like a stray `|>`.
//!
//! The body is split into items at its top-level statement boundaries: a
//! `;`, or a `}` that closed a block statement rather than an expression
//! (the same rule the pipeline-head tracker uses). The run after the last
//! boundary is the block's value expression.

use super::cursor::{Cursor, dotted_at};
use crate::ast::{ResultBind, ResultBlock, ResultItem, Span};
use crate::lexer::TokenKind;

/// What a `result` + `{` candidate turned out to be.
pub(super) enum Attempt<'t> {
    /// A parsed block: the advanced cursor, the byte just past the closing
    /// brace, and the block.
    Claimed(Cursor<'t>, usize, ResultBlock),
    /// The block carries a Result binding but does not parse as a whole —
    /// recorded for the semantic phase (it cannot pass through either).
    Malformed,
    /// Not an rl construct: an ordinary `result` identifier and a block.
    Pass,
}

/// `cur` is positioned at the `{` token following an undotted `result`
/// identifier (`kw_span`, used only by the caller for error reporting).
pub(super) fn parse_result_block<'t>(mut cur: Cursor<'t>, kw_span: Span) -> Attempt<'t> {
    let open = cur.idx;
    let Some(close) = cur.find_close() else {
        return Attempt::Pass; // unbalanced braces — nothing to claim
    };
    let body_span = Span {
        start: cur.tokens[open].span.end,
        end: cur.tokens[close].span.start,
    };

    let mut items: Vec<ResultItem> = Vec::new();
    let mut saw_bind = false;
    // Where the next item starts, in bytes and in tokens.
    let mut cut = body_span.start;
    let mut tok_cut = open + 1;
    // Where the statement run currently being scanned starts.
    let mut run_start = open + 1;
    let mut depth = 0usize;

    for k in open + 1..close {
        let t = &cur.tokens[k];
        let boundary = match t.kind {
            TokenKind::Punct(b'(' | b'[' | b'{') => {
                depth += 1;
                false
            }
            TokenKind::Punct(b')' | b']') => {
                depth = depth.saturating_sub(1);
                false
            }
            TokenKind::Punct(b'}') => {
                depth = depth.saturating_sub(1);
                // a `}` that ended a block statement, not an object literal
                // or a function-expression body, ends the statement too
                depth == 0 && !cur.parser.brace_ends_expression(cur.tokens, k)
            }
            TokenKind::Punct(b';') => depth == 0,
            _ => false,
        };
        if !boundary {
            continue;
        }
        match scan_bind(&cur, run_start, k) {
            BindRun::NotBind => {} // ordinary statements — keep accumulating
            BindRun::Malformed => return Attempt::Malformed,
            BindRun::Bind {
                kw,
                binding_start,
                binding_end,
                arrow_end,
                expr_from,
            } => {
                let kw_start = cur.tokens[run_start].span.start;
                items.push(ResultItem::Stmts(cur.parser.parse_tokens(
                    &cur.tokens[tok_cut..run_start],
                    cut,
                    kw_start,
                )));
                let expr_span = Span {
                    start: arrow_end,
                    end: t.span.start,
                };
                items.push(ResultItem::Bind(ResultBind {
                    kw: kw.to_string(),
                    binding_span: Span {
                        start: binding_start,
                        end: binding_end,
                    },
                    expr: cur.parser.parse_tokens(
                        &cur.tokens[expr_from..k],
                        expr_span.start,
                        expr_span.end,
                    ),
                }));
                saw_bind = true;
                cut = t.span.end;
                tok_cut = k + 1;
            }
        }
        run_start = k + 1;
    }

    // The run after the last boundary is the block's value — a binding
    // there is one whose `;` is missing, which is rl syntax either way.
    if run_start < close && !matches!(scan_bind(&cur, run_start, close), BindRun::NotBind) {
        return Attempt::Malformed;
    }
    if !saw_bind {
        return Attempt::Pass; // an ordinary identifier and a block statement
    }
    if run_start == close {
        return Attempt::Malformed; // nothing after the last `;`
    }
    let value_start = cur.tokens[run_start].span.start;
    if let TokenKind::Ident = cur.tokens[run_start].kind
        && super::tries::STMT_ONLY_WORDS.contains(&cur.text(&cur.tokens[run_start]))
    {
        return Attempt::Malformed; // a statement, not the block's value
    }
    items.push(ResultItem::Stmts(cur.parser.parse_tokens(
        &cur.tokens[tok_cut..run_start],
        cut,
        value_start,
    )));
    let value = cur
        .parser
        .parse_tokens(&cur.tokens[run_start..close], value_start, body_span.end);

    let byte_end = cur.tokens[close].span.end;
    cur.idx = close + 1;
    Attempt::Claimed(
        cur,
        byte_end,
        ResultBlock {
            keyword_off: kw_span.start,
            body_span,
            items,
            value,
        },
    )
}

/// What one statement run of a block body is.
enum BindRun<'t> {
    /// Ordinary TypeScript — emitted as it stands.
    NotBind,
    /// Bind-shaped (`const ... <- ...`) but not parseable as one.
    Malformed,
    Bind {
        /// The declaration keyword.
        kw: &'t str,
        /// Source byte range of the text between the keyword and `<-`,
        /// trimmed of the whitespace around it.
        binding_start: usize,
        binding_end: usize,
        /// The byte just past the `<-`.
        arrow_end: usize,
        /// The token index of the expression's first token.
        expr_from: usize,
    },
}

/// Classifies the statement run `tokens[from..boundary]` (the token at
/// `boundary` is its terminator). A Result binding is a declaration keyword
/// whose first top-level operator is `<-` — written as two adjacent bytes,
/// so the valid TypeScript comparison `a < -b` (which needs whitespace only
/// by convention) is still reachable behind an initializer `=`, and a
/// declaration keyword followed by `<-` is not valid TypeScript at all.
fn scan_bind<'t>(cur: &Cursor<'t>, from: usize, boundary: usize) -> BindRun<'t> {
    let kw_tok = &cur.tokens[from];
    if !matches!(kw_tok.kind, TokenKind::Ident) {
        return BindRun::NotBind;
    }
    let kw = cur.text(kw_tok);
    if !matches!(kw, "const" | "let" | "var") {
        return BindRun::NotBind;
    }

    let mut depth = 0usize;
    let mut arrow: Option<usize> = None;
    for k in from + 1..boundary {
        match cur.tokens[k].kind {
            TokenKind::Punct(b'(' | b'[' | b'{') => depth += 1,
            TokenKind::Punct(b')' | b']' | b'}') => depth = depth.saturating_sub(1),
            // an initializer means an ordinary declaration (`const x = a < -b;`)
            TokenKind::Punct(b'=') if depth == 0 => return BindRun::NotBind,
            TokenKind::Punct(b'<') if depth == 0 => {
                let lt = cur.tokens[k].span;
                if matches!(cur.tokens.get(k + 1),
                    Some(n) if matches!(n.kind, TokenKind::Punct(b'-')) && n.span.start == lt.end)
                {
                    arrow = Some(k);
                    break;
                }
            }
            _ => {}
        }
    }
    let Some(lt) = arrow else {
        return BindRun::NotBind;
    };

    // One scan over the tail decides the two questions it answers:
    //
    // - An unmatched closing `>` at this level means the `<-` opened a
    //   generic type argument, not a binding — `let x: Foo<-1>;` is the one
    //   valid-TypeScript shape that puts `<-` after a declaration keyword,
    //   and an expression cannot leave a `>` unopened. The run passes
    //   through untouched.
    // - A statement-only keyword at this level means the scan ran past a
    //   missing `;` into the next statement — rl syntax, so an error.
    let mut depth = 0usize;
    let mut opened = false;
    let mut ran_on = false;
    for k in lt + 2..boundary {
        match cur.tokens[k].kind {
            TokenKind::Punct(b'(' | b'[' | b'{') => depth += 1,
            TokenKind::Punct(b')' | b']' | b'}') => depth = depth.saturating_sub(1),
            TokenKind::Punct(b'<') if depth == 0 => opened = true,
            TokenKind::Punct(b'>') if depth == 0 && !opened => return BindRun::NotBind,
            TokenKind::Ident
                if depth == 0
                    && !dotted_at(cur.tokens, lt + 2, k)
                    && super::tries::STMT_ONLY_WORDS.contains(&cur.text(&cur.tokens[k])) =>
            {
                ran_on = true;
            }
            _ => {}
        }
    }
    if ran_on {
        return BindRun::Malformed;
    }

    // From here the run is rl syntax: anything unexpected is an error, not
    // a passthrough.
    if !matches!(cur.tokens[boundary].kind, TokenKind::Punct(b';')) {
        return BindRun::Malformed; // a binding must end with `;`
    }
    // The span, not a copy: what is emitted for the binding is the source
    // itself, so the emitted declaration maps back to the name written here.
    let raw_start = cur.tokens[from].span.end;
    let raw = &cur.parser.src[raw_start..cur.tokens[lt].span.start];
    let binding_start = raw_start + (raw.len() - raw.trim_start().len());
    let binding = raw.trim();
    let binding_end = binding_start + binding.len();
    if binding.is_empty() {
        return BindRun::Malformed;
    }
    // A real binding starts with a name or a destructuring pattern; a
    // leading reserved word means the run is something else entirely.
    match cur.tokens[from + 1].kind {
        TokenKind::Ident => {
            if super::is_reserved(cur.text(&cur.tokens[from + 1])) {
                return BindRun::Malformed;
            }
        }
        TokenKind::Punct(b'{' | b'[') => {}
        _ => return BindRun::Malformed,
    }

    let arrow_end = cur.tokens[lt + 1].span.end;
    if cur.parser.src[arrow_end..cur.tokens[boundary].span.start]
        .trim()
        .is_empty()
    {
        return BindRun::Malformed; // no expression after `<-`
    }
    BindRun::Bind {
        kw,
        binding_start,
        binding_end,
        arrow_end,
        expr_from: lt + 2,
    }
}
