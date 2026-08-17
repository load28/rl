//! Mapping-aware output assembly for codegen.
//!
//! Emission builds a [`Rope`] — a sequence of pieces that are either glue
//! text the compiler writes ([`Piece::Lit`]) or text copied from the source
//! ([`Piece::Src`], carrying its source byte offset). Flattening the rope
//! yields the exact same string the old `String` concatenation produced,
//! plus the source↔output mappings language tooling consumes (TASK-048).
//! Byte-identical output is the invariant: a rope is just the old string,
//! remembered in pieces.

use crate::EmitMapping;

enum Piece {
    /// Compiler-written glue (IIFE scaffolding, destructurings, labels).
    Lit(String),
    /// Text copied from the source, starting at source byte offset `src`.
    Src { text: String, src: usize },
}

impl Piece {
    fn text(&self) -> &str {
        match self {
            Piece::Lit(t) => t,
            Piece::Src { text, .. } => text,
        }
    }
}

#[derive(Default)]
pub(crate) struct Rope {
    pieces: Vec<Piece>,
}

impl Rope {
    pub(crate) fn new() -> Rope {
        Rope::default()
    }

    pub(crate) fn push_lit(&mut self, text: impl Into<String>) {
        let text = text.into();
        if !text.is_empty() {
            self.pieces.push(Piece::Lit(text));
        }
    }

    pub(crate) fn push_src(&mut self, text: &str, src: usize) {
        if !text.is_empty() {
            self.pieces.push(Piece::Src {
                text: text.to_string(),
                src,
            });
        }
    }

    pub(crate) fn append(&mut self, mut other: Rope) {
        self.pieces.append(&mut other.pieces);
    }

    /// The rope's text — what flattening will produce. Used for the string
    /// inspections codegen already did (trailing line-comment checks).
    pub(crate) fn text(&self) -> String {
        self.pieces.iter().map(Piece::text).collect()
    }

    /// Trims whitespace from both ends, exactly like `str::trim` on the
    /// flattened text (Unicode whitespace included). Trimming the front of
    /// a source piece advances its source offset by the removed bytes, so
    /// mappings stay exact.
    pub(crate) fn trim(mut self) -> Rope {
        // front
        while let Some(first) = self.pieces.first_mut() {
            let text = first.text();
            let trimmed = text.trim_start();
            if trimmed.is_empty() {
                self.pieces.remove(0);
                continue;
            }
            let cut = text.len() - trimmed.len();
            if cut > 0 {
                match first {
                    Piece::Lit(t) => *t = t[cut..].to_string(),
                    Piece::Src { text, src } => {
                        *text = text[cut..].to_string();
                        *src += cut;
                    }
                }
            }
            break;
        }
        // back
        while let Some(last) = self.pieces.last_mut() {
            let text = last.text();
            let trimmed = text.trim_end();
            if trimmed.is_empty() {
                self.pieces.pop();
                continue;
            }
            let keep = trimmed.len();
            match last {
                Piece::Lit(t) => t.truncate(keep),
                Piece::Src { text, .. } => text.truncate(keep),
            }
            break;
        }
        self
    }

    /// Flattens into the output string and the source↔output mappings.
    /// Adjacent pieces that continue each other in both coordinate spaces
    /// merge into one mapping.
    pub(crate) fn flatten(self) -> (String, Vec<EmitMapping>) {
        let mut out = String::new();
        let mut mappings: Vec<EmitMapping> = Vec::new();
        for piece in self.pieces {
            match piece {
                Piece::Lit(text) => out.push_str(&text),
                Piece::Src { text, src } => {
                    let at = out.len();
                    if let Some(last) = mappings.last_mut()
                        && last.src + last.len == src
                        && last.out + last.len == at
                    {
                        last.len += text.len();
                    } else {
                        mappings.push(EmitMapping {
                            src,
                            out: at,
                            len: text.len(),
                        });
                    }
                    out.push_str(&text);
                }
            }
        }
        (out, mappings)
    }
}
