//! Semantic checks over the AST.
//!
//! Everything the parser deliberately does not do lives here: rl-level rules
//! whose violation is a compile error rather than a passthrough. The checker
//! walks the AST depth-first and **accumulates** every violation as an
//! [`RlError`] with a byte offset and a [`DiagnosticCode`], then reports
//! them in source order — one broken construct does not stop the next
//! independent one from being checked (TASK-117; the recovery boundary is
//! per construct: a match whose names did not resolve keeps its own
//! exhaustiveness question suppressed, nobody else's).
//!
//! Error layering: every rule here is an
//! rl-level rule, reported by rlc itself with an exact position. Nothing is
//! delegated to tsc — in particular exhaustiveness, which this module
//! *reports* but no longer computes: [`crate::analysis`] owns the subject
//! table and the coverage rule, and sema turns its answer into positioned
//! errors after the walk (so a match may precede the enum it matches on).
//! One rule, one implementation — see `docs/design/match-analysis.md` §5.
//!
//! Checks performed:
//! - `enum`: no duplicate case tags; with verification enabled, every field
//!   type parses as a TypeScript type fragment (via [`crate::verify`]).
//! - `match`: the wildcard `_` arm is last; no arm repeats a tag already
//!   covered by an unguarded arm (guarded arms may share tags with each
//!   other); or-pattern alternatives all bind the same (field, name) set.
//! - `match` literal patterns: tag and literal patterns never mix in one
//!   match (their emitted discriminants differ); or-pattern alternatives
//!   are all the same kind of literal; no unguarded arm repeats a literal
//!   *value* another already covers (`200` and `0xc8` are one case).
//!   Whether a literal match is exhaustive is a question about the
//!   scrutinee's TypeScript type and is deliberately left to the
//!   `--types` pipeline ([`crate::literal_matches`]).
//! - `try`: must sit inside a function written in its parse region
//!   ([`crate::flow::in_function_body`], carried on the statement by the
//!   parser) — its emitted `return` needs a user function to exit. That
//!   rules out the module's top level, and the statement regions of rl
//!   constructs (a match arm, a `result` block, a template interpolation)
//!   *except* where the user wrote a function there: a `try` inside an
//!   arrow inside an arm is Rust's `?` inside a closure, and is fine.
//! - let-else: the same flow fact decides placement, except the module's
//!   top level is fine (the lowering emits no `return` of its own; a
//!   `throw`-diverging `else` is valid anywhere) — only rl constructs'
//!   statement regions need a user function around it. And the `else`
//!   block must diverge on every path (`return`/`throw`/`break`/
//!   `continue`; a CFG answer) — otherwise the destructuring after the
//!   block would run with the case unproven.
//! - exhaustiveness: a wildcard-free match whose arm tags all belong to an
//!   enum declared in this file, an imported declaration
//!   ([`crate::Options::extern_enums`], collected by the CLI from direct
//!   relative `.rl` imports), or a built-in enum (`Option`, `Result`; the
//!   analysis' declaration table) — must cover every case of that
//!   enum with unguarded arms (a guard may be false, so guarded arms
//!   identify the enum but cover nothing). A tuple match must cover the
//!   cartesian product of its positions. Same-name shadowing runs
//!   local > imported > built-in. Matches whose tags belong to no known
//!   enum (hand-written unions, unresolved imports) are not checked — rlc
//!   has no type information for them. The whole computation lives in
//!   [`crate::analysis`]; what is here is the reporting.

use crate::ExternEnum;
use crate::analysis::{CoveredEnum, NameKind, Origin, has_nested};
use crate::ast::*;
use crate::diagnostics::{DiagnosticCode, non_exhaustive_message};
use crate::error::RlError;
use crate::verify;

