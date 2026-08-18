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
//! delegated to tsc — in particular exhaustiveness, which is resolved from
//! the enum registry collected during the walk, *after* the walk (so a match
//! may precede the enum it matches on).
//!
//! Checks performed:
//! - `enum`: no duplicate case tags; with verification enabled, every field
//!   type parses as a TypeScript type fragment (via [`crate::verify`]).
//! - `match`: the wildcard `_` arm is last; no arm repeats a tag already
//!   covered by an unguarded arm (guarded arms may share tags with each
//!   other); or-pattern alternatives all bind the same (field, name) set.
//! - `try`: only allowed in the top-level statement stream — inside a match
//!   expression, a template interpolation, or another try's expression its
//!   emitted `return` would not exit the enclosing function, so it is an
//!   error there.
//! - let-else: same placement rule as `try` (it emits statements into the
//!   enclosing scope), plus the `else` block must end with a diverging
//!   statement (`return`/`throw`/`break`/`continue`) — otherwise the
//!   destructuring after the block would run with the case unproven.
//! - exhaustiveness: a wildcard-free match whose arm tags all belong to an
//!   enum declared in this file, an imported declaration
//!   ([`crate::Options::extern_enums`], collected by the CLI from direct
//!   relative `.rl` imports), or a built-in enum (`Option`, `Result`; see
//!   [`crate::stdlib::BUILTIN_ENUMS`]) — must cover every case of that
//!   enum with unguarded arms (a guard may be false, so guarded arms
//!   identify the enum but cover nothing). Same-name shadowing runs
//!   local > imported > built-in. Matches whose tags belong to no known
//!   enum (hand-written unions, unresolved imports) are not checked — rlc
//!   has no type information for them.

use std::collections::BTreeMap;

use crate::ExternEnum;
use crate::ast::*;
use crate::error::RlError;
use crate::verify;

/// A deferred exhaustiveness check for one wildcard-free `match`, resolved
/// once the whole file has been walked (so declaration order doesn't matter).
struct MatchCheck {
    /// Offset of the `match` keyword, for error reporting.
    offset: usize,
    /// Every non-wildcard arm tag, guarded or not — used to identify which
    /// enum the match is over.
    tags: Vec<String>,
    /// Tags of unguarded arms only — a guard may be false, so only these
    /// count as covering a case.
    covered: Vec<String>,
}

/// A deferred product-exhaustiveness check for one tuple match without a
/// bare `_` arm. Each scrutinee position resolves to an enum independently
/// (same shadowing order as single matches); the check then walks the
/// cartesian product of the case sets.
struct TupleMatchCheck {
    /// Offset of the `match` keyword, for error reporting.
    offset: usize,
    /// Per position: every tag any arm (guarded or not) uses there — the
    /// identification set. Empty when every arm has `_` at that position.
    position_tags: Vec<Vec<String>>,
    /// Per unguarded arm, per position: `None` for `_` (covers the whole
    /// position), `Some(tags)` for the or-alternative tags it covers.
    covered: Vec<Vec<Option<Vec<String>>>>,
}

/// Checks a whole program; `verify` enables swc validation of field types;
/// `externs` are enum declarations collected from imported modules
/// ([`crate::Options::extern_enums`]).
pub(crate) fn check(
    program: &Program,
    verify: bool,
    externs: &[ExternEnum],
) -> Result<(), RlError> {
    let mut checker = Checker {
        verify,
        enums: BTreeMap::new(),
        externs,
        match_checks: Vec::new(),
        tuple_checks: Vec::new(),
    };
    checker.visit_program(program, Ctx::Top)?;
    checker.check_exhaustiveness()?;
    checker.check_tuple_exhaustiveness()
}

struct Checker<'a> {
    verify: bool,
    /// rl enums declared in this file: name → case tags.
    enums: BTreeMap<String, Vec<String>>,
    /// Enum declarations imported from other modules.
    externs: &'a [ExternEnum],
    /// Wildcard-free matches to exhaustiveness-check after the walk.
    match_checks: Vec<MatchCheck>,
    /// Tuple matches without a bare `_` arm, checked after the walk.
    tuple_checks: Vec<TupleMatchCheck>,
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

