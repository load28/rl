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
use crate::ast::{Arm, Binding, MatchExpr, Pattern};
use crate::scanner::contains_await;

pub(super) fn emit_match(e: &Emitter, expr: &MatchExpr) -> String {
    let scrutinee = e.emit_program(&expr.scrutinee);
    let is_async = contains_await(e.bytes, expr.scrutinee_span.start, expr.scrutinee_span.end)
        || expr.arms.iter().any(|a| {
            contains_await(e.bytes, a.body_span.start, a.body_span.end)
                || a.guard
                    .as_ref()
                    .is_some_and(|g| contains_await(e.bytes, g.span.start, g.span.end))
        });

    let inner = if expr.arms.iter().any(|a| a.guard.is_some()) {
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
            format!("const {{ {} }} = $rl_m; ", parts)
        }
        _ => String::new(),
    }
}

/// An expression body's text plus the newline that rescues a trailing line
/// comment from swallowing the closing paren.
fn expr_body(e: &Emitter, arm: &Arm, indent: &str) -> (String, &'static str) {
    let body = e.emit_program(&arm.body);
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
                let cond = alts
                    .iter()
                    .map(|t| format!("$rl_m.kind === \"{}\"", t.tag))
                    .collect::<Vec<_>>()
                    .join(" || ");
                let bind = bind_str(&alts[0].bindings);
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
