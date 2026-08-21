//! rl `match` emission.
//!
//! A guard-free match becomes a `switch` IIFE discriminating on the `kind`
//! field — or on the scrutinee value itself when the arms are literal
//! patterns, which is the only difference between the two match kinds
//! (sema guarantees they never mix).
//! A match with at least one guarded arm becomes an if-chain IIFE
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
use super::rope::Rope;
use crate::ast::{
    Arm, Binding, LiteralPattern, MatchExpr, Pattern, TagPattern, TupleMatchExpr, TuplePattern,
};
use crate::scanner::contains_await;
use std::collections::HashSet;

fn binding_name(binding: &Binding) -> &str {
    binding.alias.as_deref().unwrap_or(&binding.name)
}

struct BindingRecovery {
    available: HashSet<String>,
    emitted: HashSet<String>,
    discard_sequence: usize,
}

impl BindingRecovery {
    fn from_bindings(bindings: &[&Binding]) -> Self {
        let mut available = HashSet::new();
        for binding in bindings {
            Self::collect_names(binding, &mut available);
        }
        Self {
            available,
            emitted: HashSet::new(),
            discard_sequence: 0,
        }
    }

    fn collect_names(binding: &Binding, names: &mut HashSet<String>) {
        if let Some(nested) = &binding.nested {
            for inner in nested.bindings.as_deref().unwrap_or_default() {
                Self::collect_names(inner, names);
            }
        } else {
            names.insert(binding_name(binding).to_owned());
        }
    }

    fn replacement(&mut self, binding: &Binding) -> Option<String> {
        let name = binding_name(binding);
        if self.emitted.insert(name.to_owned()) {
            return None;
        }
        loop {
            let candidate = format!("$rl_discard{}", self.discard_sequence);
            self.discard_sequence += 1;
            if self.available.insert(candidate.clone()) {
                return Some(candidate);
            }
        }
    }
}

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

/// True when the match discriminates on the scrutinee value rather than on
/// its `kind` field. sema rejects mixing, so one literal arm settles it; a
/// match whose only arm is `_` keeps the tag shape it always had.
fn is_literal_match(arms: &[Arm]) -> bool {
    arms.iter()
        .any(|a| matches!(a.pattern, Pattern::Literals(_)))
}

/// The scrutinee text a literal match's runtime guard reports.
fn unexpected(literal: bool) -> &'static str {
    if literal {
        "    default: { throw new Error(\"rl match: unexpected literal \" + JSON.stringify($rl_m)); }\n"
    } else {
        "    default: { throw new Error(\"rl match: unexpected case \" + JSON.stringify($rl_m)); }\n"
    }
}

pub(super) fn emit_match<'a>(e: &Emitter<'a>, expr: &MatchExpr) -> Rope<'a> {
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
    let mut body = Rope::new();
    if is_async {
        body.push_lit("(await (");
    } else {
        body.push_lit("((");
    }
    body.push_lit(format!("{f}\n  const "));
    // The temporary's name is where the scrutinee's *type* can be asked
    // about — see [`rlc::ScrutineeTemp`].
    body.push_mark(expr.keyword_off);
    body.push_lit("$rl_m = (");
    body.append(scrutinee);
    body.push_lit(");\n");
    body.append(inner);
    body.push_lit("})())");
    body
}

/// The `name`/`name: alias` list inside a destructuring's braces, every
/// name copied from the source (not rebuilt) so the emitted binding maps
/// back to the pattern the user wrote — which is what lets the editor
/// answer hover, go-to-definition and rename on a pattern binding.
pub(super) fn binding_list<'a, 'b>(
    e: &Emitter<'a>,
    bindings: impl Iterator<Item = &'b Binding>,
) -> Rope<'a> {
    let bindings: Vec<&Binding> = bindings.collect();
    let mut recovery = BindingRecovery::from_bindings(&bindings);
    binding_list_with_recovery(e, bindings.into_iter(), &mut recovery)
}

