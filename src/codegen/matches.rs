//! rl `match` emission.
//!
//! A guard-free match becomes a `switch` IIFE discriminating on the `kind`
//! field. A match with at least one guarded arm becomes an if-chain IIFE
//! instead — switch fallthrough cannot express "guard failed, try the next
//! arm". The if-chain reproduces the switch's semantics exactly: expression
//! bodies `return`, block bodies exit via `break $rl_b` out of a labeled
//! block (the switch `break` equivalent, so a block that always returns
//! doesn't widen the match's type with `undefined`), and a missing `_` arm
//! gets the same fail-fast runtime guard.
//!
//! If the scrutinee, a guard, or any arm body contains `await`, the IIFE
//! becomes `(await (async () => { ... })())` so the surrounding expression
//! keeps its value semantics.

use super::Emitter;
use crate::ast::{Arm, Binding, MatchExpr, Pattern, TagPattern, TupleMatchExpr, TuplePattern};
use crate::scanner::contains_await;

/// True when the alternative carries a nested pattern — the arm then needs
/// per-path conditions, which only the if-chain form can express.
fn alt_has_nested(alt: &TagPattern) -> bool {
    alt.bindings
        .as_deref()
        .unwrap_or_default()
        .iter()
        .any(|b| b.nested.is_some())
}

fn arm_has_nested(arm: &Arm) -> bool {
    matches!(&arm.pattern, Pattern::Tags(alts) if alts.iter().any(alt_has_nested))
}

pub(super) fn emit_match(e: &Emitter, expr: &MatchExpr) -> String {
    let scrutinee = e.emit_program(&expr.scrutinee);
    let is_async = contains_await(e.bytes, expr.scrutinee_span.start, expr.scrutinee_span.end)
        || expr.arms.iter().any(|a| {
            contains_await(e.bytes, a.body_span.start, a.body_span.end)
                || a.guard
                    .as_ref()
                    .is_some_and(|g| contains_await(e.bytes, g.span.start, g.span.end))
        });

    let inner = if expr
        .arms
        .iter()
        .any(|a| a.guard.is_some() || arm_has_nested(a))
    {
        emit_if_chain(e, expr)
    } else {
        emit_switch(e, expr)
    };

    let f = if is_async { "async () => {" } else { "() => {" };
    let body = format!("({}\n  const $rl_m = ({});\n{}}})()", f, scrutinee, inner);

    if is_async {
        format!("(await {})", body)
    } else {
        format!("({})", body)
    }
}

/// The shared destructuring statement of an arm, or `""` when it binds
/// nothing. sema guarantees every or-pattern alternative binds the same
/// set, so the first alternative speaks for all of them.
fn bind_str(bindings: &Option<Vec<Binding>>) -> String {
    bind_str_from(bindings, "$rl_m")
}

