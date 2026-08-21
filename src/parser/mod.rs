//! Structural parsing of rl source into the AST.
//!
//! The parser is **infallible**: it never reports an error. The source is
//! first lexed into a significant-token stream ([`crate::lexer`]); the
//! parser walks that stream and lifts every construct that *fully* parses
//! as rl syntax — an `enum` declaration, a `match` expression, a `try` or
//! let-else statement, a `val` binding modifier, a relative `.rl` import
//! specifier — into a typed AST node; everything else, including any candidate that deviates even
//! slightly from rl syntax, is left as a verbatim byte range. This is how
//! the "every valid TypeScript file is a valid .rl file" contract is
//! implemented: construct-hood is a purely structural decision made here,
//! and all rl-level *errors* (duplicate cases, misplaced wildcard,
//! non-exhaustive match, bad field types) are the semantic phase's job
//! ([`crate::sema`]).
//!
//! Plain TypeScript enums keep working: an `enum` declaration is treated as
//! an rl enum only when at least one case carries a payload `(...)` or the
//! declaration has generics — neither is valid TypeScript enum syntax, so no
//! valid TS enum is ever lifted.
//!
//! Nested code (match scrutinees, arm bodies, template interpolations) is
//! parsed recursively from sub-slices of the same token stream, with
//! absolute byte spans, so every later phase can report exact positions.
//!
//! Module layout: this file owns the main token loop and shared token
//! rules; [`cursor`] is the token cursor sub-parsers consume; [`enums`]
//! parses rl `enum` declarations; [`matches`] parses `match` expressions;
//! [`tries`] parses `try` statements; [`lets`] parses let-else statements;
//! [`results`] parses `result { ... }` computation blocks;
//! [`imports`] lifts relative `.rl` module specifiers out of static
//! import/re-export statements. The `val` binding modifier is recognized
//! here too, through the shared structural rule in [`crate::val`].

mod cursor;
mod enums;
mod iflets;
mod imports;
mod lets;
mod literals;
mod matches;
mod pipes;
mod results;
mod tries;

use crate::ast::*;
use crate::lexer::{self, Token, TokenKind, TplPart};
use crate::val;
use cursor::Cursor;

pub(crate) use cursor::{dotted_at, find_close_at};

// Words that can never be a variant tag, match pattern tag, or binding name.
// Meeting one of these while trying to parse an rl construct aborts the
// attempt, so ordinary TypeScript (e.g. a class method named `match`) is
// left untouched.
//
// Written as a `match` rather than a slice scan: the token loop asks this of
// every identifier in the file, and rustc compiles a string `match` into a
// length switch plus a handful of comparisons, where `[&str]::contains`
// walks the whole list.
pub(crate) fn is_reserved(word: &str) -> bool {
    matches!(
        word,
        "async"
            | "await"
            | "break"
            | "case"
            | "catch"
            | "class"
            | "const"
            | "continue"
            | "debugger"
            | "default"
            | "delete"
            | "do"
            | "else"
            | "enum"
            | "export"
            | "extends"
            | "false"
            | "finally"
            | "for"
            | "function"
            | "if"
            | "import"
            | "in"
            | "instanceof"
            | "let"
            | "new"
            | "null"
            | "of"
            | "return"
            | "static"
            | "super"
            | "switch"
            | "this"
            | "throw"
            | "true"
            | "try"
            | "typeof"
            | "var"
            | "void"
            | "while"
            | "with"
            | "yield"
    )
}

// Words that terminate the expression currently being scanned — an
// undotted occurrence means whatever follows starts a new expression, so
// the pipeline-head tracker resets there. Prefix operators that *continue*
// an expression (`new`, `typeof`, `void`, `delete`, `await`) and
// expression-capable keywords (`function`, `class`) are deliberately
// absent; `in`/`of` reset for the sake of `for` heads (their rare binary
// use next to a pipeline needs parens, documented). A `match` for the same
// reason as [`is_reserved`] — every identifier in the file reaches it.
fn is_pipe_boundary_word(word: &str) -> bool {
    matches!(
        word,
        "break"
            | "case"
            | "catch"
            | "const"
            | "continue"
            | "debugger"
            | "default"
            | "do"
            | "else"
            | "export"
            | "finally"
            | "for"
            | "if"
            | "in"
            | "let"
            | "of"
            | "return"
            | "switch"
            | "throw"
            | "var"
            | "while"
            | "with"
            | "yield"
    )
}

