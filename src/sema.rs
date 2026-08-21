//! Semantic checks over the AST.
//!
//! Everything the parser deliberately does not do lives here: rl-level rules
//! whose violation is a compile error rather than a passthrough. The checker
//! walks the AST depth-first in source order — a construct's own rules are
//! checked before its children, matching the positions users expect — and
//! reports the first violation as an [`RlError`] with a byte offset.
//!
//! Error layering (see `docs/reference/errors.md`): every rule here is an
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
//! - `try`: only allowed in the top-level statement stream — inside a match
//!   expression, a `result` block, a template interpolation, or another
//!   try's expression its emitted `return` would not exit the enclosing
//!   function, so it is an error there.
//! - let-else: same placement rule as `try` (it emits statements into the
//!   enclosing scope), plus the `else` block must end with a diverging
//!   statement (`return`/`throw`/`break`/`continue`) — otherwise the
//!   destructuring after the block would run with the case unproven.
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
use crate::analysis::{Coverage, CoveredEnum, NameKind, Origin, has_nested};
use crate::ast::*;
use crate::error::RlError;
use crate::verify;

/// Checks a whole program; `verify` enables swc validation of field types;
/// `externs` are enum declarations collected from imported modules
/// ([`crate::Options::extern_enums`]). With `defer_to_checker` the two
/// exhaustiveness passes are skipped, because a TypeScript backend answers
/// the question better than this file's declaration table can
/// ([`crate::Options::defer_to_checker`]); every other rl-level rule is
/// checked either way.
pub(crate) fn check(
    program: &Program,
    verify: bool,
    externs: &[ExternEnum],
    defer_to_checker: bool,
) -> Result<(), RlError> {
    let mut checker = Checker { verify };
    checker.visit_program(program, Ctx::Top)?;
    // One analysis, two reports. Resolution comes first — a pattern whose
    // names do not resolve has no exhaustiveness question worth asking,
    // and answering both at once would bury the cause under its effect.
    let analyses = crate::analysis::coverage_analyses(program, externs);
    report_resolution(&analyses)?;
    if defer_to_checker {
        return Ok(());
    }
    report_coverage(&analyses)
}

/// Turns [`crate::analysis`]'s resolution answer into positioned rl
/// errors, in source order.
///
/// Every entry the analysis produced is an error: the *decision* whether
/// an unresolved name is reportable belongs to the analysis (which is what
/// keeps one rule in one place), and it only produces entries it can name
/// a replacement for. This function is the wording.
fn report_resolution(analyses: &crate::analysis::PatternAnalyses) -> Result<(), RlError> {
    let Some(unresolved) = analyses.unresolved.first() else {
        return Ok(());
    };
    let described = describe(&CoveredEnum {
        name: unresolved.enum_name.clone(),
        origin: unresolved.origin.clone(),
    });
    let message = match (&unresolved.kind, &unresolved.tag) {
        (NameKind::Field, Some(tag)) => format!(
            "{described}: case `{tag}` has no field `{}` — did you mean `{}`?",
            unresolved.name, unresolved.suggestion
        ),
        _ => format!(
            "{described} has no case `{}` — did you mean `{}`?",
            unresolved.name, unresolved.suggestion
        ),
    };
    Err(RlError::span(unresolved.start, unresolved.end, message))
}

