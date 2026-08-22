//! TypeScript target lowering from validated Core IR.
//!
//! This module is intentionally independent of `ast`: source text enters
//! only through HIR nodes and the source map. Every rl surface reaches this
//! module through a shared Core primitive.

use std::cell::Cell;
use std::collections::HashSet;

use super::rope::{Flat, Rope};
use crate::analysis::SemanticFile;
use crate::core_ir::*;
use crate::hir::ids::Idx;
use crate::hir::{self, ArmBodyKind, BindingMode, ExprId, NodeId};
use crate::scanner::{at, ident_end, is_ident_start, scan_type_end, skip_ws_comments};
use crate::{AnchorKind, ImportRewrite};

pub(crate) fn emit_with_map<'a>(
    semantic: &'a SemanticFile,
    core: &'a CoreFile,
    source: &'a str,
    rewrite_imports: ImportRewrite,
    std_import: Option<&'a str>,
) -> Flat {
    let emitter = Emitter {
        semantic,
        core,
        source,
        rewrite_imports,
        std_import,
        used_pipe: Cell::new(false),
        used_flow: Cell::new(false),
    };
    let mut flat = emitter.emit_body(core.root).flatten();
    if emitter.used_pipe.get() {
        if !flat.code.ends_with('\n') {
            flat.code.push('\n');
        }
        flat.code
            .push_str("function $rl_ap<A, B>(v: A, f: (v: A) => B): B { return f(v); }\n");
    }
    if emitter.used_flow.get() {
        if !flat.code.ends_with('\n') {
            flat.code.push('\n');
        }
        flat.code.push_str(
            "function $rl_fl<A extends unknown[], B, C>(f: (...a: A) => B, g: (b: B) => C): (...a: A) => C { return (...a: A) => g(f(...a)); }\n",
        );
    }
    flat
}

struct Emitter<'a> {
    semantic: &'a SemanticFile,
    core: &'a CoreFile,
    source: &'a str,
    rewrite_imports: ImportRewrite,
    std_import: Option<&'a str>,
    used_pipe: Cell<bool>,
    used_flow: Cell<bool>,
}

impl<'a> Emitter<'a> {
    fn span(&self, node: NodeId) -> hir::Span {
        self.semantic
            .hir
            .source_map
            .node_span(node)
            .unwrap_or_else(|| panic!("internal compiler error: target node has no source span"))
    }

    fn source_node(&self, node: NodeId) -> (&'a str, usize) {
        let span = self.span(node);
        (&self.source[span.start..span.end], span.start)
    }

    fn source_span(&self, span: hir::Span) -> (&'a str, usize) {
        (&self.source[span.start..span.end], span.start)
    }