/// Checks a whole program and returns **every** rl-level violation, in
/// source order. `verify` enables swc validation of field types; `externs`
/// are enum declarations collected from imported modules
/// ([`crate::Options::extern_enums`]). With `defer_to_checker` the two
/// exhaustiveness passes are skipped, because a TypeScript backend answers
/// the question better than this file's declaration table can
/// ([`crate::Options::defer_to_checker`]); every other rl-level rule is
/// checked either way.
pub(crate) fn check_all(
    program: &Program,
    verify: bool,
    externs: &[ExternEnum],
    defer_to_checker: bool,
) -> Vec<RlError> {
    let mut checker = Checker {
        verify,
        errors: Vec::new(),
        coverage_suppressed: Vec::new(),
    };
    checker.visit_program(program, Ctx::Top, Place::Module);
    // One analysis, two reports. Resolution comes first — a pattern whose
    // names do not resolve has no exhaustiveness question worth asking, and
    // answering both at once would bury the cause under its effect. With
    // accumulation the suppression is per match ([`MatchAnalysis::
    // has_unresolved`]), not per file: match B's coverage is not match A's
    // typo's business.
    let analyses = crate::analysis::coverage_analyses(program, externs);
    report_resolution(&analyses, &mut checker.errors);
    if !defer_to_checker {
        report_coverage(&analyses, &checker.coverage_suppressed, &mut checker.errors);
    }
    // Source order, whatever order the categories ran in — the reader fixes
    // a file top to bottom. Stable, so equal positions keep report order.
    checker
        .errors
        .sort_by_key(|e| e.offset.unwrap_or(usize::MAX));
    checker.errors
}

/// Turns [`crate::analysis`]'s resolution answer into positioned rl
/// errors.
///
/// Every entry the analysis produced is an error: the *decision* whether
/// an unresolved name is reportable belongs to the analysis (which is what
/// keeps one rule in one place), and it only produces entries it can name
/// a replacement for. This function is the wording.
fn report_resolution(analyses: &crate::analysis::PatternAnalyses, errors: &mut Vec<RlError>) {
    for unresolved in &analyses.unresolved {
        let described = describe(&CoveredEnum {
            name: unresolved.enum_name.clone(),
            origin: unresolved.origin.clone(),
        });
        let (message, code) = match (&unresolved.kind, &unresolved.tag) {
            (NameKind::Field, Some(tag)) => (
                format!(
                    "{described}: case `{tag}` has no field `{}` — did you mean `{}`?",
                    unresolved.name, unresolved.suggestion
                ),
                DiagnosticCode::UnknownField,
            ),
            _ => (
                format!(
                    "{described} has no case `{}` — did you mean `{}`?",
                    unresolved.name, unresolved.suggestion
                ),
                DiagnosticCode::UnknownCase,
            ),
        };
        errors.push(RlError::span(unresolved.start, unresolved.end, message).code(code));
    }
}

struct Checker {
    verify: bool,
    /// Every violation found so far — the walk keeps going after each one.
    errors: Vec<RlError>,
    /// Keyword offsets of matches whose *structure* is broken (mixed tag
    /// and literal patterns). Their coverage answer would be an effect
    /// stacked on a cause, so [`report_coverage`] skips them — the same
    /// per-match recovery boundary resolution failures use.
    coverage_suppressed: Vec<usize>,
}

/// The (field, bound name) pairs a tag alternative destructures, sorted so
/// alternatives compare as sets. No parens and empty parens both bind nothing.
/// Nested patterns never reach this (they are rejected inside or-patterns).
fn binding_set(bindings: &Option<Vec<Binding>>) -> Vec<(&str, &str)> {
    let mut set: Vec<(&str, &str)> = bindings
        .as_deref()
        .unwrap_or_default()
        .iter()
        .filter(|b| b.nested.is_none())
        .map(|b| (b.name.as_str(), b.alias.as_deref().unwrap_or(&b.name)))
        .collect();
    set.sort_unstable();
    set
}

/// Why two or-pattern alternatives do not bind the same set — the first
/// difference, named, so the message points at the binding to fix instead
/// of restating the rule: a name only one side binds, or a name the two
/// sides bind from different fields.
fn binding_mismatch(first: &TagPattern, other: &TagPattern) -> String {
    let a = binding_set(&first.bindings);
    let b = binding_set(&other.bindings);
    let bound = |set: &[(&str, &str)], name: &str| set.iter().any(|&(_, n)| n == name);
    for &(_, name) in &a {
        if !bound(&b, name) {
            return format!(
                "`{name}` is bound in `{}(...)` but not in `{}(...)`",
                first.tag, other.tag
            );
        }
    }
    for &(_, name) in &b {
        if !bound(&a, name) {
            return format!(
                "`{name}` is bound in `{}(...)` but not in `{}(...)`",
                other.tag, first.tag
            );
        }
    }
    // Same names on both sides, so some name is bound from different
    // fields (`A(x) | B(v: x)`).
    for &(field, name) in &a {
        if let Some(&(other_field, _)) = b.iter().find(|&&(_, n)| n == name)
            && field != other_field
        {
            return format!(
                "`{name}` is bound from field `{field}` in `{}(...)` but from field `{other_field}` in `{}(...)`",
                first.tag, other.tag
            );
        }
    }
    // The caller only asks when the sets differ, but stay total.
    "the alternatives bind different sets".to_string()
}

