//! Token cursor — the parser's view over the lexed token stream.
//!
//! A [`Cursor`] is a cheap `Copy` handle (token slice + position), so a
//! sub-parser takes one by value, advances its own copy, and returns it on
//! success; on failure the caller's original cursor is untouched — that is
//! the whole backtracking story, mirroring how the byte parser re-scanned
//! from a saved offset.
//!
//! Byte positions still matter: constructs and sub-programs are byte
//! ranges of the original source. `range_end` is the byte just past the
//! cursor's token range, used as the stop position for open-ended scans
//! (an arm body running to the end of the region, a type annotation
//! running to the closing paren) when the tokens run out.

use crate::ast::Span;
use crate::lexer::{Token, TokenKind};

use super::Parser;

#[derive(Clone, Copy)]
pub(super) struct Cursor<'t> {
    pub(super) parser: &'t Parser<'t>,
    pub(super) tokens: &'t [Token],
    pub(super) idx: usize,
    /// Byte offset just past this token range in the source.
    pub(super) range_end: usize,
}

impl<'t> Cursor<'t> {
    pub(super) fn new(
        parser: &'t Parser<'t>,
        tokens: &'t [Token],
        idx: usize,
        range_end: usize,
    ) -> Self {
        Cursor {
            parser,
            tokens,
            idx,
            range_end,
        }
    }

    pub(super) fn peek(&self) -> Option<&'t Token> {
        self.tokens.get(self.idx)
    }

    pub(super) fn bump(&mut self) -> Option<&'t Token> {
        let t = self.tokens.get(self.idx)?;
        self.idx += 1;
        Some(t)
    }

    pub(super) fn text(&self, t: &Token) -> &'t str {
        &self.parser.src[t.span.start..t.span.end]
    }

    pub(super) fn at_punct(&self, b: u8) -> bool {
        matches!(self.peek().map(|t| &t.kind), Some(TokenKind::Punct(x)) if *x == b)
    }

    pub(super) fn eat_punct(&mut self, b: u8) -> Option<Span> {
        if self.at_punct(b) {
            return self.bump().map(|t| t.span);
        }
        None
    }

    /// Consumes the current token if it is an identifier.
    pub(super) fn eat_ident(&mut self) -> Option<(&'t str, Span)> {
        match self.peek()?.kind {
            TokenKind::Ident => {
                let t = self.bump()?;
                Some((&self.parser.src[t.span.start..t.span.end], t.span))
            }
            _ => None,
        }
    }

    /// The token index of the closer matching the opener at `self.idx`
    /// (which must be a `( [ { <` punct). Like the byte scanner, only the
    /// matching pair is counted — and `=>` can never miscount a `< >`
    /// match because it is a fused [`TokenKind::Arrow`].
    pub(super) fn find_close(&self) -> Option<usize> {
        find_close_at(self.tokens, self.idx)
    }

    /// The byte where the token at `idx` starts, or `range_end` past the
    /// last token — the natural stop position for open-ended scans.
    pub(super) fn stop_byte_at(&self, idx: usize) -> usize {
        match self.tokens.get(idx) {
            Some(t) => t.span.start,
            None => self.range_end,
        }
    }

    /// A cursor over `tokens[from..to]` whose open-ended scans stop at
    /// byte `range_end` (typically the closing delimiter of the region).
    pub(super) fn sub(&self, from: usize, to: usize, range_end: usize) -> Cursor<'t> {
        Cursor {
            parser: self.parser,
            tokens: &self.tokens[from..to],
            idx: 0,
            range_end,
        }
    }
}

/// See [`Cursor::find_close`].
pub(super) fn find_close_at(tokens: &[Token], open_idx: usize) -> Option<usize> {
    let open = match tokens.get(open_idx)?.kind {
        TokenKind::Punct(b @ (b'(' | b'[' | b'{' | b'<')) => b,
        _ => return None,
    };
    let close = match open {
        b'{' => b'}',
        b'(' => b')',
        b'[' => b']',
        _ => b'>',
    };
    let mut depth = 0usize;
    for (k, t) in tokens.iter().enumerate().skip(open_idx) {
        match t.kind {
            TokenKind::Punct(x) if x == open => depth += 1,
            TokenKind::Punct(x) if x == close => {
                depth -= 1;
                if depth == 0 {
                    return Some(k);
                }
            }
            _ => {}
        }
    }
    None
}

/// True when the token before `k` (within a scan that started at `from`)
/// is a member-access dot — `.` or the `?.` of optional chaining — i.e.
/// the identifier at `k` is a property name, not a keyword.
pub(super) fn dotted_at(tokens: &[Token], from: usize, k: usize) -> bool {
    k > from
        && matches!(
            tokens[k - 1].kind,
            TokenKind::Punct(b'.') | TokenKind::OptChain
        )
}

/// `tokens[k]` is an undotted `match` identifier inside an expression
/// scan: if it is followed by `( ... ) { ... }`, returns the index just
/// past the closing brace so the scan can step over the whole shape (a
/// match expression carries its own top-level braces; whether it really
/// is an rl match is the recursive parse's decision).
pub(super) fn skip_match_shape(tokens: &[Token], k: usize) -> Option<usize> {
    if !matches!(tokens.get(k + 1)?.kind, TokenKind::Punct(b'(')) {
        return None;
    }
    let close_paren = find_close_at(tokens, k + 1)?;
    if !matches!(tokens.get(close_paren + 1)?.kind, TokenKind::Punct(b'{')) {
        return None;
    }
    let close_brace = find_close_at(tokens, close_paren + 1)?;
    Some(close_brace + 1)
}