/// Parses a whole source file into a [`Program`].
pub(crate) fn parse(src: &str) -> Program {
    lex_and_parse(src).0
}

/// [`parse`], also returning the file's token stream — the `val` analysis
/// ([`crate::val::check`]) reads the same tokens, and lexing twice is the
/// compiler's most expensive avoidable work.
pub(crate) fn lex_and_parse(src: &str) -> (Program, Vec<Token>) {
    let parser = Parser {
        src,
        bytes: src.as_bytes(),
    };
    let tokens = lexer::lex(src, 0, src.len());
    let program = parser.parse_tokens(&tokens, 0, src.len());
    (program, tokens)
}

/// Shared state for one parse: the source in both views. The parser holds no
/// mutable state — recursion carries explicit token slices and byte ranges.
pub(crate) struct Parser<'a> {
    pub src: &'a str,
    pub bytes: &'a [u8],
}

fn flush_verbatim(segments: &mut Vec<Segment>, start: usize, end: usize) {
    if start < end {
        segments.push(Segment::Verbatim(Span { start, end }));
    }
}

/// The byte where a segment starts in the source (approximate for enums —
/// the name offset — which is fine: rewinding only compares against a
/// pipeline head start, and an enum declaration cannot sit inside one).
fn segment_start(seg: &Segment) -> usize {
    match seg {
        Segment::Verbatim(span) => span.start,
        Segment::Enum(d) => d.name_off,
        Segment::Match(m) => m.keyword_off,
        Segment::TupleMatch(m) => m.keyword_off,
        Segment::Try(t) => t.keyword_off,
        Segment::LetElse(l) => l.keyword_off,
        Segment::IfLet(s) => s.keyword_off,
        Segment::RlImport(d) => d.spec.start,
        Segment::Template(t) => match t.chunks.first() {
            Some(TemplateChunk::Raw(span)) => span.start,
            Some(TemplateChunk::Interp(_)) | None => 0, // first chunk is always Raw
        },
        Segment::Pipe(p) => p.head_span.start,
        Segment::ResultBlock(b) => b.keyword_off,
        Segment::ValModifier(span) => span.start,
    }
}

/// Pops (and truncates) segments back to `boundary` so a pipeline head can
/// re-own bytes that were already lifted (a template or match inside the
/// head). Segments are contiguous, so the returned byte — the new "flushed
/// up to here" position — is the start of the last popped segment, or
/// `boundary` when a verbatim segment crossing it was truncated.
fn rewind_segments(segments: &mut Vec<Segment>, boundary: usize, seg_start: usize) -> usize {
    let mut cover = seg_start;
    while let Some(last) = segments.last_mut() {
        match last {
            Segment::Verbatim(span) => {
                if span.start >= boundary {
                    cover = span.start;
                    segments.pop();
                } else if span.end > boundary {
                    span.end = boundary;
                    cover = boundary;
                    break;
                } else {
                    break;
                }
            }
            other => {
                let s = segment_start(other);
                if s >= boundary {
                    cover = s;
                    segments.pop();
                } else {
                    break;
                }
            }
        }
    }
    cover
}