/// True when the alternative carries a nested pattern (any depth starts
/// with one at the first binding level).
fn has_nested(alt: &TagPattern) -> bool {
    alt.bindings
        .as_deref()
        .unwrap_or_default()
        .iter()
        .any(|b| b.nested.is_some())
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

impl Checker<'_> {
    fn visit_program(&mut self, program: &Program, ctx: Ctx) -> Result<(), RlError> {
        // A stray `|>` or `if let` cannot be passed through: neither is
        // valid TypeScript, so the output self-check would fail without a
        // position. Report them as rl errors here instead (error-layering
        // contract).
        if let Some(&off) = program.stray_pipes.first() {
            return Err(RlError::at(
                off,
                "pipeline: `|>` could not be parsed here (steps must be expressions; \
                 parenthesize ternaries and arrow functions)"
                    .to_string(),
            ));
        }
        if let Some(&off) = program.stray_if_lets.first() {
            return Err(RlError::at(
                off,
                "`if let` could not be parsed here (pattern parens are mandatory, and the \
                 `else` must be a block or another `if let`)"
                    .to_string(),
            ));
        }
        for segment in &program.segments {
            match segment {
                Segment::Verbatim(_) | Segment::RlImport(_) => {}
                Segment::Enum(decl) => self.check_enum(decl)?,
                Segment::Match(expr) => self.check_match(expr)?,
                Segment::TupleMatch(expr) => self.check_tuple_match(expr)?,
                Segment::Try(stmt) => self.check_try(stmt, ctx)?,
                Segment::LetElse(stmt) => self.check_let_else(stmt, ctx)?,
                Segment::IfLet(stmt) => self.check_if_let(stmt, ctx)?,
                Segment::Pipe(pipe) => {
                    // A `flow` composition has no value to chain a method
                    // onto until its first function has produced one, so
                    // its first step must be an ordinary function step.
                    if pipe.head.is_none()
                        && let Some(first) = pipe.steps.first()
                        && first.postfix
                    {
                        return Err(RlError::at(
                            first.span.start,
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
            return Err(RlError::at(
                stmt.keyword_off,
                "`try` cannot be used inside a match expression, a template interpolation, \
                 or another `try` — it compiles to a `return` from the enclosing function"
                    .to_string(),
            ));
        }
        self.visit_program(&stmt.expr, Ctx::Expr)
    }

    fn check_let_else(&mut self, stmt: &LetElseStmt, ctx: Ctx) -> Result<(), RlError> {
        if ctx != Ctx::Top {
            return Err(RlError::at(
                stmt.keyword_off,
                "let-else cannot be used inside a match expression, a template interpolation, \
                 or a `try` — it compiles to statements in the enclosing function"
                    .to_string(),
            ));
        }
        if !stmt.diverges {
            return Err(RlError::at(
                stmt.else_off,
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
            return Err(RlError::at(
                stmt.keyword_off,
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

    fn check_enum(&mut self, decl: &EnumDecl) -> Result<(), RlError> {
        let mut seen: Vec<&str> = Vec::new();
        for case in &decl.cases {
            if seen.contains(&case.tag.as_str()) {
                return Err(RlError::at(
                    case.tag_off,
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
                            return Err(RlError::at(
                                field.ty_off,
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

        self.enums.insert(
            decl.name.clone(),
            decl.cases.iter().map(|c| c.tag.clone()).collect(),
        );
        Ok(())
    }

    /// Bound names must be unique within one pattern — they all land in the
    /// same scope, so a duplicate would emit two `const`s of one name.
    fn check_leaf_bindings(&self, alt: &TagPattern) -> Result<(), RlError> {
        let mut leaves = Vec::new();
        leaf_bindings(alt, &mut leaves);
        for (i, name) in leaves.iter().enumerate() {
            if leaves[..i].contains(name) {
                return Err(RlError::at(
                    alt.tag_off,
                    format!(
                        "match: binding `{name}` is used more than once in this pattern (rename one with `field: alias`)"
                    ),
                ));
            }
        }
        Ok(())
    }

    fn check_match(&mut self, expr: &MatchExpr) -> Result<(), RlError> {
        // Tags covered by an unguarded arm. Any later arm repeating one of
        // these is unreachable (duplicate); a guarded arm covers nothing, so
        // guarded arms may repeat each other's tags.
        let mut covered: Vec<&str> = Vec::new();
        for (idx, arm) in expr.arms.iter().enumerate() {
            match &arm.pattern {
                Pattern::Wildcard => {
                    if idx != expr.arms.len() - 1 {
                        return Err(RlError::at(
                            arm.pattern_off,
                            "match: the wildcard arm `_` must be the last arm".to_string(),
                        ));
                    }
                }
                Pattern::Tags(alts) => {
                    // Codegen emits one destructuring shared by every
                    // alternative (switch fallthrough), so all alternatives
                    // must bind the exact same (field, name) set — which is
                    // also why a nested pattern (per-alternative conditions
                    // and paths) cannot appear inside an or-pattern.
                    if alts.len() > 1 && alts.iter().any(has_nested) {
                        return Err(RlError::at(
                            alts.iter().find(|a| has_nested(a)).unwrap().tag_off,
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
                            return Err(RlError::at(
                                alt.tag_off,
                                format!("match: duplicate arm \"{}\"", alt.tag),
                            ));
                        }
                        arm_tags.push(&alt.tag);
                        if binding_set(&alt.bindings) != first_set {
                            return Err(RlError::at(
                                alt.tag_off,
                                "match: or-pattern alternatives must bind the same fields"
                                    .to_string(),
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

        if !expr
            .arms
            .iter()
            .any(|a| matches!(a.pattern, Pattern::Wildcard))
        {
            let arm_tags = |covering_only: bool| {
                expr.arms
                    .iter()
                    .filter(|a| {
                        !covering_only
                            || (a.guard.is_none()
                                && !matches!(&a.pattern, Pattern::Tags(alts) if alts.iter().any(has_nested)))
                    })
                    .flat_map(|a| match &a.pattern {
                        Pattern::Tags(alts) => {
                            alts.iter().map(|t| t.tag.clone()).collect::<Vec<_>>()
                        }
                        Pattern::Wildcard => Vec::new(),
                    })
                    .collect::<Vec<_>>()
            };
            self.match_checks.push(MatchCheck {
                offset: expr.keyword_off,
                tags: arm_tags(false),
                covered: arm_tags(true),
            });
        }

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
        let mut has_bare_wildcard = false;
        for (idx, arm) in expr.arms.iter().enumerate() {
            match &arm.pattern {
                TuplePattern::Wildcard => {
                    if idx != expr.arms.len() - 1 {
                        return Err(RlError::at(
                            arm.pattern_off,
                            "match: the wildcard arm `_` must be the last arm".to_string(),
                        ));
                    }
                    has_bare_wildcard = true;
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
                            return Err(RlError::at(
                                alts.iter().find(|a| has_nested(a)).unwrap().tag_off,
                                "match: nested patterns cannot be combined with or-patterns"
                                    .to_string(),
                            ));
                        }
                        let first_set = binding_set(&alts[0].bindings);
                        for alt in alts {
                            if binding_set(&alt.bindings) != first_set {
                                return Err(RlError::at(
                                    alt.tag_off,
                                    "match: or-pattern alternatives must bind the same fields"
                                        .to_string(),
                                ));
                            }
                        }
                        let mut leaves = Vec::new();
                        leaf_bindings(&alts[0], &mut leaves);
                        for name in leaves {
                            if bound.contains(&name) {
                                return Err(RlError::at(
                                    alts[0].tag_off,
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

        if !has_bare_wildcard {
            let mut position_tags: Vec<Vec<String>> = vec![Vec::new(); arity];
            let mut covered: Vec<Vec<Option<Vec<String>>>> = Vec::new();
            for arm in &expr.arms {
                let TuplePattern::Elems(elems) = &arm.pattern else {
                    continue;
                };
                let mut row: Vec<Option<Vec<String>>> = Vec::with_capacity(arity);
                for (p, elem) in elems.iter().enumerate() {
                    match elem {
                        Pattern::Wildcard => row.push(None),
                        Pattern::Tags(alts) => {
                            let tags: Vec<String> = alts.iter().map(|t| t.tag.clone()).collect();
                            for tag in &tags {
                                if !position_tags[p].contains(tag) {
                                    position_tags[p].push(tag.clone());
                                }
                            }
                            row.push(Some(tags));
                        }
                    }
                }
                // Like a guard, a nested pattern may mismatch — the arm
                // then covers nothing.
                let nested = elems
                    .iter()
                    .any(|e| matches!(e, Pattern::Tags(alts) if alts.iter().any(has_nested)));
                if arm.guard.is_none() && !nested {
                    covered.push(row);
                }
            }
            self.tuple_checks.push(TupleMatchCheck {
                offset: expr.keyword_off,
                position_tags,
                covered,
            });
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

    /// Resolves the deferred exhaustiveness checks against the collected
    /// enum registry, the imported declarations, and the built-in enums
    /// (`Option`, `Result`), in that order — so on a tie the nearer origin
    /// wins. A local enum shadows an imported or built-in enum of the same
    /// name entirely; an imported enum likewise shadows a built-in.
    fn check_exhaustiveness(&self) -> Result<(), RlError> {
        // The candidate table does not depend on the match being checked, so
        // it is built once for the file rather than once per match — with
        // many enums and many matches the per-pair rebuild was the dominant
        // cost of this phase.
        let candidates = self.candidate_enums();
        for check in &self.match_checks {
            // candidate with fewest missing cases: (name, origin, missing)
            let mut best: Option<(&str, Origin, Vec<&str>)> = None;
            let mut satisfied = false;
            for (name, origin, cases) in candidates.iter().map(|(n, o, c)| (*n, *o, c)) {
                if !check.tags.iter().all(|t| cases.contains(&t.as_str())) {
                    continue; // not a candidate: some arm tag is not a case of this enum
                }
                // guarded arms identify the enum but do not cover its cases
                let missing: Vec<&str> = cases
                    .iter()
                    .filter(|c| !check.covered.iter().any(|t| t.as_str() == **c))
                    .copied()
                    .collect();
                if missing.is_empty() {
                    satisfied = true;
                    break;
                }
                if best
                    .as_ref()
                    .is_none_or(|(_, _, m)| missing.len() < m.len())
                {
                    best = Some((name, origin, missing));
                }
            }
            if let (false, Some((name, origin, missing))) = (satisfied, best) {
                let list = missing
                    .iter()
                    .map(|m| format!("\"{m}\""))
                    .collect::<Vec<_>>()
                    .join(", ");
                let described = match origin {
                    Origin::Local => format!("enum {name}"),
                    Origin::Builtin => format!("built-in enum {name}"),
                    Origin::Extern(Some(from)) => format!("enum {name} (imported from \"{from}\")"),
                    Origin::Extern(None) => format!("imported enum {name}"),
                };
                return Err(RlError::at(
                    check.offset,
                    format!(
                        "match on {described} is not exhaustive: missing {list} (add the missing arms or a final `_` arm)"
                    ),
                ));
            }
        }
        Ok(())
    }

    /// Resolves a tuple-match position's tag set to an enum: local >
    /// imported > built-in, the first whose cases contain every tag.
    fn resolve_enum<'c>(
        candidates: &'c [(&'c str, Origin<'c>, Vec<&'c str>)],
        tags: &[String],
    ) -> Option<(&'c str, &'c [&'c str])> {
        candidates
            .iter()
            .find(|(_, _, cases)| tags.iter().all(|t| cases.contains(&t.as_str())))
            .map(|(name, _, cases)| (*name, cases.as_slice()))
    }

    /// Every enum a match in this file could be over, in shadowing order:
    /// local declarations first, then imported ones, then the built-ins —
    /// each name appearing only once, so the nearer origin wins. Built once
    /// per file and shared by both exhaustiveness passes.
    fn candidate_enums(&self) -> Vec<(&str, Origin<'_>, Vec<&str>)> {
        let mut out: Vec<(&str, Origin<'_>, Vec<&str>)> = Vec::new();
        for (name, cases) in &self.enums {
            out.push((
                name.as_str(),
                Origin::Local,
                cases.iter().map(String::as_str).collect(),
            ));
        }
        for e in self.externs {
            if self.enums.contains_key(&e.name) {
                continue;
            }
            out.push((
                e.name.as_str(),
                Origin::Extern(e.from.as_deref()),
                e.tags.iter().map(String::as_str).collect(),
            ));
        }
        for (name, cases) in crate::stdlib::BUILTIN_ENUMS {
            if self.enums.contains_key(*name) || self.externs.iter().any(|e| e.name == *name) {
                continue;
            }
            out.push((name, Origin::Builtin, cases.to_vec()));
        }
        out
    }

    /// Walks the cartesian product of each tuple match's per-position case
    /// sets and reports uncovered combinations. A position where every arm
    /// has `_` is universal — it constrains nothing and shows as `_` in the
    /// message. A match with a tagged position that resolves to no known
    /// enum is not checked (same as single matches over unknown unions).
    fn check_tuple_exhaustiveness(&self) -> Result<(), RlError> {
        let candidates = self.candidate_enums();
        'checks: for check in &self.tuple_checks {
            let arity = check.position_tags.len();
            let mut names: Vec<&str> = Vec::with_capacity(arity);
            let mut cases: Vec<Vec<&str>> = Vec::with_capacity(arity);
            for tags in &check.position_tags {
                if tags.is_empty() {
                    names.push("_");
                    cases.push(Vec::new()); // universal position
                    continue;
                }
                match Self::resolve_enum(&candidates, tags) {
                    Some((name, cs)) => {
                        names.push(name);
                        cases.push(cs.to_vec());
                    }
                    None => continue 'checks,
                }
            }

            let tagged: Vec<usize> = (0..arity).filter(|&p| !cases[p].is_empty()).collect();
            if tagged.is_empty() {
                continue; // every position universal — nothing enumerable
            }

            let mut missing: Vec<String> = Vec::new();
            let mut idx = vec![0usize; tagged.len()];
            loop {
                let covered = check.covered.iter().any(|row| {
                    tagged.iter().enumerate().all(|(ti, &p)| match &row[p] {
                        None => true,
                        Some(tags) => tags.iter().any(|t| t == cases[p][idx[ti]]),
                    })
                });
                if !covered {
                    let mut parts = vec!["_"; arity];
                    for (ti, &p) in tagged.iter().enumerate() {
                        parts[p] = cases[p][idx[ti]];
                    }
                    missing.push(format!("({})", parts.join(", ")));
                }
                // odometer over the tagged positions
                let mut ti = tagged.len();
                loop {
                    if ti == 0 {
                        break;
                    }
                    ti -= 1;
                    idx[ti] += 1;
                    if idx[ti] < cases[tagged[ti]].len() {
                        break;
                    }
                    idx[ti] = 0;
                    if ti == 0 {
                        ti = usize::MAX;
                        break;
                    }
                }
                if ti == usize::MAX {
                    break;
                }
            }

            if !missing.is_empty() {
                let shown = if missing.len() > 4 {
                    format!(
                        "{}, … ({} combinations in total)",
                        missing[..3].join(", "),
                        missing.len()
                    )
                } else {
                    missing.join(", ")
                };
                return Err(RlError::at(
                    check.offset,
                    format!(
                        "match on ({}) is not exhaustive: missing {} (add the missing arms or a final `_` arm)",
                        names.join(", "),
                        shown
                    ),
                ));
            }
        }
        Ok(())
    }
}

/// Where an exhaustiveness candidate was declared, for error messages and
/// resolution order.
#[derive(Clone, Copy)]
enum Origin<'a> {
    Local,
    Extern(Option<&'a str>),
    Builtin,
}