    fn source_rope(&self, node: NodeId) -> Rope<'a> {
        let (text, at) = self.source_node(node);
        let mut rope = Rope::new();
        rope.push_src(text, at);
        rope
    }

    fn emit_body(&self, body: hir::BodyId) -> Rope<'a> {
        let mut out = Rope::new();
        for statement in &self.core.bodies[body.index()].statements {
            match statement {
                Statement::Opaque(node) => out.append(self.source_rope(*node)),
                Statement::Adt(adt) => out.push_lit(emit_adt_text(adt)),
                Statement::Import(import) => self.emit_import(import, &mut out),
                Statement::Propagate(propagate) => {
                    let span = self.span(propagate.node);
                    out.anchored(
                        AnchorKind::Try,
                        span.start,
                        span.end,
                        span.end,
                        self.emit_propagate(propagate),
                    );
                }
                Statement::Decision(decision) => self.emit_statement_decision(decision, &mut out),
                Statement::Expr(expr) => out.append(self.emit_expr(*expr)),
            }
        }
        out
    }

    fn emit_expr(&self, expr: ExprId) -> Rope<'a> {
        match &self.core.exprs[expr.index()] {
            Expr::Opaque(node) => self.source_rope(*node),
            Expr::Sequence(body) => self.emit_body(*body),
            Expr::Decision(decision) => {
                let head = self.span(decision.head);
                let extent = self.span(decision.extent);
                let inner = self.emit_value_decision(decision);
                let mut out = Rope::new();
                out.anchored(AnchorKind::Match, head.start, head.end, extent.end, inner);
                out
            }
            Expr::Apply(apply) => self.emit_apply(apply),
            Expr::ResultRegion(region) => self.emit_result_region(region),
            Expr::Template(template) => self.emit_template(template),
        }
    }

    fn emit_propagate(&self, propagate: &Propagate) -> Rope<'a> {
        let temp = temp_name(propagate.temporary);
        let mut out = Rope::new();
        out.push_lit(format!("const {temp} = ("));
        out.append(self.emit_expr(propagate.value).trim());
        out.push_lit(format!(
            "); if ({temp}.{} !== \"{}\") return {temp};",
            propagate.layout.discriminant_field, propagate.layout.success_tag
        ));
        if let Some(binding) = propagate.binding {
            out.push_lit(format!(" {} ", binding_keyword(binding.mode)));
            out.append(self.source_rope(binding.node));
            out.push_lit(format!(" = {temp}.{};", propagate.layout.payload_field));
        }
        out
    }

    fn emit_result_region(&self, region: &ResultRegion) -> Rope<'a> {
        let mut out = Rope::new();
        out.push_lit(if region.is_async {
            "(await (async () => {"
        } else {
            "((() => {"
        });
        for item in &region.items {
            match item {
                ResultRegionItem::Statements(body) => {
                    out.append(guard_line_comment(self.emit_body(*body)));
                }
                ResultRegionItem::Propagate(propagate) => {
                    let span = self.span(propagate.node);
                    let binding = propagate.binding.unwrap_or_else(|| {
                        panic!("internal compiler error: result propagation has no binding")
                    });
                    let binding_span = self.span(binding.node);
                    let one = self.emit_propagate(propagate);
                    out.anchored(
                        AnchorKind::ResultBind,
                        binding_span.start,
                        trimmed_end(self.source, binding_span.start, span.end),
                        trimmed_end(self.source, binding_span.start, span.end),
                        one,
                    );
                }
            }
        }
        out.push_lit("return { kind: \"Ok\" as const, value: (");
        out.append(guard_line_comment(self.emit_expr(region.value)));
        out.push_lit(") }; })())");
        out
    }

    fn emit_apply(&self, apply: &Apply) -> Rope<'a> {
        let inner = match apply.head {
            Some(head) => {
                self.used_pipe.set(true);
                let mut acc = Rope::new();
                acc.push_lit("(");
                acc.append(guard_line_comment(self.emit_expr(head).trim()));
                acc.push_lit(")");
                for step in &apply.steps {
                    let body = guard_line_comment(self.emit_expr(step.value).trim());
                    let mut next = Rope::new();
                    match step.mode {
                        ApplyMode::Postfix => {
                            next.push_lit("(");
                            next.append(acc);
                            next.push_lit(")");
                            next.append(body);
                        }
                        ApplyMode::Call => {
                            next.push_lit("$rl_ap(");
                            next.append(acc);
                            next.push_lit(", (");
                            next.append(body);
                            next.push_lit("))");
                        }
                    }
                    acc = next;
                }
                acc
            }
            None => self.emit_flow(apply),
        };
        let start = self.span(apply.node).start;
        let end = apply.steps.last().map_or_else(
            || self.span(apply.node).end,
            |step| self.span(step.node).end,
        );
        let mut out = Rope::new();
        out.anchored(AnchorKind::Pipe, start, end, end, inner);
        out
    }

    fn emit_flow(&self, apply: &Apply) -> Rope<'a> {
        let mut steps = apply.steps.iter();
        let first = steps
            .next()
            .unwrap_or_else(|| panic!("internal compiler error: flow has no step"));
        let mut acc = Rope::new();
        acc.push_lit("(");
        acc.append(guard_line_comment(self.emit_expr(first.value).trim()));
        acc.push_lit(")");
        for step in steps {
            self.used_flow.set(true);
            let body = guard_line_comment(self.emit_expr(step.value).trim());
            let mut next = Rope::new();
            next.push_lit("$rl_fl(");
            next.append(acc);
            match step.mode {
                ApplyMode::Postfix => {
                    next.push_lit(", (($rl_v) => ($rl_v)");
                    next.append(body);
                    next.push_lit("))");
                }
                ApplyMode::Call => {
                    next.push_lit(", (");
                    next.append(body);
                    next.push_lit("))");
                }
            }
            acc = next;
        }
        acc
    }

    fn emit_template(&self, template: &Template) -> Rope<'a> {
        let mut out = Rope::new();
        for part in &template.parts {
            match part {
                TemplatePart::Raw(node) => out.append(self.source_rope(*node)),
                TemplatePart::Interpolation(expr) => {
                    out.push_lit("${");
                    out.append(self.emit_expr(*expr));
                    out.push_lit("}");
                }
            }
        }
        out
    }

    fn emit_import(&self, import: &Import, out: &mut Rope<'a>) {
        let (specifier, at) = self.source_node(import.specifier);
        if import.kind == hir::ImportKind::Std {
            match self.std_import {
                Some(path) => {
                    let quote = &specifier[..1];
                    out.push_lit(format!("{quote}{path}{quote}"));
                }
                None => out.push_src(specifier, at),
            }
            return;
        }
        match self.rewrite_imports {
            ImportRewrite::Off => out.push_src(specifier, at),
            ImportRewrite::Js => {
                out.push_src(&specifier[..specifier.len() - 4], at);
                out.push_lit(format!(".js{}", &specifier[specifier.len() - 1..]));
            }
            ImportRewrite::Ts => {
                out.push_src(&specifier[..specifier.len() - 4], at);
                out.push_lit(format!(".ts{}", &specifier[specifier.len() - 1..]));
            }
        }
    }

    fn emit_statement_decision(&self, decision: &Decision, out: &mut Rope<'a>) {
        let span = self.span(decision.head);
        let (kind, inner) = match &decision.kind {
            DecisionKind::LetElse { binding_mode, .. } => (
                AnchorKind::LetElse,
                self.emit_let_else(decision, *binding_mode),
            ),
            DecisionKind::IfLet => (AnchorKind::IfLet, self.emit_if_let(decision)),
            DecisionKind::Match { .. } => {
                panic!("internal compiler error: expression decision in statement position")
            }
        };
        out.anchored(kind, span.start, span.end, span.end, inner);
    }

    fn emit_let_else(&self, decision: &Decision, mode: BindingMode) -> Rope<'a> {
        let subject = &decision.subjects[0];
        let temp = temp_name(subject.temporary);
        let arm = &decision.arms[0];
        let mut out = Rope::new();
        out.push_lit(format!("const {temp} = ("));
        out.append(self.emit_expr(subject.value).trim());
        out.push_lit("); if (");
        let DecisionKind::LetElse {
            direct_variants, ..
        } = &decision.kind
        else {
            panic!("internal compiler error: let-else has wrong Core decision kind")
        };
        if let Some(variants) = direct_variants {
            for (index, constructor) in variants.iter().enumerate() {
                if index > 0 {
                    out.push_lit(" && ");
                }
                out.push_lit(format!(
                    "{temp}.kind !== \"{}\"",
                    self.constructor_name(constructor)
                ));
            }
        } else {
            out.push_lit("!(");
            out.append(self.emit_condition(&arm.pattern, decision));
            out.push_lit(")");
        }
        out.push_lit(") { ");
        let MissAction::Execute(body) = decision.miss else {
            panic!("internal compiler error: let-else has no else body")
        };
        let body = self.emit_body(body).trim();
        let newline = if body.last_line_has_line_comment() {
            "\n"
        } else {
            ""
        };
        out.append(body);
        out.push_lit(format!("{newline} }}"));
        let mut recovery = BindingRecovery::new(self, &arm.pattern);
        out.append(self.emit_bindings(&arm.pattern, decision, Some(mode), &mut recovery));
        out
    }

    fn emit_if_let(&self, decision: &Decision) -> Rope<'a> {
        let subject = &decision.subjects[0];
        let temp = temp_name(subject.temporary);
        let arm = &decision.arms[0];
        let mut out = Rope::new();
        out.push_lit(format!("{{ const {temp} = ("));
        out.append(self.emit_expr(subject.value).trim());
        out.push_lit("); if (");
        out.append(self.emit_condition(&arm.pattern, decision));
        out.push_lit(") { ");
        let mut recovery = BindingRecovery::new(self, &arm.pattern);
        out.append(self.emit_bindings(&arm.pattern, decision, None, &mut recovery));
        let ArmAction::Execute(body) = arm.action else {
            panic!("internal compiler error: if-let has no then body")
        };
        out.append(guard_line_comment(self.emit_body(body).trim()));
        out.push_lit(" }");
        match &decision.miss {
            MissAction::Execute(body) => {
                out.push_lit(" else { ");
                out.append(guard_line_comment(self.emit_body(*body).trim()));
                out.push_lit(" }");
            }
            MissAction::Decision(inner) => {
                out.push_lit(" else ");
                out.append(self.emit_if_let(inner));
            }
            MissAction::Nothing => {}
            MissAction::ThrowUnexpected(_) => {
                panic!("internal compiler error: if-let has match miss action")
            }
        }
        out.push_lit(" }");
        out
    }

    fn emit_value_decision(&self, decision: &Decision) -> Rope<'a> {
        let DecisionKind::Match { dispatch, .. } = decision.kind else {
            panic!("internal compiler error: value decision is not a match")
        };
        let inner = match dispatch {
            MatchDispatch::Conditional => self.emit_if_chain(decision),
            MatchDispatch::VariantSwitch | MatchDispatch::LiteralSwitch => {
                self.emit_switch(decision)
            }
        };
        let mut out = Rope::new();
        out.push_lit(if decision.is_async {
            "(await (async () => {"
        } else {
            "((() => {"
        });
        for subject in &decision.subjects {
            out.push_lit("\n  const ");
            out.push_mark(self.span(decision.head).start);
            out.push_lit(format!("{} = (", temp_name(subject.temporary)));
            out.append(self.emit_expr(subject.value).trim());
            out.push_lit(");");
        }
        out.push_lit("\n");
        out.append(inner);
        out.push_lit("})())");
        out
    }

    fn emit_switch(&self, decision: &Decision) -> Rope<'a> {
        let DecisionKind::Match { dispatch, .. } = decision.kind else {
            panic!("internal compiler error: switch decision is not a match")
        };
        let literal = dispatch == MatchDispatch::LiteralSwitch;
        let temp = temp_name(decision.subjects[0].temporary);
        let mut out = Rope::new();
        out.push_lit(if literal {
            format!("  switch ({temp}) {{\n")
        } else {
            format!("  switch ({temp}.kind) {{\n")
        });
        let mut wildcard = false;
        for arm in &decision.arms {
            out.push_lit("    ");
            if matches!(arm.pattern, PatternPlan::Any) {
                wildcard = true;
                out.push_lit("default");
            } else {
                let alternatives = pattern_alternatives(&arm.pattern);
                for (index, alternative) in alternatives.iter().enumerate() {
                    out.push_lit(if index == 0 { "case " } else { ": case " });
                    if pattern_has_literal_test(alternative) {
                        out.append(self.literal_label(alternative));
                    } else {
                        out.push_lit(format!("\"{}\"", self.variant_label(alternative)));
                    }
                }
            }
            let mut recovery = BindingRecovery::new(self, &arm.pattern);
            out.push_lit(": { ");
            out.append(self.emit_bindings(&arm.pattern, decision, None, &mut recovery));
            self.emit_arm_action(arm, "    ", false, &mut out);
        }
        if !wildcard {
            out.push_lit(unexpected_switch(literal));
        }
        out.push_lit("  }\n");
        out
    }

    fn emit_if_chain(&self, decision: &Decision) -> Rope<'a> {
        let DecisionKind::Match { needs_label, .. } = decision.kind else {
            panic!("internal compiler error: conditional decision is not a match")
        };
        let mut out = Rope::new();
        if needs_label {
            out.push_lit("  $rl_b: {\n");
        }
        let mut unconditional = false;
        for arm in &decision.arms {
            let is_any = !pattern_has_test(&arm.pattern);
            if is_any && arm.guard.is_none() {
                unconditional = true;
                out.push_lit("  ");
            } else {
                out.push_lit("  if (");
                out.append(self.emit_condition(&arm.pattern, decision));
                out.push_lit(") { ");
                let mut recovery = BindingRecovery::new(self, &arm.pattern);
                out.append(self.emit_bindings(&arm.pattern, decision, None, &mut recovery));
            }
            self.emit_arm_action(arm, "  ", true, &mut out);
            if !is_any || arm.guard.is_some() {
                out.push_lit(" }\n");
            } else {
                out.push_lit("\n");
            }
        }
        if !unconditional {
            out.push_lit(self.unexpected_throw(decision));
        }
        if needs_label {
            out.push_lit("  }\n");
        }
        out
    }

    fn emit_arm_action(&self, arm: &DecisionArm, indent: &str, chain: bool, out: &mut Rope<'a>) {
        let ArmAction::Yield { body, kind } = arm.action else {
            panic!("internal compiler error: match arm does not yield")
        };
        let body = self.emit_body(body).trim();
        let mut action = Rope::new();
        match kind {
            ArmBodyKind::Expression => {
                let newline = if body.last_line_has_line_comment() {
                    if indent == "    " { "\n    " } else { "\n  " }
                } else {
                    ""
                };
                action.push_lit("return (");
                action.append(body);
                action.push_lit(format!("{newline});"));
            }
            ArmBodyKind::Block if chain => {
                action.push_lit("{ ");
                action.append(body);
                action.push_lit("\n    break $rl_b; }");
            }
            ArmBodyKind::Block => {
                action.append(body);
                action.push_lit("\n      break;");
            }
        }
        if let Some(guard) = arm.guard {
            let guard = self.emit_expr(guard).trim();
            let newline = if guard.last_line_has_line_comment() {
                "\n  "
            } else {
                ""
            };
            out.push_lit("if ((");
            out.append(guard);
            out.push_lit(format!("{newline})) "));
        }
        out.append(action);
        if !chain {
            out.push_lit(" }\n");
        }
    }

    fn emit_condition(&self, plan: &PatternPlan, decision: &Decision) -> Rope<'a> {
        match plan {
            PatternPlan::Any | PatternPlan::Bind(_) => Rope::new(),
            PatternPlan::Test(test) => self.emit_test(test, decision),
            PatternPlan::AllOf(parts) => {
                let mut out = Rope::new();
                let tests = parts
                    .iter()
                    .filter(|part| pattern_has_test(part))
                    .collect::<Vec<_>>();
                for (index, part) in tests.iter().enumerate() {
                    if index > 0 {
                        out.push_lit(" && ");
                    }
                    let parenthesize = matches!(part, PatternPlan::AnyOf(_));
                    if parenthesize {
                        out.push_lit("(");
                    }
                    out.append(self.emit_condition(part, decision));
                    if parenthesize {
                        out.push_lit(")");
                    }
                }
                out
            }
            PatternPlan::AnyOf(parts) => {
                let mut out = Rope::new();
                for (index, part) in parts.iter().enumerate() {
                    if index > 0 {
                        out.push_lit(" || ");
                    }
                    out.append(self.emit_condition(part, decision));
                }
                out
            }
        }
    }

    fn emit_test(&self, test: &Test, decision: &Decision) -> Rope<'a> {
        match test {
            Test::Variant { place, constructor } => {
                let mut out = self.emit_place(place, decision, Some(constructor_node(constructor)));
                out.push_lit(format!(
                    ".kind === \"{}\"",
                    self.constructor_name(constructor)
                ));
                out
            }
            Test::Literal { place, pattern } => {
                let mut out = self.emit_place(place, decision, None);
                out.push_lit(" === ");
                let span = self
                    .semantic
                    .hir
                    .source_map
                    .pattern_span(*pattern)
                    .unwrap_or_else(|| panic!("internal compiler error: literal has no span"));
                let (literal, at) = self.source_span(span);
                out.push_src(literal, at);
                out
            }
        }
    }

    fn emit_place(
        &self,
        place: &Place,
        decision: &Decision,
        payload_for: Option<NodeId>,
    ) -> Rope<'a> {
        let mut out = Rope::new();
        out.push_lit(temp_name(decision.subjects[place.subject].temporary));
        for (index, field) in place.fields.iter().enumerate() {
            out.push_lit(".");
            if index + 1 == place.fields.len()
                && let Some(node) = payload_for
            {
                out.push_payload_mark(self.span(node).start);
            }
            out.push_lit(self.field_name(field));
        }
        out
    }

    fn emit_bindings(
        &self,
        plan: &PatternPlan,
        decision: &Decision,
        declaration: Option<BindingMode>,
        recovery: &mut BindingRecovery,
    ) -> Rope<'a> {
        let selected = if let PatternPlan::AnyOf(parts) = plan {
            parts.first().unwrap_or(plan)
        } else {
            plan
        };
        let mut groups: Vec<BindingGroup<'_>> = Vec::new();
        collect_binding_groups(
            selected,
            !matches!(plan, PatternPlan::AnyOf(_)),
            &mut groups,
        );
        let mut out = Rope::new();
        for (group_index, (receiver, bindings)) in groups.into_iter().enumerate() {
            if group_index == 0 {
                if let Some(mode) = declaration {
                    out.push_lit(format!(" {} {{ ", binding_keyword(mode)));
                } else {
                    out.push_lit("const { ");
                }
            } else {
                out.push_lit("const { ");
            }
            for (index, (binding, mapped)) in bindings.iter().enumerate() {
                if index > 0 {
                    out.push_lit(", ");
                }
                self.emit_binding(binding, *mapped, recovery, &mut out);
            }
            out.push_lit(" } = ");
            out.append(self.emit_place(&receiver, decision, None));
            out.push_lit(if declaration.is_some() { ";" } else { "; " });
        }
        out
    }

    fn emit_binding(
        &self,
        binding: &Bind,
        mapped: bool,
        recovery: &mut BindingRecovery,
        out: &mut Rope<'a>,
    ) {
        let field = binding
            .source
            .fields
            .last()
            .unwrap_or_else(|| panic!("internal compiler error: binding has no source field"));
        let field_node = field_node(field);
        let field_text = self.field_name(field);
        if mapped {
            let span = self.span(field_node);
            let (text, at) = self.source_span(span);
            out.push_src(text, at);
        } else {
            out.push_lit(field_text);
        }
        if let Some(replacement) = recovery.replacement(self, binding) {
            out.push_lit(format!(": {replacement}"));
        } else if binding.binding != field_node {
            out.push_lit(": ");
            if mapped {
                out.append(self.source_rope(binding.binding));
            } else {
                out.push_lit(self.source_node(binding.binding).0.to_owned());
            }
        }
    }

    fn constructor_name(&self, constructor: &Constructor) -> String {
        self.source_node(constructor_node(constructor)).0.to_owned()
    }

    fn field_name(&self, field: &FieldAccess) -> String {
        self.source_node(field_node(field)).0.to_owned()
    }

    fn literal_label(&self, plan: &PatternPlan) -> Rope<'a> {
        let PatternPlan::Test(Test::Literal { pattern, .. }) = plan else {
            panic!("internal compiler error: switch literal alternative is not literal")
        };
        let span = self.semantic.hir.source_map.pattern_span(*pattern).unwrap();
        let (text, at) = self.source_span(span);
        let mut out = Rope::new();
        out.push_src(text, at);
        out
    }

    fn variant_label(&self, plan: &PatternPlan) -> String {
        let PatternPlan::AllOf(parts) = plan else {
            panic!("internal compiler error: switch variant alternative is not constructor")
        };
        let constructor = parts.iter().find_map(|part| match part {
            PatternPlan::Test(Test::Variant { constructor, .. }) => Some(constructor),
            _ => None,
        });
        self.constructor_name(constructor.unwrap())
    }

    fn unexpected_throw(&self, decision: &Decision) -> String {
        match decision.miss {
            MissAction::ThrowUnexpected(UnexpectedKind::Tuple) => {
                let temps = decision
                    .subjects
                    .iter()
                    .map(|subject| temp_name(subject.temporary))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "  throw new Error(\"rl match: unexpected case \" + JSON.stringify([{temps}]));\n"
                )
            }
            MissAction::ThrowUnexpected(UnexpectedKind::Literal) => {
                "  throw new Error(\"rl match: unexpected literal \" + JSON.stringify($rl_m));\n"
                    .to_owned()
            }
            MissAction::ThrowUnexpected(UnexpectedKind::Case) => {
                "  throw new Error(\"rl match: unexpected case \" + JSON.stringify($rl_m));\n"
                    .to_owned()
            }
            _ => panic!("internal compiler error: match has non-match miss action"),
        }
    }
}

