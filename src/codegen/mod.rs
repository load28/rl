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

use std::cell::Cell;

use crate::ImportRewrite;
use crate::ast::*;

/// A trailing line comment would swallow whatever codegen appends on the
/// same line — rescue it with a newline (same rule as match arm bodies).
fn guard_line_comment(text: &str) -> String {
    if text.rsplit('\n').next().unwrap_or("").contains("//") {
        format!("{text}\n")
    } else {
        text.to_string()
    }
}

/// Emits a whole program back to TypeScript text.
pub(crate) fn emit(
    program: &Program,
    src: &str,
    rewrite_imports: ImportRewrite,
    std_import: Option<&str>,
) -> String {
    let emitter = Emitter {
        bytes: src.as_bytes(),
        rewrite_imports,
        std_import,
        try_seq: Cell::new(0),
        used_pipe: Cell::new(false),
    };
    let mut code = emitter.emit_program(program);
    // The pipeline apply helper, once per file. A function declaration
    // hoists, so appending at the end keeps every original line in place
    // while top-level pipelines still evaluate correctly at module init.
    if emitter.used_pipe.get() {
        if !code.ends_with('\n') {
            code.push('\n');
        }
        code.push_str("function $rl_ap<A, B>(v: A, f: (v: A) => B): B { return f(v); }\n");
    }
    code
}

pub(super) struct Emitter<'a> {
    pub(super) bytes: &'a [u8],
    rewrite_imports: ImportRewrite,
    /// Replacement for the standard library's bare specifier, if the caller
    /// supplied one (see [`crate::Options::std_import`]).
    std_import: Option<&'a str>,
    /// File-wide counter for `try` and let-else temporaries — unlike the
    /// match IIFE's `$rl_m`, these statements emit into the enclosing
    /// scope, so every temporary needs a unique name (`$rl_t0`, `$rl_t1`, ...).
    try_seq: Cell<usize>,
    /// Set when a pipeline was emitted — the file then gets the `$rl_ap`
    /// apply helper appended once.
    used_pipe: Cell<bool>,
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
                Segment::TupleMatch(expr) => {
                    out.extend_from_slice(matches::emit_tuple_match(self, expr).as_bytes());
                }
                Segment::Try(stmt) => out.extend_from_slice(self.emit_try(stmt).as_bytes()),
                Segment::LetElse(stmt) => {
                    out.extend_from_slice(self.emit_let_else(stmt).as_bytes());
                }
                Segment::Pipe(pipe) => out.extend_from_slice(self.emit_pipe(pipe).as_bytes()),
                Segment::RlImport(decl) => self.emit_rl_import(decl.spec, decl.kind, &mut out),
                Segment::Template(template) => self.emit_template(template, &mut out),
            }
        }
        // Safe: the output is a recombination of valid UTF-8 slices of the
        // input plus ASCII text emitted by the compiler.
        String::from_utf8(out).expect("codegen output is valid UTF-8")
    }

    /// Emits a `try` statement: evaluate once, early-return the `Err` from
    /// the enclosing function, then bind the `Ok` value (declaration form).
    /// Emitted on one line so source lines keep lining up roughly.
    fn emit_try(&self, stmt: &TryStmt) -> String {
        let n = self.try_seq.get();
        self.try_seq.set(n + 1);
        let tmp = format!("$rl_t{n}");
        let expr = self.emit_program(&stmt.expr);
        let expr = expr.trim();
        let mut code = format!("const {tmp} = ({expr}); if ({tmp}.kind !== \"Ok\") return {tmp};");
        if let Some((kw, binding)) = &stmt.decl {
            code.push_str(&format!(" {kw} {binding} = {tmp}.value;"));
        }
        code
    }

    /// Emits a let-else statement: evaluate once, run the (diverging) `else`
    /// block unless the tag matches, then destructure the bindings. Like
    /// `try`, emitted on one line into the enclosing scope; the diverging
    /// `else` block is what lets tsc narrow the temporary to the matched
    /// case for the destructuring — no type-level tricks.
    fn emit_let_else(&self, stmt: &LetElseStmt) -> String {
        let n = self.try_seq.get();
        self.try_seq.set(n + 1);
        let tmp = format!("$rl_t{n}");
        let expr = self.emit_program(&stmt.expr);
        let expr = expr.trim();
        let body = self.emit_program(&stmt.else_body);
        let body = body.trim();
        // a trailing line comment would swallow the closing brace
        let nl = if body.rsplit('\n').next().unwrap_or("").contains("//") {
            "\n"
        } else {
            ""
        };
        let mut code = format!(
            "const {tmp} = ({expr}); if ({tmp}.kind !== \"{}\") {{ {body}{nl} }}",
            stmt.tag
        );
        if !stmt.bindings.is_empty() {
            let parts = stmt
                .bindings
                .iter()
                .map(|b| match &b.alias {
                    Some(alias) => format!("{}: {}", b.name, alias),
                    None => b.name.clone(),
                })
                .collect::<Vec<_>>()
                .join(", ");
            code.push_str(&format!(" {} {{ {} }} = {tmp};", stmt.kw, parts));
        }
        code
    }

    /// Emits a pipeline as nested `$rl_ap` calls (apply steps) and postfix
    /// chains (method steps). Argument evaluation order makes the chain run
    /// left to right, and the absence of an IIFE means `await` inside the
    /// head or a step works in the surrounding async context untouched.
    /// Putting each step in argument position is what gives it a contextual
    /// type, so curried combinator steps infer fully (the fp-ts `pipe`
    /// mechanism — see docs/design/pipeline-operator.md §4.1).
    fn emit_pipe(&self, pipe: &PipeExpr) -> String {
        self.used_pipe.set(true);
        let head = self.emit_program(&pipe.head);
        let mut acc = format!("({})", guard_line_comment(head.trim()));
        for step in &pipe.steps {
            let body = self.emit_program(&step.body);
            let body = guard_line_comment(body.trim());
            if step.postfix {
                acc = format!("({}){}", acc, body);
            } else {
                acc = format!("$rl_ap({}, ({}))", acc, body);
            }
        }
        acc
    }

    /// Emits a lifted `.rl` module specifier (quotes included), swapping
    /// the extension per [`ImportRewrite`]. The parser guarantees the span
    /// is a quoted string whose content ends in `.rl`, so the last four
    /// bytes are `.rl` plus the closing quote.
    fn emit_rl_import(&self, span: Span, kind: RlSpecifier, out: &mut Vec<u8>) {
        let spec = &self.bytes[span.start..span.end];

        // The standard library is not on disk until rlc writes it, so its
        // specifier is only rewritten when the caller says where it went.
        if kind == RlSpecifier::Std {
            match self.std_import {
                Some(path) => {
                    let quote = spec[0];
                    out.push(quote);
                    out.extend_from_slice(path.as_bytes());
                    out.push(quote);
                }
                None => out.extend_from_slice(spec),
            }
            return;
        }

        match self.rewrite_imports {
            ImportRewrite::Off => out.extend_from_slice(spec),
            ImportRewrite::Js => {
                out.extend_from_slice(&spec[..spec.len() - 4]);
                out.extend_from_slice(b".js");
                out.push(spec[spec.len() - 1]);
            }
            ImportRewrite::Ts => {
                out.extend_from_slice(&spec[..spec.len() - 4]);
                out.extend_from_slice(b".ts");
                out.push(spec[spec.len() - 1]);
            }
        }
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
