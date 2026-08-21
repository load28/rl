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

use crate::{AnchorKind, EmitAnchor, EmitMapping, PayloadTemp, ScrutineeTemp};

/// What a [`Piece::Mark`] marks — the two things codegen writes that a
/// type checker can be *asked about*, each paired with the source
/// construct it stands for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum MarkKind {
    /// A `match`'s scrutinee temporary ([`crate::ScrutineeTemp`]).
    Scrutinee,
    /// The receiver a nested pattern tests ([`crate::PayloadTemp`]).
    Payload,
}

enum Piece<'a> {
    /// Compiler-written glue (IIFE scaffolding, destructurings, labels).
    Lit(Cow<'a, str>),
    /// Text copied from the source, starting at source byte offset `src`.
    Src { text: &'a str, src: usize },
    /// A zero-length note about the *next* byte the rope emits: the name
    /// codegen is about to write stands for the construct at source offset
    /// `src`. Carries no text, so it changes nothing about the output.
    Mark { src: usize, kind: MarkKind },
    /// A zero-length note that everything up to the matching [`Piece::Close`]
    /// is glue one construct wrote ([`EmitAnchor`]). Nests.
    Open { src: usize, kind: AnchorKind },
    /// Closes the innermost open anchor.
    Close,
}

impl<'a> Piece<'a> {
    fn text(&self) -> &str {
        match self {
            Piece::Lit(t) => t,
            Piece::Src { text, .. } => text,
            Piece::Mark { .. } | Piece::Open { .. } | Piece::Close => "",
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
            Piece::Mark { .. } | Piece::Open { .. } | Piece::Close => {}
        }
    }

    /// Keeps only the first `keep` bytes (a char boundary) of the piece.
    fn truncate(&mut self, keep: usize) {
        match self {
            Piece::Lit(Cow::Borrowed(t)) => *t = &t[..keep],
            Piece::Lit(Cow::Owned(t)) => t.truncate(keep),
            Piece::Src { text, .. } => *text = &text[..keep],
            Piece::Mark { .. } | Piece::Open { .. } | Piece::Close => {}
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

    /// Notes that the next thing pushed is the name codegen writes for the
    /// construct at source offset `src`. See [`crate::ScrutineeTemp`].
    pub(crate) fn push_mark(&mut self, src: usize) {
        self.pieces.push(Piece::Mark {
            src,
            kind: MarkKind::Scrutinee,
        });
    }

    /// Notes that the next thing pushed is the receiver expression of the
    /// nested pattern whose tag starts at `src` — the one place a checker
    /// can be asked what that payload's type admits.
    pub(crate) fn push_payload_mark(&mut self, src: usize) {
        self.pieces.push(Piece::Mark {
            src,
            kind: MarkKind::Payload,
        });
    }

    /// Appends `inner` as one construct's glue: everything it emits belongs
    /// to the construct at source offset `src` ([`crate::EmitAnchor`]).
    pub(crate) fn anchored(&mut self, kind: AnchorKind, src: usize, inner: Rope<'a>) {
        self.pieces.push(Piece::Open { src, kind });
        self.append(inner);
        self.pieces.push(Piece::Close);
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
    ///
    /// Marks carry no text, so they are stepped over rather than trimmed
    /// away — a mark at the edge of a trimmed rope still points at the byte
    /// that ends up there.
    pub(crate) fn trim(mut self) -> Rope<'a> {
        // front
        let mut front = 0;
        while let Some(first) = self.pieces.get_mut(front) {
            if first.text().is_empty() && !matches!(first, Piece::Lit(_) | Piece::Src { .. }) {
                front += 1;
                continue;
            }
            let text = first.text();
            let trimmed = text.trim_start();
            if trimmed.is_empty() {
                self.len -= text.len();
                self.pieces.remove(front);
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
        let mut back = self.pieces.len();
        while back > 0 {
            let last = &mut self.pieces[back - 1];
            if last.text().is_empty() && !matches!(last, Piece::Lit(_) | Piece::Src { .. }) {
                back -= 1;
                continue;
            }
            let text = last.text();
            let trimmed = text.trim_end();
            if trimmed.is_empty() {
                self.len -= text.len();
                self.pieces.remove(back - 1);
                back -= 1;
                continue;
            }
            self.len -= text.len() - trimmed.len();
            let keep = trimmed.len();
            last.truncate(keep);
            break;
        }
        self
    }

    /// Flattens into the output string, the source↔output mappings, and the
    /// marks codegen left. Adjacent pieces that continue each other in both
    /// coordinate spaces merge into one mapping.
    pub(crate) fn flatten(self) -> Flat {
        let mut out = String::with_capacity(self.len);
        let mut mappings: Vec<EmitMapping> = Vec::new();
        let mut marks: Vec<ScrutineeTemp> = Vec::new();
        let mut payloads: Vec<PayloadTemp> = Vec::new();
        let mut anchors: Vec<EmitAnchor> = Vec::new();
        let mut open: Vec<(usize, usize, AnchorKind)> = Vec::new();
        for piece in &self.pieces {
            match piece {
                Piece::Open { src, kind } => open.push((out.len(), *src, *kind)),
                Piece::Close => {
                    if let Some((start, src, kind)) = open.pop() {
                        // Innermost first: a closing anchor is pushed
                        // before every anchor still open around it.
                        anchors.push(EmitAnchor {
                            out: start,
                            end: out.len(),
                            src,
                            kind,
                        });
                    }
                }
                Piece::Mark {
                    src,
                    kind: MarkKind::Scrutinee,
                } => marks.push(ScrutineeTemp {
                    src: *src,
                    out: out.len(),
                }),
                Piece::Mark {
                    src,
                    kind: MarkKind::Payload,
                } => payloads.push(PayloadTemp {
                    src: *src,
                    out: out.len(),
                }),
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
        marks.sort_by_key(|mark| mark.out);
        payloads.sort_by_key(|mark| mark.out);
        Flat {
            code: out,
            mappings,
            scrutinee_temps: marks,
            payload_temps: payloads,
            anchors,
        }
    }
}

/// A flattened rope: the text, and everything language tooling reads off
/// the emission.
pub(crate) struct Flat {
    pub code: String,
    pub mappings: Vec<EmitMapping>,
    pub scrutinee_temps: Vec<ScrutineeTemp>,
    pub payload_temps: Vec<PayloadTemp>,
    pub anchors: Vec<EmitAnchor>,
}