fn guard_line_comment(mut rope: Rope<'_>) -> Rope<'_> {
    if rope.last_line_has_line_comment() {
        rope.push_lit("\n");
    }
    rope
}

fn trimmed_end(source: &str, start: usize, end: usize) -> usize {
    start + source[start..end].trim_end().len()
}

fn binding_keyword(mode: BindingMode) -> &'static str {
    match mode {
        BindingMode::Const => "const",
        BindingMode::Let => "let",
        BindingMode::Var => "var",
    }
}

fn temp_name(temp: TempId) -> String {
    match temp {
        TempId::Statement(sequence) => format!("$rl_t{sequence}"),
        TempId::Result(sequence) => format!("$rl_r{sequence}"),
        TempId::Decision => "$rl_m".to_owned(),
        TempId::DecisionElement(sequence) => format!("$rl_m{sequence}"),
    }
}

fn constructor_node(constructor: &Constructor) -> NodeId {
    match constructor {
        Constructor::Resolved { node, .. } | Constructor::Recovery { node, .. } => *node,
    }
}

fn field_node(field: &FieldAccess) -> NodeId {
    match field {
        FieldAccess::Resolved { node, .. } | FieldAccess::Recovery { node, .. } => *node,
    }
}

fn pattern_has_test(plan: &PatternPlan) -> bool {
    match plan {
        PatternPlan::Any | PatternPlan::Bind(_) => false,
        PatternPlan::Test(_) => true,
        PatternPlan::AllOf(parts) | PatternPlan::AnyOf(parts) => parts.iter().any(pattern_has_test),
    }
}

