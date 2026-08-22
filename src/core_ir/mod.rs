//! Surface-independent IR for rl-owned semantics.
//!
//! Pattern-bearing syntax is normalized to [`Decision`]; Result propagation
//! is normalized to [`Propagate`]. The IR refers to HIR and resolution by ID,
//! never by reparsing source text. TypeScript-specific statement shapes are
//! introduced by the next lowering stage.

mod lower;

use crate::hir;
use crate::hir::{ArmBodyKind, BindingMode, BindingText, BodyId, ExprId, FieldId, NodeId};
use crate::resolve::{FieldRef, VariantRef};

pub(crate) use lower::lower_semantic;

#[derive(Debug)]
pub(crate) struct CoreFile {
    pub root: BodyId,
    pub bodies: Vec<Body>,
    pub exprs: Vec<Expr>,
    pub temporary_count: u32,
}

#[derive(Debug)]
pub(crate) struct Body {
    pub statements: Vec<Statement>,
}

#[derive(Debug)]
pub(crate) enum Statement {
    Opaque(NodeId),
    Adt(Adt),
    Import(Import),
    Propagate(Propagate),
    Decision(Decision),
    Expr(ExprId),
}

#[derive(Debug)]
pub(crate) enum Expr {
    Opaque(NodeId),
    Sequence(BodyId),
    Decision(Decision),
    Apply(Apply),
    ResultRegion(ResultRegion),
    Template(Template),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum TempId {
    /// File-unique `$rl_tN`, shared by `try` and statement decisions.
    Statement(u32),
    /// File-unique `$rl_rN`, sharing the same ordinal space.
    Result(u32),
    /// A single-subject decision's local (`$rl_m`).
    Decision,
    /// One position of a tuple decision (`$rl_mN`).
    DecisionElement(u32),
}

#[derive(Debug)]
pub(crate) struct Decision {
    pub subjects: Vec<Subject>,
    pub arms: Vec<DecisionArm>,
    pub miss: MissAction,
    pub head: NodeId,
    pub extent: NodeId,
    pub is_async: bool,
    pub kind: DecisionKind,
}

#[derive(Debug)]
pub(crate) enum DecisionKind {
    Match {
        dispatch: MatchDispatch,
        needs_label: bool,
    },
    IfLet,
    LetElse {
        binding_mode: BindingMode,
        direct_variants: Option<Vec<Constructor>>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MatchDispatch {
    Conditional,
    VariantSwitch,
    LiteralSwitch,
}

#[derive(Debug)]
pub(crate) struct Subject {
    pub value: ExprId,
    pub temporary: TempId,
}

#[derive(Debug)]
pub(crate) struct DecisionArm {
    pub pattern: PatternPlan,
    pub guard: Option<ExprId>,
    pub action: ArmAction,
}

#[derive(Debug)]
pub(crate) enum PatternPlan {
    Any,
    Test(Test),
    Bind(Bind),
    AllOf(Vec<PatternPlan>),
    AnyOf(Vec<PatternPlan>),
}

#[derive(Debug)]
pub(crate) enum Test {
    Variant {
        place: Place,
        constructor: Constructor,
    },
    Literal {
        place: Place,
        pattern: crate::hir::PatternId,
    },
}

#[derive(Debug, Clone)]
pub(crate) enum Constructor {
    Resolved { reference: VariantRef, node: NodeId },
    Recovery { node: NodeId, name: String },
}

#[derive(Debug)]
pub(crate) struct Import {
    pub specifier: NodeId,
    pub kind: hir::ImportKind,
}

#[derive(Debug)]
pub(crate) struct Adt {
    pub name: String,
    pub exported: bool,
    pub generics: String,
    pub variants: Vec<AdtVariant>,
}

#[derive(Debug)]
pub(crate) struct AdtVariant {
    pub name: String,
    pub fields: Option<Vec<AdtField>>,
    pub emit_constructor: bool,
}

#[derive(Debug)]
pub(crate) struct AdtField {
    pub name: String,
    pub optional: bool,
    pub ty_text: String,
}

#[derive(Debug, Clone)]
pub(crate) struct Place {
    pub subject: usize,
    pub fields: Vec<FieldAccess>,
}

#[derive(Debug, Clone)]
pub(crate) enum FieldAccess {
    Resolved { reference: FieldRef, node: NodeId },
    Recovery { node: NodeId, name: String },
}

#[derive(Debug)]
pub(crate) struct Bind {
    pub source: Place,
    pub source_field: Option<FieldId>,
    pub binding: NodeId,
}

#[derive(Debug)]
pub(crate) enum ArmAction {
    Yield { body: BodyId, kind: ArmBodyKind },
    Execute(BodyId),
    BindThrough(BindingMode),
}

#[derive(Debug)]
pub(crate) enum MissAction {
    ThrowUnexpected(UnexpectedKind),
    Execute(BodyId),
    Decision(Box<Decision>),
    Nothing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnexpectedKind {
    Case,
    Literal,
    Tuple,
}

#[derive(Debug)]
pub(crate) struct Propagate {
    pub node: NodeId,
    pub value: ExprId,
    pub temporary: TempId,
    pub binding: Option<BindingText>,
    pub exit: ExitTarget,
    pub layout: ResultLayout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExitTarget {
    EnclosingFunction,
    Region,
}

/// The structural Result ABI. It is fixed once in semantic lowering rather
/// than rediscovered by each backend emission site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResultLayout {
    pub success_tag: &'static str,
    pub discriminant_field: &'static str,
    pub payload_field: &'static str,
}

pub(crate) const RESULT_LAYOUT: ResultLayout = ResultLayout {
    success_tag: "Ok",
    discriminant_field: "kind",
    payload_field: "value",
};

#[derive(Debug)]
pub(crate) struct Apply {
    pub node: NodeId,
    pub head: Option<ExprId>,
    pub steps: Vec<ApplyStep>,
}

#[derive(Debug)]
pub(crate) struct ApplyStep {
    pub node: NodeId,
    pub value: ExprId,
    pub mode: ApplyMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ApplyMode {
    Call,
    Postfix,
}

#[derive(Debug)]
pub(crate) struct ResultRegion {
    pub node: NodeId,
    pub items: Vec<ResultRegionItem>,
    pub value: ExprId,
    pub is_async: bool,
}

#[derive(Debug)]
pub(crate) enum ResultRegionItem {
    Statements(BodyId),
    Propagate(Propagate),
}

#[derive(Debug)]
pub(crate) struct Template {
    pub node: NodeId,
    pub parts: Vec<TemplatePart>,
}

#[derive(Debug)]
pub(crate) enum TemplatePart {
    Raw(NodeId),
    Interpolation(ExprId),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lower(source: &str) -> CoreFile {
        let program = crate::parser::parse(source);
        let semantic = crate::analysis::coverage_semantics(&program, &[]);
        lower_semantic(&semantic, source)
    }

    #[test]
    fn every_pattern_surface_is_one_decision_ir() {
        let source = "enum E { A(value: number), B }\n\
            function f(e: E) {\n\
              if let A(value) = e { use(value); }\n\
              const A(value) = e else { return 0; };\n\
              return match (e) { A(value) => value, B => 0 };\n\
            }\n";
        let core = lower(source);
        let statement_decisions = core
            .bodies
            .iter()
            .flat_map(|body| &body.statements)
            .filter(|statement| matches!(statement, Statement::Decision(_)))
            .count();
        let expression_decisions = core
            .exprs
            .iter()
            .filter(|expr| matches!(expr, Expr::Decision(_)))
            .count();
        assert_eq!((statement_decisions, expression_decisions), (2, 1));
    }

    #[test]
    fn try_and_result_binding_are_one_propagate_ir() {
        let source = "function f() {\n\
              const a = try read();\n\
              return result { const b <- parse(a); b };\n\
            }\n";
        let core = lower(source);
        let statements = core
            .bodies
            .iter()
            .flat_map(|body| &body.statements)
            .filter_map(|statement| match statement {
                Statement::Propagate(propagate) => Some(propagate),
                _ => None,
            })
            .count();
        let regions = core
            .exprs
            .iter()
            .filter_map(|expr| match expr {
                Expr::ResultRegion(region) => Some(region),
                _ => None,
            })
            .flat_map(|region| &region.items)
            .filter(|item| matches!(item, ResultRegionItem::Propagate(_)))
            .count();
        assert_eq!((statements, regions), (1, 1));
    }

    #[test]
    fn resolved_patterns_carry_definition_identity() {
        let source = "enum E { A(value: number), B }\n\
            const n = match (e) { A(value) => value, B => 0 };\n";
        let core = lower(source);
        let decision = core
            .exprs
            .iter()
            .find_map(|expr| match expr {
                Expr::Decision(decision) => Some(decision),
                _ => None,
            })
            .expect("decision");
        assert!(decision.arms.iter().all(|arm| {
            fn resolved(plan: &PatternPlan) -> bool {
                match plan {
                    PatternPlan::Any | PatternPlan::Bind(_) => true,
                    PatternPlan::Test(Test::Variant { constructor, .. }) => {
                        matches!(constructor, Constructor::Resolved { .. })
                    }
                    PatternPlan::Test(Test::Literal { .. }) => true,
                    PatternPlan::AllOf(parts) | PatternPlan::AnyOf(parts) => {
                        parts.iter().all(resolved)
                    }
                }
            }
            resolved(&arm.pattern)
        }));
    }

    #[test]
    fn execution_shape_is_fixed_before_target_lowering() {
        let source = "async function f(e: E) {\n\
            const x = match (e) { A => await read(), _ => 0 };\n\
            return result { const y <- await parse(x); y };\n\
        }\n";
        let core = lower(source);
        assert!(
            core.exprs
                .iter()
                .any(|expr| { matches!(expr, Expr::Decision(Decision { is_async: true, .. })) })
        );
        assert!(core.exprs.iter().any(|expr| {
            matches!(
                expr,
                Expr::ResultRegion(ResultRegion { is_async: true, .. })
            )
        }));
    }

    #[test]
    fn target_metadata_is_present_for_every_generated_surface() {
        let source = "const a = try read();\n\
            const p = value |> step;\n\
            const r = result { const b <- parse(a); b };\n";
        let core = lower(source);
        let propagate = core
            .bodies
            .iter()
            .flat_map(|body| &body.statements)
            .find_map(|statement| match statement {
                Statement::Propagate(propagate) => Some(propagate),
                _ => None,
            })
            .expect("propagate");
        assert!(matches!(propagate.temporary, TempId::Statement(_)));
        assert!(core.exprs.iter().any(|expr| {
            matches!(expr, Expr::Apply(Apply { steps, .. }) if steps.iter().all(|step| step.node.0 > 0))
        }));
        assert!(core.exprs.iter().any(|expr| {
            matches!(expr, Expr::ResultRegion(ResultRegion { items, .. }) if items.iter().any(|item| matches!(item, ResultRegionItem::Propagate(Propagate { node, .. }) if node.0 > 0)))
        }));
    }

    #[test]
    fn decision_tree_preserves_boolean_pattern_structure() {
        let source = "const n = match (left, right) {\n\
            (A | B, C | D) => 1,\n\
            _ => 0,\n\
        };\n";
        let core = lower(source);
        let decision = core
            .exprs
            .iter()
            .find_map(|expr| match expr {
                Expr::Decision(decision) => Some(decision),
                _ => None,
            })
            .expect("decision");
        let PatternPlan::AllOf(elements) = &decision.arms[0].pattern else {
            panic!("tuple pattern must remain a conjunction");
        };
        assert_eq!(elements.len(), 2);
        assert!(
            elements
                .iter()
                .all(|element| matches!(element, PatternPlan::AnyOf(alts) if alts.len() == 2))
        );
    }

    #[test]
    fn match_dispatch_is_fixed_before_target_lowering() {
        let source = "enum E { A, B }\n\
            const tagged = match (e) { A => 1, B => 2 };\n\
            const literal = match (n) { 0 => 1, _ => 2 };\n\
            const nested = match (e) { A => 1, B if ready => 2, _ => 3 };\n";
        let core = lower(source);
        let dispatches = core
            .exprs
            .iter()
            .filter_map(|expr| match expr {
                Expr::Decision(Decision {
                    kind: DecisionKind::Match { dispatch, .. },
                    ..
                }) => Some(*dispatch),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            dispatches,
            vec![
                MatchDispatch::VariantSwitch,
                MatchDispatch::LiteralSwitch,
                MatchDispatch::Conditional,
            ]
        );
    }

    #[test]
    fn statement_decision_kind_is_fixed_before_target_lowering() {
        let source = "enum E { A(value: number), B }\n\
            if let A(value) = e { use(value); }\n\
            const A(value) | B = e else { return; };\n";
        let core = lower(source);
        let kinds = core
            .bodies
            .iter()
            .flat_map(|body| &body.statements)
            .filter_map(|statement| match statement {
                Statement::Decision(decision) => Some(&decision.kind),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(matches!(kinds[0], DecisionKind::IfLet));
        assert!(matches!(
            kinds[1],
            DecisionKind::LetElse {
                direct_variants: Some(variants),
                ..
            } if variants.len() == 2
        ));
    }
}
