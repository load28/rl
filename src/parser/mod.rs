//! Structural parsing of rl source into the AST.
//!
//! The parser is **infallible**: it never reports an error. It scans the
//! source byte by byte (skipping strings, comments, and regex literals) and
//! lifts every construct that *fully* parses as rl syntax — an `enum`
//! declaration, a `match` expression, or a `try` statement — into a typed
//! AST node; everything else, including any candidate that deviates even
//! slightly from rl syntax, is left as a verbatim byte range. This is how the "every valid TypeScript
//! file is a valid .rl file" contract is implemented: construct-hood is a
//! purely structural decision made here, and all rl-level *errors* (duplicate
//! cases, misplaced wildcard, non-exhaustive match, bad field types) are the
//! semantic phase's job ([`crate::sema`]).
//!
//! Plain TypeScript enums keep working: an `enum` declaration is treated as
//! an rl enum only when at least one case carries a payload `(...)` or the
//! declaration has generics — neither is valid TypeScript enum syntax, so no
//! valid TS enum is ever lifted.
//!
//! Nested code (match scrutinees, arm bodies, template interpolations) is
//! parsed recursively into sub-[`Program`]s with absolute byte spans, so
//! every later phase can report exact positions.
//!
//! Module layout: this file owns the main scan loop and shared token rules;
//! [`enums`] parses rl `enum` declarations; [`matches`] parses `match`
//! expressions; [`tries`] parses `try` statements; [`lets`] parses let-else
//! statements.

mod enums;
mod lets;
mod matches;
mod tries;

use crate::ast::*;
use crate::scanner::*;

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

// After one of these words, a `/` starts a regex literal, not division.
const REGEX_PRECEDING_WORDS: &[&str] = &[
    "return",
    "typeof",
    "instanceof",
    "in",
    "of",
    "new",
    "delete",
    "void",
    "throw",
    "case",
    "do",
    "else",
    "yield",
    "await",
];

pub(crate) fn is_reserved(word: &str) -> bool {
    RESERVED.contains(&word)
}

fn regex_allowed(prev_sig: u8, prev_word: &str) -> bool {
    if !prev_word.is_empty() {
        return REGEX_PRECEDING_WORDS.contains(&prev_word);
    }
    if prev_sig == 0 {
        return true;
    }
    b"(,=:[!&|?{};~+-*%^<>".contains(&prev_sig)
}

/// Parses a whole source file into a [`Program`].
pub(crate) fn parse(src: &str) -> Program {
    let parser = Parser {
        src,
        bytes: src.as_bytes(),
    };
    parser.parse_range(0, src.len())
}