fn binding_list_with_recovery<'a, 'b>(
    e: &Emitter<'a>,
    bindings: impl Iterator<Item = &'b Binding>,
    recovery: &mut BindingRecovery,
) -> Rope<'a> {
    let mut rope = Rope::new();
    for (i, b) in bindings.enumerate() {
        if i > 0 {
            rope.push_lit(", ");
        }
        let (name, at) = e.src_slice(b.name_span.start, b.name_span.end);
        rope.push_src(name, at);
        if let Some(replacement) = recovery.replacement(b) {
            rope.push_lit(": ");
            rope.push_lit(replacement);
        } else if let Some(span) = b.alias_span {
            rope.push_lit(": ");
            let (alias, at) = e.src_slice(span.start, span.end);
            rope.push_src(alias, at);
        }
    }
    rope
}

/// [`binding_list`]'s unmapped twin: the same bytes written by the
/// compiler instead of copied. Used for an or-pattern, whose one
/// destructuring stands for every alternative — sema guarantees they bind
/// the same set, so the bytes are the same either way, but they come from
/// several patterns at once and belong to none of them. Claiming one would
/// point the editor at an arbitrary alternative and let a rename rewrite
/// that one alone.
pub(super) fn binding_list_lit(bindings: &[Binding]) -> String {
    let refs: Vec<&Binding> = bindings.iter().collect();
    let mut recovery = BindingRecovery::from_bindings(&refs);
    bindings
        .iter()
        .map(|b| {
            if let Some(replacement) = recovery.replacement(b) {
                format!("{}: {replacement}", b.name)
            } else {
                match &b.alias {
                    Some(alias) => format!("{}: {}", b.name, alias),
                    None => b.name.clone(),
                }
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The shared destructuring statement of an arm, or an empty rope when it
/// binds nothing. sema guarantees every or-pattern alternative binds the
/// same set, so the first alternative speaks for all of them — and
/// `mapped` says whether it speaks for itself alone (see
/// [`binding_list_lit`]).
fn bind_rope<'a>(e: &Emitter<'a>, bindings: &Option<Vec<Binding>>, mapped: bool) -> Rope<'a> {
    bind_rope_from(e, bindings, "$rl_m", mapped)
}

/// [`bind_rope`] against an explicit temporary (tuple matches destructure
/// each scrutinee separately: `$rl_m0`, `$rl_m1`, ...).
fn bind_rope_from<'a>(
    e: &Emitter<'a>,
    bindings: &Option<Vec<Binding>>,
    var: &str,
    mapped: bool,
) -> Rope<'a> {
    let refs: Vec<&Binding> = bindings.as_deref().unwrap_or_default().iter().collect();
    let mut recovery = BindingRecovery::from_bindings(&refs);
    bind_rope_from_with_recovery(e, bindings, var, mapped, &mut recovery)
}

fn bind_rope_from_with_recovery<'a>(
    e: &Emitter<'a>,
    bindings: &Option<Vec<Binding>>,
    var: &str,
    mapped: bool,
    recovery: &mut BindingRecovery,
) -> Rope<'a> {
    let mut rope = Rope::new();
    match bindings {
        Some(bindings) if !bindings.is_empty() => {
            rope.push_lit("const { ");
            if mapped {
                rope.append(binding_list_with_recovery(e, bindings.iter(), recovery));
            } else {
                let rendered = bindings
                    .iter()
                    .map(|binding| match recovery.replacement(binding) {
                        Some(replacement) => format!("{}: {replacement}", binding.name),
                        None => match &binding.alias {
                            Some(alias) => format!("{}: {alias}", binding.name),
                            None => binding.name.clone(),
                        },
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                rope.push_lit(rendered);
            }
            rope.push_lit(format!(" }} = {var}; "));
        }
        _ => {}
    }
    rope
}

/// The if-chain condition and destructuring statements of one alternative,
/// nested patterns included: each nested level adds a `<path>.kind` test to
/// the condition and destructures its plain bindings from that path. tsc
/// narrows every path through the condition chain, so the destructurings
/// need no type tricks. For an alternative without nested patterns this is
/// exactly the old `kind === "Tag"` + [`bind_rope`] pair.
pub(super) fn pattern_conds_binds<'a>(
    e: &Emitter<'a>,
    alt: &TagPattern,
    root: &str,
) -> (Rope<'a>, Rope<'a>) {
    let binding_refs: Vec<&Binding> = alt.bindings.as_deref().unwrap_or_default().iter().collect();
    let mut recovery = BindingRecovery::from_bindings(&binding_refs);
    pattern_conds_binds_with_recovery(e, alt, root, &mut recovery)
}

