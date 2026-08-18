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
//! Emission assembles a [`rope::Rope`] so every source-copied chunk keeps
//! its source offset; [`emit`] discards the resulting mappings, while
//! [`emit_with_map`] returns them for language tooling (`--emit-map`).
//!
//! Module layout: this file owns program/template emission; [`enums`] emits
//! `enum` declarations (union type + constructor object); [`matches`] emits
//! `match` expressions (switch IIFE).

mod enums;
mod matches;
mod rope;

use std::cell::Cell;

use crate::EmitMapping;
use crate::ImportRewrite;
use crate::ast::*;
use rope::Rope;

/// A trailing line comment would swallow whatever codegen appends on the
/// same line — rescue it with a newline (same rule as match arm bodies).
fn guard_line_comment(rope: Rope) -> Rope {
    let mut rope = rope;
    if rope.text().rsplit('\n').next().unwrap_or("").contains("//") {
        rope.push_lit("\n");
    }
    rope
}

/// Emits a whole program back to TypeScript text.
pub(crate) fn emit(
    program: &Program,
    src: &str,
    rewrite_imports: ImportRewrite,
    std_import: Option<&str>,
) -> String {
    emit_with_map(program, src, rewrite_imports, std_import).0
}

/// [`emit`], also returning the source↔output byte mappings of every chunk
/// copied verbatim from the source (see [`crate::emit_mapped`]).
pub(crate) fn emit_with_map(
    program: &Program,
    src: &str,
    rewrite_imports: ImportRewrite,
    std_import: Option<&str>,
) -> (String, Vec<EmitMapping>) {
    let emitter = Emitter {
        src,
        bytes: src.as_bytes(),
        rewrite_imports,
        std_import,
        try_seq: Cell::new(0),
        used_pipe: Cell::new(false),
    };
    let (mut code, mappings) = emitter.emit_program(program).flatten();
    // The pipeline apply helper, once per file. A function declaration
    // hoists, so appending at the end keeps every original line in place
    // while top-level pipelines still evaluate correctly at module init.
    if emitter.used_pipe.get() {
        if !code.ends_with('\n') {
            code.push('\n');
        }
        code.push_str("function $rl_ap<A, B>(v: A, f: (v: A) => B): B { return f(v); }\n");
    }
    (code, mappings)
}