/// Collects every variable name the alternative binds, nested patterns
/// included, in source order.
fn leaf_bindings<'a>(alt: &'a TagPattern, out: &mut Vec<&'a str>) {
    for b in alt.bindings.as_deref().unwrap_or_default() {
        match &b.nested {
            Some(inner) => leaf_bindings(inner, out),
            None => out.push(b.alias.as_deref().unwrap_or(&b.name)),
        }
    }
}

/// Where a sub-program sits, syntactically.
#[derive(Clone, Copy, PartialEq)]
enum Ctx {
    Top,
    Stmt,
    Expr,
}

/// Where a region's statements ultimately *run* — the placement fact the
/// flow-based rules combine with each statement's own
/// [`crate::flow::in_function_body`] answer. An `if let` body and a
/// let-else `else` block are **inline**: their statements run where the
/// statement itself stands, so they inherit its place (upgraded to
/// [`Place::Function`] when the statement sits inside a function written
/// in its region). A match arm body, a `result` block's statements, and
/// every expression region reset to [`Place::Iife`] — a `return` emitted
/// there exits the construct's own IIFE, never the user's function.
#[derive(Clone, Copy, PartialEq)]
enum Place {
    /// The module's top level (or an inline chain that bottoms out there).
    Module,
    /// Inside a user-written function (directly or through inline chains).
    Function,
    /// Inside an rl construct's own IIFE or an expression region.
    Iife,
}

impl Place {
    /// The place of an inline sub-region (an `if let` body, a let-else
    /// `else` block) of a statement whose own region fact is
    /// `in_function`.
    fn inline(self, in_function: bool) -> Place {
        if in_function { Place::Function } else { self }
    }
}

impl Checker {
    fn error(&mut self, error: RlError) {
        self.errors.push(error);
    }

    fn visit_program(&mut self, program: &Program, ctx: Ctx, place: Place) {
        for error in &program.malformed {
            self.error(error.clone());
        }
        // A stray `|>` or `if let` cannot be passed through: neither is
        // valid TypeScript, so the output self-check would fail without a
        // position. Report them as rl errors here instead (error-layering
        // contract) — all of them, not the first.
        for &off in &program.stray_pipes {
            self.error(
                RlError::span(
                    off,
                    off + "|>".len(),
                    "pipeline: `|>` could not be parsed here (steps must be expressions; \
                     parenthesize ternaries and arrow functions)"
                        .to_string(),
                )
                .code(DiagnosticCode::StrayPipe),
            );
        }
        for &off in &program.stray_if_lets {
            self.error(
                RlError::span(
                    off,
                    off + "if".len(),
                    "`if let` could not be parsed here (pattern parens are mandatory, and the \
                     `else` must be a block or another `if let`)"
                        .to_string(),
                )
                .code(DiagnosticCode::StrayIfLet),
            );
        }
        for &off in &program.stray_results {
            self.error(
                RlError::span(
                    off,
                    off + "result".len(),
                    "`result` block could not be parsed here (every binding is \
                     `const <binding> <- <expression>;`, and the block must end with an \
                     expression)"
                        .to_string(),
                )
                .code(DiagnosticCode::StrayResult),
            );
        }
        for &span in &program.result_missing_kw {
            self.error(
                RlError::span(
                    span.start,
                    span.end,
                    "`result` binding is missing its declaration keyword \
                     (write `const <binding> <- <expression>;`, or `let`/`var`)"
                        .to_string(),
                )
                .code(DiagnosticCode::ResultMissingKeyword),
            );
        }
        for &span in &program.result_nested_binds {
            self.error(
                RlError::span(
                    span.start,
                    span.end,
                    "`<-` binding must be a top-level statement of the `result` block — \
                     nested inside a block or a function it cannot early-return the \
                     block's `Err`. Hoist it to the top level, or use `match` on the \
                     `Result` instead"
                        .to_string(),
                )
                .code(DiagnosticCode::ResultNestedBinding),
            );
        }
        for segment in &program.segments {
            match segment {
                Segment::Verbatim(_) | Segment::RlImport(_) | Segment::ValModifier(_) => {}
                Segment::Enum(decl) => self.check_enum(decl),
                Segment::Match(expr) => self.check_match(expr),
                Segment::TupleMatch(expr) => self.check_tuple_match(expr),
                Segment::Try(stmt) => self.check_try(stmt, place),
                Segment::LetElse(stmt) => self.check_let_else(stmt, place),
                Segment::IfLet(stmt) => self.check_if_let(stmt, ctx, place),
                Segment::ResultBlock(block) => self.check_result_block(block),
                Segment::Pipe(pipe) => {
                    // A `flow` composition has no value to chain a method
                    // onto until its first function has produced one, so
                    // its first step must be an ordinary function step.
                    if pipe.head.is_none()
                        && let Some(first) = pipe.steps.first()
                        && first.postfix
                    {
                        self.error(
                            RlError::span(
                                first.span.start,
                                first.span.end,
                                "`flow`: the first step cannot be a method step — it is the \
                                 composed function's input, so it must be a function \
                                 (`flow |> ((s: string) => s.trim()) |> ...`)"
                                    .to_string(),
                            )
                            .code(DiagnosticCode::FlowFirstStepMethod),
                        );
                    }
                    // Head and steps are expressions — `try` inside them is
                    // rejected for the same reason as inside a match.
                    if let Some(head) = &pipe.head {
                        self.visit_program(head, Ctx::Expr, Place::Iife);
                    }
                    for step in &pipe.steps {
                        self.visit_program(&step.body, Ctx::Expr, Place::Iife);
                    }
                }
                Segment::Template(template) => {
                    for chunk in &template.chunks {
                        if let TemplateChunk::Interp(interp) = chunk {
                            self.visit_program(interp, Ctx::Expr, Place::Iife);
                        }
                    }
                }
            }
        }
    }