fn pattern_conds_binds_with_recovery<'a>(
    e: &Emitter<'a>,
    alt: &TagPattern,
    root: &str,
    recovery: &mut BindingRecovery,
) -> (Rope<'a>, Rope<'a>) {
    let mut conds: Vec<(String, Option<(usize, usize)>)> =
        vec![(format!("{root}.kind === \"{}\"", alt.tag), None)];
    let mut binds = Rope::new();
    collect_conds_binds(
        e,
        alt.bindings.as_deref().unwrap_or_default(),
        root,
        &mut conds,
        &mut binds,
        recovery,
    );
    // The conditions are glue, but each nested link *starts* with the
    // receiver expression that link tests — the one place a checker can be
    // asked what that payload admits. A zero-length mark before it records
    // where ([`crate::PayloadTemp`]); the text is unchanged.
    let mut cond = Rope::new();
    for (index, (text, payload)) in conds.into_iter().enumerate() {
        if index > 0 {
            cond.push_lit(" && ");
        }
        match payload {
            // The mark sits on the *field name* of the receiver
            // (`$rl_m.inner`), not at its start: a position at `$rl_m`
            // names the scrutinee's own type, and the question here is
            // about the payload.
            Some((at, src)) => {
                cond.push_lit(text[..at].to_string());
                cond.push_payload_mark(src);
                cond.push_lit(text[at..].to_string());
            }
            None => cond.push_lit(text),
        }
    }
    (cond, binds)
}

/// The condition and shared destructuring of a multi-alternative pattern
/// (an `if let` or-pattern): a disjunction of `kind` tests, and the first
/// alternative's bindings written unmapped ([`binding_list_lit`]) — the
/// one destructuring stands for every alternative at once, so it maps to
/// none of them, the same rule as a match or-arm's.
pub(super) fn or_conds_binds<'a>(alts: &[TagPattern], root: &str) -> (Rope<'a>, Rope<'a>) {
    let mut cond = Rope::new();
    cond.push_lit(
        alts.iter()
            .map(|alt| format!("{root}.kind === \"{}\"", alt.tag))
            .collect::<Vec<_>>()
            .join(" || "),
    );
    let mut binds = Rope::new();
    let bindings = alts[0].bindings.as_deref().unwrap_or_default();
    if !bindings.is_empty() {
        binds.push_lit(format!(
            "const {{ {} }} = {root}; ",
            binding_list_lit(bindings)
        ));
    }
    (cond, binds)
}

fn collect_conds_binds<'a>(
    e: &Emitter<'a>,
    bindings: &[Binding],
    root: &str,
    conds: &mut Vec<(String, Option<(usize, usize)>)>,
    binds: &mut Rope<'a>,
    recovery: &mut BindingRecovery,
) {
    let mut plain = bindings.iter().filter(|b| b.nested.is_none()).peekable();
    if plain.peek().is_some() {
        binds.push_lit("const { ");
        binds.append(binding_list_with_recovery(e, plain, recovery));
        binds.push_lit(format!(" }} = {root}; "));
    }
    for b in bindings {
        if let Some(inner) = &b.nested {
            let path = format!("{root}.{}", b.name);
            conds.push((
                format!("{path}.kind === \"{}\"", inner.tag),
                // The field name starts just past `root.`.
                Some((root.len() + 1, inner.tag_off)),
            ));
            collect_conds_binds(
                e,
                inner.bindings.as_deref().unwrap_or_default(),
                &path,
                conds,
                binds,
                recovery,
            );
        }
    }
}