fn pattern_has_literal_test(plan: &PatternPlan) -> bool {
    match plan {
        PatternPlan::Test(Test::Literal { .. }) => true,
        PatternPlan::AllOf(parts) | PatternPlan::AnyOf(parts) => {
            parts.iter().any(pattern_has_literal_test)
        }
        PatternPlan::Any | PatternPlan::Bind(_) | PatternPlan::Test(Test::Variant { .. }) => false,
    }
}

fn pattern_alternatives(plan: &PatternPlan) -> Vec<&PatternPlan> {
    match plan {
        PatternPlan::AnyOf(parts) => parts.iter().collect(),
        _ => vec![plan],
    }
}

type BindingGroup<'a> = (Place, Vec<(&'a Bind, bool)>);

fn collect_binding_groups<'a>(
    plan: &'a PatternPlan,
    mapped: bool,
    groups: &mut Vec<BindingGroup<'a>>,
) {
    match plan {
        PatternPlan::Bind(binding) => {
            let mut receiver = binding.source.clone();
            receiver.fields.pop();
            if let Some((_, bindings)) = groups
                .iter_mut()
                .find(|(existing, _)| same_place(existing, &receiver))
            {
                bindings.push((binding, mapped));
            } else {
                groups.push((receiver, vec![(binding, mapped)]));
            }
        }
        PatternPlan::AllOf(parts) => {
            for part in parts
                .iter()
                .filter(|part| matches!(part, PatternPlan::Bind(_)))
            {
                collect_binding_groups(part, mapped, groups);
            }
            for part in parts
                .iter()
                .filter(|part| !matches!(part, PatternPlan::Bind(_)))
            {
                collect_binding_groups(part, mapped, groups);
            }
        }
        PatternPlan::AnyOf(parts) => {
            if let Some(first) = parts.first() {
                collect_binding_groups(first, false, groups);
            }
        }
        PatternPlan::Any | PatternPlan::Test(_) => {}
    }
}

