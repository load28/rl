//! Tokenization of rl/TypeScript source — the layer between the byte
//! scanner and the parser, in the spirit of swc's lexer.
//!
//! [`lex`] turns a byte range into a stream of *significant* tokens:
//! whitespace and comments are trivia and produce no tokens (verbatim
//! emission copies original bytes, so trivia never needs representing).
//! Every token carries its absolute byte [`Span`], and the whole stream is
//! ordered and non-overlapping, so any construct the parser lifts maps
//! back to an exact byte range of the source.
//!
//! Like the previous byte scan loop, the lexer decides regex-vs-division
//! with the preceding-token heuristic, and template literals are lexed
//! hierarchically: a [`TokenKind::Template`] token carries its raw chunks
//! and the pre-lexed token stream of every `${ }` interpolation.
//!
//! The only multi-byte operators fused into single tokens are the five the
//! parser must treat as units: `=>` (never an `=` or a `< >` bracket),
//! `||` (never an or-pattern separator), `?.`/`??` (never ternary
//! openers), and `|>` (the pipeline operator — never a union `|` followed
//! by a comparison, because that byte sequence cannot occur in valid
//! TypeScript). Everything else significant is a one-byte
//! [`TokenKind::Punct`].

use crate::ast::Span;
use crate::scanner::*;

/// One significant token.
#[derive(Debug)]
pub(crate) struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

/// What a [`Token`] is. Only the distinctions the parser consumes exist;
/// everything else is a single-byte `Punct`.
#[derive(Debug)]
pub(crate) enum TokenKind {
    /// ASCII identifier or keyword (`[A-Za-z_$][A-Za-z0-9_$]*`).
    Ident,
    /// `'...'` / `"..."` string literal (possibly unterminated at a
    /// newline or EOF, exactly as the byte scanner tolerates).
    Str,
    /// A template literal with its interpolations pre-lexed.
    Template(Vec<TplPart>),
    /// A regex literal, decided by the preceding-token heuristic.
    Regex,
    /// `=>`
    Arrow,
    /// `||`
    OrOr,
    /// `?.`
    OptChain,
    /// `??`
    Coalesce,
    /// `|>`
    PipeOp,
    /// Any other significant byte.
    Punct(u8),
}

/// One piece of a [`TokenKind::Template`], in source order. `Raw` spans
/// include the surrounding backticks and the literal text (matching
/// [`crate::ast::TemplateChunk::Raw`]); an interpolation's span excludes
/// its `${` and `}` delimiters.
#[derive(Debug)]
pub(crate) enum TplPart {
    Raw(Span),
    Interp { span: Span, tokens: Vec<Token> },
}

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

fn regex_allowed(prev_sig: u8, prev_word: &str) -> bool {
    if !prev_word.is_empty() {
        return REGEX_PRECEDING_WORDS.contains(&prev_word);
    }
    if prev_sig == 0 {
        return true;
    }
    b"(,=:[!&|?{};~+-*%^<>".contains(&prev_sig)
}

/// Lexes `src[start..end]` into significant tokens.
pub(crate) fn lex(src_str: &str, start: usize, end: usize) -> Vec<Token> {
    let src = src_str.as_bytes();
    let mut tokens: Vec<Token> = Vec::new();
    let mut i = start;
    // Regex-heuristic state, same rules as the previous scan loop: the last
    // identifier scanned, or the last significant byte otherwise.
    let mut prev_word: &str = "";
    let mut prev_sig: u8 = 0;

    let span = |start: usize, end: usize| Span { start, end };

    while i < end {
        let c = src[i];

        if is_ws(c) {
            i += 1;
            continue;
        }

        // comments — trivia
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
            let e = scan_string(src, i, end);
            tokens.push(Token {
                kind: TokenKind::Str,
                span: span(i, e),
            });
            prev_sig = c;
            prev_word = "";
            i = e;
            continue;
        }

        if c == b'`' {
            let (e, parts) = lex_template(src_str, i, end);
            tokens.push(Token {
                kind: TokenKind::Template(parts),
                span: span(i, e),
            });
            prev_sig = b'`';
            prev_word = "";
            i = e;
            continue;
        }

        if c == b'/'
            && regex_allowed(prev_sig, prev_word)
            && let Some(e) = scan_regex(src, i, end)
        {
            tokens.push(Token {
                kind: TokenKind::Regex,
                span: span(i, e),
            });
            prev_sig = b'/';
            prev_word = "";
            i = e;
            continue;
        }

        if is_ident_start(c) {
            let j = ident_end(src, i, end);
            tokens.push(Token {
                kind: TokenKind::Ident,
                span: span(i, j),
            });
            prev_word = &src_str[i..j];
            prev_sig = src[j - 1];
            i = j;
            continue;
        }

        let (kind, len) = match (c, at(src, i + 1, end)) {
            (b'=', Some(b'>')) => (TokenKind::Arrow, 2),
            (b'|', Some(b'|')) => (TokenKind::OrOr, 2),
            (b'|', Some(b'>')) => (TokenKind::PipeOp, 2),
            (b'?', Some(b'.')) => (TokenKind::OptChain, 2),
            (b'?', Some(b'?')) => (TokenKind::Coalesce, 2),
            _ => (TokenKind::Punct(c), 1),
        };
        tokens.push(Token {
            kind,
            span: span(i, i + len),
        });
        prev_sig = src[i + len - 1];
        prev_word = "";
        i += len;
    }
    tokens
}

/// `src[start]` is a backtick — lexes the template into raw chunks and
/// recursively lexed `${ }` interpolations. Returns the index just past
/// the closing backtick (or `end` if unterminated).
fn lex_template(src_str: &str, start: usize, end: usize) -> (usize, Vec<TplPart>) {
    let src = src_str.as_bytes();
    let mut parts: Vec<TplPart> = Vec::new();
    let mut raw_start = start; // includes the opening backtick
    let push_raw = |parts: &mut Vec<TplPart>, start: usize, end: usize| {
        if start < end {
            parts.push(TplPart::Raw(Span { start, end }));
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
            push_raw(&mut parts, raw_start, i);
            return (i, parts);
        }
        if c == b'$' && at(src, i + 1, end) == Some(b'{') {
            push_raw(&mut parts, raw_start, i);
            let close = find_matching(src, i + 1, end).unwrap_or(end);
            parts.push(TplPart::Interp {
                span: Span {
                    start: i + 2,
                    end: close,
                },
                tokens: lex(src_str, i + 2, close),
            });
            i = (close + 1).min(end);
            raw_start = i;
            continue;
        }
        i += 1;
    }
    push_raw(&mut parts, raw_start, end);
    (end, parts)
}