    /// `try` placement is a **flow** fact, not a nesting rule: the lowering
    /// emits a `return`, so the statement must run inside a user-written
    /// function — one written in its own region (a `try` inside an arrow
    /// in a match arm, a scrutinee, a pipeline step is fine, exactly like
    /// `?` inside a closure in Rust), or one an inline chain (an `if let`
    /// body, a let-else `else` block) bottoms out in. Without one, the
    /// `return` would exit the construct's own IIFE, or fall at the
    /// module's top level, where there is nothing to return from.
    fn check_try(&mut self, stmt: &TryStmt, place: Place) {
        if !stmt.in_function && place != Place::Function {
            let message = if place == Place::Module {
                "`try` must be inside a function — it compiles to a `return` that \
                 propagates the `Err`, and at the top level of a module there is no \
                 function to return from"
            } else {
                "`try` cannot be used here — it compiles to a `return`, which would exit \
                 this construct's own IIFE instead of the enclosing function. Extract the \
                 logic into a function (a `try` inside a function written here is fine), \
                 or use a `<-` binding in a `result` block"
            };
            self.error(
                RlError::span(stmt.span.start, stmt.span.end, message.to_string())
                    .code(DiagnosticCode::TryPlacement),
            );
        }
        self.visit_program(&stmt.expr, Ctx::Expr, Place::Iife);
    }

    /// let-else placement is the same flow fact as `try`'s, except the
    /// module's top level is fine: the lowering emits no `return` of its
    /// own (a `throw`-diverging `else` is valid anywhere), so only
    /// [`Place::Iife`] regions — where the `else`'s exits would leave the
    /// construct's IIFE — need a function written in the region.
    fn check_let_else(&mut self, stmt: &LetElseStmt, place: Place) {
        if place == Place::Iife && !stmt.in_function {
            self.error(
                RlError::span(
                    stmt.head_span.start,
                    stmt.head_span.end,
                    "let-else cannot be used here — its `else` block's exit (`return`, \
                     `break`, `continue`) would leave this construct's own IIFE instead of \
                     the enclosing function. Extract the logic into a function (a let-else \
                     inside a function written here is fine), or match the value instead"
                        .to_string(),
                )
                .code(DiagnosticCode::LetElsePlacement),
            );
        }
        if !stmt.diverges {
            self.error(
                RlError::span(
                    stmt.else_off,
                    stmt.else_off + "else".len(),
                    "let-else: every path through the `else` block must diverge — end it \
                     with `return`, `throw`, `break`, or `continue` (an `if`/`else` counts \
                     when both branches do)"
                        .to_string(),
                )
                .code(DiagnosticCode::LetElseNotDiverging),
            );
        }
        self.check_leaf_bindings(&stmt.alternatives[0]);
        self.check_alternatives(&stmt.alternatives, "let-else");
        self.visit_program(&stmt.expr, Ctx::Expr, Place::Iife);
        // The `else` block is inline: its statements run where the
        // statement stands.
        self.visit_program(&stmt.else_body, Ctx::Stmt, place.inline(stmt.in_function));
    }