/// An expression body's rope plus the newline that rescues a trailing line
/// comment from swallowing the closing paren.
fn expr_body<'a>(e: &Emitter<'a>, arm: &Arm, indent: &str) -> (Rope<'a>, &'static str) {
    expr_body_text(e, &arm.body, indent)
}

/// See [`expr_body`] — shared with tuple arms.
fn expr_body_text<'a>(
    e: &Emitter<'a>,
    body: &crate::ast::Program,
    indent: &str,
) -> (Rope<'a>, &'static str) {
    let body = e.emit_program(body).trim();
    let nl = if body.last_line_has_line_comment() {
        match indent {
            "    " => "\n    ",
            _ => "\n  ",
        }
    } else {
        ""
    };
    (body, nl)
}

fn emit_switch<'a>(e: &Emitter<'a>, expr: &MatchExpr) -> Rope<'a> {
    let literal = is_literal_match(&expr.arms);
    let mut cases = Rope::new();
    let mut has_wildcard = false;
    for arm in &expr.arms {
        // The case label, its alternatives sharing one body via switch
        // fallthrough. A literal's own bytes are copied from the source, so
        // `0xff` stays `0xff` — and a tsc diagnostic over the case label
        // maps back to the literal the user wrote.
        let mut label = Rope::new();
        label.push_lit("    ");
        match &arm.pattern {
            Pattern::Wildcard => {
                has_wildcard = true;
                label.push_lit("default");
            }
            Pattern::Tags(alts) => label.push_lit(
                alts.iter()
                    .map(|t| format!("case \"{}\"", t.tag))
                    .collect::<Vec<_>>()
                    .join(": "),
            ),
            Pattern::Literals(alts) => {
                for (i, alt) in alts.iter().enumerate() {
                    label.push_lit(if i == 0 { "case " } else { ": case " });
                    label.append(literal_text(e, alt));
                }
            }
        }

        let bind = match &arm.pattern {
            Pattern::Tags(alts) => bind_rope(e, &alts[0].bindings, alts.len() == 1),
            Pattern::Wildcard | Pattern::Literals(_) => Rope::new(),
        };

        cases.append(label);
        cases.push_lit(": { ");
        cases.append(bind);
        if arm.block {
            let body = e.emit_program(&arm.body).trim();
            // `break` (not `return`) so an arm whose block always returns doesn't
            // widen the match's type with `undefined`; if the block doesn't return,
            // the arm evaluates to undefined, which the inferred type then reflects.
            cases.append(body);
            cases.push_lit("\n      break; }\n");
        } else {
            let (body, nl) = expr_body(e, arm, "    ");
            cases.push_lit("return (");
            cases.append(body);
            cases.push_lit(format!("{nl}); }}\n"));
        }
    }

    if !has_wildcard {
        cases.push_lit(unexpected(literal));
    }

    let mut out = Rope::new();
    out.push_lit(if literal {
        "  switch ($rl_m) {\n"
    } else {
        "  switch ($rl_m.kind) {\n"
    });
    out.append(cases);
    out.push_lit("  }\n");
    out
}

/// A literal pattern's source bytes, kept as a mapped chunk.
fn literal_text<'a>(e: &Emitter<'a>, alt: &LiteralPattern) -> Rope<'a> {
    let (text, at) = e.src_slice(alt.span.start, alt.span.end);
    let mut rope = Rope::new();
    rope.push_src(text, at);
    rope
}