fn same_place(left: &Place, right: &Place) -> bool {
    left.subject == right.subject
        && left.fields.len() == right.fields.len()
        && left
            .fields
            .iter()
            .zip(&right.fields)
            .all(|(left, right)| field_node(left) == field_node(right))
}

struct BindingRecovery {
    available: HashSet<String>,
    emitted: HashSet<String>,
    discard_sequence: usize,
}

impl BindingRecovery {
    fn new(emitter: &Emitter<'_>, plan: &PatternPlan) -> BindingRecovery {
        let selected = if let PatternPlan::AnyOf(parts) = plan {
            parts.first().unwrap_or(plan)
        } else {
            plan
        };
        let mut groups = Vec::new();
        collect_binding_groups(
            selected,
            !matches!(plan, PatternPlan::AnyOf(_)),
            &mut groups,
        );
        let available = groups
            .into_iter()
            .flat_map(|(_, bindings)| bindings)
            .map(|(binding, _)| emitter.source_node(binding.binding).0.to_owned())
            .collect();
        BindingRecovery {
            available,
            emitted: HashSet::new(),
            discard_sequence: 0,
        }
    }

    fn replacement(&mut self, emitter: &Emitter<'_>, binding: &Bind) -> Option<String> {
        let name = emitter.source_node(binding.binding).0;
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

fn unexpected_switch(literal: bool) -> &'static str {
    if literal {
        "    default: { throw new Error(\"rl match: unexpected literal \" + JSON.stringify($rl_m)); }\n"
    } else {
        "    default: { throw new Error(\"rl match: unexpected case \" + JSON.stringify($rl_m)); }\n"
    }
}

fn emit_adt_text(adt: &Adt) -> String {
    let export = if adt.exported { "export " } else { "" };
    let arms = adt
        .variants
        .iter()
        .map(|variant| match &variant.fields {
            Some(fields) if !fields.is_empty() => format!(
                "{{ kind: \"{}\"; {} }}",
                variant.name,
                fields
                    .iter()
                    .map(|field| format!(
                        "{}{}: {}",
                        field.name,
                        if field.optional { "?" } else { "" },
                        field.ty_text
                    ))
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
            _ => format!("{{ kind: \"{}\" }}", variant.name),
        })
        .collect::<Vec<_>>()
        .join("\n  | ");
    let type_decl = format!("{export}type {}{} =\n  | {arms};", adt.name, adt.generics);
    let type_args = if adt.generics.is_empty() {
        String::new()
    } else {
        format!("<{}>", generic_param_names(&adt.generics).join(", "))
    };
    let constructors = adt
        .variants
        .iter()
        .filter_map(|variant| {
            if !variant.emit_constructor {
                return None;
            }
            Some(match &variant.fields {
                None => format!(
                    "  {}: {{ kind: \"{}\" }} as const,",
                    variant.name, variant.name
                ),
                Some(fields) => {
                    let params = fields
                        .iter()
                        .map(|field| {
                            format!(
                                "{}{}: {}",
                                field.name,
                                if field.optional { "?" } else { "" },
                                field.ty_text
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    let object = std::iter::once(format!("kind: \"{}\"", variant.name))
                        .chain(fields.iter().map(|field| field.name.clone()))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!(
                        "  {}: {}({params}): {}{type_args} => ({{ {object} }}),",
                        variant.name, adt.generics, adt.name
                    )
                }
            })
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "{type_decl}\n{export}const {} = {{\n{constructors}\n}};",
        adt.name
    )
}

fn generic_param_names(generics: &str) -> Vec<String> {
    let inner = &generics[1..generics.len() - 1];
    let source = inner.as_bytes();
    let mut names = Vec::new();
    let mut index = 0usize;
    while index < source.len() {
        index = skip_ws_comments(source, index, source.len());
        if index >= source.len() || !is_ident_start(source[index]) {
            break;
        }
        let end = ident_end(source, index, source.len());
        let word = &inner[index..end];
        if word == "const" || word == "in" || word == "out" {
            index = end;
            continue;
        }
        names.push(word.to_owned());
        index = scan_type_end(source, end, source.len());
        if at(source, index, source.len()) == Some(b',') {
            index += 1;
        }
    }
    names
}
