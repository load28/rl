use super::*;
use crate::analysis::SemanticFile;
use crate::hir::ids::Idx;
use crate::hir::{self, FieldBinding, Pat};
use crate::resolve::{Res, Resolution};
use crate::scanner::contains_await;
use std::collections::HashMap;
use std::collections::HashSet;

pub(crate) fn lower_semantic(semantic: &SemanticFile, source: &str) -> CoreFile {
    let temp_ordinals = temp_ordinals(semantic);
    let mut cx = Lowering {
        semantic,
        source,
        temp_ordinals,
    };
    let bodies = semantic
        .hir
        .bodies
        .iter()
        .map(|(_, body)| cx.lower_body(body))
        .collect();
    let exprs = semantic
        .hir
        .exprs
        .iter()
        .map(|(_, expr)| cx.lower_expr(expr))
        .collect();
    let file = CoreFile {
        root: semantic.hir.root,
        bodies,
        exprs,
        temporary_count: u32::try_from(cx.temp_ordinals.len())
            .unwrap_or_else(|_| panic!("internal compiler error: Core IR temporary overflow")),
    };
    validate(&file, semantic);
    file
}

struct Lowering<'a> {
    semantic: &'a SemanticFile,
    source: &'a str,
    temp_ordinals: HashMap<NodeId, u32>,
}