/// Tuple match emission: each scrutinee is evaluated once into its own
/// temporary (`$rl_m0`, `$rl_m1`, ...), then an if-chain tests the joint
/// `kind` conditions arm by arm — the same shape as a guarded single match,
/// which is what "guard failed / tuple mismatched, try the next arm" needs.
/// tsc narrows each temporary through its own `kind` comparison, so the
/// per-element destructurings need no type tricks.
pub(super) fn emit_tuple_match<'a>(e: &Emitter<'a>, expr: &TupleMatchExpr) -> Rope<'a> {
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
    let mut inner = Rope::new();
    if needs_label {
        inner.push_lit("  $rl_b: {\n");
    }

    let mut unconditional = false;
    for arm in &expr.arms {
        let exit = arm_exit(e, &arm.body, arm.block, &arm.guard, "  ");

        match &arm.pattern {
            TuplePattern::Wildcard => {
                unconditional = true; // a bare `_` arm never has a guard
                inner.push_lit("  ");
                inner.append(exit);
                inner.push_lit("\n");
            }
            TuplePattern::Elems(elems) => {
                let binding_refs: Vec<&Binding> = elems
                    .iter()
                    .filter_map(|elem| match elem {
                        Pattern::Tags(alts) => alts.first(),
                        Pattern::Wildcard | Pattern::Literals(_) => None,
                    })
                    .flat_map(|alt| alt.bindings.as_deref().unwrap_or_default())
                    .collect();
                let mut recovery = BindingRecovery::from_bindings(&binding_refs);
                let mut conds: Vec<Rope> = Vec::new();
                let mut bind = Rope::new();
                for (i, elem) in elems.iter().enumerate() {
                    let var = format!("$rl_m{i}");
                    if let Pattern::Tags(alts) = elem {
                        if alts.len() == 1 {
                            let (cond, b) =
                                pattern_conds_binds_with_recovery(e, &alts[0], &var, &mut recovery);
                            conds.push(cond);
                            bind.append(b);
                        } else {
                            // or-pattern element: shared bindings, never
                            // nested (sema)
                            let cond = alts
                                .iter()
                                .map(|t| format!("{var}.kind === \"{}\"", t.tag))
                                .collect::<Vec<_>>()
                                .join(" || ");
                            let mut alt_cond = Rope::new();
                            alt_cond.push_lit(format!("({cond})"));
                            conds.push(alt_cond);
                            bind.append(bind_rope_from_with_recovery(
                                e,
                                &alts[0].bindings,
                                &var,
                                false,
                                &mut recovery,
                            ));
                        }
                    }
                }
                if conds.is_empty() {
                    // `(_, _)` — unconditionally selected (modulo a guard)
                    if arm.guard.is_none() {
                        unconditional = true;
                    }
                    inner.push_lit("  ");
                    inner.append(exit);
                    inner.push_lit("\n");
                } else {
                    inner.push_lit("  if (");
                    for (index, cond) in conds.into_iter().enumerate() {
                        if index > 0 {
                            inner.push_lit(" && ");
                        }
                        inner.append(cond);
                    }
                    inner.push_lit(") { ");
                    inner.append(bind);
                    inner.append(exit);
                    inner.push_lit(" }\n");
                }
            }
        }
    }

    if !unconditional {
        let list = (0..expr.scrutinees.len())
            .map(|i| format!("$rl_m{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        inner.push_lit(format!(
            "  throw new Error(\"rl match: unexpected case \" + JSON.stringify([{list}]));\n"
        ));
    }
    if needs_label {
        inner.push_lit("  }\n");
    }

    let mut header = Rope::new();
    for (i, (_, scrutinee)) in expr.scrutinees.iter().enumerate() {
        header.push_lit("\n  const ");
        // One mark per position, in order: a tuple match's positions are
        // told apart by where their temporaries landed, since they all
        // stand for the same `match` keyword.
        header.push_mark(expr.keyword_off);
        header.push_lit(format!("$rl_m{i} = ("));
        header.append(e.emit_program(scrutinee).trim());
        header.push_lit(");");
    }

    let f = if is_async { "async () => {" } else { "() => {" };
    let mut body = Rope::new();
    if is_async {
        body.push_lit("(await (");
    } else {
        body.push_lit("((");
    }
    body.push_lit(f);
    body.append(header);
    body.push_lit("\n");
    body.append(inner);
    body.push_lit("})())");
    body
}

