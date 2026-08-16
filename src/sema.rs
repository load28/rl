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
//! - exhaustiveness: a wildcard-free match whose arm tags all belong to an
//!   enum declared in this file — or to a built-in enum (`Option`, `Result`;
//!   see [`crate::stdlib::BUILTIN_ENUMS`]) — must cover every case of that
//!   enum with unguarded arms (a guard may be false, so guarded arms
//!   identify the enum but cover nothing). A file-local enum shadows a built-in of the same name. Matches
//!   whose tags belong to no known enum (imported enums, hand-written
//!   unions) are not checked — rlc has no type information for them.

use std::collections::BTreeMap;

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

/// Checks a whole program; `verify` enables swc validation of field types.
pub(crate) fn check(program: &Program, verify: bool) -> Result<(), RlError> {
    let mut checker = Checker {
        verify,
        enums: BTreeMap::new(),
        match_checks: Vec::new(),
    };
    checker.visit_program(program, false)?;
    checker.check_exhaustiveness()
}

struct Checker {
    verify: bool,
    /// rl enums declared in this file: name → case tags.
    enums: BTreeMap<String, Vec<String>>,
    /// Wildcard-free matches to exhaustiveness-check after the walk.
    match_checks: Vec<MatchCheck>,
}

/// The (field, bound name) pairs a tag alternative destructures, sorted so
/// alternatives compare as sets. No parens and empty parens both bind nothing.
fn binding_set(bindings: &Option<Vec<Binding>>) -> Vec<(&str, &str)> {
    let mut set: Vec<(&str, &str)> = bindings
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|b| (b.name.as_str(), b.alias.as_deref().unwrap_or(&b.name)))
        .collect();
    set.sort_unstable();
    set
}

impl Checker {
    /// `nested` is true inside any recursively parsed sub-program (match
    /// scrutinee, arm body, template interpolation, try expression) — the
    /// contexts where a `try` statement is not allowed.
    fn visit_program(&mut self, program: &Program, nested: bool) -> Result<(), RlError> {
        for segment in &program.segments {
            match segment {
                Segment::Verbatim(_) => {}
                Segment::Enum(decl) => self.check_enum(decl)?,
                Segment::Match(expr) => self.check_match(expr)?,
                Segment::Try(stmt) => self.check_try(stmt, nested)?,
                Segment::Template(template) => {
                    for chunk in &template.chunks {
                        if let TemplateChunk::Interp(interp) = chunk {
                            self.visit_program(interp, true)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn check_try(&mut self, stmt: &TryStmt, nested: bool) -> Result<(), RlError> {
        if nested {
            return Err(RlError::at(
                stmt.keyword_off,
                "`try` cannot be used inside a match expression, a template interpolation, \
                 or another `try` — it compiles to a `return` from the enclosing function"
                    .to_string(),
            ));
        }
        self.visit_program(&stmt.expr, true)
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
                    // must bind the exact same (field, name) set.
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
                    if arm.guard.is_none() {
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
            let arm_tags = |guarded_too: bool| {
                expr.arms
                    .iter()
                    .filter(|a| guarded_too || a.guard.is_none())
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
                tags: arm_tags(true),
                covered: arm_tags(false),
            });
        }

        // children, in source order: scrutinee first, then guards and bodies
        self.visit_program(&expr.scrutinee, true)?;
        for arm in &expr.arms {
            if let Some(guard) = &arm.guard {
                self.visit_program(&guard.expr, true)?;
            }
            self.visit_program(&arm.body, true)?;
        }
        Ok(())
    }

    /// Resolves the deferred exhaustiveness checks against the collected
    /// enum registry plus the built-in enums (`Option`, `Result`). Local
    /// enums are tried first, so on a tie they win, and a local enum shadows
    /// a built-in of the same name entirely.
    fn check_exhaustiveness(&self) -> Result<(), RlError> {
        for check in &self.match_checks {
            // candidate with fewest missing cases: (name, is_builtin, missing)
            let mut best: Option<(&str, bool, Vec<&str>)> = None;
            let mut satisfied = false;
            let locals = self.enums.iter().map(|(name, cases)| {
                let cases: Vec<&str> = cases.iter().map(String::as_str).collect();
                (name.as_str(), false, cases)
            });
            let builtins = crate::stdlib::BUILTIN_ENUMS
                .iter()
                .filter(|(name, _)| !self.enums.contains_key(*name))
                .map(|(name, cases)| (*name, true, cases.to_vec()));
            for (name, builtin, cases) in locals.chain(builtins) {
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
                    best = Some((name, builtin, missing));
                }
            }
            if let (false, Some((name, builtin, missing))) = (satisfied, best) {
                let list = missing
                    .iter()
                    .map(|m| format!("\"{m}\""))
                    .collect::<Vec<_>>()
                    .join(", ");
                let qualifier = if builtin { "built-in " } else { "" };
                return Err(RlError::at(
                    check.offset,
                    format!(
                        "match on {qualifier}enum {name} is not exhaustive: missing {list} (add the missing arms or a final `_` arm)"
                    ),
                ));
            }
        }
        Ok(())
    }
}