impl Lowering<'_> {
    fn temp(&self, node: NodeId, result: bool) -> TempId {
        let sequence = *self.temp_ordinals.get(&node).unwrap_or_else(|| {
            panic!("internal compiler error: semantic temporary has no assigned ordinal")
        });
        if result {
            TempId::Result(sequence)
        } else {
            TempId::Statement(sequence)
        }
    }

    fn lower_body(&mut self, body: &hir::Body) -> Body {
        Body {
            statements: body
                .stmts
                .iter()
                .filter_map(|stmt| self.lower_stmt(stmt))
                .collect(),
        }
    }

    fn lower_stmt(&mut self, stmt: &hir::Stmt) -> Option<Statement> {
        match stmt {
            hir::Stmt::Opaque(node) => {
                if self.semantic.hir.source_map.ast_origin(*node)
                    == Some(hir::AstOrigin::ValModifier)
                {
                    None
                } else {
                    Some(Statement::Opaque(*node))
                }
            }
            hir::Stmt::Item(owner) => match &self.semantic.hir.items[owner.0 as usize] {
                hir::Item::Enum(item) => {
                    let mut emitted = HashSet::new();
                    Some(Statement::Adt(Adt {
                        name: item.name.clone(),
                        exported: item.exported,
                        generics: item.generics.clone(),
                        variants: item
                            .variants
                            .iter()
                            .map(|variant_id| {
                                let variant = &self.semantic.hir.variants[*variant_id];
                                AdtVariant {
                                    name: variant.name.clone(),
                                    fields: variant.fields.as_ref().map(|field_ids| {
                                        field_ids
                                            .iter()
                                            .map(|field_id| {
                                                let field = &self.semantic.hir.fields[*field_id];
                                                AdtField {
                                                    name: field.name.clone(),
                                                    optional: field.optional,
                                                    ty_text: field.ty_text.clone(),
                                                }
                                            })
                                            .collect()
                                    }),
                                    emit_constructor: emitted.insert(variant.name.clone()),
                                }
                            })
                            .collect(),
                    }))
                }
                hir::Item::Import(item) => Some(Statement::Import(Import {
                    specifier: item.node,
                    kind: item.kind,
                })),
            },
            hir::Stmt::Try(stmt) => Some(Statement::Propagate(Propagate {
                node: stmt.node,
                value: stmt.expr,
                temporary: self.temp(stmt.node, false),
                binding: stmt.binding,
                exit: ExitTarget::EnclosingFunction,
                layout: RESULT_LAYOUT,
            })),
            hir::Stmt::LetElse(stmt) => {
                let site = &self.semantic.hir.sites[stmt.site];
                Some(Statement::Decision(self.lower_decision(
                    site,
                    stmt.node,
                    |_, _| ArmAction::BindThrough(stmt.binding_mode),
                    MissAction::Execute(stmt.else_body),
                    true,
                    DecisionKind::LetElse {
                        binding_mode: stmt.binding_mode,
                        direct_variants: direct_variant_alternatives(
                            &self.lower_pattern(site.arms[0].pattern),
                        ),
                    },
                )))
            }
            hir::Stmt::IfLet(stmt) => Some(Statement::Decision(self.lower_if_let(stmt))),
            hir::Stmt::Expr(expr) => Some(Statement::Expr(*expr)),
        }
    }

    fn lower_expr(&mut self, expr: &hir::Expr) -> Expr {
        match expr {
            hir::Expr::OpaqueTs(node) => Expr::Opaque(*node),
            hir::Expr::Seq { body, .. } => Expr::Sequence(*body),
            hir::Expr::Match { site, extent, .. } => {
                let site = &self.semantic.hir.sites[*site];
                let literal = site
                    .arms
                    .iter()
                    .any(|arm| pattern_has_literal(&self.semantic.hir, arm.pattern));
                let mut decision = self.lower_decision(
                    site,
                    *extent,
                    |arm, kind| ArmAction::Yield {
                        body: arm.body.unwrap_or_else(|| {
                            panic!("internal compiler error: match arm has no body")
                        }),
                        kind: kind.unwrap_or_else(|| {
                            panic!("internal compiler error: match arm has no body kind")
                        }),
                    },
                    MissAction::ThrowUnexpected(if site.subjects.len() > 1 {
                        UnexpectedKind::Tuple
                    } else if literal {
                        UnexpectedKind::Literal
                    } else {
                        UnexpectedKind::Case
                    }),
                    false,
                    DecisionKind::Match {
                        dispatch: MatchDispatch::Conditional,
                        needs_label: false,
                    },
                );
                decision.kind = match_kind(&decision);
                Expr::Decision(decision)
            }
            hir::Expr::Pipe { node, head, steps } => Expr::Apply(Apply {
                node: *node,
                head: *head,
                steps: steps
                    .iter()
                    .map(|step| ApplyStep {
                        node: step.node,
                        value: step.body,
                        mode: if step.postfix {
                            ApplyMode::Postfix
                        } else {
                            ApplyMode::Call
                        },
                    })
                    .collect(),
            }),
            hir::Expr::ResultBlock { node, items, value } => Expr::ResultRegion(ResultRegion {
                node: *node,
                items: items
                    .iter()
                    .map(|item| match item {
                        hir::ResultItem::Stmts(body) => ResultRegionItem::Statements(*body),
                        hir::ResultItem::Bind {
                            node,
                            binding,
                            expr,
                        } => ResultRegionItem::Propagate(Propagate {
                            node: *node,
                            value: *expr,
                            temporary: self.temp(*node, true),
                            binding: Some(*binding),
                            exit: ExitTarget::Region,
                            layout: RESULT_LAYOUT,
                        }),
                    })
                    .collect(),
                value: *value,
                is_async: self.node_contains_await(*node),
            }),
            hir::Expr::Template { node, chunks } => Expr::Template(Template {
                node: *node,
                parts: chunks
                    .iter()
                    .map(|part| match part {
                        hir::TemplatePart::Raw(node) => TemplatePart::Raw(*node),
                        hir::TemplatePart::Interp(expr) => TemplatePart::Interpolation(*expr),
                    })
                    .collect(),
            }),
        }
    }

    fn lower_decision(
        &mut self,
        site: &hir::PatternSite,
        extent: NodeId,
        action: impl Fn(&hir::SiteArm, Option<ArmBodyKind>) -> ArmAction,
        miss: MissAction,
        file_unique_temps: bool,
        kind: DecisionKind,
    ) -> Decision {
        let subjects = site
            .subjects
            .iter()
            .enumerate()
            .map(|(index, value)| Subject {
                value: *value,
                temporary: if file_unique_temps {
                    self.temp(extent, false)
                } else if site.subjects.len() == 1 {
                    TempId::Decision
                } else {
                    TempId::DecisionElement(u32::try_from(index).unwrap_or_else(|_| {
                        panic!("internal compiler error: decision subject index overflow")
                    }))
                },
            })
            .collect();
        let arms = site
            .arms
            .iter()
            .map(|arm| DecisionArm {
                pattern: self.lower_pattern(arm.pattern),
                guard: arm.guard,
                action: action(arm, arm.body_kind),
            })
            .collect();
        Decision {
            subjects,
            arms,
            miss,
            head: site.node,
            extent,
            is_async: !file_unique_temps && self.node_contains_await(extent),
            kind,
        }
    }

    fn lower_if_let(&mut self, stmt: &hir::IfLetStmt) -> Decision {
        let miss = match &stmt.else_part {
            Some(hir::IfLetElse::Block(body)) => MissAction::Execute(*body),
            Some(hir::IfLetElse::IfLet(inner)) => {
                MissAction::Decision(Box::new(self.lower_if_let(inner)))
            }
            None => MissAction::Nothing,
        };
        let site = &self.semantic.hir.sites[stmt.site];
        self.lower_decision(
            site,
            stmt.node,
            |arm, _| {
                ArmAction::Execute(
                    arm.body.unwrap_or_else(|| {
                        panic!("internal compiler error: if-let arm has no body")
                    }),
                )
            },
            miss,
            true,
            DecisionKind::IfLet,
        )
    }

    fn lower_pattern(&self, pattern: hir::PatternId) -> PatternPlan {
        self.pattern_at(
            pattern,
            Place {
                subject: 0,
                fields: Vec::new(),
            },
        )
    }

    fn pattern_at(&self, pattern: hir::PatternId, place: Place) -> PatternPlan {
        match &self.semantic.hir.patterns[pattern] {
            Pat::Wildcard => PatternPlan::Any,
            Pat::Or(alternatives) => PatternPlan::AnyOf(
                alternatives
                    .iter()
                    .map(|pattern| self.pattern_at(*pattern, place.clone()))
                    .collect(),
            ),
            Pat::Tuple(elements) => PatternPlan::AllOf(
                elements
                    .iter()
                    .enumerate()
                    .map(|(subject, element)| {
                        self.pattern_at(
                            *element,
                            Place {
                                subject,
                                fields: Vec::new(),
                            },
                        )
                    })
                    .collect(),
            ),
            Pat::Literal(_) => PatternPlan::Test(Test::Literal { place, pattern }),
            Pat::Constructor { path, fields } => {
                let constructor = match self.semantic.resolution.uses.get(&path.node) {
                    Some(Res::Variant(reference)) => Constructor::Resolved {
                        reference: *reference,
                        node: path.node,
                    },
                    _ => Constructor::Recovery {
                        node: path.node,
                        name: path.name.clone(),
                    },
                };
                let mut parts = vec![PatternPlan::Test(Test::Variant {
                    place: place.clone(),
                    constructor,
                })];
                for field in fields.as_deref().unwrap_or_default() {
                    let access = match self.semantic.resolution.uses.get(&field.node) {
                        Some(Res::Field(reference)) => FieldAccess::Resolved {
                            reference: *reference,
                            node: field.node,
                        },
                        _ => FieldAccess::Recovery {
                            node: field.node,
                            name: field.name.clone(),
                        },
                    };
                    let mut field_place = place.clone();
                    field_place.fields.push(access);
                    match &field.binding {
                        FieldBinding::Named { alias } => {
                            let binding = alias.as_ref().map_or(field.node, |(_, node)| *node);
                            parts.push(PatternPlan::Bind(Bind {
                                source: field_place.clone(),
                                source_field: resolved_hir_field(
                                    &self.semantic.resolution,
                                    field.node,
                                ),
                                binding,
                            }));
                        }
                        FieldBinding::Nested(inner) => {
                            parts.push(self.pattern_at(*inner, field_place));
                        }
                    }
                }
                PatternPlan::AllOf(parts)
            }
        }
    }

    fn node_contains_await(&self, node: NodeId) -> bool {
        let span = self
            .semantic
            .hir
            .source_map
            .node_span(node)
            .unwrap_or_else(|| {
                panic!("internal compiler error: Core IR async node has no source span")
            });
        contains_await(self.source.as_bytes(), span.start, span.end)
    }
}

