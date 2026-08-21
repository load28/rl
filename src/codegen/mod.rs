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

use crate::ast::*;
use crate::scanner::contains_await;
use crate::{AnchorKind, ImportRewrite};
use rope::Rope;

/// A trailing line comment would swallow whatever codegen appends on the
/// same line — rescue it with a newline (same rule as match arm bodies).
fn guard_line_comment(rope: Rope<'_>) -> Rope<'_> {
    let mut rope = rope;
    if rope.last_line_has_line_comment() {
        rope.push_lit("\n");
    }
    rope
}

/// Emits a whole program back to TypeScript text, together with the
/// source↔output byte mappings of every chunk copied verbatim from the
/// source (see [`crate::emit_mapped`]).
pub(crate) fn emit_with_map(
    program: &Program,
    src: &str,
    rewrite_imports: ImportRewrite,
    std_import: Option<&str>,
) -> rope::Flat {
    let emitter = Emitter {
        src,
        bytes: src.as_bytes(),
        rewrite_imports,
        std_import,
        try_seq: Cell::new(0),
        used_pipe: Cell::new(false),
        used_flow: Cell::new(false),
    };
    let mut flat = emitter.emit_program(program).flatten();
    let code = &mut flat.code;
    // The pipeline apply helper, once per file. A function declaration
    // hoists, so appending at the end keeps every original line in place
    // while top-level pipelines still evaluate correctly at module init.
    if emitter.used_pipe.get() {
        if !code.ends_with('\n') {
            code.push('\n');
        }
        code.push_str("function $rl_ap<A, B>(v: A, f: (v: A) => B): B { return f(v); }\n");
    }
    // The `flow` composition helper, on the same terms. Binary and nested,
    // so composition has no arity ceiling and every step infers concretely.
    if emitter.used_flow.get() {
        if !code.ends_with('\n') {
            code.push('\n');
        }
        code.push_str(
            "function $rl_fl<A extends unknown[], B, C>(f: (...a: A) => B, g: (b: B) => C): (...a: A) => C { return (...a: A) => g(f(...a)); }\n",
        );
    }
    flat
}

pub(super) struct Emitter<'a> {
    src: &'a str,
    pub(super) bytes: &'a [u8],
    rewrite_imports: ImportRewrite,
    /// Replacement for the standard library's bare specifier, if the caller
    /// supplied one (see [`crate::Options::std_import`]).
    std_import: Option<&'a str>,
    /// File-wide counter for `try`, let-else, `if let` and `result` block
    /// temporaries — unlike the match IIFE's `$rl_m`, these emit into a
    /// surrounding scope, so every temporary needs a unique name
    /// (`$rl_t0`, `$rl_t1`, ... and `$rl_r0`, `$rl_r1`, ... for bindings).
    try_seq: Cell<usize>,
    /// Set when a pipeline was emitted — the file then gets the `$rl_ap`
    /// apply helper appended once.
    used_pipe: Cell<bool>,
    /// Set when a multi-step `flow` composition was emitted — the file then
    /// gets the `$rl_fl` composition helper appended once.
    used_flow: Cell<bool>,
}

impl<'a> Emitter<'a> {
    /// `end`, moved back past the whitespace before it — the end of the
    /// construct's own text when the range up to the next keyword is what
    /// bounds it.
    fn trimmed_end(&self, start: usize, end: usize) -> usize {
        start + self.src[start..end].trim_end().len()
    }

    /// The source slice of a byte range, with its offset — the unit of
    /// verbatim copying.
    pub(super) fn src_slice(&self, start: usize, end: usize) -> (&'a str, usize) {
        (&self.src[start..end], start)
    }