impl Parser<'_> {
    /// Parses a lexed token range covering `bytes[start..end]` into a
    /// [`Program`] whose segments cover the byte range exactly, in source
    /// order. Bytes between lifted constructs — trivia included — become
    /// verbatim segments.
    pub(crate) fn parse_tokens(&self, tokens: &[Token], start: usize, end: usize) -> Program {
        let mut segments: Vec<Segment> = Vec::new();
        let mut stray_pipes: Vec<usize> = Vec::new();
        let mut stray_if_lets: Vec<usize> = Vec::new();
        let mut stray_results: Vec<usize> = Vec::new();
        let mut result_missing_kw: Vec<usize> = Vec::new();
        let mut result_nested_binds: Vec<usize> = Vec::new();
        let mut seg_start = start;
        let mut i = 0usize;

        // Expression-start tracking for pipeline heads: the index of the
        // token starting the expression currently being scanned, plus a
        // taint flag for unparenthesized ternary punctuation (a tainted
        // head aborts the claim — the normative "parenthesize ternaries"
        // rule). Brackets save and restore the enclosing expression's
        // state, so `f(a(b) |> g)` finds `a(b)`, not `b`.
        let mut expr: (usize, bool) = (0, false);
        let mut expr_stack: Vec<(usize, bool)> = Vec::new();

        while i < tokens.len() {
            let tok = &tokens[i];
            let word = match tok.kind {
                TokenKind::Template(ref parts) => {
                    flush_verbatim(&mut segments, seg_start, tok.span.start);
                    segments.push(Segment::Template(self.build_template(parts)));
                    seg_start = tok.span.end;
                    i += 1;
                    continue;
                }
                TokenKind::PipeOp => {
                    if !expr.1
                        && expr.0 < i
                        && let Some((next_i, pipe)) = pipes::parse_pipeline(self, tokens, expr.0, i)
                    {
                        // The head may span constructs already lifted as
                        // segments (a template, a match) — rewind them and
                        // let the head's sub-program own those bytes.
                        let head_start = pipe.head_span.start;
                        let pipe_end = pipe.steps.last().map(|s| s.span.end).unwrap_or(end);
                        seg_start = rewind_segments(&mut segments, head_start, seg_start);
                        flush_verbatim(&mut segments, seg_start, head_start);
                        segments.push(Segment::Pipe(pipe));
                        seg_start = pipe_end;
                        i = next_i;
                        expr = (i, false);
                        continue;
                    }
                    stray_pipes.push(tok.span.start);
                    i += 1;
                    continue;
                }
                TokenKind::Ident => &self.src[tok.span.start..tok.span.end],
                _ => {
                    self.track_expr_boundary(tok, i, tokens, &mut expr, &mut expr_stack);
                    i += 1;
                    continue;
                }
            };

            // property access like `str.match(...)` never starts a construct
            let dotted = cursor::dotted_at(tokens, 0, i);
            let prev_word = match i.checked_sub(1).map(|p| &tokens[p]) {
                Some(t) if matches!(t.kind, TokenKind::Ident) => {
                    &self.src[t.span.start..t.span.end]
                }
                _ => "",
            };

            // `const enum` / `declare enum` are TypeScript-only forms — never rl.
            let ts_enum_prefix = prev_word == "const" || prev_word == "declare";
            if !dotted && !ts_enum_prefix && (word == "enum" || word == "export") {
                let (kw_idx, exported) = if word == "enum" {
                    (Some(i), false)
                } else {
                    match tokens.get(i + 1) {
                        Some(t)
                            if matches!(t.kind, TokenKind::Ident)
                                && &self.src[t.span.start..t.span.end] == "enum" =>
                        {
                            (Some(i + 1), true)
                        }
                        _ => (None, false),
                    }
                };
                if let Some(kw_idx) = kw_idx
                    && let Some((cur, byte_end, decl)) =
                        enums::parse_enum(Cursor::new(self, tokens, kw_idx + 1, end), exported)
                {
                    flush_verbatim(&mut segments, seg_start, tok.span.start);
                    segments.push(Segment::Enum(decl));
                    seg_start = byte_end;
                    i = cur.idx;
                    expr = (i, false);
                    continue;
                }
            }

            // Static import / re-export of a relative `.rl` path — only
            // the specifier string is lifted; the clause before it and
            // the rest of the statement stay verbatim.
            if !dotted
                && (word == "import" || word == "export")
                && let Some((cur, decl)) =
                    imports::parse_rl_import(Cursor::new(self, tokens, i + 1, end), word)
            {
                flush_verbatim(&mut segments, seg_start, decl.spec.start);
                seg_start = decl.spec.end;
                segments.push(Segment::RlImport(decl));
                i = cur.idx;
                expr = (i, false);
                continue;
            }

            if !dotted
                && word == "match"
                && let Some((cur, byte_end, parsed)) =
                    matches::parse_match(Cursor::new(self, tokens, i + 1, end), tok.span)
            {
                flush_verbatim(&mut segments, seg_start, tok.span.start);
                segments.push(match parsed {
                    matches::ParsedMatch::Single(expr) => Segment::Match(expr),
                    matches::ParsedMatch::Tuple(expr) => Segment::TupleMatch(expr),
                });
                seg_start = byte_end;
                i = cur.idx;
                continue;
            }

            // `try <expr>;` — never valid TypeScript in expression
            // position (`try { ... }` blocks and member names are
            // structurally excluded by the sub-parser).
            if !dotted
                && word == "try"
                && let Some((cur, byte_end, mut stmt)) =
                    tries::parse_try_stmt(Cursor::new(self, tokens, i + 1, end), tok.span)
            {
                stmt.in_function = crate::flow::in_function_body(self.src, tokens, i);
                flush_verbatim(&mut segments, seg_start, tok.span.start);
                segments.push(Segment::Try(stmt));
                seg_start = byte_end;
                i = cur.idx;
                continue;
            }

            // `const|let|var <binding> = try <expr>;` — the `= try`
            // sequence is never valid TypeScript — and
            // `const|let|var Tag(...) = <expr> else { ... };` — a
            // declaration keyword is never followed by `<ident>(` in
            // valid TypeScript.
            if !dotted && (word == "const" || word == "let" || word == "var") {
                if let Some((cur, byte_end, mut stmt)) =
                    tries::parse_try_decl(Cursor::new(self, tokens, i + 1, end), tok.span)
                {
                    stmt.in_function = crate::flow::in_function_body(self.src, tokens, i);
                    flush_verbatim(&mut segments, seg_start, tok.span.start);
                    segments.push(Segment::Try(stmt));
                    seg_start = byte_end;
                    i = cur.idx;
                    expr = (i, false);
                    continue;
                }
                if let Some((cur, byte_end, mut stmt)) =
                    lets::parse_let_else(Cursor::new(self, tokens, i + 1, end), tok.span)
                {
                    stmt.in_function = crate::flow::in_function_body(self.src, tokens, i);
                    flush_verbatim(&mut segments, seg_start, tok.span.start);
                    segments.push(Segment::LetElse(stmt));
                    seg_start = byte_end;
                    i = cur.idx;
                    expr = (i, false);
                    continue;
                }
            }

            // `if let ...` — an undotted `if` followed by `let` is never
            // valid TypeScript, so a candidate that fails to parse cannot
            // be passed through either; it is recorded for sema.
            if !dotted
                && word == "if"
                && matches!(tokens.get(i + 1),
                    Some(t) if matches!(t.kind, TokenKind::Ident)
                        && &self.src[t.span.start..t.span.end] == "let")
            {
                if let Some((cur, byte_end, stmt)) =
                    iflets::parse_if_let(Cursor::new(self, tokens, i + 1, end), tok.span)
                {
                    flush_verbatim(&mut segments, seg_start, tok.span.start);
                    segments.push(Segment::IfLet(stmt));
                    seg_start = byte_end;
                    i = cur.idx;
                    expr = (i, false);
                    continue;
                }
                stray_if_lets.push(tok.span.start);
            }

            // `result { ... }` — a computation block. Only a block that
            // carries a Result binding (`const x <- ...;`) is claimed: a
            // declaration keyword followed by `<-` is never valid
            // TypeScript, while `result` alone is an ordinary identifier
            // that a block statement may legally follow.
            if !dotted
                && word == "result"
                && matches!(tokens.get(i + 1), Some(t) if matches!(t.kind, TokenKind::Punct(b'{')))
            {
                let (attempt, nested) =
                    results::parse_result_block(Cursor::new(self, tokens, i + 1, end), tok.span);
                result_nested_binds.extend(nested);
                match attempt {
                    results::Attempt::Claimed(cur, byte_end, block) => {
                        flush_verbatim(&mut segments, seg_start, tok.span.start);
                        segments.push(Segment::ResultBlock(*block));
                        seg_start = byte_end;
                        i = cur.idx;
                        continue;
                    }
                    results::Attempt::Malformed => stray_results.push(tok.span.start),
                    results::Attempt::MissingKeyword(at) => result_missing_kw.push(at),
                    results::Attempt::Pass => {}
                }
            }

            // `val` — a binding modifier, dropped from the output. The
            // two accepted shapes (`val const|let|var` on one line, and
            // `val <binding>` at the start of a parameter-list entry)
            // cannot occur in valid TypeScript, so every other `val` is
            // an ordinary identifier and stays verbatim.
            if !dotted && word == "val" && val::modifier_at(self.src, tokens, i).is_some() {
                flush_verbatim(&mut segments, seg_start, tok.span.start);
                let end = val::modifier_end(self.src, tok.span.end);
                segments.push(Segment::ValModifier(Span {
                    start: tok.span.start,
                    end,
                }));
                seg_start = end;
                i += 1;
                continue;
            }

            if !dotted && is_pipe_boundary_word(word) {
                expr = (i + 1, false);
            }
            i += 1;
        }

        flush_verbatim(&mut segments, seg_start, end);
        Program {
            segments,
            stray_pipes,
            stray_if_lets,
            stray_results,
            result_missing_kw,
            result_nested_binds,
        }
    }

    /// Advances the pipeline-head tracker over one non-identifier token.
    /// Openers save the enclosing expression's state; closers restore it —
    /// a `}` only when what follows can *continue* an expression (an object
    /// literal or function-expression body), otherwise it closed a block
    /// and the next token starts fresh. `?`/`:` reset while carrying the
    /// ternary taint that makes a later claim abort.
    fn track_expr_boundary(
        &self,
        tok: &Token,
        i: usize,
        tokens: &[Token],
        expr: &mut (usize, bool),
        stack: &mut Vec<(usize, bool)>,
    ) {
        match tok.kind {
            TokenKind::Punct(b'(' | b'[' | b'{') => {
                stack.push(*expr);
                *expr = (i + 1, false);
            }
            TokenKind::Punct(b')' | b']') => {
                *expr = stack.pop().unwrap_or((i + 1, false));
            }
            TokenKind::Punct(b'}') => {
                let outer = stack.pop().unwrap_or((i + 1, false));
                *expr = if self.brace_ends_expression(tokens, i) {
                    outer
                } else {
                    (i + 1, false)
                };
            }
            TokenKind::Punct(b';' | b',') => *expr = (i + 1, false),
            TokenKind::Punct(b'=') if pipes::is_assignment_eq(self.bytes, tok.span) => {
                *expr = (i + 1, false);
            }
            TokenKind::Punct(b':') => *expr = (i + 1, expr.1),
            TokenKind::Punct(b'?') => *expr = (i + 1, true),
            TokenKind::Arrow => *expr = (i + 1, false),
            _ => {}
        }
    }

    /// True when the token after the `}` at `close_idx` continues the
    /// surrounding expression (`.m`, `)`, an operator, `|>`, ...) rather
    /// than starting a new statement or expression.
    pub(super) fn brace_ends_expression(&self, tokens: &[Token], close_idx: usize) -> bool {
        match tokens.get(close_idx + 1) {
            None => true,
            Some(t) => match &t.kind {
                TokenKind::Ident => {
                    matches!(&self.src[t.span.start..t.span.end], "instanceof" | "in")
                }
                TokenKind::Str | TokenKind::Template(_) | TokenKind::Regex => false,
                TokenKind::Punct(c) => {
                    !matches!(c, b'(' | b'[' | b'{' | b'!' | b'~') && !c.is_ascii_digit()
                }
                _ => true, // |>, =>, ||, ?., ?? all continue an expression
            },
        }
    }

    /// Turns a lexed template token into the AST template, recursively
    /// parsing each interpolation's token stream.
    fn build_template(&self, parts: &[TplPart]) -> Template {
        let chunks = parts
            .iter()
            .map(|part| match part {
                TplPart::Raw(span) => TemplateChunk::Raw(*span),
                TplPart::Interp { span, tokens } => {
                    TemplateChunk::Interp(self.parse_tokens(tokens, span.start, span.end))
                }
            })
            .collect();
        Template { chunks }
    }
}