fn temp_ordinals(semantic: &SemanticFile) -> HashMap<NodeId, u32> {
    fn if_let_nodes(stmt: &hir::IfLetStmt, out: &mut Vec<NodeId>) {
        out.push(stmt.node);
        if let Some(hir::IfLetElse::IfLet(inner)) = &stmt.else_part {
            if_let_nodes(inner, out);
        }
    }

    let mut nodes = Vec::new();
    for (_, body) in semantic.hir.bodies.iter() {
        for statement in &body.stmts {
            match statement {
                hir::Stmt::Try(stmt) => nodes.push(stmt.node),
                hir::Stmt::LetElse(stmt) => nodes.push(stmt.node),
                hir::Stmt::IfLet(stmt) => if_let_nodes(stmt, &mut nodes),
                hir::Stmt::Opaque(_) | hir::Stmt::Item(_) | hir::Stmt::Expr(_) => {}
            }
        }
    }
    for (_, expr) in semantic.hir.exprs.iter() {
        if let hir::Expr::ResultBlock { items, .. } = expr {
            for item in items {
                if let hir::ResultItem::Bind { node, .. } = item {
                    nodes.push(*node);
                }
            }
        }
    }
    nodes.sort_unstable_by_key(|node| {
        semantic
            .hir
            .source_map
            .node_span(*node)
            .map_or(usize::MAX, |span| span.start)
    });
    nodes.dedup();
    nodes
        .into_iter()
        .enumerate()
        .map(|(index, node)| {
            (
                node,
                u32::try_from(index).unwrap_or_else(|_| {
                    panic!("internal compiler error: Core IR temporary overflow")
                }),
            )
        })
        .collect()
}