    pub(super) fn emit_program(&self, program: &Program) -> Rope<'a> {
        let mut out = Rope::new();
        for segment in &program.segments {
            match segment {
                Segment::Verbatim(span) => {
                    let (text, at) = self.src_slice(span.start, span.end);
                    out.push_src(text, at);
                }
                Segment::Enum(decl) => out.push_lit(enums::emit_enum(decl)),
                // Every construct's emission is anchored: the glue inside
                // has no source of its own, but it has an origin, and a
                // diagnostic that lands there belongs on that construct's
                // own text (`crate::EmitAnchor`).
                Segment::Match(expr) => out.anchored(
                    AnchorKind::Match,
                    expr.keyword_off,
                    // `match (scrutinee)` — the arms are the user's own
                    // code and carry mappings of their own.
                    expr.scrutinee_span.end + 1,
                    matches::emit_match(self, expr),
                ),
                Segment::TupleMatch(expr) => out.anchored(
                    AnchorKind::Match,
                    expr.keyword_off,
                    expr.scrutinees
                        .last()
                        .map_or(expr.keyword_off, |(span, _)| span.end + 1),
                    matches::emit_tuple_match(self, expr),
                ),
                Segment::Try(stmt) => out.anchored(
                    AnchorKind::Try,
                    stmt.span.start,
                    stmt.span.end,
                    self.emit_try(stmt),
                ),
                Segment::LetElse(stmt) => out.anchored(
                    AnchorKind::LetElse,
                    stmt.keyword_off,
                    // The refutable binding and what it destructures; the
                    // `else` block is ordinary code.
                    stmt.head_span.end,
                    self.emit_let_else(stmt),
                ),
                Segment::IfLet(stmt) => out.anchored(
                    AnchorKind::IfLet,
                    stmt.keyword_off,
                    stmt.head_span.end,
                    self.emit_if_let(stmt),
                ),
                Segment::Pipe(pipe) => out.anchored(
                    AnchorKind::Pipe,
                    pipe.head_span.start,
                    pipe.steps
                        .last()
                        .map_or(pipe.head_span.end, |step| step.span.end),
                    self.emit_pipe(pipe),
                ),
                Segment::ResultBlock(block) => out.append(self.emit_result_block(block)),
                // `val` is compile-time only — the keyword leaves no trace
                Segment::ValModifier(_) => {}
                Segment::RlImport(decl) => self.emit_rl_import(decl.spec, decl.kind, &mut out),
                Segment::Template(template) => self.emit_template(template, &mut out),
            }
        }
        out
    }

    /// Emits a `try` statement: evaluate once, early-return the `Err` from
    /// the enclosing function, then bind the `Ok` value (declaration form).
    /// Emitted on one line so source lines keep lining up roughly.
    fn emit_try(&self, stmt: &TryStmt) -> Rope<'a> {
        let n = self.try_seq.get();
        self.try_seq.set(n + 1);
        let tmp = format!("$rl_t{n}");
        let expr = self.emit_program(&stmt.expr).trim();
        let mut code = Rope::new();
        code.push_lit(format!("const {tmp} = ("));
        code.append(expr);
        code.push_lit(format!("); if ({tmp}.kind !== \"Ok\") return {tmp};"));
        if let Some((kw, binding_span)) = &stmt.decl {
            // The binding is copied from the source, not rebuilt, so the
            // emitted declaration carries a mapping back to the name the
            // user wrote — which is what gives the editor a type for it.
            code.push_lit(format!(" {kw} "));
            let (binding, at) = self.src_slice(binding_span.start, binding_span.end);
            code.push_src(binding, at);
            code.push_lit(format!(" = {tmp}.value;"));
        }
        code
    }

    /// Emits a let-else statement: evaluate once, run the (diverging) `else`
    /// block unless the tag matches, then destructure the bindings. Like
    /// `try`, emitted on one line into the enclosing scope; the diverging
    /// `else` block is what lets tsc narrow the temporary to the matched
    /// case for the destructuring — no type-level tricks.
    fn emit_let_else(&self, stmt: &LetElseStmt) -> Rope<'a> {
        let n = self.try_seq.get();
        self.try_seq.set(n + 1);
        let tmp = format!("$rl_t{n}");
        let expr = self.emit_program(&stmt.expr).trim();
        let body = self.emit_program(&stmt.else_body).trim();
        // a trailing line comment would swallow the closing brace
        let nl = if body.last_line_has_line_comment() {
            "\n"
        } else {
            ""
        };
        let alts = &stmt.alternatives;
        let misses = alts
            .iter()
            .map(|alt| format!("{tmp}.kind !== \"{}\"", alt.tag))
            .collect::<Vec<_>>()
            .join(" && ");
        let mut code = Rope::new();
        code.push_lit(format!("const {tmp} = ("));
        code.append(expr);
        code.push_lit(format!("); if ({misses}) {{ "));
        code.append(body);
        code.push_lit(format!("{nl} }}"));
        let bindings = alts[0].bindings.as_deref().unwrap_or_default();
        if !bindings.is_empty() {
            code.push_lit(format!(" {} {{ ", stmt.kw));
            if alts.len() == 1 {
                // Same as `try`: the names come from the source, so the
                // emitted destructuring maps back to the pattern.
                code.append(matches::binding_list(self, bindings.iter()));
            } else {
                // An or-pattern's one destructuring stands for every
                // alternative at once, so it maps to none of them — the
                // same rule as a match or-arm's.
                code.push_lit(matches::binding_list_lit(bindings));
            }
            code.push_lit(format!(" }} = {tmp};"));
        }
        code
    }

    /// Emits an `if let` statement as a self-contained block: evaluate
    /// once into a fresh temporary, test the pattern's path conditions,
    /// and destructure the bindings inside the then-branch — tsc narrows
    /// the temporary through the condition, so no type tricks. The whole
    /// thing is a block statement (no IIFE, no `return` of its own), which
    /// is why it is valid in any statement position.
    fn emit_if_let(&self, stmt: &IfLetStmt) -> Rope<'a> {
        let n = self.try_seq.get();
        self.try_seq.set(n + 1);
        let tmp = format!("$rl_t{n}");
        let expr = self.emit_program(&stmt.expr).trim();
        let (cond, binds) = if stmt.alternatives.len() == 1 {
            matches::pattern_conds_binds(self, &stmt.alternatives[0], &tmp)
        } else {
            matches::or_conds_binds(&stmt.alternatives, &tmp)
        };
        let body = guard_line_comment(self.emit_program(&stmt.body).trim());
        let mut code = Rope::new();
        code.push_lit(format!("{{ const {tmp} = ("));
        code.append(expr);
        code.push_lit("); if (");
        code.append(cond);
        code.push_lit(") { ");
        code.append(binds);
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

    /// Emits a `result { ... }` computation block as an IIFE of plain
    /// statements: every binding evaluates once, early-`return`s its `Err`
    /// out of the block, and binds the `Ok` value — exactly the `try`
    /// statement's shape, scoped to the block instead of the enclosing
    /// function. The trailing expression is returned wrapped in an `Ok`
    /// object literal.
    ///
    /// No type tricks and no helper: tsc narrows each temporary through its
    /// own `if`, so the block's type is the union of the returned `Err`
    /// members and the final `Ok` — which is how the error types of the
    /// individual steps end up unioned in the block's type. `await` inside
    /// the block makes the IIFE async, like a `match`.
    fn emit_result_block(&self, block: &ResultBlock) -> Rope<'a> {
        let is_async = contains_await(self.bytes, block.body_span.start, block.body_span.end);
        let mut code = Rope::new();
        code.push_lit(if is_async {
            "(await (async () => {"
        } else {
            "((() => {"
        });
        for item in &block.items {
            match item {
                ResultItem::Stmts(stmts) => {
                    code.append(guard_line_comment(self.emit_program(stmts)));
                }
                ResultItem::Bind(bind) => {
                    let n = self.try_seq.get();
                    self.try_seq.set(n + 1);
                    let tmp = format!("$rl_r{n}");
                    let mut one = Rope::new();
                    one.push_lit(format!("const {tmp} = ("));
                    one.append(guard_line_comment(self.emit_program(&bind.expr).trim()));
                    one.push_lit(format!("); if ({tmp}.kind !== \"Ok\") return {tmp};"));
                    // The binding is copied from the source, not rebuilt, so
                    // the emitted declaration carries a mapping back to the
                    // name the user wrote.
                    one.push_lit(format!(" {} ", bind.kw));
                    let (binding, at) =
                        self.src_slice(bind.binding_span.start, bind.binding_span.end);
                    one.push_src(binding, at);
                    one.push_lit(format!(" = {tmp}.value;"));
                    code.anchored(
                        AnchorKind::ResultBind,
                        bind.binding_span.start,
                        self.trimmed_end(bind.binding_span.start, bind.expr_span.end),
                        one,
                    );
                }
            }
        }
        code.push_lit("return { kind: \"Ok\" as const, value: (");
        code.append(guard_line_comment(self.emit_program(&block.value)));
        code.push_lit(") }; })())");
        code
    }

    /// Emits a pipeline as nested `$rl_ap` calls (apply steps) and postfix
    /// chains (method steps). Argument evaluation order makes the chain run
    /// left to right, and the absence of an IIFE means `await` inside the
    /// head or a step works in the surrounding async context untouched.
    /// Putting each step in argument position is what gives it a contextual
    /// type, so curried combinator steps infer fully (the fp-ts `pipe`
    /// mechanism — see docs/design/pipeline-operator.md §4.1).
    fn emit_pipe(&self, pipe: &PipeExpr) -> Rope<'a> {
        let Some(head) = &pipe.head else {
            return self.emit_flow(pipe);
        };
        self.used_pipe.set(true);
        let head = guard_line_comment(self.emit_program(head).trim());
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

    /// Emits a `flow` composition as nested binary `$rl_fl` calls: the
    /// first step is the composed function's input end, every later step
    /// wraps what came before. Binary nesting means no arity ceiling, and
    /// each helper call infers concretely from the previous step's return
    /// type — the composed generics tsc cannot infer are the ones a
    /// library `flow(f, g, h)` loses too, so the first step is what fixes
    /// the input type (docs/reference/language.md §7.5).
    ///
    /// A method step becomes an arrow in the helper's argument position,
    /// where the parameter type `(b: B) => C` types `$rl_v` contextually —
    /// no annotation, and no implicit `any` from rlc-emitted code. As the
    /// *first* step there would be nothing to infer it from, which sema
    /// rejects.
    fn emit_flow(&self, pipe: &PipeExpr) -> Rope<'a> {
        let mut steps = pipe.steps.iter();
        let first = steps
            .next()
            .expect("the parser never claims a pipeline without a step");
        let mut acc = Rope::new();
        acc.push_lit("(");
        acc.append(guard_line_comment(self.emit_program(&first.body).trim()));
        acc.push_lit(")");
        for step in steps {
            self.used_flow.set(true);
            let body = guard_line_comment(self.emit_program(&step.body).trim());
            let mut next = Rope::new();
            next.push_lit("$rl_fl(");
            next.append(acc);
            if step.postfix {
                next.push_lit(", (($rl_v) => ($rl_v)");
                next.append(body);
                next.push_lit("))");
            } else {
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
    fn emit_rl_import(&self, span: Span, kind: RlSpecifier, out: &mut Rope<'a>) {
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

    fn emit_template(&self, template: &Template, out: &mut Rope<'a>) {
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