    /// `if let` emits a self-contained block statement, so it needs a
    /// statement position — which an expression region provides exactly
    /// when the user wrote a function there (the same flow fact that
    /// places `try`, judged from the other side: no IIFE to escape, just
    /// a statement stream to stand in).
    fn check_if_let(&mut self, stmt: &IfLetStmt, ctx: Ctx, place: Place) {
        if ctx == Ctx::Expr && !stmt.in_function {
            self.error(
                RlError::span(
                    stmt.head_span.start,
                    stmt.head_span.end,
                    "`if let` cannot be used in expression position (a template interpolation, \
                     a scrutinee or guard, an expression arm body, a `try` expression, or a \
                     pipeline) — it compiles to a block statement. Inside a function written \
                     here it is fine"
                        .to_string(),
                )
                .code(DiagnosticCode::IfLetPlacement),
            );
        }
        self.check_leaf_bindings(&stmt.alternatives[0]);
        self.check_alternatives(&stmt.alternatives, "if let");
        self.visit_program(&stmt.expr, Ctx::Expr, Place::Iife);
        // The then/else bodies are inline: their statements run where the
        // statement stands, so a `try` inside them exits the function the
        // chain bottoms out in.
        let inline = place.inline(stmt.in_function);
        self.visit_program(&stmt.body, Ctx::Stmt, inline);
        match &stmt.else_part {
            Some(IfLetElse::Block(block)) => self.visit_program(block, Ctx::Stmt, inline),
            Some(IfLetElse::IfLet(inner)) => self.check_if_let(inner, Ctx::Stmt, inline),
            None => {}
        }
    }

    /// The rules every multi-alternative pattern shares with a match
    /// or-arm ([`Checker::check_match`] keeps its own interleaved copy —
    /// its duplicate-arm bookkeeping decides which alternatives are even
    /// compared): the alternatives share one emitted destructuring, so a
    /// nested pattern cannot ride in them and every alternative must bind
    /// the same (field, name) set. `construct` prefixes the message.
    fn check_alternatives(&mut self, alts: &[TagPattern], construct: &str) {
        if alts.len() < 2 {
            return;
        }
        if alts.iter().any(has_nested) {
            let at = alts.iter().find(|a| has_nested(a)).unwrap();
            self.error(
                RlError::span(
                    at.tag_off,
                    at.tag_off + at.tag.len(),
                    format!("{construct}: nested patterns cannot be combined with or-patterns"),
                )
                .code(DiagnosticCode::MatchNestedInOrPattern),
            );
        }
        let first_set = binding_set(&alts[0].bindings);
        for alt in &alts[1..] {
            if binding_set(&alt.bindings) != first_set {
                self.error(
                    RlError::span(
                        alt.tag_off,
                        alt.tag_off + alt.tag.len(),
                        format!(
                            "{construct}: or-pattern alternatives must bind the same names — {}",
                            binding_mismatch(&alts[0], alt)
                        ),
                    )
                    .code(DiagnosticCode::MatchOrBindingMismatch),
                );
            }
        }
    }

    /// A `result` block is an expression, so it is allowed anywhere; its
    /// body is the IIFE's statement stream ([`Place::Iife`] — a `try` or
    /// let-else there would return from the *block*, not the enclosing
    /// function), and the bindings and the trailing value are expressions.
    fn check_result_block(&mut self, block: &ResultBlock) {
        for item in &block.items {
            match item {
                ResultItem::Stmts(stmts) => self.visit_program(stmts, Ctx::Stmt, Place::Iife),
                ResultItem::Bind(bind) => self.visit_program(&bind.expr, Ctx::Expr, Place::Iife),
            }
        }
        self.visit_program(&block.value, Ctx::Expr, Place::Iife);
    }

