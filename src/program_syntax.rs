//! Whole-program TypeScript syntax for rl-aware target lowering.
//!
//! The lossless rl parser remains authoritative for claiming rl syntax.
//! This module projects Core IR primitives to category-preserving TypeScript
//! placeholders, parses the complete projection with SWC, and joins every
//! placeholder to its exact SWC parent path and stable minimum host owner.
//!
//! SWC is the compiler's in-process TypeScript **syntax substrate**, not a
//! substitute for TypeScript's type checker. Its whole-program AST supplies
//! the parent/owner/evaluation structure that lowering must retain while it
//! rewrites an rl value. Sending that work to the TypeScript 7 backend would
//! turn a local compiler invariant into an external semantic-service call and
//! would duplicate the source-preserving target model maintained here.
//!
//! TypeScript 7 has a deliberately narrower boundary: the compiler asks it
//! only for facts that syntax cannot prove, such as inferred types, narrowing,
//! and symbol identity (`crate::typescript`). Requiring that backend as part
//! of the toolchain does not transfer syntax ownership away from this SWC AST.

use std::collections::{HashMap, HashSet};

use swc_common::input::StringInput;
use swc_common::sync::Lrc;
use swc_common::{FileName, SourceMap, Spanned};
use swc_ecma_ast::{
    ArrayLit, ArrowExpr, AssignExpr, AwaitExpr, BinExpr, BinaryOp, CallExpr, CondExpr, Constructor,
    Function, Ident, MemberExpr, MemberProp, Module, ModuleItem, NewExpr, ObjectLit, OptCall, Prop,
    PropName, PropOrSpread, ReturnStmt, SeqExpr, Stmt, TaggedTpl, Tpl, UnaryExpr, YieldExpr,
};
use swc_ecma_parser::lexer::Lexer;
use swc_ecma_parser::{Parser, Syntax, TsSyntax};
use swc_ecma_visit::{AstNodePath, AstParentKind, VisitAstPath, VisitWithAstPath, fields};