/// Shared state for one parse: the source in both views. The parser holds no
/// mutable state — recursion carries explicit `(start, end)` ranges.
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
    /// Parses `bytes[start..end]` into a [`Program`] whose segments cover the
    /// range exactly, in source order.
    pub(crate) fn parse_range(&self, start: usize, end: usize) -> Program {
        let src = self.bytes;
        let mut segments: Vec<Segment> = Vec::new();
        let mut seg_start = start;
        let mut i = start;
        let mut prev_sig: u8 = 0; // last significant byte scanned
        let mut prev_word: &str = ""; // last identifier/keyword scanned

        while i < end {
            let c = src[i];

            // comments — stay verbatim
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

            // string literals — stay verbatim
            if c == b'"' || c == b'\'' {
                i = scan_string(src, i, end);
                prev_sig = c;
                prev_word = "";
                continue;
            }

            // template literals — interpolations are parsed recursively
            if c == b'`' {
                flush_verbatim(&mut segments, seg_start, i);
                let (e, template) = self.parse_template(i, end);
                segments.push(Segment::Template(template));
                seg_start = e;
                i = e;
                prev_sig = b'`';
                prev_word = "";
                continue;
            }

            // regex literals — stay verbatim (heuristic: by preceding token)
            if c == b'/'
                && regex_allowed(prev_sig, prev_word)
                && let Some(e) = scan_regex(src, i, end)
            {
                i = e;
                prev_sig = b'/';
                prev_word = "";
                continue;
            }

            if is_ident_start(c) {
                let j = ident_end(src, i, end);
                let word = &self.src[i..j];
                let dotted = prev_sig == b'.'; // property access like `str.match(...)`

                // `const enum` / `declare enum` are TypeScript-only forms — never rl.
                let ts_enum_prefix = prev_word == "const" || prev_word == "declare";
                if !dotted && !ts_enum_prefix && (word == "enum" || word == "export") {
                    let mut exported = false;
                    let mut kw_end = j;
                    if word == "export" {
                        let k = skip_ws_comments(src, j, end);
                        if k < end && is_ident_start(src[k]) {
                            let m = ident_end(src, k, end);
                            if &self.src[k..m] == "enum" {
                                exported = true;
                                kw_end = m;
                            }
                        }
                    }
                    if (word == "enum" || exported)
                        && let Some((parsed_end, decl)) =
                            enums::parse_enum(self, kw_end, end, exported)
                    {
                        flush_verbatim(&mut segments, seg_start, i);
                        segments.push(Segment::Enum(decl));
                        seg_start = parsed_end;
                        i = parsed_end;
                        prev_sig = b';';
                        prev_word = "";
                        continue;
                    }
                }

                if !dotted
                    && word == "match"
                    && let Some((parsed_end, expr)) = matches::parse_match(self, j, end)
                {
                    flush_verbatim(&mut segments, seg_start, i);
                    segments.push(Segment::Match(expr));
                    seg_start = parsed_end;
                    i = parsed_end;
                    prev_sig = b')';
                    prev_word = "";
                    continue;
                }

                // `try <expr>;` — never valid TypeScript in expression
                // position (`try { ... }` blocks and member names are
                // structurally excluded by the sub-parser).
                if !dotted
                    && word == "try"
                    && let Some((parsed_end, stmt)) = tries::parse_try_stmt(self, j, end)
                {
                    flush_verbatim(&mut segments, seg_start, i);
                    segments.push(Segment::Try(stmt));
                    seg_start = parsed_end;
                    i = parsed_end;
                    prev_sig = b';';
                    prev_word = "";
                    continue;
                }

                // `const|let|var <binding> = try <expr>;` — the `= try`
                // sequence is never valid TypeScript — and
                // `const|let|var Tag(...) = <expr> else { ... };` — a
                // declaration keyword is never followed by `<ident>(` in
                // valid TypeScript.
                if !dotted && (word == "const" || word == "let" || word == "var") {
                    if let Some((parsed_end, stmt)) = tries::parse_try_decl(self, i, j, end) {
                        flush_verbatim(&mut segments, seg_start, i);
                        segments.push(Segment::Try(stmt));
                        seg_start = parsed_end;
                        i = parsed_end;
                        prev_sig = b';';
                        prev_word = "";
                        continue;
                    }
                    if let Some((parsed_end, stmt)) = lets::parse_let_else(self, i, j, end) {
                        flush_verbatim(&mut segments, seg_start, i);
                        segments.push(Segment::LetElse(stmt));
                        seg_start = parsed_end;
                        i = parsed_end;
                        prev_sig = b';';
                        prev_word = "";
                        continue;
                    }
                }

                i = j;
                prev_word = word;
                prev_sig = *word.as_bytes().last().unwrap();
                continue;
            }

            if !is_ws(c) {
                prev_sig = c;
                prev_word = "";
            }
            i += 1;
        }

        flush_verbatim(&mut segments, seg_start, end);
        Program { segments }
    }

    /// `bytes[start]` is a backtick — splits the template into raw chunks and
    /// recursively parsed `${ }` interpolations. Returns the index just past
    /// the closing backtick (or `end` if unterminated).
    fn parse_template(&self, start: usize, end: usize) -> (usize, Template) {
        let src = self.bytes;
        let mut chunks: Vec<TemplateChunk> = Vec::new();
        let mut raw_start = start; // includes the opening backtick
        let push_raw = |chunks: &mut Vec<TemplateChunk>, start: usize, end: usize| {
            if start < end {
                chunks.push(TemplateChunk::Raw(Span { start, end }));
            }
        };
        let mut i = start + 1;
        while i < end {
            let c = src[i];
            if c == b'\\' {
                i = (i + 2).min(end);
                continue;
            }
            if c == b'`' {
                i += 1;
                push_raw(&mut chunks, raw_start, i);
                return (i, Template { chunks });
            }
            if c == b'$' && at(src, i + 1, end) == Some(b'{') {
                push_raw(&mut chunks, raw_start, i);
                let close = find_matching(src, i + 1, end).unwrap_or(end);
                chunks.push(TemplateChunk::Interp(self.parse_range(i + 2, close)));
                i = (close + 1).min(end);
                raw_start = i;
                continue;
            }
            i += 1;
        }
        push_raw(&mut chunks, raw_start, end);
        (end, Template { chunks })
    }
}