/// How a selected arm produces the match's value and stops the chain —
/// shared by the if-chain and tuple emissions, guard wrapping included.
fn arm_exit<'a>(
    e: &Emitter<'a>,
    body: &crate::ast::Program,
    block: bool,
    guard: &Option<crate::ast::GuardExpr>,
    indent: &str,
) -> Rope<'a> {
    let mut exit = Rope::new();
    if block {
        let body = e.emit_program(body).trim();
        exit.push_lit("{ ");
        exit.append(body);
        exit.push_lit("\n    break $rl_b; }");
    } else {
        let (body, nl) = expr_body_text(e, body, indent);
        exit.push_lit("return (");
        exit.append(body);
        exit.push_lit(format!("{nl});"));
    }

    match guard {
        Some(guard) => {
            let g = e.emit_program(&guard.expr).trim();
            let g_nl = if g.last_line_has_line_comment() {
                "\n  "
            } else {
                ""
            };
            let mut wrapped = Rope::new();
            wrapped.push_lit("if ((");
            wrapped.append(g);
            wrapped.push_lit(format!("{g_nl})) "));
            wrapped.append(exit);
            wrapped
        }
        None => exit,
    }
}

fn emit_if_chain<'a>(e: &Emitter<'a>, expr: &MatchExpr) -> Rope<'a> {
    // `break $rl_b` is only needed by block bodies; skip the label otherwise
    let literal = is_literal_match(&expr.arms);
    let needs_label = expr.arms.iter().any(|a| a.block);
    let mut out = Rope::new();
    if needs_label {
        out.push_lit("  $rl_b: {\n");
    }

    let mut has_wildcard = false;
    for arm in &expr.arms {
        let exit = arm_exit(e, &arm.body, arm.block, &arm.guard, "  ");

        match &arm.pattern {
            Pattern::Tags(alts) => {
                let (cond, bind) = if alts.len() == 1 {
                    pattern_conds_binds(e, &alts[0], "$rl_m")
                } else {
                    // or-pattern: shared bindings, never nested (sema)
                    let mut cond = Rope::new();
                    cond.push_lit(
                        alts.iter()
                            .map(|t| format!("$rl_m.kind === \"{}\"", t.tag))
                            .collect::<Vec<_>>()
                            .join(" || "),
                    );
                    (cond, bind_rope(e, &alts[0].bindings, false))
                };
                out.push_lit("  if (");
                out.append(cond);
                out.push_lit(") { ");
                out.append(bind);
                out.append(exit);
                out.push_lit(" }\n");
            }
            Pattern::Literals(alts) => {
                // `===` per alternative — the if-chain's answer to switch
                // fallthrough, with the same strict comparison.
                let mut cond = Rope::new();
                for (i, alt) in alts.iter().enumerate() {
                    cond.push_lit(if i == 0 {
                        "$rl_m === "
                    } else {
                        " || $rl_m === "
                    });
                    cond.append(literal_text(e, alt));
                }
                out.push_lit("  if (");
                out.append(cond);
                out.push_lit(") { ");
                out.append(exit);
                out.push_lit(" }\n");
            }
            Pattern::Wildcard => {
                has_wildcard = true;
                out.push_lit("  ");
                out.append(exit);
                out.push_lit("\n");
            }
        }
    }

    if !has_wildcard {
        out.push_lit(if literal {
            "  throw new Error(\"rl match: unexpected literal \" + JSON.stringify($rl_m));\n"
        } else {
            "  throw new Error(\"rl match: unexpected case \" + JSON.stringify($rl_m));\n"
        });
    }
    if needs_label {
        out.push_lit("  }\n");
    }
    out
}