    fn check_enum(&mut self, decl: &EnumDecl) {
        let mut seen: Vec<&str> = Vec::new();
        for case in &decl.cases {
            if seen.contains(&case.tag.as_str()) {
                self.error(
                    RlError::span(
                        case.tag_off,
                        case.tag_off + case.tag.len(),
                        format!("enum {}: duplicate case \"{}\"", decl.name, case.tag),
                    )
                    .code(DiagnosticCode::EnumDuplicateCase),
                );
                continue;
            }
            seen.push(&case.tag);
        }

        if self.verify {
            for case in &decl.cases {
                if let Some(fields) = &case.fields {
                    for field in fields {
                        if let Err(msg) = verify::check_type_fragment(&field.ty) {
                            self.error(
                                RlError::span(
                                    field.ty_off,
                                    field.ty_off + field.ty.len(),
                                    format!(
                                        "enum {}: invalid type for field `{}`: {}",
                                        decl.name, field.name, msg
                                    ),
                                )
                                .code(DiagnosticCode::EnumInvalidFieldType),
                            );
                        }
                    }
                }
            }
        }
    }

    /// Bound names must be unique within one pattern — they all land in the
    /// same scope, so a duplicate would emit two `const`s of one name.
    fn check_leaf_bindings(&mut self, alt: &TagPattern) {
        let mut leaves = Vec::new();
        leaf_bindings(alt, &mut leaves);
        for (i, name) in leaves.iter().enumerate() {
            if leaves[..i].contains(name) {
                self.error(
                    RlError::span(
                        alt.tag_off,
                        alt.tag_off + alt.tag.len(),
                        format!(
                            "match: binding `{name}` is used more than once in this pattern (rename one with `field: alias`)"
                        ),
                    )
                    .code(DiagnosticCode::PatternDuplicateBinding),
                );
            }
        }
    }

