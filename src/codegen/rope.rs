//! Mapping-aware output assembly for codegen.
//!
//! Emission builds a [`Rope`] — a sequence of pieces that are either glue
//! text the compiler writes ([`Piece::Lit`]) or text copied from the source
//! ([`Piece::Src`], carrying its source byte offset). Flattening the rope
//! yields the exact same string the old `String` concatenation produced,
//! plus the source↔output mappings language tooling consumes (TASK-050).
//! Byte-identical output is the invariant: a rope is just the old string,
//! remembered in pieces.
//!
//! Pieces *borrow*: a source piece is a `&str` into the original source and
//! a literal piece is usually a `&'static str`, so building a rope copies
//! no text at all — the single copy happens in [`Rope::flatten`], into an
//! output buffer pre-sized from the running byte length (TASK-056).

use std::borrow::Cow;

use crate::EmitMapping;

enum Piece<'a> {
    /// Compiler-written glue (IIFE scaffolding, destructurings, labels).
    Lit(Cow<'a, str>),
    /// Text copied from the source, starting at source byte offset `src`.
    Src { text: &'a str, src: usize },
}

impl<'a> Piece<'a> {
    fn text(&self) -> &str {
        match self {
            Piece::Lit(t) => t,
            Piece::Src { text, .. } => text,
        }
    }

    /// Drops the first `cut` bytes (a char boundary) from the piece.
    fn cut_front(&mut self, cut: usize) {
        match self {
            Piece::Lit(Cow::Borrowed(t)) => *t = &t[cut..],
            Piece::Lit(Cow::Owned(t)) => drop(t.drain(..cut)),
            Piece::Src { text, src } => {
                *text = &text[cut..];
                *src += cut;
            }
        }
    }

    /// Keeps only the first `keep` bytes (a char boundary) of the piece.
    fn truncate(&mut self, keep: usize) {
        match self {
            Piece::Lit(Cow::Borrowed(t)) => *t = &t[..keep],
            Piece::Lit(Cow::Owned(t)) => t.truncate(keep),
            Piece::Src { text, .. } => *text = &text[..keep],
        }
    }
}

#[derive(Default)]
pub(crate) struct Rope<'a> {
    pieces: Vec<Piece<'a>>,
    /// Total byte length of the pieces — [`Rope::flatten`]'s exact capacity.
    len: usize,
}

impl<'a> Rope<'a> {
    pub(crate) fn new() -> Rope<'a> {
        Rope::default()
    }

    pub(crate) fn push_lit(&mut self, text: impl Into<Cow<'a, str>>) {
        let text = text.into();
        if !text.is_empty() {
            self.len += text.len();
            self.pieces.push(Piece::Lit(text));
        }
    }

    pub(crate) fn push_src(&mut self, text: &'a str, src: usize) {
        if !text.is_empty() {
            self.len += text.len();
            self.pieces.push(Piece::Src { text, src });
        }
    }

    pub(crate) fn append(&mut self, mut other: Rope<'a>) {
        self.len += other.len;
        self.pieces.append(&mut other.pieces);
    }

    /// True when the rope's last line carries a `//` line comment — it would
    /// swallow whatever codegen appends on that line. Only the last line is
    /// inspected (pieces are walked back to the nearest newline), so the
    /// check costs a line, not the whole rope.
    pub(crate) fn last_line_has_line_comment(&self) -> bool {
        // `//` can straddle a piece boundary, so the last line is stitched
        // back together before the search — it is one line, not the rope.
        let mut tail: Vec<&str> = Vec::new();
        for piece in self.pieces.iter().rev() {
            let text = piece.text();
            match text.rfind('\n') {
                Some(nl) => {
                    tail.push(&text[nl + 1..]);
                    break;
                }
                None => tail.push(text),
            }
        }
        match tail.len() {
            0 => false,
            1 => tail[0].contains("//"),
            _ => {
                let line: String = tail.iter().rev().copied().collect();
                line.contains("//")
            }
        }
    }

    /// Trims whitespace from both ends, exactly like `str::trim` on the
    /// flattened text (Unicode whitespace included). Trimming the front of
    /// a source piece advances its source offset by the removed bytes, so
    /// mappings stay exact.
    pub(crate) fn trim(mut self) -> Rope<'a> {
        // front
        while let Some(first) = self.pieces.first_mut() {
            let text = first.text();
            let trimmed = text.trim_start();
            if trimmed.is_empty() {
                self.len -= text.len();
                self.pieces.remove(0);
                continue;
            }
            let cut = text.len() - trimmed.len();
            if cut > 0 {
                self.len -= cut;
                first.cut_front(cut);
            }
            break;
        }
        // back
        while let Some(last) = self.pieces.last_mut() {
            let text = last.text();
            let trimmed = text.trim_end();
            if trimmed.is_empty() {
                self.len -= text.len();
                self.pieces.pop();
                continue;
            }
            self.len -= text.len() - trimmed.len();
            let keep = trimmed.len();
            last.truncate(keep);
            break;
        }
        self
    }

    /// Flattens into the output string and the source↔output mappings.
    /// Adjacent pieces that continue each other in both coordinate spaces
    /// merge into one mapping.
    pub(crate) fn flatten(self) -> (String, Vec<EmitMapping>) {
        let mut out = String::with_capacity(self.len);
        let mut mappings: Vec<EmitMapping> = Vec::new();
        for piece in &self.pieces {
            match piece {
                Piece::Lit(text) => out.push_str(text),
                Piece::Src { text, src } => {
                    let at = out.len();
                    if let Some(last) = mappings.last_mut()
                        && last.src + last.len == *src
                        && last.out + last.len == at
                    {
                        last.len += text.len();
                    } else {
                        mappings.push(EmitMapping {
                            src: *src,
                            out: at,
                            len: text.len(),
                        });
                    }
                    out.push_str(text);
                }
            }
        }
        (out, mappings)
    }
}