pub(super) struct Emitter<'a> {
    src: &'a str,
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
    /// The source slice of a byte range, with its offset — the unit of
    /// verbatim copying.
    fn src_slice(&self, start: usize, end: usize) -> (&str, usize) {
        (&self.src[start..end], start)
    }

    pub(super) fn emit_program(&self, program: &Program) -> Rope {
        let mut out = Rope::new();
        for segment in &program.segments {
            match segment {
                Segment::Verbatim(span) => {
                    let (text, at) = self.src_slice(span.start, span.end);
                    out.push_src(text, at);
                }
                Segment::Enum(decl) => out.push_lit(enums::emit_enum(decl)),
                Segment::Match(expr) => out.append(matches::emit_match(self, expr)),
                Segment::TupleMatch(expr) => out.append(matches::emit_tuple_match(self, expr)),
                Segment::Try(stmt) => out.append(self.emit_try(stmt)),
                Segment::LetElse(stmt) => out.append(self.emit_let_else(stmt)),
                Segment::IfLet(stmt) => out.append(self.emit_if_let(stmt)),
                Segment::Pipe(pipe) => out.append(self.emit_pipe(pipe)),
                Segment::RlImport(decl) => self.emit_rl_import(decl.spec, decl.kind, &mut out),
                Segment::Template(template) => self.emit_template(template, &mut out),
            }
        }
        out
    }

    /// Emits a `try` statement: evaluate once, early-return the `Err` from
    /// the enclosing function, then bind the `Ok` value (declaration form).
    /// Emitted on one line so source lines keep lining up roughly.
    fn emit_try(&self, stmt: &TryStmt) -> Rope {
        let n = self.try_seq.get();
        self.try_seq.set(n + 1);
        let tmp = format!("$rl_t{n}");
        let expr = self.emit_program(&stmt.expr).trim();
        let mut code = Rope::new();
        code.push_lit(format!("const {tmp} = ("));
        code.append(expr);
        code.push_lit(format!("); if ({tmp}.kind !== \"Ok\") return {tmp};"));
        if let Some((kw, binding)) = &stmt.decl {
            code.push_lit(format!(" {kw} {binding} = {tmp}.value;"));
        }
        code
    }

    /// Emits a let-else statement: evaluate once, run the (diverging) `else`
    /// block unless the tag matches, then destructure the bindings. Like
    /// `try`, emitted on one line into the enclosing scope; the diverging
    /// `else` block is what lets tsc narrow the temporary to the matched
    /// case for the destructuring — no type-level tricks.
    fn emit_let_else(&self, stmt: &LetElseStmt) -> Rope {
        let n = self.try_seq.get();
        self.try_seq.set(n + 1);
        let tmp = format!("$rl_t{n}");
        let expr = self.emit_program(&stmt.expr).trim();
        let body = self.emit_program(&stmt.else_body).trim();
        // a trailing line comment would swallow the closing brace
        let nl = if body.text().rsplit('\n').next().unwrap_or("").contains("//") {
            "\n"
        } else {
            ""
        };
        let mut code = Rope::new();
        code.push_lit(format!("const {tmp} = ("));
        code.append(expr);
        code.push_lit(format!("); if ({tmp}.kind !== \"{}\") {{ ", stmt.tag));
        code.append(body);
        code.push_lit(format!("{nl} }}"));
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
            code.push_lit(format!(" {} {{ {} }} = {tmp};", stmt.kw, parts));
        }
        code
    }

    /// Emits an `if let` statement as a self-contained block: evaluate
    /// once into a fresh temporary, test the pattern's path conditions,
    /// and destructure the bindings inside the then-branch — tsc narrows
    /// the temporary through the condition, so no type tricks. The whole
    /// thing is a block statement (no IIFE, no `return` of its own), which
    /// is why it is valid in any statement position.
    fn emit_if_let(&self, stmt: &IfLetStmt) -> Rope {
        let n = self.try_seq.get();
        self.try_seq.set(n + 1);
        let tmp = format!("$rl_t{n}");
        let expr = self.emit_program(&stmt.expr).trim();
        let (cond, binds) = matches::pattern_conds_binds(&stmt.pattern, &tmp);
        let body = guard_line_comment(self.emit_program(&stmt.body).trim());
        let mut code = Rope::new();
        code.push_lit(format!("{{ const {tmp} = ("));
        code.append(expr);
        code.push_lit(format!("); if ({cond}) {{ {binds}"));
        code.append(body);
        code.push_lit(" }");
        match &stmt.else_part {
            Some(IfLetElse::Block(block)) => {
                let block = guard_line_comment(self.emit_program(block).trim());
                code.push_lit(" else { ");
                code.append(block);
                code.push_lit(" }");
            }
            Some(IfLetElse::IfLet(inner)) => {
                // the chained statement is itself a block — `else { ... }`
                code.push_lit(" else ");
                code.append(self.emit_if_let(inner));
            }
            None => {}
        }
        code.push_lit(" }");
        code
    }

    /// Emits a pipeline as nested `$rl_ap` calls (apply steps) and postfix
    /// chains (method steps). Argument evaluation order makes the chain run
    /// left to right, and the absence of an IIFE means `await` inside the
    /// head or a step works in the surrounding async context untouched.
    /// Putting each step in argument position is what gives it a contextual
    /// type, so curried combinator steps infer fully (the fp-ts `pipe`
    /// mechanism — see docs/design/pipeline-operator.md §4.1).
    fn emit_pipe(&self, pipe: &PipeExpr) -> Rope {
        self.used_pipe.set(true);
        let head = guard_line_comment(self.emit_program(&pipe.head).trim());
        let mut acc = Rope::new();
        acc.push_lit("(");
        acc.append(head);
        acc.push_lit(")");
        for step in &pipe.steps {
            let body = guard_line_comment(self.emit_program(&step.body).trim());
            let mut next = Rope::new();
            if step.postfix {
                next.push_lit("(");
                next.append(acc);
                next.push_lit(")");
                next.append(body);
            } else {
                next.push_lit("$rl_ap(");
                next.append(acc);
                next.push_lit(", (");
                next.append(body);
                next.push_lit("))");
            }
            acc = next;
        }
        acc
    }

    /// Emits a lifted `.rl` module specifier (quotes included), swapping
    /// the extension per [`ImportRewrite`]. The parser guarantees the span
    /// is a quoted string whose content ends in `.rl`, so the last four
    /// bytes are `.rl` plus the closing quote. The untouched prefix keeps
    /// its source mapping even when the extension is rewritten.
    fn emit_rl_import(&self, span: Span, kind: RlSpecifier, out: &mut Rope) {
        let (spec, at) = self.src_slice(span.start, span.end);

        // The standard library is not on disk until rlc writes it, so its
        // specifier is only rewritten when the caller says where it went.
        if kind == RlSpecifier::Std {
            match self.std_import {
                Some(path) => {
                    let quote = &spec[..1];
                    out.push_lit(format!("{quote}{path}{quote}"));
                }
                None => out.push_src(spec, at),
            }
            return;
        }

        match self.rewrite_imports {
            ImportRewrite::Off => out.push_src(spec, at),
            ImportRewrite::Js => {
                out.push_src(&spec[..spec.len() - 4], at);
                out.push_lit(format!(".js{}", &spec[spec.len() - 1..]));
            }
            ImportRewrite::Ts => {
                out.push_src(&spec[..spec.len() - 4], at);
                out.push_lit(format!(".ts{}", &spec[spec.len() - 1..]));
            }
        }
    }

    fn emit_template(&self, template: &Template, out: &mut Rope) {
        for chunk in &template.chunks {
            match chunk {
                TemplateChunk::Raw(span) => {
                    let (text, at) = self.src_slice(span.start, span.end);
                    out.push_src(text, at);
                }
                TemplateChunk::Interp(interp) => {
                    out.push_lit("${");
                    out.append(self.emit_program(interp));
                    out.push_lit("}");
                }
            }
        }
    }
}
