//! Structural parsing of rl source into the AST.
//!
//! The parser is **infallible**: it never reports an error. The source is
//! first lexed into a significant-token stream ([`crate::lexer`]); the
//! parser walks that stream and lifts every construct that *fully* parses
//! as rl syntax — an `enum` declaration, a `match` expression, a `try` or
//! let-else statement, a relative `.rl` import specifier — into a typed
//! AST node; everything else, including any candidate that deviates even
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
//! [`imports`] lifts relative `.rl` module specifiers out of static
//! import/re-export statements.

mod cursor;
mod enums;
mod imports;
mod lets;
mod matches;
mod tries;

use crate::ast::*;
use crate::lexer::{self, Token, TokenKind, TplPart};
use cursor::Cursor;

// Words that can never be a variant tag, match pattern tag, or binding name.
// Meeting one of these while trying to parse an rl construct aborts the
// attempt, so ordinary TypeScript (e.g. a class method named `match`) is
// left untouched.
const RESERVED: &[&str] = &[
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "let",
    "new",
    "null",
    "of",
    "return",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "yield",
];

pub(crate) fn is_reserved(word: &str) -> bool {
    RESERVED.contains(&word)
}

/// Parses a whole source file into a [`Program`].
pub(crate) fn parse(src: &str) -> Program {
    let parser = Parser {
        src,
        bytes: src.as_bytes(),
    };
    let tokens = lexer::lex(src, 0, src.len());
    parser.parse_tokens(&tokens, 0, src.len())
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

impl Parser<'_> {
    /// Parses a lexed token range covering `bytes[start..end]` into a
    /// [`Program`] whose segments cover the byte range exactly, in source
    /// order. Bytes between lifted constructs — trivia included — become
    /// verbatim segments.
    pub(crate) fn parse_tokens(&self, tokens: &[Token], start: usize, end: usize) -> Program {
        let mut segments: Vec<Segment> = Vec::new();
        let mut seg_start = start;
        let mut i = 0usize;

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
                TokenKind::Ident => &self.src[tok.span.start..tok.span.end],
                _ => {
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
                    continue;
                }
            }

            // Static import / re-export of a relative `.rl` path — only
            // the specifier string is lifted; the clause before it and
            // the rest of the statement stay verbatim.
            if !dotted
                && (word == "import" || word == "export")
                && let Some((cur, spec)) =
                    imports::parse_rl_import(Cursor::new(self, tokens, i + 1, end), word)
            {
                flush_verbatim(&mut segments, seg_start, spec.start);
                segments.push(Segment::RlImport(spec));
                seg_start = spec.end;
                i = cur.idx;
                continue;
            }

            if !dotted
                && word == "match"
                && let Some((cur, byte_end, expr)) =
                    matches::parse_match(Cursor::new(self, tokens, i + 1, end), tok.span)
            {
                flush_verbatim(&mut segments, seg_start, tok.span.start);
                segments.push(Segment::Match(expr));
                seg_start = byte_end;
                i = cur.idx;
                continue;
            }

            // `try <expr>;` — never valid TypeScript in expression
            // position (`try { ... }` blocks and member names are
            // structurally excluded by the sub-parser).
            if !dotted
                && word == "try"
                && let Some((cur, byte_end, stmt)) =
                    tries::parse_try_stmt(Cursor::new(self, tokens, i + 1, end), tok.span)
            {
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
                if let Some((cur, byte_end, stmt)) =
                    tries::parse_try_decl(Cursor::new(self, tokens, i + 1, end), tok.span)
                {
                    flush_verbatim(&mut segments, seg_start, tok.span.start);
                    segments.push(Segment::Try(stmt));
                    seg_start = byte_end;
                    i = cur.idx;
                    continue;
                }
                if let Some((cur, byte_end, stmt)) =
                    lets::parse_let_else(Cursor::new(self, tokens, i + 1, end), tok.span)
                {
                    flush_verbatim(&mut segments, seg_start, tok.span.start);
                    segments.push(Segment::LetElse(stmt));
                    seg_start = byte_end;
                    i = cur.idx;
                    continue;
                }
            }

            i += 1;
        }

        flush_verbatim(&mut segments, seg_start, end);
        Program { segments }
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
