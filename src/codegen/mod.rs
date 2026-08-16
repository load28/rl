//! TypeScript emission from the AST.
//!
//! Codegen is **infallible**: by the time it runs, the parser has decided
//! construct-hood and the semantic phase has rejected every rl-level error,
//! so emission is a pure AST → text mapping. Verbatim segments are copied
//! from the original source byte for byte — that, plus the parser lifting
//! only fully-parsed rl constructs, is what upholds the passthrough contract.
//!
//! The emitted code is plain TypeScript with no type-level tricks; the code
//! shapes are normative and documented in `docs/reference/language.md`.
//!
//! Module layout: this file owns program/template emission; [`enums`] emits
//! `enum` declarations (union type + constructor object); [`matches`] emits
//! `match` expressions (switch IIFE).

mod enums;
mod matches;

use crate::ast::*;

/// Emits a whole program back to TypeScript text.
pub(crate) fn emit(program: &Program, src: &str) -> String {
    let emitter = Emitter {
        bytes: src.as_bytes(),
    };
    emitter.emit_program(program)
}

pub(super) struct Emitter<'a> {
    pub(super) bytes: &'a [u8],
}

impl Emitter<'_> {
    pub(super) fn emit_program(&self, program: &Program) -> String {
        let mut out: Vec<u8> = Vec::new();
        for segment in &program.segments {
            match segment {
                Segment::Verbatim(span) => {
                    out.extend_from_slice(&self.bytes[span.start..span.end]);
                }
                Segment::Enum(decl) => out.extend_from_slice(enums::emit_enum(decl).as_bytes()),
                Segment::Match(expr) => {
                    out.extend_from_slice(matches::emit_match(self, expr).as_bytes());
                }
                Segment::Template(template) => self.emit_template(template, &mut out),
            }
        }
        // Safe: the output is a recombination of valid UTF-8 slices of the
        // input plus ASCII text emitted by the compiler.
        String::from_utf8(out).expect("codegen output is valid UTF-8")
    }

    fn emit_template(&self, template: &Template, out: &mut Vec<u8>) {
        for chunk in &template.chunks {
            match chunk {
                TemplateChunk::Raw(span) => {
                    out.extend_from_slice(&self.bytes[span.start..span.end]);
                }
                TemplateChunk::Interp(interp) => {
                    out.extend_from_slice(b"${");
                    out.extend_from_slice(self.emit_program(interp).as_bytes());
                    out.push(b'}');
                }
            }
        }
    }
}
