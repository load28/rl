//! Mapping-aware output assembly for codegen.
//!
//! Emission builds a [`Rope`] — a sequence of glue, source, mark, and anchor
//! pieces. Finalization assigns every text piece an exact, construct, or
//! synthetic provenance in [`TargetFile`], validates the target, then prints
//! the code and language-tooling metadata. Byte-identical output remains the
//! invariant (TASK-050, TASK-158).
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
    /// Compiler-written glue (region scaffolding, destructurings, labels).
    Lit(Cow<'a, str>),
    /// Text copied from the source, starting at source byte offset `src`.
    Src { text: &'a str, src: usize },
    /// A zero-length note about the *next* byte the rope emits: the name
    /// codegen is about to write stands for the construct at source offset
    /// `src`. Carries no text, so it changes nothing about the output.
    Mark { src: usize, kind: MarkKind },
    /// A zero-length note that everything up to the matching [`Piece::Close`]
    /// is glue one construct wrote ([`EmitAnchor`]). Nests.
    Open {
        src: usize,
        src_end: usize,
        owner_end: usize,
        kind: AnchorKind,
    },
    /// Closes the innermost open anchor.
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceOrigin {
    Construct {
        src: usize,
        src_end: usize,
        owner_end: usize,
        kind: AnchorKind,
    },
    Synthetic {
        parent: ExactOrigin,
        reason: SyntheticReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ExactOrigin {
    start: usize,
    end: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SyntheticReason {
    UnanchoredGenerated,
}

enum TargetPiece<'a> {
    Generated {
        text: Cow<'a, str>,
        origin: SourceOrigin,
    },
    Source {
        text: &'a str,
        origin: ExactOrigin,
    },
    Mark {
        src: usize,
        kind: MarkKind,
    },
    Open {
        src: usize,
        src_end: usize,
        owner_end: usize,
        kind: AnchorKind,
    },
    Close,
}

impl TargetPiece<'_> {
    fn text(&self) -> &str {
        match self {
            TargetPiece::Generated { text, .. } => text,
            TargetPiece::Source { text, .. } => text,
            TargetPiece::Mark { .. } | TargetPiece::Open { .. } | TargetPiece::Close => "",
        }
    }
}

struct TargetFile<'a> {
    pieces: Vec<TargetPiece<'a>>,
    len: usize,
    source_len: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TargetError {
    LengthMismatch { expected: usize, actual: usize },
    SourceOutOfBounds { start: usize, end: usize },
    CloseWithoutOpen,
    UnclosedAnchors { count: usize },
}

impl<'a> TargetFile<'a> {
    fn from_rope(rope: Rope<'a>, source_len: usize) -> Self {
        let mut source_hints = vec![None; rope.pieces.len()];
        let mut nearest = None;
        for (index, piece) in rope.pieces.iter().enumerate() {
            if let Piece::Src { text, src } = piece {
                nearest = Some(ExactOrigin {
                    start: *src,
                    end: src.saturating_add(text.len()),
                });
            }
            source_hints[index] = nearest;
        }
        nearest = None;
        for (index, piece) in rope.pieces.iter().enumerate().rev() {
            if let Piece::Src { text, src } = piece {
                nearest = Some(ExactOrigin {
                    start: *src,
                    end: src.saturating_add(text.len()),
                });
            }
            if source_hints[index].is_none() {
                source_hints[index] = nearest;
            }
        }
        let mut pieces = Vec::with_capacity(rope.pieces.len());
        let mut origins = Vec::new();
        for (index, piece) in rope.pieces.into_iter().enumerate() {
            match piece {
                Piece::Lit(text) => pieces.push(TargetPiece::Generated {
                    text,
                    origin: origins.last().copied().unwrap_or(SourceOrigin::Synthetic {
                        parent: source_hints[index].unwrap_or(ExactOrigin {
                            start: 0,
                            end: source_len,
                        }),
                        reason: SyntheticReason::UnanchoredGenerated,
                    }),
                }),
                Piece::Src { text, src } => pieces.push(TargetPiece::Source {
                    text,
                    origin: ExactOrigin {
                        start: src,
                        end: src.saturating_add(text.len()),
                    },
                }),
                Piece::Mark { src, kind } => pieces.push(TargetPiece::Mark { src, kind }),
                Piece::Open {
                    src,
                    src_end,
                    owner_end,
                    kind,
                } => {
                    origins.push(SourceOrigin::Construct {
                        src,
                        src_end,
                        owner_end,
                        kind,
                    });
                    pieces.push(TargetPiece::Open {
                        src,
                        src_end,
                        owner_end,
                        kind,
                    });
                }
                Piece::Close => {
                    origins.pop();
                    pieces.push(TargetPiece::Close);
                }
            }
        }
        Self {
            pieces,
            len: rope.len,
            source_len,
        }
    }

    fn validate(&self) -> Result<(), TargetError> {
        let actual = self.pieces.iter().map(|piece| piece.text().len()).sum();
        if actual != self.len {
            return Err(TargetError::LengthMismatch {
                expected: self.len,
                actual,
            });
        }
        let mut open = 0usize;
        for piece in &self.pieces {
            match piece {
                TargetPiece::Source {
                    origin: ExactOrigin { start, end },
                    ..
                } if start > end || *end > self.source_len => {
                    return Err(TargetError::SourceOutOfBounds {
                        start: *start,
                        end: *end,
                    });
                }
                TargetPiece::Open { .. } => open += 1,
                TargetPiece::Close if open == 0 => return Err(TargetError::CloseWithoutOpen),
                TargetPiece::Close => open -= 1,
                TargetPiece::Generated { origin, .. } => match origin {
                    SourceOrigin::Construct {
                        src,
                        src_end,
                        owner_end,
                        ..
                    } if src > src_end || src_end > owner_end || *owner_end > self.source_len => {
                        return Err(TargetError::SourceOutOfBounds {
                            start: *src,
                            end: *owner_end,
                        });
                    }
                    SourceOrigin::Synthetic { parent, .. }
                        if parent.start > parent.end || parent.end > self.source_len =>
                    {
                        return Err(TargetError::SourceOutOfBounds {
                            start: parent.start,
                            end: parent.end,
                        });
                    }
                    SourceOrigin::Construct { .. } | SourceOrigin::Synthetic { .. } => {}
                },
                TargetPiece::Source { .. } | TargetPiece::Mark { .. } => {}
            }
        }
        if open != 0 {
            return Err(TargetError::UnclosedAnchors { count: open });
        }
        Ok(())
    }

    fn print(self) -> Flat {
        let mut out = String::with_capacity(self.len);
        let mut mappings: Vec<EmitMapping> = Vec::new();
        let mut marks: Vec<ScrutineeTemp> = Vec::new();
        let mut payloads: Vec<PayloadTemp> = Vec::new();
        let mut anchors: Vec<EmitAnchor> = Vec::new();
        let mut open: Vec<(usize, usize, usize, usize, AnchorKind)> = Vec::new();
        for piece in &self.pieces {
            match piece {
                TargetPiece::Open {
                    src,
                    src_end,
                    owner_end,
                    kind,
                } => open.push((out.len(), *src, *src_end, *owner_end, *kind)),
                TargetPiece::Close => {
                    if let Some((start, src, src_end, owner_end, kind)) = open.pop() {
                        anchors.push(EmitAnchor {
                            out: start,
                            end: out.len(),
                            src,
                            src_end,
                            owner_end,
                            kind,
                        });
                    }
                }
                TargetPiece::Mark {
                    src,
                    kind: MarkKind::Scrutinee,
                } => marks.push(ScrutineeTemp {
                    src: *src,
                    out: out.len(),
                }),
                TargetPiece::Mark {
                    src,
                    kind: MarkKind::Payload,
                } => payloads.push(PayloadTemp {
                    src: *src,
                    out: out.len(),
                }),
                TargetPiece::Generated { text, .. } => out.push_str(text),
                TargetPiece::Source {
                    text,
                    origin: ExactOrigin { start, .. },
                } => {
                    let at = out.len();
                    if let Some(last) = mappings.last_mut()
                        && last.src + last.len == *start
                        && last.out + last.len == at
                    {
                        last.len += text.len();
                    } else {
                        mappings.push(EmitMapping {
                            src: *start,
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

    /// Appends `inner` as one construct's glue. `src..src_end` is its
    /// primary display range; `src..owner_end` is the complete syntax node
    /// that owns consequences of this lowering ([`crate::EmitAnchor`]).
    pub(crate) fn anchored(
        &mut self,
        kind: AnchorKind,
        src: usize,
        src_end: usize,
        owner_end: usize,
        inner: Rope<'a>,
    ) {
        self.pieces.push(Piece::Open {
            src,
            src_end,
            owner_end,
            kind,
        });
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

    pub(crate) fn ends_with_newline(&self) -> bool {
        self.pieces
            .iter()
            .rev()
            .map(Piece::text)
            .find(|text| !text.is_empty())
            .is_some_and(|text| text.ends_with('\n'))
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

    /// Builds, validates, and prints the source-preserving target.
    pub(crate) fn flatten(self, source_len: usize) -> Flat {
        let target = TargetFile::from_rope(self, source_len);
        let validation = target.validate();
        debug_assert!(
            validation.is_ok(),
            "internal compiler error: invalid TypeScript target: {validation:?}"
        );
        target.print()
    }
}

/// A flattened rope: the text, and everything language tooling reads off
/// the emission.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Flat {
    pub code: String,
    pub mappings: Vec<EmitMapping>,
    pub scrutinee_temps: Vec<ScrutineeTemp>,
    pub payload_temps: Vec<PayloadTemp>,
    pub anchors: Vec<EmitAnchor>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_assigns_exact_construct_and_file_origins() {
        let mut rope = Rope::new();
        rope.push_src("const ", 0);
        let mut generated = Rope::new();
        generated.push_lit("value");
        rope.anchored(AnchorKind::Match, 6, 11, 11, generated);
        rope.push_lit(";\n");

        let target = TargetFile::from_rope(rope, 11);
        assert_eq!(target.validate(), Ok(()));
        assert!(matches!(
            target.pieces[0],
            TargetPiece::Source {
                origin: ExactOrigin { start: 0, end: 6 },
                ..
            }
        ));
        assert!(matches!(
            target.pieces[2],
            TargetPiece::Generated {
                origin: SourceOrigin::Construct {
                    kind: AnchorKind::Match,
                    ..
                },
                ..
            }
        ));
        assert!(matches!(
            target.pieces[4],
            TargetPiece::Generated {
                origin: SourceOrigin::Synthetic {
                    reason: SyntheticReason::UnanchoredGenerated,
                    ..
                },
                ..
            }
        ));
        let flat = target.print();
        assert_eq!(flat.code, "const value;\n");
        assert_eq!(
            flat.mappings,
            [EmitMapping {
                src: 0,
                out: 0,
                len: 6
            }]
        );
    }

    #[test]
    fn target_rejects_a_source_piece_outside_the_input() {
        let mut rope = Rope::new();
        rope.push_src("x", 2);
        let target = TargetFile::from_rope(rope, 2);
        assert_eq!(
            target.validate(),
            Err(TargetError::SourceOutOfBounds { start: 2, end: 3 })
        );
    }

    #[test]
    fn target_rejects_unbalanced_anchor_structure() {
        let rope = Rope {
            pieces: vec![Piece::Close],
            len: 0,
        };
        let target = TargetFile::from_rope(rope, 0);
        assert_eq!(target.validate(), Err(TargetError::CloseWithoutOpen));
    }
}