struct Checker {
    verify: bool,
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

/// Where a sub-program sits, for placement rules. `try`/let-else need the
/// [`Ctx::Top`] statement stream (their emitted `return` must exit the
/// enclosing function, and every nested context either is an expression or
/// sits inside a match's IIFE). `if let` compiles to a self-contained block
/// with no `return` of its own, so any statement context ([`Ctx::Top`] or
/// [`Ctx::Stmt`] — a block arm body, a let-else `else` block, an `if let`
/// body) is fine; only expression positions ([`Ctx::Expr`]) are out.
#[derive(Clone, Copy, PartialEq)]
enum Ctx {
    Top,
    Stmt,
    Expr,
}

impl Checker {
    fn visit_program(&mut self, program: &Program, ctx: Ctx) -> Result<(), RlError> {
        // A stray `|>` or `if let` cannot be passed through: neither is
        // valid TypeScript, so the output self-check would fail without a
        // position. Report them as rl errors here instead (error-layering
        // contract).
        if let Some(&off) = program.stray_pipes.first() {
            return Err(RlError::span(
                off,
                off + "|>".len(),
                "pipeline: `|>` could not be parsed here (steps must be expressions; \
                 parenthesize ternaries and arrow functions)"
                    .to_string(),
            ));
        }
        if let Some(&off) = program.stray_if_lets.first() {
            return Err(RlError::span(
                off,
                off + "if".len(),
                "`if let` could not be parsed here (pattern parens are mandatory, and the \
                 `else` must be a block or another `if let`)"
                    .to_string(),
            ));
        }
        if let Some(&off) = program.stray_results.first() {
            return Err(RlError::span(
                off,
                off + "result".len(),
                "`result` block could not be parsed here (every binding is \
                 `const <binding> <- <expression>;`, and the block must end with an \
                 expression)"
                    .to_string(),
            ));
        }
        if let Some(&off) = program.result_missing_kw.first() {
            return Err(RlError::at(
                off,
                "`result` binding is missing its declaration keyword \
                 (write `const <binding> <- <expression>;`, or `let`/`var`)"
                    .to_string(),
            ));
        }
        for segment in &program.segments {
            match segment {
                Segment::Verbatim(_) | Segment::RlImport(_) | Segment::ValModifier(_) => {}
                Segment::Enum(decl) => self.check_enum(decl)?,
                Segment::Match(expr) => self.check_match(expr)?,
                Segment::TupleMatch(expr) => self.check_tuple_match(expr)?,
                Segment::Try(stmt) => self.check_try(stmt, ctx)?,
                Segment::LetElse(stmt) => self.check_let_else(stmt, ctx)?,
                Segment::IfLet(stmt) => self.check_if_let(stmt, ctx)?,
                Segment::ResultBlock(block) => self.check_result_block(block)?,
                Segment::Pipe(pipe) => {
                    // A `flow` composition has no value to chain a method
                    // onto until its first function has produced one, so
                    // its first step must be an ordinary function step.
                    if pipe.head.is_none()
                        && let Some(first) = pipe.steps.first()
                        && first.postfix
                    {
                        return Err(RlError::span(
                            first.span.start,
                            first.span.end,
                            "`flow`: the first step cannot be a method step — it is the \
                             composed function's input, so it must be a function \
                             (`flow |> ((s: string) => s.trim()) |> ...`)"
                                .to_string(),
                        ));
                    }
                    // Head and steps are expressions — `try` inside them is
                    // rejected for the same reason as inside a match.
                    if let Some(head) = &pipe.head {
                        self.visit_program(head, Ctx::Expr)?;
                    }
                    for step in &pipe.steps {
                        self.visit_program(&step.body, Ctx::Expr)?;
                    }
                }
                Segment::Template(template) => {
                    for chunk in &template.chunks {
                        if let TemplateChunk::Interp(interp) = chunk {
                            self.visit_program(interp, Ctx::Expr)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn check_try(&mut self, stmt: &TryStmt, ctx: Ctx) -> Result<(), RlError> {
        if ctx != Ctx::Top {
            return Err(RlError::span(
                stmt.span.start,
                stmt.span.end,
                "`try` cannot be used inside a match expression, a `result` block, a \
                 template interpolation, or another `try` — it compiles to a `return` \
                 from the enclosing function"
                    .to_string(),
            ));
        }
        self.visit_program(&stmt.expr, Ctx::Expr)
    }

    fn check_let_else(&mut self, stmt: &LetElseStmt, ctx: Ctx) -> Result<(), RlError> {
        if ctx != Ctx::Top {
            return Err(RlError::span(
                stmt.head_span.start,
                stmt.head_span.end,
                "let-else cannot be used inside a match expression, a `result` block, a \
                 template interpolation, or a `try` — it compiles to statements in the \
                 enclosing function"
                    .to_string(),
            ));
        }
        if !stmt.diverges {
            return Err(RlError::span(
                stmt.else_off,
                stmt.else_off + "else".len(),
                "let-else: the `else` block must end with a `return`, `throw`, `break`, or \
                 `continue` statement"
                    .to_string(),
            ));
        }
        self.visit_program(&stmt.expr, Ctx::Expr)?;
        self.visit_program(&stmt.else_body, Ctx::Stmt)
    }

    fn check_if_let(&mut self, stmt: &IfLetStmt, ctx: Ctx) -> Result<(), RlError> {
        if ctx == Ctx::Expr {
            return Err(RlError::span(
                stmt.head_span.start,
                stmt.head_span.end,
                "`if let` cannot be used in expression position (a template interpolation, \
                 a scrutinee or guard, an expression arm body, a `try` expression, or a \
                 pipeline) — it compiles to a block statement"
                    .to_string(),
            ));
        }
        self.check_leaf_bindings(&stmt.pattern)?;
        self.visit_program(&stmt.expr, Ctx::Expr)?;
        self.visit_program(&stmt.body, Ctx::Stmt)?;
        match &stmt.else_part {
            Some(IfLetElse::Block(block)) => self.visit_program(block, Ctx::Stmt)?,
            Some(IfLetElse::IfLet(inner)) => self.check_if_let(inner, Ctx::Stmt)?,
            None => {}
        }
        Ok(())
    }

    /// A `result` block is an expression, so it is allowed anywhere; its
    /// body is the IIFE's statement stream ([`Ctx::Stmt`] — a `try` or
    /// let-else there would return from the *block*, not the enclosing
    /// function), and the bindings and the trailing value are expressions.
    fn check_result_block(&mut self, block: &ResultBlock) -> Result<(), RlError> {
        for item in &block.items {
            match item {
                ResultItem::Stmts(stmts) => self.visit_program(stmts, Ctx::Stmt)?,
                ResultItem::Bind(bind) => self.visit_program(&bind.expr, Ctx::Expr)?,
            }
        }
        self.visit_program(&block.value, Ctx::Expr)
    }

    fn check_enum(&mut self, decl: &EnumDecl) -> Result<(), RlError> {
        let mut seen: Vec<&str> = Vec::new();
        for case in &decl.cases {
            if seen.contains(&case.tag.as_str()) {
                return Err(RlError::span(
                    case.tag_off,
                    case.tag_off + case.tag.len(),
                    format!("enum {}: duplicate case \"{}\"", decl.name, case.tag),
                ));
            }
            seen.push(&case.tag);
        }

        if self.verify {
            for case in &decl.cases {
                if let Some(fields) = &case.fields {
                    for field in fields {
                        if let Err(msg) = verify::check_type_fragment(&field.ty) {
                            return Err(RlError::span(
                                field.ty_off,
                                field.ty_off + field.ty.len(),
                                format!(
                                    "enum {}: invalid type for field `{}`: {}",
                                    decl.name, field.name, msg
                                ),
                            ));
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Bound names must be unique within one pattern — they all land in the
    /// same scope, so a duplicate would emit two `const`s of one name.
    fn check_leaf_bindings(&self, alt: &TagPattern) -> Result<(), RlError> {
        let mut leaves = Vec::new();
        leaf_bindings(alt, &mut leaves);
        for (i, name) in leaves.iter().enumerate() {
            if leaves[..i].contains(name) {
                return Err(RlError::span(
                    alt.tag_off,
                    alt.tag_off + alt.tag.len(),
                    format!(
                        "match: binding `{name}` is used more than once in this pattern (rename one with `field: alias`)"
                    ),
                ));
            }
        }
        Ok(())
    }

    fn check_match(&mut self, expr: &MatchExpr) -> Result<(), RlError> {
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
            return Err(RlError::at(
                other.pattern_off,
                format!(
                    "match: cannot mix tag patterns and literal patterns in the same match \
                     (this match starts with {first} patterns) — the two compare different \
                     things (`$rl_m.kind` vs `$rl_m`); split them into two matches"
                ),
            ));
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
                        return Err(RlError::span(
                            arm.pattern_off,
                            arm.pattern_off + 1,
                            "match: the wildcard arm `_` must be the last arm".to_string(),
                        ));
                    }
                }
                Pattern::Literals(alts) => {
                    let mut arm_values: Vec<&LiteralValue> = Vec::new();
                    for alt in alts {
                        if alt.value.kind() != alts[0].value.kind() {
                            return Err(RlError::span(
                                alt.span.start,
                                alt.span.end,
                                format!(
                                    "match: or-pattern alternatives must all be the same kind of \
                                     literal (found {} after {})",
                                    alt.value.kind(),
                                    alts[0].value.kind()
                                ),
                            ));
                        }
                        if covered_literals.contains(&&alt.value)
                            || arm_values.contains(&&alt.value)
                        {
                            return Err(RlError::span(
                                alt.span.start,
                                alt.span.end,
                                format!("match: duplicate arm {}", alt.value.render()),
                            ));
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
                        return Err(RlError::span(
                            at.tag_off,
                            at.tag_off + at.tag.len(),
                            "match: nested patterns cannot be combined with or-patterns"
                                .to_string(),
                        ));
                    }
                    self.check_leaf_bindings(&alts[0])?;
                    let first_set = binding_set(&alts[0].bindings);
                    let mut arm_tags: Vec<&str> = Vec::new();
                    for alt in alts {
                        if covered.contains(&alt.tag.as_str())
                            || arm_tags.contains(&alt.tag.as_str())
                        {
                            return Err(RlError::span(
                                alt.tag_off,
                                alt.tag_off + alt.tag.len(),
                                format!("match: duplicate arm \"{}\"", alt.tag),
                            ));
                        }
                        arm_tags.push(&alt.tag);
                        if binding_set(&alt.bindings) != first_set {
                            return Err(RlError::span(
                                alt.tag_off,
                                alt.tag_off + alt.tag.len(),
                                format!(
                                    "match: or-pattern alternatives must bind the same names — {}",
                                    binding_mismatch(&alts[0], alt)
                                ),
                            ));
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
        self.visit_program(&expr.scrutinee, Ctx::Expr)?;
        for arm in &expr.arms {
            if let Some(guard) = &arm.guard {
                self.visit_program(&guard.expr, Ctx::Expr)?;
            }
            // A block arm body is a statement context (inside the IIFE)
            self.visit_program(&arm.body, if arm.block { Ctx::Stmt } else { Ctx::Expr })?;
        }
        Ok(())
    }

    fn check_tuple_match(&mut self, expr: &TupleMatchExpr) -> Result<(), RlError> {
        let arity = expr.scrutinees.len();
        for (idx, arm) in expr.arms.iter().enumerate() {
            match &arm.pattern {
                TuplePattern::Wildcard => {
                    if idx != expr.arms.len() - 1 {
                        return Err(RlError::span(
                            arm.pattern_off,
                            arm.pattern_off + 1,
                            "match: the wildcard arm `_` must be the last arm".to_string(),
                        ));
                    }
                }
                TuplePattern::Elems(elems) => {
                    if elems.len() != arity {
                        return Err(RlError::at(
                            arm.pattern_off,
                            format!(
                                "match: tuple pattern has {} elements but the match has {} scrutinees",
                                elems.len(),
                                arity
                            ),
                        ));
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
                            return Err(RlError::span(
                                at.tag_off,
                                at.tag_off + at.tag.len(),
                                "match: nested patterns cannot be combined with or-patterns"
                                    .to_string(),
                            ));
                        }
                        let first_set = binding_set(&alts[0].bindings);
                        for alt in alts {
                            if binding_set(&alt.bindings) != first_set {
                                return Err(RlError::span(
                                    alt.tag_off,
                                    alt.tag_off + alt.tag.len(),
                                    format!(
                                        "match: or-pattern alternatives must bind the same names — {}",
                                        binding_mismatch(&alts[0], alt)
                                    ),
                                ));
                            }
                        }
                        let mut leaves = Vec::new();
                        leaf_bindings(&alts[0], &mut leaves);
                        for name in leaves {
                            if bound.contains(&name) {
                                return Err(RlError::span(
                                    alts[0].tag_off,
                                    alts[0].tag_off + alts[0].tag.len(),
                                    format!(
                                        "match: binding `{name}` is used more than once in this tuple pattern (rename one with `field: alias`)"
                                    ),
                                ));
                            }
                            bound.push(name);
                        }
                    }
                }
            }
        }

        // children, in source order
        for (_, scrutinee) in &expr.scrutinees {
            self.visit_program(scrutinee, Ctx::Expr)?;
        }
        for arm in &expr.arms {
            if let Some(guard) = &arm.guard {
                self.visit_program(&guard.expr, Ctx::Expr)?;
            }
            self.visit_program(&arm.body, if arm.block { Ctx::Stmt } else { Ctx::Expr })?;
        }
        Ok(())
    }
}

/// Turns [`crate::analysis`]'s coverage into positioned rl errors.
///
/// Reporting order is the order these checks have always run in: every
/// single match first, in source order, then every tuple match — so the
/// message a file produces does not depend on how the two kinds interleave.
/// A tuple match always has at least two scrutinees (the parser requires
/// the comma), so one position means a single match.
fn report_coverage(analyses: &crate::analysis::PatternAnalyses) -> Result<(), RlError> {
    // `match (scrutinee)` — the head, which is what the error is about;
    // the arms below it are the user's own code.
    let uncovered: Vec<((usize, usize), &Coverage)> = analyses
        .matches
        .iter()
        .filter_map(|m| {
            m.coverage
                .as_ref()
                .map(|c| ((m.keyword_off, m.head_end), c))
        })
        .filter(|(_, c)| !c.missing.is_empty())
        .collect();

    // The first uncovered match decides the error; each `find` is that
    // match, not a loop over several.
    if let Some(((offset, head_end), coverage)) = uncovered.iter().find(|(_, c)| c.positions.len() == 1)
        // A single match's one position always resolved — that is what
        // makes it a coverage answer at all.
        && let Some(subject) = coverage.positions[0].as_ref()
    {
        let list = coverage
            .missing_tags()
            .iter()
            .map(|m| format!("\"{m}\""))
            .collect::<Vec<_>>()
            .join(", ");
        let described = describe(subject);
        return Err(RlError::span(
            *offset,
            *head_end,
            format!(
                "match on {described} is not exhaustive: missing {list} (add the missing arms or a final `_` arm)"
            ),
        ));
    }

    if let Some(((offset, head_end), coverage)) =
        uncovered.iter().find(|(_, c)| c.positions.len() > 1)
    {
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
        let shown = if combinations.len() > 4 {
            format!(
                "{}, … ({} combinations in total)",
                combinations[..3].join(", "),
                combinations.len()
            )
        } else {
            combinations.join(", ")
        };
        return Err(RlError::span(
            *offset,
            *head_end,
            format!(
                "match on ({names}) is not exhaustive: missing {shown} (add the missing arms or a final `_` arm)"
            ),
        ));
    }
    Ok(())
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
