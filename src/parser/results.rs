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

use super::cursor::{Cursor, dotted_at, skip_braced_construct};
use crate::ast::{ResultBind, ResultBlock, ResultItem, Span};
use crate::lexer::TokenKind;

/// What a `result` + `{` candidate turned out to be.
pub(super) enum Attempt<'t> {
    /// A parsed block: the advanced cursor, the byte just past the closing
    /// brace, and the block.
    /// Boxed: every other variant is a word, and the block is the rare
    /// case (one allocation per claimed block).
    Claimed(Cursor<'t>, usize, Box<ResultBlock>),
    /// The block carries a Result binding but does not parse as a whole —
    /// recorded for the semantic phase (it cannot pass through either).
    Malformed,
    /// A binding that forgot its declaration keyword (`b <- f();`), at the
    /// byte offset of the name. Only reported where the text cannot be
    /// TypeScript — see [`no_keyword_is_certain`].
    MissingKeyword(usize),
    /// Not an rl construct: an ordinary `result` identifier and a block.
    Pass,
}

/// `cur` is positioned at the `{` token following an undotted `result`
/// identifier (`kw_span`, used only by the caller for error reporting).
/// Besides the attempt, returns the byte offsets of bind-shaped runs
/// nested below the block's top level ([`nested_binds`]) — reportable
/// whatever the attempt turned out to be, since the shape is never
/// TypeScript.
pub(super) fn parse_result_block<'t>(
    mut cur: Cursor<'t>,
    kw_span: Span,
) -> (Attempt<'t>, Vec<usize>) {
    let open = cur.idx;
    let Some(close) = cur.find_close() else {
        return (Attempt::Pass, Vec::new()); // unbalanced braces — nothing to claim
    };
    let nested = nested_binds(&cur, open, close);
    let body_span = Span {
        start: cur.tokens[open].span.end,
        end: cur.tokens[close].span.start,
    };

    let mut items: Vec<ResultItem> = Vec::new();
    let mut saw_bind = false;
    // Runs shaped like a binding whose declaration keyword is missing.
    // Whether they are an error is decided once the whole block is read.
    let mut missing: Vec<usize> = Vec::new();
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
            BindRun::NoKeyword { at } => missing.push(at),
            BindRun::Malformed => return (Attempt::Malformed, nested),
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
                    expr_span,
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
    // A keyword-less run cannot reach here (it must end with a `;`).
    if run_start < close {
        match scan_bind(&cur, run_start, close) {
            BindRun::NotBind | BindRun::NoKeyword { .. } => {}
            BindRun::Bind { .. } | BindRun::Malformed => return (Attempt::Malformed, nested),
        }
    }
    if !saw_bind {
        // An ordinary identifier and a block statement — unless the text
        // cannot be TypeScript at all, in which case a keyword-less
        // binding is the author's `result` block and saying so beats
        // letting the output fail to parse.
        return (
            match missing.first() {
                Some(&at) if no_keyword_is_certain(&cur, open) => Attempt::MissingKeyword(at),
                _ => Attempt::Pass,
            },
            nested,
        );
    }
    // The block is claimed, so the file is not TypeScript and a
    // keyword-less binding here is a mistake rlc can name.
    if let Some(&at) = missing.first() {
        return (Attempt::MissingKeyword(at), nested);
    }
    if run_start == close {
        return (Attempt::Malformed, nested); // nothing after the last `;`
    }
    let value_start = cur.tokens[run_start].span.start;
    if let TokenKind::Ident = cur.tokens[run_start].kind
        && super::tries::STMT_ONLY_WORDS.contains(&cur.text(&cur.tokens[run_start]))
    {
        return (Attempt::Malformed, nested); // a statement, not the block's value
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
    (
        Attempt::Claimed(
            cur,
            byte_end,
            Box::new(ResultBlock {
                keyword_off: kw_span.start,
                body_span,
                items,
                value,
            }),
        ),
        nested,
    )
}

/// Byte offsets of bind-shaped runs (`const|let|var … <- …;`) **below**
/// the block's top level — inside an `if` body, a loop, a function
/// written in the block. A binding compiles to an early return of the
/// block's IIFE, and only a top-level statement can promise that (a
/// nested function's `return` exits the function; and codegen only lowers
/// top-level runs) — so these are mistakes to report, not bindings. The
/// shape cannot pass through either: a declaration keyword followed by
/// `<-` is never TypeScript (the one look-alike, a generic type argument
/// `let x: Foo<-1>;`, is ruled out by [`scan_bind`]'s own tail checks).
///
/// A nested `match`/`result` region is skipped — it is re-parsed as its
/// own construct, and an inner `result` block answers for its own runs.
fn nested_binds(cur: &Cursor<'_>, open: usize, close: usize) -> Vec<usize> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut k = open + 1;
    while k < close {
        let t = &cur.tokens[k];
        match t.kind {
            TokenKind::Punct(b'(' | b'[' | b'{') => depth += 1,
            TokenKind::Punct(b')' | b']' | b'}') => depth = depth.saturating_sub(1),
            TokenKind::Ident if !dotted_at(cur.tokens, open + 1, k) => {
                let word = cur.text(t);
                if let Some(past) = skip_braced_construct(cur.tokens, word, k) {
                    // The region is balanced, so the depth is unchanged.
                    k = past.min(close);
                    continue;
                }
                if depth > 0
                    && matches!(word, "const" | "let" | "var")
                    && let Some(semi) = run_end(cur, k, close)
                    && matches!(scan_bind(cur, k, semi), BindRun::Bind { .. })
                {
                    out.push(t.span.start);
                    k = semi + 1;
                    continue;
                }
            }
            _ => {}
        }
        k += 1;
    }
    out
}

/// The token index of the `;` ending the statement run that starts at
/// `from`, staying at the run's own bracket level. `None` when the run
/// leaves its enclosing brace or reaches `close` first.
fn run_end(cur: &Cursor<'_>, from: usize, close: usize) -> Option<usize> {
    let mut depth = 0usize;
    for k in from + 1..close {
        match cur.tokens[k].kind {
            TokenKind::Punct(b'(' | b'[' | b'{') => depth += 1,
            TokenKind::Punct(b')' | b']' | b'}') => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
            }
            TokenKind::Punct(b';') if depth == 0 => return Some(k),
            _ => {}
        }
    }
    None
}