    fn check_match(&mut self, expr: &MatchExpr) {
        // Literal and tag patterns discriminate on different things
        // (`$rl_m` vs `$rl_m.kind`), so one match cannot hold both.
        let arm_kind = |arm: &Arm| match &arm.pattern {
            Pattern::Wildcard => None,
            Pattern::Tags(_) => Some("tag"),
            Pattern::Literals(_) => Some("literal"),
        };
        if let Some(first) = expr.arms.iter().find_map(arm_kind)
            && let Some(other) = expr
                .arms
                .iter()
                .find(|a| arm_kind(a).is_some_and(|k| k != first))
        {
            self.error(
                RlError::span(
                    other.pattern_span.start,
                    other.pattern_span.end,
                    format!(
                        "match: cannot mix tag patterns and literal patterns in the same match \
                         (this match starts with {first} patterns) — the two compare different \
                         things (`$rl_m.kind` vs `$rl_m`); split them into two matches"
                    ),
                )
                .code(DiagnosticCode::MatchMixedPatterns)
                .owner(expr.keyword_off, expr.body_close + 1),
            );
            // A mixed match has no one discriminant, so its coverage answer
            // is not worth asking — report the cause, not its effects.
            self.coverage_suppressed.push(expr.keyword_off);
        }

        // Tags covered by an unguarded arm. Any later arm repeating one of
        // these is unreachable (duplicate); a guarded arm covers nothing, so
        // guarded arms may repeat each other's tags.
        let mut covered: Vec<&str> = Vec::new();
        // The same, for literal patterns.
        let mut covered_literals: Vec<&LiteralValue> = Vec::new();
        for (idx, arm) in expr.arms.iter().enumerate() {
            match &arm.pattern {
                Pattern::Wildcard => {
                    if idx != expr.arms.len() - 1 {
                        self.error(
                            RlError::span(
                                arm.pattern_span.start,
                                arm.pattern_span.end,
                                "match: the wildcard arm `_` must be the last arm".to_string(),
                            )
                            .code(DiagnosticCode::MatchWildcardNotLast),
                        );
                    }
                }
                Pattern::Literals(alts) => {
                    let mut arm_values: Vec<&LiteralValue> = Vec::new();
                    for alt in alts {
                        if alt.value.kind() != alts[0].value.kind() {
                            self.error(
                                RlError::span(
                                    alt.span.start,
                                    alt.span.end,
                                    format!(
                                        "match: or-pattern alternatives must all be the same kind of \
                                         literal (found {} after {})",
                                        alt.value.kind(),
                                        alts[0].value.kind()
                                    ),
                                )
                                .code(DiagnosticCode::MatchOrLiteralKindMismatch),
                            );
                            continue;
                        }
                        if covered_literals.contains(&&alt.value)
                            || arm_values.contains(&&alt.value)
                        {
                            self.error(
                                RlError::span(
                                    alt.span.start,
                                    alt.span.end,
                                    format!("match: duplicate arm {}", alt.value.render()),
                                )
                                .code(DiagnosticCode::MatchDuplicateArm),
                            );
                            continue;
                        }
                        arm_values.push(&alt.value);
                    }
                    if arm.guard.is_none() {
                        covered_literals.append(&mut arm_values);
                    }
                }
                Pattern::Tags(alts) => {
                    // Codegen emits one destructuring shared by every
                    // alternative (switch fallthrough), so all alternatives
                    // must bind the exact same (field, name) set — which is
                    // also why a nested pattern (per-alternative conditions
                    // and paths) cannot appear inside an or-pattern.
                    if alts.len() > 1 && alts.iter().any(has_nested) {
                        let at = alts.iter().find(|a| has_nested(a)).unwrap();
                        self.error(
                            RlError::span(
                                at.tag_off,
                                at.tag_off + at.tag.len(),
                                "match: nested patterns cannot be combined with or-patterns"
                                    .to_string(),
                            )
                            .code(DiagnosticCode::MatchNestedInOrPattern),
                        );
                    }
                    self.check_leaf_bindings(&alts[0]);
                    let first_set = binding_set(&alts[0].bindings);
                    let mut arm_tags: Vec<&str> = Vec::new();
                    for alt in alts {
                        if covered.contains(&alt.tag.as_str())
                            || arm_tags.contains(&alt.tag.as_str())
                        {
                            self.error(
                                RlError::span(
                                    alt.tag_off,
                                    alt.tag_off + alt.tag.len(),
                                    format!("match: duplicate arm \"{}\"", alt.tag),
                                )
                                .code(DiagnosticCode::MatchDuplicateArm),
                            );
                            continue;
                        }
                        arm_tags.push(&alt.tag);
                        if binding_set(&alt.bindings) != first_set {
                            self.error(
                                RlError::span(
                                    alt.tag_off,
                                    alt.tag_off + alt.tag.len(),
                                    format!(
                                        "match: or-pattern alternatives must bind the same names — {}",
                                        binding_mismatch(&alts[0], alt)
                                    ),
                                )
                                .code(DiagnosticCode::MatchOrBindingMismatch),
                            );
                        }
                    }
                    // A nested pattern may mismatch, so — like a guard —
                    // the arm identifies the enum but covers nothing.
                    if arm.guard.is_none() && !alts.iter().any(has_nested) {
                        covered.append(&mut arm_tags);
                    }
                }
            }
        }

        // Exhaustiveness is not recorded here: the analysis walks the same
        // program and answers for every match at once (`report_coverage`).

        // children, in source order: scrutinee first, then guards and bodies
        self.visit_program(&expr.scrutinee, Ctx::Expr, Place::Iife);
        for arm in &expr.arms {
            if let Some(guard) = &arm.guard {
                self.visit_program(&guard.expr, Ctx::Expr, Place::Iife);
            }
            // A block arm body is a statement context (inside the IIFE)
            self.visit_program(
                &arm.body,
                if arm.block { Ctx::Stmt } else { Ctx::Expr },
                Place::Iife,
            );
        }
    }

