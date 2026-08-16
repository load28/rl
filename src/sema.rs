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
//! - `match`: the wildcard `_` arm is last; no duplicate arm tags.
//! - exhaustiveness: a wildcard-free match whose arm tags all belong to an
//!   enum declared in this file must cover every case of that enum. Matches
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
    /// Non-wildcard arm tags.
    tags: Vec<String>,
}

/// Checks a whole program; `verify` enables swc validation of field types.
pub(crate) fn check(program: &Program, verify: bool) -> Result<(), RlError> {
    let mut checker = Checker {
        verify,
        enums: BTreeMap::new(),
        match_checks: Vec::new(),
    };
    checker.visit_program(program)?;
    checker.check_exhaustiveness()
}

struct Checker {
    verify: bool,
    /// rl enums declared in this file: name → case tags.
    enums: BTreeMap<String, Vec<String>>,
    /// Wildcard-free matches to exhaustiveness-check after the walk.
    match_checks: Vec<MatchCheck>,
}

impl Checker {
    fn visit_program(&mut self, program: &Program) -> Result<(), RlError> {
        for segment in &program.segments {
            match segment {
                Segment::Verbatim(_) => {}
                Segment::Enum(decl) => self.check_enum(decl)?,
                Segment::Match(expr) => self.check_match(expr)?,
                Segment::Template(template) => {
                    for chunk in &template.chunks {
                        if let TemplateChunk::Interp(interp) = chunk {
                            self.visit_program(interp)?;
                        }
                    }
                }
            }
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

    fn check_match(&mut self, expr: &MatchExpr) -> Result<(), RlError> {
        let mut seen: Vec<&str> = Vec::new();
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
                Pattern::Tag { tag, .. } => {
                    if seen.contains(&tag.as_str()) {
                        return Err(RlError::at(
                            arm.pattern_off,
                            format!("match: duplicate arm \"{}\"", tag),
                        ));
                    }
                    seen.push(tag);
                }
            }
        }

        if !expr
            .arms
            .iter()
            .any(|a| matches!(a.pattern, Pattern::Wildcard))
        {
            self.match_checks.push(MatchCheck {
                offset: expr.keyword_off,
                tags: expr
                    .arms
                    .iter()
                    .filter_map(|a| match &a.pattern {
                        Pattern::Tag { tag, .. } => Some(tag.clone()),
                        Pattern::Wildcard => None,
                    })
                    .collect(),
            });
        }

        // children, in source order: scrutinee first, then arm bodies
        self.visit_program(&expr.scrutinee)?;
        for arm in &expr.arms {
            self.visit_program(&arm.body)?;
        }
        Ok(())
    }

    /// Resolves the deferred exhaustiveness checks against the collected
    /// enum registry.
    fn check_exhaustiveness(&self) -> Result<(), RlError> {
        for check in &self.match_checks {
            let mut best: Option<(&str, Vec<&str>)> = None; // candidate with fewest missing cases
            let mut satisfied = false;
            for (name, cases) in self.enums.iter() {
                if !check.tags.iter().all(|t| cases.contains(t)) {
                    continue; // not a candidate: some arm tag is not a case of this enum
                }
                let missing: Vec<&str> = cases
                    .iter()
                    .filter(|c| !check.tags.contains(c))
                    .map(String::as_str)
                    .collect();
                if missing.is_empty() {
                    satisfied = true;
                    break;
                }
                if best.as_ref().is_none_or(|(_, m)| missing.len() < m.len()) {
                    best = Some((name, missing));
                }
            }
            if let (false, Some((name, missing))) = (satisfied, best) {
                let list = missing
                    .iter()
                    .map(|m| format!("\"{m}\""))
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(RlError::at(
                    check.offset,
                    format!(
                        "match on enum {name} is not exhaustive: missing {list} (add the missing arms or a final `_` arm)"
                    ),
                ));
            }
        }
        Ok(())
    }
}