use crate::analysis::SemanticFile;
use crate::core_ir::{
    Adt, Apply, CoreFile, Decision, Expr, Import, Propagate, ResultRegion, Statement, Template,
    TemplatePart,
};
use crate::hir::ids::Idx;
use crate::hir::{self, BodyId, ExprId, NodeId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// Stable identity assigned to an RL node in the projected syntax overlay.
pub(crate) struct RlNodeId(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
/// Byte coordinate in the original source buffer.
pub(crate) struct SourceByte(usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct ProjectedByte(usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct SourceSpan {
    pub(crate) start: usize,
    pub(crate) end: usize,
}

impl From<hir::Span> for SourceSpan {
    fn from(span: hir::Span) -> Self {
        Self {
            start: span.start,
            end: span.end,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ProjectedSpan {
    start: ProjectedByte,
    end: ProjectedByte,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SyntaxCategory {
    Expression,
    Statement,
    Item,
}

#[derive(Debug)]
struct OverlayEntry {
    id: RlNodeId,
    category: SyntaxCategory,
    source: SourceSpan,
    projected: ProjectedSpan,
    parents: Vec<AstParentKind>,
    context: EvaluationContext,
    protocol: HostEvaluationProtocol,
    core_root: CoreRoot,
    host_owner: HostOwner,
    exits: Vec<HostExit>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HostExit {
    pub(crate) statement: SourceSpan,
    pub(crate) argument: Option<SourceSpan>,
}

/// Ordered JavaScript evaluation obligations between one RL value and its
/// minimum source-backed owner. Target lowering must consume every step.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct HostEvaluationProtocol {
    steps: Vec<HostEvaluationStep>,
}

impl HostEvaluationProtocol {
    pub(crate) fn steps(&self) -> &[HostEvaluationStep] {
        &self.steps
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HostEvaluationStep {
    pub(crate) parent: SourceSpan,
    pub(crate) operation: HostEvaluationOperation,
    pub(crate) inputs: Vec<HostEvaluationInput>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HostEvaluationInput {
    pub(crate) source: SourceSpan,
    pub(crate) mode: EvaluationInputMode,
    pub(crate) receiver: Option<SourceSpan>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EvaluationInputMode {
    Value,
    DirectReference,
    MemberReference,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HostEvaluationOperation {
    Eager(EagerPosition),
    Conditional(ConditionalBranch),
    Reference(ReferencePosition),
    Suspend(SuspensionKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EagerPosition {
    BinaryLeft,
    BinaryRight,
    ArrayElement(u32),
    ObjectEvaluation(u32),
    AssignmentRight,
    SequenceElement(u32),
    UnaryOperand,
    CallArgument(u32),
    ConstructArgument(u32),
    TemplateInterpolation(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConditionalBranch {
    LogicalAndRight,
    LogicalOrRight,
    NullishRight,
    Consequent,
    Alternate,
    OptionalCallArgument(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReferencePosition {
    CallCallee,
    OptionalCallCallee,
    MemberObject,
    MemberProperty,
    ConstructorCallee,
    TaggedTemplateTag,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SuspensionKind {
    Await,
    Yield,
    YieldDelegate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct HostOwnerId(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct HostOwner {
    pub(crate) id: HostOwnerId,
    pub(crate) kind: HostOwnerKind,
    pub(crate) span: SourceSpan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum HostOwnerKind {
    Statement,
    ModuleItem,
}

/// The Core IR node that owns one TypeScript host placeholder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum CoreRoot {
    Adt(NodeId),
    Propagate(NodeId),
    Decision(NodeId),
    Expr(ExprId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EvaluationFrequency {
    Once,
    Conditional,
    Repeated,
    Indeterminate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EvaluationOwner {
    Module,
    FunctionBody,
    ParameterInitializer,
    ClassInitializer,
    StaticBlock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ValueRole {
    None,
    Value,
    AssignmentTarget,
    Pattern,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HostContinuation {
    Return,
    ArrowReturn,
    Initialize,
    Discard,
    Compose,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EvaluationContext {
    pub(crate) frequency: EvaluationFrequency,
    pub(crate) owner: EvaluationOwner,
    pub(crate) value_role: ValueRole,
    pub(crate) continuation: HostContinuation,
}

impl EvaluationContext {
    fn from_path(category: SyntaxCategory, parents: &[AstParentKind]) -> Self {
        let (owner, owner_edge) = evaluation_owner(parents);
        if category != SyntaxCategory::Expression {
            return Self {
                frequency: frequency_within_owner(parents, owner_edge),
                owner,
                value_role: ValueRole::None,
                continuation: HostContinuation::Discard,
            };
        }

        let local_path = &parents[owner_edge..];
        let value_role = value_role(local_path);
        let frequency = frequency_within_owner(parents, owner_edge);
        let continuation = host_continuation(local_path);
        Self {
            frequency,
            owner,
            value_role,
            continuation,
        }
    }
}

fn evaluation_owner(parents: &[AstParentKind]) -> (EvaluationOwner, usize) {
    for (index, parent) in parents.iter().enumerate().rev() {
        match parent {
            AstParentKind::Function(fields::FunctionField::Params(_))
            | AstParentKind::ArrowExpr(fields::ArrowExprField::Params(_))
            | AstParentKind::Constructor(fields::ConstructorField::Params(_)) => {
                return (EvaluationOwner::ParameterInitializer, index + 1);
            }
            AstParentKind::Function(fields::FunctionField::Body)
            | AstParentKind::ArrowExpr(fields::ArrowExprField::Body)
            | AstParentKind::Constructor(fields::ConstructorField::Body) => {
                return (EvaluationOwner::FunctionBody, index + 1);
            }
            AstParentKind::ClassProp(fields::ClassPropField::Value)
            | AstParentKind::PrivateProp(fields::PrivatePropField::Value)
            | AstParentKind::AutoAccessor(fields::AutoAccessorField::Value) => {
                return (EvaluationOwner::ClassInitializer, index + 1);
            }
            AstParentKind::StaticBlock(fields::StaticBlockField::Body) => {
                return (EvaluationOwner::StaticBlock, index + 1);
            }
            _ => {}
        }
    }
    (EvaluationOwner::Module, 0)
}

fn frequency_within_owner(parents: &[AstParentKind], owner_edge: usize) -> EvaluationFrequency {
    let mut frequency = EvaluationFrequency::Once;
    for parent in &parents[owner_edge..] {
        if matches!(
            parent,
            AstParentKind::ForStmt(
                fields::ForStmtField::Test
                    | fields::ForStmtField::Update
                    | fields::ForStmtField::Body
            ) | AstParentKind::ForInStmt(fields::ForInStmtField::Body)
                | AstParentKind::ForOfStmt(fields::ForOfStmtField::Body)
                | AstParentKind::WhileStmt(
                    fields::WhileStmtField::Test | fields::WhileStmtField::Body
                )
                | AstParentKind::DoWhileStmt(
                    fields::DoWhileStmtField::Test | fields::DoWhileStmtField::Body
                )
        ) {
            return EvaluationFrequency::Repeated;
        }
        if matches!(parent, AstParentKind::BinExpr(fields::BinExprField::Right)) {
            frequency = EvaluationFrequency::Indeterminate;
        }
        if matches!(
            parent,
            AstParentKind::CondExpr(fields::CondExprField::Cons | fields::CondExprField::Alt)
                | AstParentKind::IfStmt(fields::IfStmtField::Cons | fields::IfStmtField::Alt)
                | AstParentKind::SwitchCase(fields::SwitchCaseField::Cons(_))
        ) {
            frequency = EvaluationFrequency::Conditional;
        }
    }
    frequency
}

fn value_role(parents: &[AstParentKind]) -> ValueRole {
    if parents.iter().rev().any(|parent| {
        matches!(
            parent,
            AstParentKind::AssignExpr(fields::AssignExprField::Left)
                | AstParentKind::AssignTarget(_)
                | AstParentKind::SimpleAssignTarget(_)
        )
    }) {
        ValueRole::AssignmentTarget
    } else if parents.iter().rev().any(|parent| {
        matches!(
            parent,
            AstParentKind::Pat(_)
                | AstParentKind::ArrayPat(_)
                | AstParentKind::ObjectPat(_)
                | AstParentKind::AssignPat(fields::AssignPatField::Left)
        )
    }) {
        ValueRole::Pattern
    } else {
        ValueRole::Value
    }
}

fn host_continuation(parents: &[AstParentKind]) -> HostContinuation {
    let significant = parents
        .iter()
        .rev()
        .find(|parent| !is_transparent_expression_edge(parent));
    match significant {
        Some(AstParentKind::ReturnStmt(fields::ReturnStmtField::Arg)) => HostContinuation::Return,
        Some(AstParentKind::ArrowFunctionBody(fields::ArrowFunctionBodyField::Expr))
        | Some(AstParentKind::ArrowExpr(fields::ArrowExprField::Body)) => {
            HostContinuation::ArrowReturn
        }
        Some(AstParentKind::VarDeclarator(fields::VarDeclaratorField::Init)) => {
            HostContinuation::Initialize
        }
        Some(AstParentKind::ExprStmt(fields::ExprStmtField::Expr)) => HostContinuation::Discard,
        _ => HostContinuation::Compose,
    }
}

fn is_transparent_expression_edge(parent: &AstParentKind) -> bool {
    matches!(
        parent,
        AstParentKind::Expr(_)
            | AstParentKind::ExprOrSpread(fields::ExprOrSpreadField::Expr)
            | AstParentKind::ParenExpr(fields::ParenExprField::Expr)
            | AstParentKind::TsAsExpr(fields::TsAsExprField::Expr)
            | AstParentKind::TsSatisfiesExpr(fields::TsSatisfiesExprField::Expr)
            | AstParentKind::TsNonNullExpr(fields::TsNonNullExprField::Expr)
            | AstParentKind::TsTypeAssertion(fields::TsTypeAssertionField::Expr)
            | AstParentKind::TsInstantiation(fields::TsInstantiationField::Expr)
    )
}

/// A complete SWC view of one rl-containing TypeScript module.
#[derive(Debug)]
pub(crate) struct ProgramSyntax {
    source_len: usize,
    projection: String,
    module: Module,
    overlay: Vec<OverlayEntry>,
    owners: Vec<HostOwnerSyntax>,
    occupied_names: HashSet<String>,
}

#[derive(Debug)]
pub(crate) struct HostOwnerSyntax {
    pub(crate) owner: HostOwner,
    pub(crate) roots: Vec<RlNodeId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProgramSyntaxError {
    MissingSourceSpan { node: NodeId },
    InvalidSourceSpan { start: SourceByte, end: SourceByte },
    NodeCountOverflow,
    Parse { message: String, projection: String },
    MissingOverlay { id: RlNodeId },
    DuplicateOverlay { id: RlNodeId },
    UnmappedEvaluationSpan { start: usize, end: usize },
}

impl ProgramSyntax {
    /// Builds and validates the shadow program model.
    pub(crate) fn build(
        semantic: &SemanticFile,
        core: &CoreFile,
        source: &str,
    ) -> Result<Self, ProgramSyntaxError> {
        let projection = ProjectionBuilder::new(semantic, core, source).build()?;
        let parsed = parse_module(&projection.code)?;
        let mut collector = ParentCollector::new(
            parsed.start,
            &projection.pending,
            &projection.source_segments,
        );
        let mut path = AstNodePath::default();
        parsed.module.visit_with_ast_path(&mut collector, &mut path);
        let collected = collector.finish(projection.pending)?;
        let syntax = Self {
            source_len: source.len(),
            projection: projection.code,
            module: parsed.module,
            overlay: collected.overlay,
            owners: collected.owners,
            occupied_names: collected.occupied_names,
        };
        syntax.validate()?;
        Ok(syntax)
    }

    /// Returns the Core roots joined to their TypeScript host contexts.
    pub(crate) fn core_contexts(
        &self,
    ) -> impl Iterator<
        Item = (
            CoreRoot,
            RlNodeId,
            EvaluationContext,
            HostEvaluationProtocol,
            SourceSpan,
            HostOwner,
            Vec<HostExit>,
        ),
    > + '_ {
        self.overlay.iter().map(|entry| {
            (
                entry.core_root,
                entry.id,
                entry.context,
                entry.protocol.clone(),
                entry.source,
                entry.host_owner,
                entry.exits.clone(),
            )
        })
    }

    pub(crate) fn owners(&self) -> impl Iterator<Item = &HostOwnerSyntax> {
        self.owners.iter()
    }

    pub(crate) fn occupied_names(&self) -> impl Iterator<Item = &str> {
        self.occupied_names.iter().map(String::as_str)
    }

    fn validate(&self) -> Result<(), ProgramSyntaxError> {
        let _module_span = self.module.span;
        let projection_len = self.projection.len();
        for entry in &self.overlay {
            let start = entry.projected.start.0;
            let end = entry.projected.end.0;
            if start >= end || end > projection_len {
                return Err(ProgramSyntaxError::InvalidSourceSpan {
                    start: SourceByte(entry.source.start),
                    end: SourceByte(entry.source.end),
                });
            }
            if entry.parents.is_empty() {
                return Err(ProgramSyntaxError::MissingOverlay { id: entry.id });
            }
            match entry.category {
                SyntaxCategory::Expression | SyntaxCategory::Statement | SyntaxCategory::Item => {}
            }
            match entry.context.continuation {
                HostContinuation::Return
                | HostContinuation::ArrowReturn
                | HostContinuation::Initialize
                | HostContinuation::Discard
                | HostContinuation::Compose => {}
            }
            for step in entry.protocol.steps() {
                if step.parent.start >= step.parent.end || step.parent.end > self.source_len {
                    return Err(ProgramSyntaxError::InvalidSourceSpan {
                        start: SourceByte(step.parent.start),
                        end: SourceByte(step.parent.end),
                    });
                }
            }
        }
        for (index, owner) in self.owners.iter().enumerate() {
            if owner.owner.id.0 as usize != index || owner.roots.is_empty() {
                return Err(ProgramSyntaxError::NodeCountOverflow);
            }
            if owner.owner.span.start >= owner.owner.span.end
                || owner.owner.span.end > self.source_len
            {
                return Err(ProgramSyntaxError::InvalidSourceSpan {
                    start: SourceByte(owner.owner.span.start),
                    end: SourceByte(owner.owner.span.end),
                });
            }
        }
        Ok(())
    }
}

struct Projection {
    code: String,
    pending: Vec<PendingOverlay>,
    source_segments: Vec<ProjectionSourceSegment>,
}

#[derive(Debug, Clone, Copy)]
struct ProjectionSourceSegment {
    projected: ProjectedSpan,
    source: SourceSpan,
    kind: ProjectionSegmentKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProjectionSegmentKind {
    Copied,
    Placeholder,
}

#[derive(Debug)]
struct PendingOverlay {
    id: RlNodeId,
    category: SyntaxCategory,
    source: SourceSpan,
    projected: ProjectedSpan,
    core_root: CoreRoot,
    marker: OverlayMarker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OverlayMarker {
    Identifier,
    CallExpression,
    DecisionCallExpression,
}

struct ProjectionBuilder<'a> {
    semantic: &'a SemanticFile,
    core: &'a CoreFile,
    source: &'a str,
    code: String,
    pending: Vec<PendingOverlay>,
    source_segments: Vec<ProjectionSourceSegment>,
}

impl<'a> ProjectionBuilder<'a> {
    fn new(semantic: &'a SemanticFile, core: &'a CoreFile, source: &'a str) -> Self {
        Self {
            semantic,
            core,
            source,
            code: String::with_capacity(source.len()),
            pending: Vec::new(),
            source_segments: Vec::new(),
        }
    }

    fn build(mut self) -> Result<Projection, ProgramSyntaxError> {
        self.emit_body(self.core.root)?;
        Ok(Projection {
            code: self.code,
            pending: self.pending,
            source_segments: self.source_segments,
        })
    }

    fn source_span(&self, node: NodeId) -> Result<SourceSpan, ProgramSyntaxError> {
        self.semantic
            .hir
            .source_map
            .node_span(node)
            .map(SourceSpan::from)
            .ok_or(ProgramSyntaxError::MissingSourceSpan { node })
    }

    fn push_source(&mut self, node: NodeId) -> Result<(), ProgramSyntaxError> {
        let span = self.source_span(node)?;
        let text =
            self.source
                .get(span.start..span.end)
                .ok_or(ProgramSyntaxError::InvalidSourceSpan {
                    start: SourceByte(span.start),
                    end: SourceByte(span.end),
                })?;
        let start = ProjectedByte(self.code.len());
        self.code.push_str(text);
        let end = ProjectedByte(self.code.len());
        self.source_segments.push(ProjectionSourceSegment {
            projected: ProjectedSpan { start, end },
            source: span,
            kind: ProjectionSegmentKind::Copied,
        });
        Ok(())
    }

    fn push_placeholder(
        &mut self,
        category: SyntaxCategory,
        source: SourceSpan,
        core_root: CoreRoot,
    ) -> Result<(), ProgramSyntaxError> {
        let ordinal =
            u32::try_from(self.pending.len()).map_err(|_| ProgramSyntaxError::NodeCountOverflow)?;
        let id = RlNodeId(ordinal);
        let owner_start = ProjectedByte(self.code.len());
        match category {
            SyntaxCategory::Expression => self.code.push('('),
            SyntaxCategory::Statement => self.code.push('{'),
            SyntaxCategory::Item => self.code.push_str("const "),
        }
        let start = ProjectedByte(self.code.len());
        let prefix = match category {
            SyntaxCategory::Expression => "$rl_syntax_expr_",
            SyntaxCategory::Statement => "$rl_syntax_stmt_",
            SyntaxCategory::Item => "$rl_syntax_item_",
        };
        self.code.push_str(prefix);
        self.code.push_str(&ordinal.to_string());
        let end = ProjectedByte(self.code.len());
        match category {
            SyntaxCategory::Expression => self.code.push(')'),
            SyntaxCategory::Statement => self.code.push_str(";}"),
            SyntaxCategory::Item => self.code.push_str(" = 0;"),
        }
        let owner_end = ProjectedByte(self.code.len());
        self.source_segments.push(ProjectionSourceSegment {
            projected: ProjectedSpan {
                start: owner_start,
                end: owner_end,
            },
            source,
            kind: ProjectionSegmentKind::Placeholder,
        });
        self.pending.push(PendingOverlay {
            id,
            category,
            source,
            projected: ProjectedSpan { start, end },
            core_root,
            marker: OverlayMarker::Identifier,
        });
        Ok(())
    }

    fn emit_body(&mut self, body: BodyId) -> Result<(), ProgramSyntaxError> {
        for statement in &self.core.bodies[body.index()].statements {
            match statement {
                Statement::Opaque(node) => self.push_source(*node)?,
                Statement::Adt(adt) => self.emit_adt(adt)?,
                Statement::Import(import) => self.emit_import(import)?,
                Statement::Propagate(propagate) => self.emit_propagate(propagate)?,
                Statement::Decision(decision) => self.emit_statement_decision(decision)?,
                Statement::Expr(expr) => self.emit_expr(*expr)?,
            }
        }
        Ok(())
    }

    fn emit_adt(&mut self, adt: &Adt) -> Result<(), ProgramSyntaxError> {
        self.push_placeholder(
            SyntaxCategory::Item,
            self.source_span(adt.node)?,
            CoreRoot::Adt(adt.node),
        )
    }

    fn emit_import(&mut self, import: &Import) -> Result<(), ProgramSyntaxError> {
        self.push_source(import.specifier)
    }

    fn emit_propagate(&mut self, propagate: &Propagate) -> Result<(), ProgramSyntaxError> {
        self.push_placeholder(
            SyntaxCategory::Statement,
            self.source_span(propagate.owner)?,
            CoreRoot::Propagate(propagate.node),
        )
    }

    fn emit_statement_decision(&mut self, decision: &Decision) -> Result<(), ProgramSyntaxError> {
        self.push_placeholder(
            SyntaxCategory::Statement,
            self.source_span(decision.extent)?,
            CoreRoot::Decision(decision.extent),
        )
    }

    fn emit_expr(&mut self, expr: ExprId) -> Result<(), ProgramSyntaxError> {
        match &self.core.exprs[expr.index()] {
            Expr::Opaque(node) => self.push_source(*node),
            Expr::Sequence(body) => self.emit_body(*body),
            Expr::Decision(decision) => self.emit_decision_region(expr, decision),
            Expr::Apply(apply) => self.emit_apply(expr, apply),
            Expr::ResultRegion(region) => self.emit_result_region(expr, region),
            Expr::Template(template) => self.emit_template(template),
        }
    }

    fn emit_apply(&mut self, expr: ExprId, apply: &Apply) -> Result<(), ProgramSyntaxError> {
        let mut source = self.source_span(apply.node)?;
        for step in &apply.steps {
            let step_span = self.source_span(step.node)?;
            source.start = source.start.min(step_span.start);
            source.end = source.end.max(step_span.end);
        }
        self.push_placeholder(SyntaxCategory::Expression, source, CoreRoot::Expr(expr))
    }

    fn emit_result_region(
        &mut self,
        expr: ExprId,
        region: &ResultRegion,
    ) -> Result<(), ProgramSyntaxError> {
        let source = self.source_span(region.node)?;
        let ordinal =
            u32::try_from(self.pending.len()).map_err(|_| ProgramSyntaxError::NodeCountOverflow)?;
        let id = RlNodeId(ordinal);
        let start = ProjectedByte(self.code.len());
        let pending_index = self.pending.len();
        self.pending.push(PendingOverlay {
            id,
            category: SyntaxCategory::Expression,
            source,
            projected: ProjectedSpan { start, end: start },
            core_root: CoreRoot::Expr(expr),
            marker: OverlayMarker::CallExpression,
        });
        self.code.push('(');
        if region.is_async {
            self.code.push_str("async ");
        }
        self.code.push_str("() => {");
        for item in &region.items {
            match item {
                crate::core_ir::ResultRegionItem::Statements(body) => self.emit_body(*body)?,
                crate::core_ir::ResultRegionItem::Propagate(_) => self.code.push(';'),
            }
        }
        self.code.push_str("0;})()");
        let end = ProjectedByte(self.code.len());
        let projected = ProjectedSpan { start, end };
        self.source_segments.insert(
            0,
            ProjectionSourceSegment {
                projected,
                source,
                kind: ProjectionSegmentKind::Placeholder,
            },
        );
        self.pending[pending_index].projected = projected;
        Ok(())
    }

    fn emit_decision_region(
        &mut self,
        expr: ExprId,
        decision: &Decision,
    ) -> Result<(), ProgramSyntaxError> {
        let source = self.source_span(decision.extent)?;
        let ordinal =
            u32::try_from(self.pending.len()).map_err(|_| ProgramSyntaxError::NodeCountOverflow)?;
        let id = RlNodeId(ordinal);
        let start = ProjectedByte(self.code.len());
        let pending_index = self.pending.len();
        self.pending.push(PendingOverlay {
            id,
            category: SyntaxCategory::Expression,
            source,
            projected: ProjectedSpan { start, end: start },
            core_root: CoreRoot::Expr(expr),
            marker: OverlayMarker::DecisionCallExpression,
        });
        self.code.push('(');
        if decision.is_async {
            self.code.push_str("async ");
        }
        self.code.push_str("() => {");
        for subject in &decision.subjects {
            self.code.push('(');
            self.emit_expr(subject.value)?;
            self.code.push_str(");");
        }
        for arm in &decision.arms {
            if let Some(guard) = arm.guard {
                self.code.push('(');
                self.emit_expr(guard)?;
                self.code.push_str(");");
            }
            let crate::core_ir::ArmAction::Yield { body, kind } = arm.action else {
                continue;
            };
            match kind {
                hir::ArmBodyKind::Expression => {
                    self.code.push('(');
                    self.emit_body(body)?;
                    self.code.push_str(");");
                }
                hir::ArmBodyKind::Block => {
                    self.code.push('{');
                    self.emit_body(body)?;
                    self.code.push('}');
                }
            }
        }
        self.code.push_str("0;})()");
        let end = ProjectedByte(self.code.len());
        let projected = ProjectedSpan { start, end };
        self.source_segments.insert(
            0,
            ProjectionSourceSegment {
                projected,
                source,
                kind: ProjectionSegmentKind::Placeholder,
            },
        );
        self.pending[pending_index].projected = projected;
        Ok(())
    }

    fn emit_template(&mut self, template: &Template) -> Result<(), ProgramSyntaxError> {
        for part in &template.parts {
            match part {
                TemplatePart::Raw(node) => self.push_source(*node)?,
                TemplatePart::Interpolation(expr) => {
                    self.code.push_str("${");
                    self.emit_expr(*expr)?;
                    self.code.push('}');
                }
            }
        }
        Ok(())
    }
}

struct ParsedModule {
    module: Module,
    start: u32,
}

fn parse_module(code: &str) -> Result<ParsedModule, ProgramSyntaxError> {
    let source_map: Lrc<SourceMap> = Default::default();
    let file = source_map.new_source_file(Lrc::new(FileName::Anon), code.to_string());
    let start = file.start_pos.0;
    let lexer = Lexer::new(
        Syntax::Typescript(TsSyntax {
            tsx: false,
            decorators: true,
            ..Default::default()
        }),
        Default::default(),
        StringInput::from(&*file),
        None,
    );
    let mut parser = Parser::new_from(lexer);
    let result = parser.parse_module();
    let mut errors = parser.take_errors();
    let module = match result {
        Ok(module) => module,
        Err(error) => {
            errors.push(error);
            return Err(ProgramSyntaxError::Parse {
                message: errors[0].kind().msg().to_string(),
                projection: code.to_owned(),
            });
        }
    };
    if let Some(error) = errors.into_iter().next() {
        return Err(ProgramSyntaxError::Parse {
            message: error.kind().msg().to_string(),
            projection: code.to_owned(),
        });
    }
    Ok(ParsedModule { module, start })
}

struct ParentCollector {
    source_start: u32,
    expected_identifiers: HashMap<ProjectedSpan, RlNodeId>,
    expected_calls: HashMap<ProjectedSpan, RlNodeId>,
    expected_exit_calls: HashSet<RlNodeId>,
    found: HashMap<RlNodeId, FoundOverlay>,
    duplicates: Vec<RlNodeId>,
    source_segments: Vec<ProjectionSourceSegment>,
    host_owners: Vec<ProjectedHostOwner>,
    protocol_frames: Vec<ProjectedProtocolFrame>,
    occupied_names: HashSet<String>,
    function_depth: usize,
    exit_regions: Vec<(RlNodeId, usize)>,
}

struct CollectedProgramSyntax {
    overlay: Vec<OverlayEntry>,
    owners: Vec<HostOwnerSyntax>,
    occupied_names: HashSet<String>,
}

struct FoundOverlay {
    parents: Vec<AstParentKind>,
    host_owners: Vec<ProjectedHostOwner>,
    protocol_frames: Vec<ProjectedProtocolFrame>,
    exits: Vec<ProjectedHostExit>,
}

#[derive(Debug, Clone, Copy)]
struct ProjectedHostExit {
    statement: ProjectedSpan,
    argument: Option<ProjectedSpan>,
}

#[derive(Debug, Clone)]
enum ProjectedProtocolFrame {
    Ordered {
        parent: ProjectedSpan,
        positions: Vec<ProjectedSpan>,
        kind: OrderedEvaluationKind,
    },
    Binary {
        parent: ProjectedSpan,
        operator: BinaryOp,
        left: ProjectedSpan,
        right: ProjectedSpan,
    },
    Conditional {
        parent: ProjectedSpan,
        test: ProjectedSpan,
        consequent: ProjectedSpan,
        alternate: ProjectedSpan,
    },
    Call {
        parent: ProjectedSpan,
        callee: Option<ProjectedSpan>,
        callee_mode: EvaluationInputMode,
        callee_receiver: Option<ProjectedSpan>,
        arguments: Vec<ProjectedSpan>,
        optional: bool,
    },
    Member {
        parent: ProjectedSpan,
        object: ProjectedSpan,
        property: Option<ProjectedSpan>,
    },
    Construct {
        parent: ProjectedSpan,
        callee: ProjectedSpan,
        arguments: Vec<ProjectedSpan>,
    },
    TaggedTemplate {
        parent: ProjectedSpan,
        tag: ProjectedSpan,
        tag_mode: EvaluationInputMode,
        tag_receiver: Option<ProjectedSpan>,
        expressions: Vec<ProjectedSpan>,
    },
    Template {
        parent: ProjectedSpan,
        expressions: Vec<ProjectedSpan>,
    },
    Suspend {
        parent: ProjectedSpan,
        kind: SuspensionKind,
        value: Option<ProjectedSpan>,
    },
}

#[derive(Debug, Clone, Copy)]
enum OrderedEvaluationKind {
    Array,
    Object,
    Assignment,
    Sequence,
    Unary,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct ProjectedHostOwner {
    kind: HostOwnerKind,
    span: ProjectedSpan,
}

fn object_evaluation_positions(node: &ObjectLit, source_start: u32) -> Vec<ProjectedSpan> {
    let mut positions = Vec::new();
    for property in &node.props {
        match property {
            PropOrSpread::Spread(spread) => {
                positions.push(projected_span(spread.expr.span(), source_start));
            }
            PropOrSpread::Prop(property) => match &**property {
                Prop::Shorthand(identifier) => {
                    positions.push(projected_span(identifier.span, source_start));
                }
                Prop::KeyValue(property) => {
                    push_computed_property(&mut positions, &property.key, source_start);
                    positions.push(projected_span(property.value.span(), source_start));
                }
                Prop::Assign(property) => {
                    positions.push(projected_span(property.value.span(), source_start));
                }
                Prop::Getter(property) => {
                    push_computed_property(&mut positions, &property.key, source_start);
                }
                Prop::Setter(property) => {
                    push_computed_property(&mut positions, &property.key, source_start);
                }
                Prop::Method(property) => {
                    push_computed_property(&mut positions, &property.key, source_start);
                }
            },
        }
    }
    positions
}

fn push_computed_property(positions: &mut Vec<ProjectedSpan>, name: &PropName, source_start: u32) {
    if let PropName::Computed(computed) = name {
        positions.push(projected_span(computed.expr.span(), source_start));
    }
}

impl ParentCollector {
    fn new(
        source_start: u32,
        pending: &[PendingOverlay],
        source_segments: &[ProjectionSourceSegment],
    ) -> Self {
        let expected_identifiers = pending
            .iter()
            .filter(|entry| entry.marker == OverlayMarker::Identifier)
            .map(|entry| (entry.projected, entry.id))
            .collect();
        let expected_calls = pending
            .iter()
            .filter(|entry| {
                matches!(
                    entry.marker,
                    OverlayMarker::CallExpression | OverlayMarker::DecisionCallExpression
                )
            })
            .map(|entry| (entry.projected, entry.id))
            .collect();
        let expected_exit_calls = pending
            .iter()
            .filter(|entry| entry.marker == OverlayMarker::DecisionCallExpression)
            .map(|entry| entry.id)
            .collect();
        Self {
            source_start,
            expected_identifiers,
            expected_calls,
            expected_exit_calls,
            found: HashMap::new(),
            duplicates: Vec::new(),
            source_segments: source_segments.to_vec(),
            host_owners: Vec::new(),
            protocol_frames: Vec::new(),
            occupied_names: HashSet::new(),
            function_depth: 0,
            exit_regions: Vec::new(),
        }
    }

    fn record_overlay(&mut self, id: RlNodeId, path: &AstNodePath<'_>) {
        if self
            .found
            .insert(
                id,
                FoundOverlay {
                    parents: path.kinds().to_vec(),
                    host_owners: self.host_owners.clone(),
                    protocol_frames: self.protocol_frames.clone(),
                    exits: Vec::new(),
                },
            )
            .is_some()
        {
            self.duplicates.push(id);
        }
    }

    fn finish(
        mut self,
        pending: Vec<PendingOverlay>,
    ) -> Result<CollectedProgramSyntax, ProgramSyntaxError> {
        if let Some(id) = self.duplicates.into_iter().next() {
            return Err(ProgramSyntaxError::DuplicateOverlay { id });
        }
        let mut owner_ids: HashMap<ProjectedHostOwner, HostOwnerId> = HashMap::new();
        let mut owners: Vec<HostOwnerSyntax> = Vec::new();
        let mut overlay: Vec<OverlayEntry> = Vec::with_capacity(pending.len());
        for entry in pending {
            let found = self
                .found
                .remove(&entry.id)
                .ok_or(ProgramSyntaxError::MissingOverlay { id: entry.id })?;
            let (projected_owner, kind, span) = found
                .host_owners
                .iter()
                .rev()
                .find_map(|owner| {
                    source_span_for_projection(&self.source_segments, owner.span)
                        .map(|span| (*owner, owner.kind, span))
                })
                .ok_or(ProgramSyntaxError::MissingOverlay { id: entry.id })?;
            let owner_id = if let Some(owner_id) = owner_ids.get(&projected_owner).copied() {
                owner_id
            } else {
                let owner_id = HostOwnerId(
                    u32::try_from(owners.len())
                        .map_err(|_| ProgramSyntaxError::NodeCountOverflow)?,
                );
                owner_ids.insert(projected_owner, owner_id);
                owners.push(HostOwnerSyntax {
                    owner: HostOwner {
                        id: owner_id,
                        kind,
                        span,
                    },
                    roots: Vec::new(),
                });
                owner_id
            };
            owners[owner_id.0 as usize].roots.push(entry.id);
            overlay.push(OverlayEntry {
                id: entry.id,
                category: entry.category,
                source: entry.source,
                projected: entry.projected,
                context: EvaluationContext::from_path(entry.category, &found.parents),
                protocol: evaluation_protocol(
                    &self.source_segments,
                    entry.projected,
                    &found.protocol_frames,
                )?,
                core_root: entry.core_root,
                parents: found.parents,
                host_owner: owners[owner_id.0 as usize].owner,
                exits: found
                    .exits
                    .into_iter()
                    .map(|exit| {
                        Ok(HostExit {
                            statement: map_evaluation_span(&self.source_segments, exit.statement)?,
                            argument: exit
                                .argument
                                .map(|argument| {
                                    map_evaluation_span(&self.source_segments, argument)
                                })
                                .transpose()?,
                        })
                    })
                    .collect::<Result<Vec<_>, ProgramSyntaxError>>()?,
            });
        }
        Ok(CollectedProgramSyntax {
            overlay,
            owners,
            occupied_names: self.occupied_names,
        })
    }
}

impl VisitAstPath for ParentCollector {
    fn visit_module_item<'ast: 'r, 'r>(
        &mut self,
        item: &'ast ModuleItem,
        path: &mut AstNodePath<'r>,
    ) {
        self.host_owners.push(ProjectedHostOwner {
            kind: HostOwnerKind::ModuleItem,
            span: projected_span(item.span(), self.source_start),
        });
        <ModuleItem as VisitWithAstPath<Self>>::visit_children_with_ast_path(item, self, path);
        self.host_owners.pop();
    }

    fn visit_stmt<'ast: 'r, 'r>(&mut self, statement: &'ast Stmt, path: &mut AstNodePath<'r>) {
        self.host_owners.push(ProjectedHostOwner {
            kind: HostOwnerKind::Statement,
            span: projected_span(statement.span(), self.source_start),
        });
        <Stmt as VisitWithAstPath<Self>>::visit_children_with_ast_path(statement, self, path);
        self.host_owners.pop();
    }

    fn visit_array_lit<'ast: 'r, 'r>(&mut self, node: &'ast ArrayLit, path: &mut AstNodePath<'r>) {
        self.protocol_frames.push(ProjectedProtocolFrame::Ordered {
            parent: projected_span(node.span, self.source_start),
            positions: node
                .elems
                .iter()
                .flatten()
                .map(|element| projected_span(element.expr.span(), self.source_start))
                .collect(),
            kind: OrderedEvaluationKind::Array,
        });
        <ArrayLit as VisitWithAstPath<Self>>::visit_children_with_ast_path(node, self, path);
        self.protocol_frames.pop();
    }

    fn visit_object_lit<'ast: 'r, 'r>(
        &mut self,
        node: &'ast ObjectLit,
        path: &mut AstNodePath<'r>,
    ) {
        self.protocol_frames.push(ProjectedProtocolFrame::Ordered {
            parent: projected_span(node.span, self.source_start),
            positions: object_evaluation_positions(node, self.source_start),
            kind: OrderedEvaluationKind::Object,
        });
        <ObjectLit as VisitWithAstPath<Self>>::visit_children_with_ast_path(node, self, path);
        self.protocol_frames.pop();
    }

    fn visit_assign_expr<'ast: 'r, 'r>(
        &mut self,
        node: &'ast AssignExpr,
        path: &mut AstNodePath<'r>,
    ) {
        self.protocol_frames.push(ProjectedProtocolFrame::Ordered {
            parent: projected_span(node.span, self.source_start),
            positions: vec![projected_span(node.right.span(), self.source_start)],
            kind: OrderedEvaluationKind::Assignment,
        });
        <AssignExpr as VisitWithAstPath<Self>>::visit_children_with_ast_path(node, self, path);
        self.protocol_frames.pop();
    }

    fn visit_seq_expr<'ast: 'r, 'r>(&mut self, node: &'ast SeqExpr, path: &mut AstNodePath<'r>) {
        self.protocol_frames.push(ProjectedProtocolFrame::Ordered {
            parent: projected_span(node.span, self.source_start),
            positions: node
                .exprs
                .iter()
                .map(|expression| projected_span(expression.span(), self.source_start))
                .collect(),
            kind: OrderedEvaluationKind::Sequence,
        });
        <SeqExpr as VisitWithAstPath<Self>>::visit_children_with_ast_path(node, self, path);
        self.protocol_frames.pop();
    }

    fn visit_unary_expr<'ast: 'r, 'r>(
        &mut self,
        node: &'ast UnaryExpr,
        path: &mut AstNodePath<'r>,
    ) {
        self.protocol_frames.push(ProjectedProtocolFrame::Ordered {
            parent: projected_span(node.span, self.source_start),
            positions: vec![projected_span(node.arg.span(), self.source_start)],
            kind: OrderedEvaluationKind::Unary,
        });
        <UnaryExpr as VisitWithAstPath<Self>>::visit_children_with_ast_path(node, self, path);
        self.protocol_frames.pop();
    }

    fn visit_bin_expr<'ast: 'r, 'r>(&mut self, node: &'ast BinExpr, path: &mut AstNodePath<'r>) {
        self.protocol_frames.push(ProjectedProtocolFrame::Binary {
            parent: projected_span(node.span, self.source_start),
            operator: node.op,
            left: projected_span(node.left.span(), self.source_start),
            right: projected_span(node.right.span(), self.source_start),
        });
        <BinExpr as VisitWithAstPath<Self>>::visit_children_with_ast_path(node, self, path);
        self.protocol_frames.pop();
    }

    fn visit_cond_expr<'ast: 'r, 'r>(&mut self, node: &'ast CondExpr, path: &mut AstNodePath<'r>) {
        self.protocol_frames
            .push(ProjectedProtocolFrame::Conditional {
                parent: projected_span(node.span, self.source_start),
                test: projected_span(node.test.span(), self.source_start),
                consequent: projected_span(node.cons.span(), self.source_start),
                alternate: projected_span(node.alt.span(), self.source_start),
            });
        <CondExpr as VisitWithAstPath<Self>>::visit_children_with_ast_path(node, self, path);
        self.protocol_frames.pop();
    }

    fn visit_call_expr<'ast: 'r, 'r>(&mut self, node: &'ast CallExpr, path: &mut AstNodePath<'r>) {
        let span = projected_span(node.span, self.source_start);
        if let Some(id) = self.expected_calls.get(&span).copied() {
            self.record_overlay(id, path);
            let collects_exits = self.expected_exit_calls.contains(&id);
            if collects_exits {
                self.exit_regions.push((id, self.function_depth + 1));
            }
            <CallExpr as VisitWithAstPath<Self>>::visit_children_with_ast_path(node, self, path);
            if collects_exits {
                self.exit_regions.pop();
            }
            return;
        }
        let (callee_mode, callee_receiver) = match &node.callee {
            swc_ecma_ast::Callee::Expr(expression) => call_callee_mode(expression),
            swc_ecma_ast::Callee::Super(_) | swc_ecma_ast::Callee::Import(_) => {
                (EvaluationInputMode::MemberReference, None)
            }
        };
        self.protocol_frames.push(ProjectedProtocolFrame::Call {
            parent: span,
            callee: Some(projected_span(
                match &node.callee {
                    swc_ecma_ast::Callee::Expr(expression) => reference_value_span(expression),
                    swc_ecma_ast::Callee::Super(_) | swc_ecma_ast::Callee::Import(_) => {
                        node.callee.span()
                    }
                },
                self.source_start,
            )),
            callee_mode,
            callee_receiver: callee_receiver.map(|span| projected_span(span, self.source_start)),
            arguments: node
                .args
                .iter()
                .map(|argument| projected_span(argument.span(), self.source_start))
                .collect(),
            optional: false,
        });
        <CallExpr as VisitWithAstPath<Self>>::visit_children_with_ast_path(node, self, path);
        self.protocol_frames.pop();
    }

    fn visit_arrow_expr<'ast: 'r, 'r>(
        &mut self,
        node: &'ast ArrowExpr,
        path: &mut AstNodePath<'r>,
    ) {
        self.function_depth += 1;
        <ArrowExpr as VisitWithAstPath<Self>>::visit_children_with_ast_path(node, self, path);
        self.function_depth -= 1;
    }

    fn visit_function<'ast: 'r, 'r>(&mut self, node: &'ast Function, path: &mut AstNodePath<'r>) {
        self.function_depth += 1;
        <Function as VisitWithAstPath<Self>>::visit_children_with_ast_path(node, self, path);
        self.function_depth -= 1;
    }

    fn visit_constructor<'ast: 'r, 'r>(
        &mut self,
        node: &'ast Constructor,
        path: &mut AstNodePath<'r>,
    ) {
        self.function_depth += 1;
        <Constructor as VisitWithAstPath<Self>>::visit_children_with_ast_path(node, self, path);
        self.function_depth -= 1;
    }

    fn visit_return_stmt<'ast: 'r, 'r>(
        &mut self,
        node: &'ast ReturnStmt,
        path: &mut AstNodePath<'r>,
    ) {
        if let Some((id, target_depth)) = self.exit_regions.last().copied()
            && target_depth == self.function_depth
            && let Some(found) = self.found.get_mut(&id)
        {
            found.exits.push(ProjectedHostExit {
                statement: projected_span(node.span, self.source_start),
                argument: node
                    .arg
                    .as_ref()
                    .map(|argument| projected_span(argument.span(), self.source_start)),
            });
        }
        <ReturnStmt as VisitWithAstPath<Self>>::visit_children_with_ast_path(node, self, path);
    }

    fn visit_opt_call<'ast: 'r, 'r>(&mut self, node: &'ast OptCall, path: &mut AstNodePath<'r>) {
        let (callee_mode, callee_receiver) = call_callee_mode(&node.callee);
        self.protocol_frames.push(ProjectedProtocolFrame::Call {
            parent: projected_span(node.span, self.source_start),
            callee: Some(projected_span(
                reference_value_span(&node.callee),
                self.source_start,
            )),
            callee_mode,
            callee_receiver: callee_receiver.map(|span| projected_span(span, self.source_start)),
            arguments: node
                .args
                .iter()
                .map(|argument| projected_span(argument.span(), self.source_start))
                .collect(),
            optional: true,
        });
        <OptCall as VisitWithAstPath<Self>>::visit_children_with_ast_path(node, self, path);
        self.protocol_frames.pop();
    }

    fn visit_member_expr<'ast: 'r, 'r>(
        &mut self,
        node: &'ast MemberExpr,
        path: &mut AstNodePath<'r>,
    ) {
        let property = match &node.prop {
            MemberProp::Computed(property) => {
                Some(projected_span(property.expr.span(), self.source_start))
            }
            MemberProp::Ident(_) | MemberProp::PrivateName(_) => None,
        };
        self.protocol_frames.push(ProjectedProtocolFrame::Member {
            parent: projected_span(node.span, self.source_start),
            object: projected_span(node.obj.span(), self.source_start),
            property,
        });
        <MemberExpr as VisitWithAstPath<Self>>::visit_children_with_ast_path(node, self, path);
        self.protocol_frames.pop();
    }

    fn visit_new_expr<'ast: 'r, 'r>(&mut self, node: &'ast NewExpr, path: &mut AstNodePath<'r>) {
        self.protocol_frames
            .push(ProjectedProtocolFrame::Construct {
                parent: projected_span(node.span, self.source_start),
                callee: projected_span(reference_value_span(&node.callee), self.source_start),
                arguments: node
                    .args
                    .iter()
                    .flatten()
                    .map(|argument| projected_span(argument.span(), self.source_start))
                    .collect(),
            });
        <NewExpr as VisitWithAstPath<Self>>::visit_children_with_ast_path(node, self, path);
        self.protocol_frames.pop();
    }

    fn visit_tagged_tpl<'ast: 'r, 'r>(
        &mut self,
        node: &'ast TaggedTpl,
        path: &mut AstNodePath<'r>,
    ) {
        let (tag_mode, tag_receiver) = call_callee_mode(&node.tag);
        self.protocol_frames
            .push(ProjectedProtocolFrame::TaggedTemplate {
                parent: projected_span(node.span, self.source_start),
                tag: projected_span(reference_value_span(&node.tag), self.source_start),
                tag_mode,
                tag_receiver: tag_receiver.map(|span| projected_span(span, self.source_start)),
                expressions: node
                    .tpl
                    .exprs
                    .iter()
                    .map(|expression| projected_span(expression.span(), self.source_start))
                    .collect(),
            });
        <TaggedTpl as VisitWithAstPath<Self>>::visit_children_with_ast_path(node, self, path);
        self.protocol_frames.pop();
    }

    fn visit_tpl<'ast: 'r, 'r>(&mut self, node: &'ast Tpl, path: &mut AstNodePath<'r>) {
        self.protocol_frames.push(ProjectedProtocolFrame::Template {
            parent: projected_span(node.span, self.source_start),
            expressions: node
                .exprs
                .iter()
                .map(|expression| projected_span(expression.span(), self.source_start))
                .collect(),
        });
        <Tpl as VisitWithAstPath<Self>>::visit_children_with_ast_path(node, self, path);
        self.protocol_frames.pop();
    }

    fn visit_await_expr<'ast: 'r, 'r>(
        &mut self,
        node: &'ast AwaitExpr,
        path: &mut AstNodePath<'r>,
    ) {
        self.protocol_frames.push(ProjectedProtocolFrame::Suspend {
            parent: projected_span(node.span, self.source_start),
            kind: SuspensionKind::Await,
            value: Some(projected_span(node.arg.span(), self.source_start)),
        });
        <AwaitExpr as VisitWithAstPath<Self>>::visit_children_with_ast_path(node, self, path);
        self.protocol_frames.pop();
    }

    fn visit_yield_expr<'ast: 'r, 'r>(
        &mut self,
        node: &'ast YieldExpr,
        path: &mut AstNodePath<'r>,
    ) {
        self.protocol_frames.push(ProjectedProtocolFrame::Suspend {
            parent: projected_span(node.span, self.source_start),
            kind: if node.delegate {
                SuspensionKind::YieldDelegate
            } else {
                SuspensionKind::Yield
            },
            value: node
                .arg
                .as_ref()
                .map(|value| projected_span(value.span(), self.source_start)),
        });
        <YieldExpr as VisitWithAstPath<Self>>::visit_children_with_ast_path(node, self, path);
        self.protocol_frames.pop();
    }

    fn visit_ident<'ast: 'r, 'r>(&mut self, ident: &'ast Ident, path: &mut AstNodePath<'r>) {
        self.occupied_names.insert(ident.sym.to_string());
        let start = ident.span.lo.0.saturating_sub(self.source_start) as usize;
        let end = ident.span.hi.0.saturating_sub(self.source_start) as usize;
        let projected = ProjectedSpan {
            start: ProjectedByte(start),
            end: ProjectedByte(end),
        };
        let Some(id) = self.expected_identifiers.get(&projected).copied() else {
            return;
        };
        self.record_overlay(id, path);
    }
}

fn evaluation_protocol(
    segments: &[ProjectionSourceSegment],
    value: ProjectedSpan,
    frames: &[ProjectedProtocolFrame],
) -> Result<HostEvaluationProtocol, ProgramSyntaxError> {
    let steps = frames
        .iter()
        .rev()
        .map(|frame| protocol_step(segments, value, frame))
        .collect::<Result<Vec<_>, ProgramSyntaxError>>()?
        .into_iter()
        .flatten()
        .collect();
    Ok(HostEvaluationProtocol { steps })
}

fn protocol_step(
    segments: &[ProjectionSourceSegment],
    value: ProjectedSpan,
    frame: &ProjectedProtocolFrame,
) -> Result<Option<HostEvaluationStep>, ProgramSyntaxError> {
    let (parent, operation, inputs) = match frame {
        ProjectedProtocolFrame::Ordered {
            parent,
            positions,
            kind,
        } => {
            let Some(position) = child_index(positions, value) else {
                return Ok(None);
            };
            let index =
                u32::try_from(position).map_err(|_| ProgramSyntaxError::NodeCountOverflow)?;
            let operation = HostEvaluationOperation::Eager(match kind {
                OrderedEvaluationKind::Array => EagerPosition::ArrayElement(index),
                OrderedEvaluationKind::Object => EagerPosition::ObjectEvaluation(index),
                OrderedEvaluationKind::Assignment => EagerPosition::AssignmentRight,
                OrderedEvaluationKind::Sequence => EagerPosition::SequenceElement(index),
                OrderedEvaluationKind::Unary => EagerPosition::UnaryOperand,
            });
            let inputs = positions[..position]
                .iter()
                .copied()
                .map(|input| (input, EvaluationInputMode::Value, None))
                .collect();
            (*parent, operation, inputs)
        }
        ProjectedProtocolFrame::Binary { parent, left, .. } if projected_contains(*left, value) => {
            (
                *parent,
                HostEvaluationOperation::Eager(EagerPosition::BinaryLeft),
                Vec::new(),
            )
        }
        ProjectedProtocolFrame::Binary {
            parent,
            operator,
            left,
            right,
            ..
        } if projected_contains(*right, value) => {
            let operation = match operator {
                BinaryOp::LogicalAnd => {
                    HostEvaluationOperation::Conditional(ConditionalBranch::LogicalAndRight)
                }
                BinaryOp::LogicalOr => {
                    HostEvaluationOperation::Conditional(ConditionalBranch::LogicalOrRight)
                }
                BinaryOp::NullishCoalescing => {
                    HostEvaluationOperation::Conditional(ConditionalBranch::NullishRight)
                }
                _ => HostEvaluationOperation::Eager(EagerPosition::BinaryRight),
            };
            (
                *parent,
                operation,
                vec![(*left, EvaluationInputMode::Value, None)],
            )
        }
        ProjectedProtocolFrame::Conditional {
            parent,
            test,
            consequent,
            ..
        } if projected_contains(*consequent, value) => (
            *parent,
            HostEvaluationOperation::Conditional(ConditionalBranch::Consequent),
            vec![(*test, EvaluationInputMode::Value, None)],
        ),
        ProjectedProtocolFrame::Conditional {
            parent,
            test,
            alternate,
            ..
        } if projected_contains(*alternate, value) => (
            *parent,
            HostEvaluationOperation::Conditional(ConditionalBranch::Alternate),
            vec![(*test, EvaluationInputMode::Value, None)],
        ),
        ProjectedProtocolFrame::Call {
            parent,
            callee: Some(callee),
            optional,
            ..
        } if projected_contains(*callee, value) => (
            *parent,
            HostEvaluationOperation::Reference(if *optional {
                ReferencePosition::OptionalCallCallee
            } else {
                ReferencePosition::CallCallee
            }),
            Vec::new(),
        ),
        ProjectedProtocolFrame::Call {
            parent,
            callee,
            callee_mode,
            callee_receiver,
            arguments,
            optional,
            ..
        } => {
            let Some(position) = child_index(arguments, value) else {
                return Ok(None);
            };
            let index =
                u32::try_from(position).map_err(|_| ProgramSyntaxError::NodeCountOverflow)?;
            let operation = if *optional {
                HostEvaluationOperation::Conditional(ConditionalBranch::OptionalCallArgument(index))
            } else {
                HostEvaluationOperation::Eager(EagerPosition::CallArgument(index))
            };
            let inputs = callee
                .iter()
                .copied()
                .map(|callee| (callee, *callee_mode, *callee_receiver))
                .chain(
                    arguments[..position]
                        .iter()
                        .copied()
                        .map(|argument| (argument, EvaluationInputMode::Value, None)),
                )
                .collect();
            (*parent, operation, inputs)
        }
        ProjectedProtocolFrame::Member { parent, object, .. }
            if projected_contains(*object, value) =>
        {
            (
                *parent,
                HostEvaluationOperation::Reference(ReferencePosition::MemberObject),
                Vec::new(),
            )
        }
        ProjectedProtocolFrame::Member {
            parent,
            object,
            property: Some(property),
        } if projected_contains(*property, value) => (
            *parent,
            HostEvaluationOperation::Reference(ReferencePosition::MemberProperty),
            vec![(*object, EvaluationInputMode::Value, None)],
        ),
        ProjectedProtocolFrame::Construct { parent, callee, .. }
            if projected_contains(*callee, value) =>
        {
            (
                *parent,
                HostEvaluationOperation::Reference(ReferencePosition::ConstructorCallee),
                Vec::new(),
            )
        }
        ProjectedProtocolFrame::Construct {
            parent,
            callee,
            arguments,
        } => {
            let Some(position) = child_index(arguments, value) else {
                return Ok(None);
            };
            let index =
                u32::try_from(position).map_err(|_| ProgramSyntaxError::NodeCountOverflow)?;
            let inputs = std::iter::once((*callee, EvaluationInputMode::Value, None))
                .chain(
                    arguments[..position]
                        .iter()
                        .copied()
                        .map(|argument| (argument, EvaluationInputMode::Value, None)),
                )
                .collect();
            (
                *parent,
                HostEvaluationOperation::Eager(EagerPosition::ConstructArgument(index)),
                inputs,
            )
        }
        ProjectedProtocolFrame::TaggedTemplate { parent, tag, .. }
            if projected_contains(*tag, value) =>
        {
            (
                *parent,
                HostEvaluationOperation::Reference(ReferencePosition::TaggedTemplateTag),
                Vec::new(),
            )
        }
        ProjectedProtocolFrame::TaggedTemplate {
            parent,
            tag,
            tag_mode,
            tag_receiver,
            expressions,
        } => {
            let Some(position) = child_index(expressions, value) else {
                return Ok(None);
            };
            let index =
                u32::try_from(position).map_err(|_| ProgramSyntaxError::NodeCountOverflow)?;
            let inputs = std::iter::once((*tag, *tag_mode, *tag_receiver))
                .chain(
                    expressions[..position]
                        .iter()
                        .copied()
                        .map(|expression| (expression, EvaluationInputMode::Value, None)),
                )
                .collect();
            (
                *parent,
                HostEvaluationOperation::Eager(EagerPosition::TemplateInterpolation(index)),
                inputs,
            )
        }
        ProjectedProtocolFrame::Template {
            parent,
            expressions,
        } => {
            let Some(position) = child_index(expressions, value) else {
                return Ok(None);
            };
            let index =
                u32::try_from(position).map_err(|_| ProgramSyntaxError::NodeCountOverflow)?;
            let inputs = expressions[..position]
                .iter()
                .copied()
                .map(|expression| (expression, EvaluationInputMode::Value, None))
                .collect();
            (
                *parent,
                HostEvaluationOperation::Eager(EagerPosition::TemplateInterpolation(index)),
                inputs,
            )
        }
        ProjectedProtocolFrame::Suspend {
            parent,
            kind,
            value: Some(argument),
        } if projected_contains(*argument, value) => {
            (*parent, HostEvaluationOperation::Suspend(*kind), Vec::new())
        }
        _ => return Ok(None),
    };
    let parent = map_evaluation_span(segments, parent)?;
    let inputs = inputs
        .into_iter()
        .map(|(source, mode, receiver)| {
            Ok(HostEvaluationInput {
                source: map_evaluation_span(segments, source)?,
                mode,
                receiver: receiver
                    .map(|receiver| map_evaluation_span(segments, receiver))
                    .transpose()?,
            })
        })
        .collect::<Result<Vec<_>, ProgramSyntaxError>>()?;
    Ok(Some(HostEvaluationStep {
        parent,
        operation,
        inputs,
    }))
}

fn map_evaluation_span(
    segments: &[ProjectionSourceSegment],
    projected: ProjectedSpan,
) -> Result<SourceSpan, ProgramSyntaxError> {
    source_span_for_projection(segments, projected).ok_or(
        ProgramSyntaxError::UnmappedEvaluationSpan {
            start: projected.start.0,
            end: projected.end.0,
        },
    )
}

fn projected_contains(container: ProjectedSpan, value: ProjectedSpan) -> bool {
    container.start <= value.start && value.end <= container.end
}

fn call_callee_mode(
    expression: &swc_ecma_ast::Expr,
) -> (EvaluationInputMode, Option<swc_common::Span>) {
    use swc_ecma_ast::{Expr as SwcExpr, OptChainBase};

    match expression {
        SwcExpr::Member(member) => (
            EvaluationInputMode::MemberReference,
            Some(member.obj.span()),
        ),
        SwcExpr::SuperProp(_) => (EvaluationInputMode::MemberReference, None),
        SwcExpr::OptChain(chain) => match &*chain.base {
            OptChainBase::Member(member) => (
                EvaluationInputMode::MemberReference,
                Some(member.obj.span()),
            ),
            OptChainBase::Call(_) => (EvaluationInputMode::DirectReference, None),
        },
        SwcExpr::Paren(paren) => call_callee_mode(&paren.expr),
        SwcExpr::TsAs(expression) => call_callee_mode(&expression.expr),
        SwcExpr::TsTypeAssertion(expression) => call_callee_mode(&expression.expr),
        SwcExpr::TsNonNull(expression) => call_callee_mode(&expression.expr),
        SwcExpr::TsInstantiation(expression) => call_callee_mode(&expression.expr),
        SwcExpr::TsSatisfies(expression) => call_callee_mode(&expression.expr),
        _ => (EvaluationInputMode::DirectReference, None),
    }
}

fn reference_value_span(expression: &swc_ecma_ast::Expr) -> swc_common::Span {
    use swc_ecma_ast::Expr as SwcExpr;

    match expression {
        SwcExpr::Paren(expression) => reference_value_span(&expression.expr),
        SwcExpr::TsAs(expression) => reference_value_span(&expression.expr),
        SwcExpr::TsTypeAssertion(expression) => reference_value_span(&expression.expr),
        SwcExpr::TsNonNull(expression) => reference_value_span(&expression.expr),
        SwcExpr::TsInstantiation(expression) => reference_value_span(&expression.expr),
        SwcExpr::TsSatisfies(expression) => reference_value_span(&expression.expr),
        _ => expression.span(),
    }
}

fn child_index(children: &[ProjectedSpan], value: ProjectedSpan) -> Option<usize> {
    children
        .iter()
        .position(|child| projected_contains(*child, value))
}

fn projected_span(span: swc_common::Span, source_start: u32) -> ProjectedSpan {
    ProjectedSpan {
        start: ProjectedByte(span.lo.0.saturating_sub(source_start) as usize),
        end: ProjectedByte(span.hi.0.saturating_sub(source_start) as usize),
    }
}

fn source_span_for_projection(
    segments: &[ProjectionSourceSegment],
    projected: ProjectedSpan,
) -> Option<SourceSpan> {
    let start = segments.iter().find_map(|segment| {
        if projected.start == segment.projected.start {
            Some(segment.source.start)
        } else if segment.projected.start < projected.start
            && projected.start < segment.projected.end
            && segment.kind == ProjectionSegmentKind::Copied
        {
            Some(segment.source.start + projected.start.0 - segment.projected.start.0)
        } else {
            None
        }
    })?;
    let end = segments.iter().find_map(|segment| {
        if projected.end == segment.projected.end {
            Some(segment.source.end)
        } else if segment.projected.start < projected.end
            && projected.end < segment.projected.end
            && segment.kind == ProjectionSegmentKind::Copied
        {
            Some(segment.source.start + projected.end.0 - segment.projected.start.0)
        } else {
            None
        }
    })?;
    Some(SourceSpan { start, end })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn syntax(source: &str) -> ProgramSyntax {
        let program = crate::parser::parse(source);
        let semantic = crate::analysis::coverage_semantics(&program, &[]);
        let core = crate::core_ir::lower_semantic(&semantic, source);
        ProgramSyntax::build(&semantic, &core, source).expect("projection should parse")
    }

    #[test]
    fn plain_typescript_projection_is_byte_identical() {
        let source = "// 주석\nconst value: { readonly x: number } = { x: 1 };\n";
        let syntax = syntax(source);
        assert_eq!(syntax.projection, source);
        assert!(syntax.overlay.is_empty());
    }

    #[test]
    fn a_match_argument_keeps_its_call_parent_path() {
        let syntax = syntax(
            "enum E { A, B(x: number) }\nconst out = render(1, match (e) { A => 0, B(x) => x });\n",
        );
        let entry = syntax
            .overlay
            .iter()
            .find(|entry| entry.category == SyntaxCategory::Expression)
            .expect("match overlay");
        assert!(
            entry
                .parents
                .iter()
                .any(|parent| matches!(parent, AstParentKind::CallExpr(_))),
            "{:?}",
            entry.parents
        );
        assert_eq!(entry.context.continuation, HostContinuation::Compose);
    }

    #[test]
    fn a_direct_return_is_a_function_statement_continuation() {
        let source = "enum E { A, B }\nfunction f(e: E) { return match (e) { A => 1, B => 2 }; }\n";
        let syntax = syntax(source);
        let context = syntax
            .overlay
            .iter()
            .find(|entry| entry.category == SyntaxCategory::Expression)
            .expect("match overlay")
            .context;
        assert_eq!(context.owner, EvaluationOwner::FunctionBody);
        assert_eq!(context.frequency, EvaluationFrequency::Once);
        assert_eq!(context.value_role, ValueRole::Value);
        assert_eq!(context.continuation, HostContinuation::Return);
        let entry = syntax
            .overlay
            .iter()
            .find(|entry| entry.category == SyntaxCategory::Expression)
            .expect("match overlay");
        let owner = entry.host_owner.span;
        assert_eq!(&source[owner.start..owner.start + 6], "return");
        assert_eq!(&source[owner.end - 1..owner.end], ";");
    }

    #[test]
    fn an_expression_bodied_arrow_has_an_arrow_return_continuation() {
        let syntax = syntax("enum E { A, B }\nconst f = (e: E) => match (e) { A => 1, B => 2 };\n");
        let entry = syntax
            .overlay
            .iter()
            .find(|entry| entry.category == SyntaxCategory::Expression)
            .expect("match overlay");
        let context = entry.context;
        assert_eq!(context.owner, EvaluationOwner::FunctionBody);
        assert_eq!(
            context.continuation,
            HostContinuation::ArrowReturn,
            "{:?}",
            entry.parents
        );
        assert!(entry.protocol.steps().is_empty(), "{:?}", entry.protocol);
    }

    #[test]
    fn a_nested_decision_keeps_the_outer_initializer_owner() {
        let syntax = syntax(
            "enum E { A, B }\nconst value = match (outer) { A => match (inner) { A => 1, B => 2 }, B => 0 };\n",
        );
        let entries = syntax
            .overlay
            .iter()
            .filter(|entry| matches!(entry.core_root, CoreRoot::Expr(_)))
            .collect::<Vec<_>>();
        assert_eq!(entries.len(), 2, "{:#?}", syntax.overlay);
        assert_eq!(
            entries[0].context.continuation,
            HostContinuation::Initialize
        );
        assert_eq!(entries[1].context.continuation, HostContinuation::Discard);
    }

    #[test]
    fn a_conditional_branch_stays_conditional() {
        let syntax =
            syntax("enum E { A, B }\nconst out = flag ? match (e) { A => 1, B => 2 } : 0;\n");
        let context = syntax
            .overlay
            .iter()
            .find(|entry| entry.category == SyntaxCategory::Expression)
            .expect("match overlay")
            .context;
        assert_eq!(context.frequency, EvaluationFrequency::Conditional);
        assert_eq!(context.continuation, HostContinuation::Compose);
    }

    #[test]
    fn an_eager_binary_rhs_has_an_explicit_evaluation_step() {
        let syntax = syntax("enum E { A, B }\nconst out = left + match (e) { A => 1, B => 2 };\n");
        let entry = syntax
            .overlay
            .iter()
            .find(|entry| entry.category == SyntaxCategory::Expression)
            .expect("match overlay");
        assert_eq!(entry.context.frequency, EvaluationFrequency::Indeterminate);
        assert_eq!(entry.context.continuation, HostContinuation::Compose);
        assert!(entry.protocol.steps().iter().any(|step| {
            step.operation == HostEvaluationOperation::Eager(EagerPosition::BinaryRight)
        }));
    }

    #[test]
    fn a_whole_variable_initializer_has_an_initialize_continuation() {
        let syntax = syntax("enum E { A, B }\nconst out = match (e) { A => 1, B => 2 };\n");
        let entry = syntax
            .overlay
            .iter()
            .find(|entry| entry.category == SyntaxCategory::Expression)
            .expect("match overlay");
        assert_eq!(entry.context.continuation, HostContinuation::Initialize);
        assert!(entry.protocol.steps().is_empty());
    }

    #[test]
    fn a_short_circuit_rhs_is_a_conditional_protocol_branch() {
        let syntax =
            syntax("enum E { A, B }\nconst out = ready && match (e) { A => 1, B => 2 };\n");
        let entry = syntax
            .overlay
            .iter()
            .find(|entry| entry.category == SyntaxCategory::Expression)
            .expect("match overlay");
        assert!(entry.protocol.steps().iter().any(|step| {
            step.operation
                == HostEvaluationOperation::Conditional(ConditionalBranch::LogicalAndRight)
        }));
    }

    #[test]
    fn a_member_call_preserves_the_reference_chain() {
        let syntax =
            syntax("enum E { A, B }\nconst out = (match (e) { A => left, B => right }).run();\n");
        let entry = syntax
            .overlay
            .iter()
            .find(|entry| entry.category == SyntaxCategory::Expression)
            .expect("match overlay");
        let references: Vec<_> = entry
            .protocol
            .steps()
            .iter()
            .filter_map(|step| match step.operation {
                HostEvaluationOperation::Reference(reference) => Some(reference),
                _ => None,
            })
            .collect();
        assert_eq!(
            references,
            vec![
                ReferencePosition::MemberObject,
                ReferencePosition::CallCallee
            ]
        );
    }

    #[test]
    fn a_call_argument_keeps_left_to_right_argument_position() {
        let syntax =
            syntax("enum E { A, B }\nconst out = render(first(), match (e) { A => 1, B => 2 });\n");
        let entry = syntax
            .overlay
            .iter()
            .find(|entry| entry.category == SyntaxCategory::Expression)
            .expect("match overlay");
        assert!(entry.protocol.steps().iter().any(|step| {
            step.operation == HostEvaluationOperation::Eager(EagerPosition::CallArgument(1))
        }));
    }

    #[test]
    fn an_await_operand_records_the_suspension_point() {
        let syntax = syntax(
            "enum E { A, B }\nasync function f(e: E) { return await match (e) { A => 1, B => 2 }; }\n",
        );
        let entry = syntax
            .overlay
            .iter()
            .find(|entry| entry.category == SyntaxCategory::Expression)
            .expect("match overlay");
        assert!(entry.protocol.steps().iter().any(|step| {
            step.operation == HostEvaluationOperation::Suspend(SuspensionKind::Await)
        }));
    }

    #[test]
    fn a_parameter_initializer_is_a_composed_owner_value() {
        let syntax = syntax(
            "enum E { A, B }\nfunction f(e: E, x = match (e) { A => 1, B => 2 }) { return x; }\n",
        );
        let context = syntax
            .overlay
            .iter()
            .find(|entry| entry.category == SyntaxCategory::Expression)
            .expect("match overlay")
            .context;
        assert_eq!(context.owner, EvaluationOwner::ParameterInitializer);
        assert_eq!(context.continuation, HostContinuation::Compose);
    }

    #[test]
    fn an_expression_in_a_loop_keeps_repeated_frequency() {
        let syntax =
            syntax("enum E { A, B }\nwhile (ready()) { consume(match (e) { A => 1, B => 2 }); }\n");
        let context = syntax
            .overlay
            .iter()
            .find(|entry| entry.category == SyntaxCategory::Expression)
            .expect("match overlay")
            .context;
        assert_eq!(context.frequency, EvaluationFrequency::Repeated);
    }

    #[test]
    fn every_core_surface_gets_a_typed_overlay() {
        let syntax = syntax(
            "enum E { A(x: number), B }\n\
             function f(e: E) {\n\
               if let A(x) = e { use(x); }\n\
               const A(y) = e else { return 0; };\n\
               const r = result { const z <- load(); z };\n\
               return match (e) { A(x) => x, B => r } |> done;\n\
             }\n",
        );
        assert!(
            syntax
                .overlay
                .iter()
                .any(|entry| entry.category == SyntaxCategory::Item)
        );
        assert!(
            syntax
                .overlay
                .iter()
                .any(|entry| entry.category == SyntaxCategory::Statement)
        );
        assert!(
            syntax
                .overlay
                .iter()
                .any(|entry| entry.category == SyntaxCategory::Expression)
        );
    }

    #[test]
    fn a_try_declaration_gets_a_whole_statement_overlay() {
        let syntax = syntax(
            "function run(r: Result<number, string>) {\n  const n = try r;\n  return n;\n}\n",
        );
        assert!(syntax.overlay.iter().any(|entry| {
            entry.category == SyntaxCategory::Statement
                && matches!(entry.core_root, CoreRoot::Propagate(_))
        }));
    }

    #[test]
    fn expressions_in_one_statement_share_one_stable_owner() {
        let syntax = syntax(
            "const out = [match (left) { A => 1, _ => 0 }, match (right) { B => 2, _ => 0 }];\n",
        );
        let owners: Vec<_> = syntax
            .overlay
            .iter()
            .filter(|entry| entry.category == SyntaxCategory::Expression)
            .map(|entry| entry.host_owner.id)
            .collect();
        assert_eq!(owners.len(), 2);
        assert_eq!(owners[0], owners[1]);
        let owner = syntax.owners().next().expect("statement owner");
        assert_eq!(owner.roots.len(), 2);
    }

    #[test]
    fn a_nested_statement_is_a_distinct_owner() {
        let syntax = syntax(
            "const out = [match (left) { A => 1, _ => 0 }, () => { return match (right) { B => 2, _ => 0 }; }];\n",
        );
        let owners: Vec<_> = syntax
            .overlay
            .iter()
            .filter(|entry| entry.category == SyntaxCategory::Expression)
            .map(|entry| entry.host_owner.id)
            .collect();
        assert_eq!(owners.len(), 2);
        assert_ne!(owners[0], owners[1]);
    }

    #[test]
    fn a_result_statement_island_exposes_its_nested_initializer_owner() {
        let syntax = syntax(
            "const outer = result {\n  const x <- f();\n  const inner = result { const y <- g(); y };\n  inner\n};\n",
        );
        let mut entries: Vec<_> = syntax
            .overlay
            .iter()
            .filter(|entry| matches!(entry.core_root, CoreRoot::Expr(_)))
            .collect();
        entries.sort_unstable_by_key(|entry| entry.source.start);
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0].context.continuation,
            HostContinuation::Initialize
        );
        assert_eq!(
            entries[1].context.continuation,
            HostContinuation::Initialize
        );
        assert_ne!(entries[0].host_owner.id, entries[1].host_owner.id);
    }

    #[test]
    fn multibyte_source_and_projection_coordinates_do_not_mix() {
        let source = "const 한글 = match (e) { A => \"안녕\", _ => \"끝\" };\n";
        let syntax = syntax(source);
        let entry = syntax
            .overlay
            .iter()
            .find(|entry| entry.category == SyntaxCategory::Expression)
            .expect("match overlay");
        assert_eq!(entry.source.start, source.find("match").expect("match"));
        let projected = &syntax.projection[entry.projected.start.0..entry.projected.end.0];
        assert!(projected.starts_with("(() => {"), "{projected}");
        assert!(projected.contains("\"안녕\""), "{projected}");
        assert!(projected.contains("\"끝\""), "{projected}");
    }
}