    fn check_tuple_match(&mut self, expr: &TupleMatchExpr) {
        let arity = expr.scrutinees.len();
        for (idx, arm) in expr.arms.iter().enumerate() {
            match &arm.pattern {
                TuplePattern::Wildcard => {
                    if idx != expr.arms.len() - 1 {
                        self.error(
                            RlError::span(
                                arm.pattern_span.start,
                                arm.pattern_span.end,
                                "match: the wildcard arm `_` must be the last arm".to_string(),
                            )
                            .code(DiagnosticCode::MatchWildcardNotLast),
                        );
                    }
                }
                TuplePattern::Elems(elems) => {
                    if elems.len() != arity {
                        self.error(
                            RlError::span(
                                arm.pattern_span.start,
                                arm.pattern_span.end,
                                format!(
                                    "match: tuple pattern has {} elements but the match has {} scrutinees",
                                    elems.len(),
                                    arity
                                ),
                            )
                            .code(DiagnosticCode::MatchTupleArity),
                        );
                    }
                    // Every element's or-alternatives share one
                    // destructuring (hence no nested patterns in them);
                    // bound names must also be unique across the whole
                    // tuple pattern (they land in one scope).
                    let mut bound: Vec<&str> = Vec::new();
                    for elem in elems {
                        let Pattern::Tags(alts) = elem else { continue };
                        if alts.len() > 1 && alts.iter().any(has_nested) {
                            let at = alts.iter().find(|a| has_nested(a)).unwrap();
                            self.error(
                                RlError::span(
                                    at.tag_off,
                                    at.tag_off + at.tag.len(),
                                    "match: nested patterns cannot be combined with or-patterns"
                                        .to_string(),
                                )
                                .code(DiagnosticCode::MatchNestedInOrPattern),
                            );
                        }
                        let first_set = binding_set(&alts[0].bindings);
                        for alt in alts {
                            if binding_set(&alt.bindings) != first_set {
                                self.error(
                                    RlError::span(
                                        alt.tag_off,
                                        alt.tag_off + alt.tag.len(),
                                        format!(
                                            "match: or-pattern alternatives must bind the same names — {}",
                                            binding_mismatch(&alts[0], alt)
                                        ),
                                    )
                                    .code(DiagnosticCode::MatchOrBindingMismatch),
                                );
                            }
                        }
                        let mut leaves = Vec::new();
                        leaf_bindings(&alts[0], &mut leaves);
                        for name in leaves {
                            if bound.contains(&name) {
                                self.error(
                                    RlError::span(
                                        alts[0].tag_off,
                                        alts[0].tag_off + alts[0].tag.len(),
                                        format!(
                                            "match: binding `{name}` is used more than once in this tuple pattern (rename one with `field: alias`)"
                                        ),
                                    )
                                    .code(DiagnosticCode::PatternDuplicateBinding),
                                );
                                continue;
                            }
                            bound.push(name);
                        }
                    }
                }
            }
        }

        // children, in source order
        for (_, scrutinee) in &expr.scrutinees {
            self.visit_program(scrutinee, Ctx::Expr, Place::Iife);
        }
        for arm in &expr.arms {
            if let Some(guard) = &arm.guard {
                self.visit_program(&guard.expr, Ctx::Expr, Place::Iife);
            }
            self.visit_program(
                &arm.body,
                if arm.block { Ctx::Stmt } else { Ctx::Expr },
                Place::Iife,
            );
        }
    }
}

/// Turns [`crate::analysis`]'s coverage into positioned rl errors — every
/// uncovered match, in the analysis' source order.
///
/// A match whose own names failed to resolve is skipped: the typo is the
/// cause and its coverage hole the effect, so only the cause is reported
/// for that match — while every *other* match keeps its own answer.
fn report_coverage(
    analyses: &crate::analysis::PatternAnalyses,
    suppressed: &[usize],
    errors: &mut Vec<RlError>,
) {
    let uncovered = analyses
        .matches
        .iter()
        .filter(|m| !m.has_unresolved && !suppressed.contains(&m.keyword_off))
        .filter_map(|m| {
            m.coverage
                .as_ref()
                .map(|c| ((m.keyword_off, m.head_end), c))
        })
        .filter(|(_, c)| !c.missing.is_empty());

    for ((offset, head_end), coverage) in uncovered {
        let message = if coverage.positions.len() == 1 {
            // A single match's one position always resolved — that is what
            // makes it a coverage answer at all.
            let Some(subject) = coverage.positions[0].as_ref() else {
                continue;
            };
            let missing: Vec<String> = coverage
                .missing_tags()
                .iter()
                .map(|m| format!("\"{m}\""))
                .collect();
            non_exhaustive_message(Some(&describe(subject)), &missing, false)
        } else {
            let names = coverage
                .positions
                .iter()
                .map(|p| p.as_ref().map_or("_", |e| e.name.as_str()))
                .collect::<Vec<_>>()
                .join(", ");
            let combinations: Vec<String> = coverage
                .missing
                .iter()
                .map(|row| format!("({})", row.pattern.join(", ")))
                .collect();
            non_exhaustive_message(Some(&format!("({names})")), &combinations, true)
        };
        errors.push(
            RlError::span(offset, head_end, message).code(DiagnosticCode::MatchNotExhaustive),
        );
    }
}

/// How an error names the enum a match is over — the declaration's origin,
/// so "which `Token`?" is answerable from the message alone.
fn describe(subject: &CoveredEnum) -> String {
    match &subject.origin {
        Origin::Local => format!("enum {}", subject.name),
        Origin::Builtin => format!("built-in enum {}", subject.name),
        Origin::Imported { from: Some(from) } => {
            format!("enum {} (imported from \"{from}\")", subject.name)
        }
        Origin::Imported { from: None } => format!("imported enum {}", subject.name),
    }
}