/// What one statement run of a block body is.
enum BindRun<'t> {
    /// Ordinary TypeScript — emitted as it stands.
    NotBind,
    /// Bind-shaped (`const ... <- ...`) but not parseable as one.
    Malformed,
    /// `<name> <- <expr>;` — a binding with no declaration keyword, at the
    /// byte offset of the name.
    NoKeyword { at: usize },
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
        return scan_no_keyword(cur, from, boundary);
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

/// Classifies a run that has no declaration keyword. `b <- readNum();` is
/// the mistake this names; `b < -readNum();` is a valid TypeScript
/// comparison, so the shape is kept deliberately narrow — a bare name, a
/// `<-` written as two adjacent bytes, an expression, and a `;` — and the
/// caller still decides whether the surrounding text could be TypeScript
/// at all before reporting it.
fn scan_no_keyword<'t>(cur: &Cursor<'t>, from: usize, boundary: usize) -> BindRun<'t> {
    // A name, not `obj.x` and not a reserved word: the mistake is a
    // binding, and anything richer is more likely to be a comparison.
    if super::is_reserved(cur.text(&cur.tokens[from])) {
        return BindRun::NotBind;
    }
    let (Some(lt), Some(minus)) = (cur.tokens.get(from + 1), cur.tokens.get(from + 2)) else {
        return BindRun::NotBind;
    };
    if !matches!(lt.kind, TokenKind::Punct(b'<'))
        || !matches!(minus.kind, TokenKind::Punct(b'-'))
        || minus.span.start != lt.span.end
    {
        return BindRun::NotBind;
    }
    // The run must end with a `;` — a `<-` in the block's value run is a
    // missing semicolon, which the caller reports as a malformed block.
    if from + 3 >= boundary || !matches!(cur.tokens[boundary].kind, TokenKind::Punct(b';')) {
        return BindRun::NotBind;
    }
    // The same two tails the keyword path rules out: a generic type
    // argument (`x <-1>`-shaped, leaving an unopened `>`) and a run that
    // ran past a missing `;` into the next statement.
    let mut depth = 0usize;
    let mut opened = false;
    for k in from + 3..boundary {
        match cur.tokens[k].kind {
            TokenKind::Punct(b'(' | b'[' | b'{') => depth += 1,
            TokenKind::Punct(b')' | b']' | b'}') => depth = depth.saturating_sub(1),
            TokenKind::Punct(b'<') if depth == 0 => opened = true,
            TokenKind::Punct(b'>') if depth == 0 && !opened => return BindRun::NotBind,
            TokenKind::Ident
                if depth == 0
                    && !dotted_at(cur.tokens, from + 3, k)
                    && super::tries::STMT_ONLY_WORDS.contains(&cur.text(&cur.tokens[k])) =>
            {
                return BindRun::NotBind;
            }
            _ => {}
        }
    }
    BindRun::NoKeyword {
        at: cur.tokens[from].span.start,
    }
}

/// True when `result {` — the `{` is at token index `open` — cannot be
/// TypeScript, so a keyword-less binding inside it is certainly an rl
/// `result` block the author mis-wrote.
///
/// Two conditions, and both are needed:
///
/// - The `{` is on the **same line** as `result`. With a line break
///   between them the two are an expression statement and a block
///   statement (ASI), which is ordinary TypeScript — and so is
///   `type X = result` followed by a block on the next line.
/// - `result` sits where an **expression** starts. That rules out the
///   shapes where an identifier legally precedes a same-line `{`:
///   `function f(): result { ... }` (a return type), `class result { }`,
///   `interface result { }`, and their kin.
fn no_keyword_is_certain(cur: &Cursor<'_>, open: usize) -> bool {
    if open < 2 {
        return false;
    }
    let keyword = &cur.tokens[open - 1];
    if cur.parser.src[keyword.span.end..cur.tokens[open].span.start].contains('\n') {
        return false;
    }
    let before = &cur.tokens[open - 2];
    match before.kind {
        TokenKind::Punct(b'(' | b'[' | b',') => true,
        TokenKind::Punct(b'=') => super::pipes::is_assignment_eq(cur.parser.bytes, before.span),
        TokenKind::Arrow => true,
        TokenKind::Ident => cur.text(before) == "return",
        _ => false,
    }
}
