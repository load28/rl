//! rl `match` emission: a `switch` IIFE discriminating on the `kind` field.
//!
//! If the scrutinee or any arm body contains `await`, the IIFE becomes
//! `(await (async () => { ... })())` so the surrounding expression keeps its
//! value semantics.

use super::Emitter;
use crate::ast::{MatchExpr, Pattern};
use crate::scanner::contains_await;

pub(super) fn emit_match(e: &Emitter, expr: &MatchExpr) -> String {
    let scrutinee = e.emit_program(&expr.scrutinee);
    let is_async = contains_await(e.bytes, expr.scrutinee_span.start, expr.scrutinee_span.end)
        || expr
            .arms
            .iter()
            .any(|a| contains_await(e.bytes, a.body_span.start, a.body_span.end));

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

        let mut bind = String::new();
        // sema guarantees every alternative binds the same set, so the
        // shared destructuring can come from the first alternative
        if let Pattern::Tags(alts) = &arm.pattern
            && let Some(bindings) = &alts[0].bindings
            && !bindings.is_empty()
        {
            let parts = bindings
                .iter()
                .map(|b| match &b.alias {
                    Some(alias) => format!("{}: {}", b.name, alias),
                    None => b.name.clone(),
                })
                .collect::<Vec<_>>()
                .join(", ");
            bind = format!("const {{ {} }} = $rl_m; ", parts);
        }

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
            let body = e.emit_program(&arm.body);
            let body = body.trim();
            // a trailing line comment would swallow the closing paren
            let nl = if body.rsplit('\n').next().unwrap_or("").contains("//") {
                "\n    "
            } else {
                ""
            };
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

    let f = if is_async { "async () => {" } else { "() => {" };
    let body = format!(
        "({}\n  const $rl_m = ({});\n  switch ($rl_m.kind) {{\n{}  }}\n}})()",
        f, scrutinee, cases
    );

    if is_async {
        format!("(await {})", body)
    } else {
        format!("({})", body)
    }
}