fn resolved_hir_field(resolution: &Resolution, node: NodeId) -> Option<FieldId> {
    let Res::Field(reference) = resolution.uses.get(&node)? else {
        return None;
    };
    resolution.field(*reference)?.hir
}

fn pattern_has_literal(hir: &hir::HirFile, pattern: hir::PatternId) -> bool {
    match &hir.patterns[pattern] {
        Pat::Literal(_) => true,
        Pat::Or(parts) | Pat::Tuple(parts) => {
            parts.iter().any(|part| pattern_has_literal(hir, *part))
        }
        Pat::Wildcard | Pat::Constructor { .. } => false,
    }
}

fn match_kind(decision: &Decision) -> DecisionKind {
    let needs_label = decision.arms.iter().any(|arm| {
        matches!(
            arm.action,
            ArmAction::Yield {
                kind: ArmBodyKind::Block,
                ..
            }
        )
    });
    let switch = decision.subjects.len() == 1
        && decision.arms.iter().all(|arm| arm.guard.is_none())
        && decision
            .arms
            .iter()
            .all(|arm| !pattern_has_nested_test(&arm.pattern));
    let dispatch = if !switch {
        MatchDispatch::Conditional
    } else if decision
        .arms
        .iter()
        .any(|arm| pattern_has_literal_test(&arm.pattern))
    {
        MatchDispatch::LiteralSwitch
    } else {
        MatchDispatch::VariantSwitch
    };
    DecisionKind::Match {
        dispatch,
        needs_label,
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

fn pattern_has_nested_test(plan: &PatternPlan) -> bool {
    match plan {
        PatternPlan::Test(Test::Variant { place, .. }) => !place.fields.is_empty(),
        PatternPlan::AllOf(parts) | PatternPlan::AnyOf(parts) => {
            parts.iter().any(pattern_has_nested_test)
        }
        PatternPlan::Any | PatternPlan::Bind(_) | PatternPlan::Test(Test::Literal { .. }) => false,
    }
}

fn pattern_alternatives(plan: &PatternPlan) -> Vec<&PatternPlan> {
    match plan {
        PatternPlan::AnyOf(parts) => parts.iter().collect(),
        _ => vec![plan],
    }
}

fn direct_variant_alternatives(plan: &PatternPlan) -> Option<Vec<Constructor>> {
    pattern_alternatives(plan)
        .into_iter()
        .map(|alternative| match alternative {
            PatternPlan::AllOf(parts) => parts.iter().find_map(|part| match part {
                PatternPlan::Test(Test::Variant { place, constructor })
                    if place.fields.is_empty() =>
                {
                    Some(constructor.clone())
                }
                _ => None,
            }),
            _ => None,
        })
        .collect()
}

fn validate(file: &CoreFile, semantic: &SemanticFile) {
    assert!(
        file.root.index() < file.bodies.len(),
        "internal compiler error: Core IR root body is invalid"
    );
    for body in &file.bodies {
        for statement in &body.statements {
            validate_statement(statement, file, semantic);
        }
    }
    for expr in &file.exprs {
        match expr {
            Expr::Opaque(node) => validate_node(*node, semantic),
            Expr::Sequence(body) => validate_body(*body, file),
            Expr::Decision(decision) => validate_decision(decision, file, semantic),
            Expr::Apply(apply) => {
                validate_node(apply.node, semantic);
                if let Some(head) = apply.head {
                    validate_expr(head, file);
                }
                assert!(
                    !apply.steps.is_empty(),
                    "internal compiler error: Core IR apply has no steps"
                );
                for step in &apply.steps {
                    validate_node(step.node, semantic);
                    validate_expr(step.value, file);
                    match step.mode {
                        ApplyMode::Call | ApplyMode::Postfix => {}
                    }
                }
            }
            Expr::ResultRegion(region) => {
                validate_node(region.node, semantic);
                let _is_async = region.is_async;
                for item in &region.items {
                    match item {
                        ResultRegionItem::Statements(body) => validate_body(*body, file),
                        ResultRegionItem::Propagate(propagate) => {
                            validate_propagate(propagate, file, semantic)
                        }
                    }
                }
                validate_expr(region.value, file);
            }
            Expr::Template(template) => {
                validate_node(template.node, semantic);
                for part in &template.parts {
                    match part {
                        TemplatePart::Raw(node) => validate_node(*node, semantic),
                        TemplatePart::Interpolation(expr) => validate_expr(*expr, file),
                    }
                }
            }
        }
    }
}

fn validate_statement(statement: &Statement, file: &CoreFile, semantic: &SemanticFile) {
    match statement {
        Statement::Opaque(node) => validate_node(*node, semantic),
        Statement::Adt(adt) => {
            assert!(
                !adt.name.is_empty(),
                "internal compiler error: empty Core ADT"
            );
            assert!(
                !adt.variants.is_empty(),
                "internal compiler error: Core ADT has no variants"
            );
        }
        Statement::Import(import) => {
            validate_node(import.specifier, semantic);
            match import.kind {
                hir::ImportKind::Std | hir::ImportKind::Relative => {}
            }
        }
        Statement::Propagate(propagate) => validate_propagate(propagate, file, semantic),
        Statement::Decision(decision) => validate_decision(decision, file, semantic),
        Statement::Expr(expr) => validate_expr(*expr, file),
    }
}

fn validate_decision(decision: &Decision, file: &CoreFile, semantic: &SemanticFile) {
    assert!(
        !decision.subjects.is_empty(),
        "internal compiler error: Core IR decision has no subject"
    );
    for subject in &decision.subjects {
        validate_temp(subject.temporary, file);
        validate_expr(subject.value, file);
    }
    validate_node(decision.head, semantic);
    validate_node(decision.extent, semantic);
    match decision.kind {
        DecisionKind::Match { dispatch, .. } => match dispatch {
            MatchDispatch::Conditional
            | MatchDispatch::VariantSwitch
            | MatchDispatch::LiteralSwitch => {}
        },
        DecisionKind::IfLet => {}
        DecisionKind::LetElse { binding_mode, .. } => validate_binding_mode(binding_mode),
    }
    if decision.is_async {
        assert!(
            decision
                .arms
                .iter()
                .any(|arm| matches!(arm.action, ArmAction::Yield { .. })),
            "internal compiler error: statement decision cannot be async"
        );
    }
    for arm in &decision.arms {
        if let Some(guard) = arm.guard {
            validate_expr(guard, file);
        }
        validate_pattern_plan(&arm.pattern, semantic);
        match arm.action {
            ArmAction::Yield { body, kind } => {
                validate_body(body, file);
                match kind {
                    ArmBodyKind::Expression | ArmBodyKind::Block => {}
                }
            }
            ArmAction::Execute(body) => validate_body(body, file),
            ArmAction::BindThrough(mode) => validate_binding_mode(mode),
        }
    }
    validate_miss(&decision.miss, file, semantic);
}

fn validate_pattern_plan(plan: &PatternPlan, semantic: &SemanticFile) {
    match plan {
        PatternPlan::Any => {}
        PatternPlan::Test(test) => validate_test(test, semantic),
        PatternPlan::Bind(binding) => {
            validate_place(&binding.source, semantic);
            if let Some(field) = binding.source_field {
                assert!(
                    field.index() < semantic.hir.fields.len(),
                    "internal compiler error: Core IR binding field is invalid"
                );
            }
            validate_node(binding.binding, semantic);
        }
        PatternPlan::AllOf(parts) | PatternPlan::AnyOf(parts) => {
            assert!(
                !parts.is_empty(),
                "internal compiler error: Core IR boolean pattern is empty"
            );
            for part in parts {
                validate_pattern_plan(part, semantic);
            }
        }
    }
}

fn validate_miss(miss: &MissAction, file: &CoreFile, semantic: &SemanticFile) {
    match miss {
        MissAction::ThrowUnexpected(kind) => match kind {
            UnexpectedKind::Case | UnexpectedKind::Literal | UnexpectedKind::Tuple => {}
        },
        MissAction::Execute(body) => validate_body(*body, file),
        MissAction::Decision(decision) => validate_decision(decision, file, semantic),
        MissAction::Nothing => {}
    }
}

fn validate_test(test: &Test, semantic: &SemanticFile) {
    match test {
        Test::Variant { place, constructor } => {
            validate_place(place, semantic);
            match constructor {
                Constructor::Resolved { reference, node } => {
                    validate_node(*node, semantic);
                    assert!(
                        semantic.resolution.variant(*reference).is_some(),
                        "internal compiler error: Core IR variant is invalid"
                    );
                }
                Constructor::Recovery { node, name } => {
                    validate_node(*node, semantic);
                    assert!(
                        !name.is_empty(),
                        "internal compiler error: empty recovery tag"
                    );
                }
            }
        }
        Test::Literal { place, pattern } => {
            validate_place(place, semantic);
            assert!(
                pattern.index() < semantic.hir.patterns.len(),
                "internal compiler error: Core IR literal pattern is invalid"
            );
        }
    }
}

fn validate_place(place: &Place, semantic: &SemanticFile) {
    for field in &place.fields {
        match field {
            FieldAccess::Resolved { reference, node } => {
                validate_node(*node, semantic);
                assert!(
                    semantic.resolution.field(*reference).is_some(),
                    "internal compiler error: Core IR field is invalid"
                );
            }
            FieldAccess::Recovery { node, name } => {
                validate_node(*node, semantic);
                assert!(
                    !name.is_empty(),
                    "internal compiler error: empty recovery field"
                );
            }
        }
    }
    let _subject = place.subject;
}

fn validate_propagate(propagate: &Propagate, file: &CoreFile, semantic: &SemanticFile) {
    validate_node(propagate.node, semantic);
    validate_expr(propagate.value, file);
    validate_temp(propagate.temporary, file);
    if let Some(binding) = propagate.binding {
        validate_node(binding.node, semantic);
        validate_binding_mode(binding.mode);
    }
    match propagate.exit {
        ExitTarget::EnclosingFunction | ExitTarget::Region => {}
    }
    assert!(
        !propagate.layout.success_tag.is_empty()
            && !propagate.layout.discriminant_field.is_empty()
            && !propagate.layout.payload_field.is_empty(),
        "internal compiler error: Core IR Result layout is empty"
    );
}

fn validate_binding_mode(mode: BindingMode) {
    match mode {
        BindingMode::Const | BindingMode::Let | BindingMode::Var => {}
    }
}

fn validate_node(node: NodeId, semantic: &SemanticFile) {
    assert!(
        semantic.hir.source_map.node_span(node).is_some(),
        "internal compiler error: Core IR node has no source span"
    );
}

fn validate_body(body: BodyId, file: &CoreFile) {
    assert!(
        body.index() < file.bodies.len(),
        "internal compiler error: Core IR body ID is invalid"
    );
}

fn validate_expr(expr: ExprId, file: &CoreFile) {
    assert!(
        expr.index() < file.exprs.len(),
        "internal compiler error: Core IR expression ID is invalid"
    );
}

fn validate_temp(temp: TempId, file: &CoreFile) {
    match temp {
        TempId::Statement(sequence) | TempId::Result(sequence) => assert!(
            sequence < file.temporary_count,
            "internal compiler error: Core IR temporary ID is invalid"
        ),
        TempId::Decision | TempId::DecisionElement(_) => {}
    }
}