/// [`bind_str`] against an explicit temporary (tuple matches destructure
/// each scrutinee separately: `$rl_m0`, `$rl_m1`, ...).
fn bind_str_from(bindings: &Option<Vec<Binding>>, var: &str) -> String {
    match bindings {
        Some(bindings) if !bindings.is_empty() => {
            let parts = bindings
                .iter()
                .map(|b| match &b.alias {
                    Some(alias) => format!("{}: {}", b.name, alias),
                    None => b.name.clone(),
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("const {{ {} }} = {var}; ", parts)
        }
        _ => String::new(),
    }
}

/// The if-chain condition and destructuring statements of one alternative,
/// nested patterns included: each nested level adds a `<path>.kind` test to
/// the condition and destructures its plain bindings from that path. tsc
/// narrows every path through the condition chain, so the destructurings
/// need no type tricks. For an alternative without nested patterns this is
/// exactly the old `kind === "Tag"` + [`bind_str`] pair.
pub(super) fn pattern_conds_binds(alt: &TagPattern, root: &str) -> (String, String) {
    let mut conds = vec![format!("{root}.kind === \"{}\"", alt.tag)];
    let mut binds = String::new();
    collect_conds_binds(
        alt.bindings.as_deref().unwrap_or_default(),
        root,
        &mut conds,
        &mut binds,
    );
    (conds.join(" && "), binds)
}

fn collect_conds_binds(
    bindings: &[Binding],
    root: &str,
    conds: &mut Vec<String>,
    binds: &mut String,
) {
    let plain: Vec<String> = bindings
        .iter()
        .filter(|b| b.nested.is_none())
        .map(|b| match &b.alias {
            Some(alias) => format!("{}: {}", b.name, alias),
            None => b.name.clone(),
        })
        .collect();
    if !plain.is_empty() {
        binds.push_str(&format!("const {{ {} }} = {root}; ", plain.join(", ")));
    }
    for b in bindings {
        if let Some(inner) = &b.nested {
            let path = format!("{root}.{}", b.name);
            conds.push(format!("{path}.kind === \"{}\"", inner.tag));
            collect_conds_binds(
                inner.bindings.as_deref().unwrap_or_default(),
                &path,
                conds,
                binds,
            );
        }
    }
}

/// An expression body's text plus the newline that rescues a trailing line
/// comment from swallowing the closing paren.
fn expr_body(e: &Emitter, arm: &Arm, indent: &str) -> (String, &'static str) {
    expr_body_text(e, &arm.body, indent)
}

/// See [`expr_body`] — shared with tuple arms.
fn expr_body_text(e: &Emitter, body: &crate::ast::Program, indent: &str) -> (String, &'static str) {
    let body = e.emit_program(body);
    let body = body.trim().to_string();
    let nl = if body.rsplit('\n').next().unwrap_or("").contains("//") {
        match indent {
            "    " => "\n    ",
            _ => "\n  ",
        }
    } else {
        ""
    };
    (body, nl)
}

fn emit_switch(e: &Emitter, expr: &MatchExpr) -> String {
    let mut cases = String::new();
    let mut has_wildcard = false;
    for arm in &expr.arms {
        let label = match &arm.pattern {
            Pattern::Wildcard => {
                has_wildcard = true;
                "default".to_string()
            }
            // or-pattern alternatives share one body via switch fallthrough
            Pattern::Tags(alts) => alts
                .iter()
                .map(|t| format!("case \"{}\"", t.tag))
                .collect::<Vec<_>>()
                .join(": "),
        };

        let bind = match &arm.pattern {
            Pattern::Tags(alts) => bind_str(&alts[0].bindings),
            Pattern::Wildcard => String::new(),
        };

        if arm.block {
            let body = e.emit_program(&arm.body);
            let body = body.trim();
            // `break` (not `return`) so an arm whose block always returns doesn't
            // widen the match's type with `undefined`; if the block doesn't return,
            // the arm evaluates to undefined, which the inferred type then reflects.
            cases.push_str(&format!(
                "    {}: {{ {}{}\n      break; }}\n",
                label, bind, body
            ));
        } else {
            let (body, nl) = expr_body(e, arm, "    ");
            cases.push_str(&format!(
                "    {}: {{ {}return ({}{}); }}\n",
                label, bind, body, nl
            ));
        }
    }

    if !has_wildcard {
        cases.push_str(
            "    default: { throw new Error(\"rl match: unexpected case \" + JSON.stringify($rl_m)); }\n",
        );
    }

    format!("  switch ($rl_m.kind) {{\n{}  }}\n", cases)
}

/// Tuple match emission: each scrutinee is evaluated once into its own
/// temporary (`$rl_m0`, `$rl_m1`, ...), then an if-chain tests the joint
/// `kind` conditions arm by arm — the same shape as a guarded single match,
/// which is what "guard failed / tuple mismatched, try the next arm" needs.
/// tsc narrows each temporary through its own `kind` comparison, so the
/// per-element destructurings need no type tricks.
pub(super) fn emit_tuple_match(e: &Emitter, expr: &TupleMatchExpr) -> String {
    let is_async = expr
        .scrutinees
        .iter()
        .any(|(span, _)| contains_await(e.bytes, span.start, span.end))
        || expr.arms.iter().any(|a| {
            contains_await(e.bytes, a.body_span.start, a.body_span.end)
                || a.guard
                    .as_ref()
                    .is_some_and(|g| contains_await(e.bytes, g.span.start, g.span.end))
        });

    let needs_label = expr.arms.iter().any(|a| a.block);
    let mut inner = String::new();
    if needs_label {
        inner.push_str("  $rl_b: {\n");
    }

    let mut unconditional = false;
    for arm in &expr.arms {
        let exit = if arm.block {
            let body = e.emit_program(&arm.body);
            let body = body.trim().to_string();
            format!("{{ {}\n    break $rl_b; }}", body)
        } else {
            let (body, nl) = expr_body_text(e, &arm.body, "  ");
            format!("return ({}{});", body, nl)
        };

        let exit = match &arm.guard {
            Some(guard) => {
                let g = e.emit_program(&guard.expr);
                let g = g.trim().to_string();
                let g_nl = if g.rsplit('\n').next().unwrap_or("").contains("//") {
                    "\n  "
                } else {
                    ""
                };
                format!("if (({}{})) {}", g, g_nl, exit)
            }
            None => exit,
        };

        match &arm.pattern {
            TuplePattern::Wildcard => {
                unconditional = true; // a bare `_` arm never has a guard
                inner.push_str(&format!("  {}\n", exit));
            }
            TuplePattern::Elems(elems) => {
                let mut conds: Vec<String> = Vec::new();
                let mut bind = String::new();
                for (i, elem) in elems.iter().enumerate() {
                    let var = format!("$rl_m{i}");
                    if let Pattern::Tags(alts) = elem {
                        if alts.len() == 1 {
                            let (cond, b) = pattern_conds_binds(&alts[0], &var);
                            conds.push(cond);
                            bind.push_str(&b);
                        } else {
                            // or-pattern element: shared bindings, never
                            // nested (sema)
                            let cond = alts
                                .iter()
                                .map(|t| format!("{var}.kind === \"{}\"", t.tag))
                                .collect::<Vec<_>>()
                                .join(" || ");
                            conds.push(format!("({cond})"));
                            bind.push_str(&bind_str_from(&alts[0].bindings, &var));
                        }
                    }
                }
                if conds.is_empty() {
                    // `(_, _)` — unconditionally selected (modulo a guard)
                    if arm.guard.is_none() {
                        unconditional = true;
                    }
                    inner.push_str(&format!("  {}\n", exit));
                } else {
                    inner.push_str(&format!(
                        "  if ({}) {{ {}{} }}\n",
                        conds.join(" && "),
                        bind,
                        exit
                    ));
                }
            }
        }
    }

    if !unconditional {
        let list = (0..expr.scrutinees.len())
            .map(|i| format!("$rl_m{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        inner.push_str(&format!(
            "  throw new Error(\"rl match: unexpected case \" + JSON.stringify([{list}]));\n"
        ));
    }
    if needs_label {
        inner.push_str("  }\n");
    }

    let mut header = String::new();
    for (i, (_, scrutinee)) in expr.scrutinees.iter().enumerate() {
        header.push_str(&format!(
            "\n  const $rl_m{i} = ({});",
            e.emit_program(scrutinee).trim()
        ));
    }

    let f = if is_async { "async () => {" } else { "() => {" };
    let body = format!("({}{}\n{}}})()", f, header, inner);
    if is_async {
        format!("(await {})", body)
    } else {
        format!("({})", body)
    }
}

fn emit_if_chain(e: &Emitter, expr: &MatchExpr) -> String {
    // `break $rl_b` is only needed by block bodies; skip the label otherwise
    let needs_label = expr.arms.iter().any(|a| a.block);
    let mut out = String::new();
    if needs_label {
        out.push_str("  $rl_b: {\n");
    }

    let mut has_wildcard = false;
    for arm in &expr.arms {
        // how the selected arm produces the match's value and stops the chain
        let exit = if arm.block {
            let body = e.emit_program(&arm.body);
            let body = body.trim().to_string();
            format!("{{ {}\n    break $rl_b; }}", body)
        } else {
            let (body, nl) = expr_body(e, arm, "  ");
            format!("return ({}{});", body, nl)
        };

        let exit = match &arm.guard {
            Some(guard) => {
                let g = e.emit_program(&guard.expr);
                let g = g.trim().to_string();
                let g_nl = if g.rsplit('\n').next().unwrap_or("").contains("//") {
                    "\n  "
                } else {
                    ""
                };
                format!("if (({}{})) {}", g, g_nl, exit)
            }
            None => exit,
        };

        match &arm.pattern {
            Pattern::Tags(alts) => {
                let (cond, bind) = if alts.len() == 1 {
                    pattern_conds_binds(&alts[0], "$rl_m")
                } else {
                    // or-pattern: shared bindings, never nested (sema)
                    let cond = alts
                        .iter()
                        .map(|t| format!("$rl_m.kind === \"{}\"", t.tag))
                        .collect::<Vec<_>>()
                        .join(" || ");
                    (cond, bind_str(&alts[0].bindings))
                };
                out.push_str(&format!("  if ({}) {{ {}{} }}\n", cond, bind, exit));
            }
            Pattern::Wildcard => {
                has_wildcard = true;
                out.push_str(&format!("  {}\n", exit));
            }
        }
    }

    if !has_wildcard {
        out.push_str(
            "  throw new Error(\"rl match: unexpected case \" + JSON.stringify($rl_m));\n",
        );
    }
    if needs_label {
        out.push_str("  }\n");
    }
    out
}
