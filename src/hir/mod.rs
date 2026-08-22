//! The HIR — rl's analysis representation.
//!
//! The AST ([`crate::ast`]) is the *syntax* tree: lossless, byte-faithful,
//! the passthrough contract's carrier. The HIR is the *analysis* tree the
//! compiler core works on (`docs/design/compiler-core.md` §4): sugar is
//! normalized away, every pattern-carrying construct becomes the same
//! [`PatternSite`], names written in patterns become unresolved path nodes
//! (resolution is Phase 2's job), and everything has an index-based
//! identity ([`ids`]) that resolves back to source bytes only through the
//! [`HirSourceMap`].
//!
//! What the HIR deliberately does **not** do:
//!
//! - It does not re-parse passthrough TypeScript. An expression rl only
//!   needs to *point at* (a scrutinee, a guard, a pipeline step's text) is
//!   an [`Expr::OpaqueTs`] span; its type is the checker's answer, asked
//!   through the backend seam.
//! - It does not print TypeScript. Core lowering consumes the HIR together
//!   with semantic facts, and the TypeScript backend consumes that Core IR.
//! - It does not resolve names. A constructor pattern carries an
//!   [`UnresolvedPath`]; `DefId`/`LocalId`/`ScopeId` exist so the source
//!   map's shape is final, and the resolver (Phase 2) fills that space.
//!
//! This surface is exported for language tooling but is **not** a stable
//! API: it moves with the compiler core's phases.

pub mod ids;
mod lower;

use std::collections::HashMap;

pub use ids::{
    Arena, BodyId, DefId, ExprId, FieldId, FileId, LocalId, NodeId, OwnerId, PatternId,
    PatternSiteId, ScopeId, VariantId,
};
pub(crate) use lower::lower_program;
pub use lower::lower_source;

/// A half-open byte range `[start, end)` in the lowered file's source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// Start byte, inclusive.
    pub start: usize,
    /// End byte, exclusive.
    pub end: usize,
}

impl Span {
    /// A span over `[start, end)`.
    pub fn new(start: usize, end: usize) -> Span {
        Span { start, end }
    }
}

/// One lowered file: arenas of everything analysis addresses by ID, plus
/// the map back to source bytes.
#[derive(Debug)]
pub struct HirFile {
    /// Which file this HIR was lowered from.
    pub file: FileId,
    /// The file's items (enum declarations, lifted imports), in source
    /// order. An item's position in this list is its [`OwnerId`].
    pub items: Vec<Item>,
    /// Every variant of every enum of the file, in declaration order.
    pub variants: Arena<VariantId, VariantData>,
    /// Every payload field of every variant, in declaration order.
    pub fields: Arena<FieldId, FieldData>,
    /// Every statement stream: the file's top level ([`HirFile::root`]),
    /// arm and `else` bodies, interpolations.
    pub bodies: Arena<BodyId, Body>,
    /// Every expression analysis points at — mostly opaque TypeScript.
    pub exprs: Arena<ExprId, Expr>,
    /// Every pattern node, alternatives and nested payloads included.
    pub patterns: Arena<PatternId, Pat>,
    /// Every pattern site, in source order — the one analysis surface all
    /// pattern syntax shares.
    pub sites: Arena<PatternSiteId, PatternSite>,
    /// The file's top-level statement stream.
    pub root: BodyId,
    /// Every ID's way back to the source.
    pub source_map: HirSourceMap,
}

/// From IDs back to source positions — the *only* way back. Nothing in the
/// HIR carries a span of its own, so nothing in analysis is tempted to use
/// one as identity.
#[derive(Debug, Default)]
pub struct HirSourceMap {
    node_spans: HashMap<NodeId, Span>,
    def_spans: HashMap<DefId, Span>,
    pattern_spans: HashMap<PatternId, Span>,
    ast_origins: HashMap<NodeId, AstOrigin>,
}

impl HirSourceMap {
    /// The byte span a node was lowered from.
    pub fn node_span(&self, node: NodeId) -> Option<Span> {
        self.node_spans.get(&node).copied()
    }

    /// The byte span a definition's name sits at (resolver-filled, Phase 2).
    pub fn def_span(&self, def: DefId) -> Option<Span> {
        self.def_spans.get(&def).copied()
    }

    /// The byte span a pattern node covers.
    pub fn pattern_span(&self, pattern: PatternId) -> Option<Span> {
        self.pattern_spans.get(&pattern).copied()
    }

    /// Which kind of AST construct a node came from.
    pub fn ast_origin(&self, node: NodeId) -> Option<AstOrigin> {
        self.ast_origins.get(&node).copied()
    }

    pub(crate) fn record_node(&mut self, node: NodeId, span: Span, origin: AstOrigin) {
        self.node_spans.insert(node, span);
        self.ast_origins.insert(node, origin);
    }

    pub(crate) fn record_pattern(&mut self, pattern: PatternId, span: Span) {
        self.pattern_spans.insert(pattern, span);
    }

    /// Records a definition's name span — the resolver's half (Phase 2).
    #[allow(dead_code)]
    pub(crate) fn record_def(&mut self, def: DefId, span: Span) {
        self.def_spans.insert(def, span);
    }
}

/// What kind of AST construct a [`NodeId`] was lowered from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AstOrigin {
    /// A verbatim passthrough run.
    Verbatim,
    /// A lifted `val` modifier (erased on emission).
    ValModifier,
    /// An enum declaration (the node's span is the declared name).
    EnumDecl,
    /// An enum case tag.
    EnumCase,
    /// An enum payload field name.
    EnumField,
    /// A lifted import's specifier.
    Import,
    /// A `match` (the node's span is `match (scrutinee)`, arms excluded).
    Match,
    /// A tuple match head.
    TupleMatch,
    /// One match arm.
    Arm,
    /// An arm guard's condition.
    Guard,
    /// A `try` statement (the node's span is `try <expr>`).
    Try,
    /// The complete statement that owns a `try`, including declarations.
    TryOwner,
    /// A let-else statement's head.
    LetElse,
    /// An `if let` statement's head.
    IfLet,
    /// A pipeline.
    Pipe,
    /// One pipeline step.
    PipeStep,
    /// A `result` block.
    ResultBlock,
    /// One `<-` binding of a `result` block.
    ResultBind,
    /// A template literal.
    Template,
    /// A pattern piece (constructor, literal, wildcard, or-group, tuple).
    Pattern,
    /// A pattern field (`name`, `name: alias`, or the receiver of a nested
    /// pattern).
    PatternField,
    /// An expression position holding passthrough TypeScript.
    OpaqueExpr,
    /// A nested statement stream lowered as an expression ([`Expr::Seq`]).
    Seq,
    /// A binding span the construct introduces verbatim (a `try`
    /// declaration's left-hand side, a `result` bind's left-hand side).
    BindingText,
}

/// One item of the file.
#[derive(Debug)]
pub enum Item {
    /// An rl enum declaration.
    Enum(EnumItem),
    /// A lifted `.rl`/`@rl/std` package import.
    Import(ImportItem),
}

/// An rl enum declaration: one type definition and one constructor-value
/// definition in the making (the resolver mints the [`DefId`]s — Phase 2).
#[derive(Debug)]
pub struct EnumItem {
    /// The declared name's node (span = the name).
    pub node: NodeId,
    /// The declared name.
    pub name: String,
    /// Whether the declaration has an `export` modifier.
    pub exported: bool,
    /// The verbatim `<...>` generic parameter list, or `""`.
    pub generics: String,
    /// The declaration's variants, in order.
    pub variants: Vec<VariantId>,
}

/// One variant of an enum, owned by it.
#[derive(Debug)]
pub struct VariantData {
    /// The tag's node (span = the tag).
    pub node: NodeId,
    /// The owning item's index in [`HirFile::items`].
    pub owner: OwnerId,
    /// The case tag.
    pub name: String,
    /// `None` = unit case without parens; `Some` = a (possibly empty)
    /// payload field list.
    pub fields: Option<Vec<FieldId>>,
}

/// One payload field of a variant, owned by it.
#[derive(Debug)]
pub struct FieldData {
    /// The field name's node (span = the name).
    pub node: NodeId,
    /// The variant this field belongs to.
    pub owner: VariantId,
    /// The field name.
    pub name: String,
    /// Whether the field is optional (`name?: T`).
    pub optional: bool,
    /// The verbatim type annotation text — a *text*, not a type; the typed
    /// pass asks the checker (Phase 4).
    pub ty_text: String,
}

/// A lifted import: a relative `.rl` specifier or an `@rl/std` entry.
#[derive(Debug)]
pub struct ImportItem {
    /// The specifier's node (span = the quoted specifier).
    pub node: NodeId,
    /// Which kind of rl specifier this is.
    pub kind: ImportKind,
    /// What the statement brings into local scope.
    pub names: ImportNames,
}

/// The two specifiers rlc understands beyond plain passthrough.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportKind {
    /// A relative path ending in `.rl`.
    Relative,
    /// One module of the standard-library package.
    Std(crate::StdModule),
}

/// What an import brings into scope — the resolver's raw material.
#[derive(Debug)]
pub enum ImportNames {
    /// `import * as ns from ...` — every export, namespace-qualified.
    Namespace(String),
    /// `(exported name, alias)` pairs.
    Named(Vec<(String, Option<String>)>),
    /// A side-effect import or a re-export — nothing enters scope.
    None,
}

/// One statement stream.
#[derive(Debug)]
pub struct Body {
    /// The statements, in source order.
    pub stmts: Vec<Stmt>,
}

/// One statement of a [`Body`], in source order.
#[derive(Debug)]
pub enum Stmt {
    /// Passthrough TypeScript (or an erased `val` modifier) — bytes rl does
    /// not interpret.
    Opaque(NodeId),
    /// An item declaration sits here in the stream.
    Item(OwnerId),
    /// A `try` statement. Its expression is evaluated **once** into a
    /// temporary; the `Err` early-returns, the `Ok` payload lands in the
    /// binding — single evaluation is part of the construct's meaning, not
    /// an emission detail.
    Try(TryStmt),
    /// A let-else statement.
    LetElse(LetElseStmt),
    /// An `if let` statement (chained `else if let` included).
    IfLet(IfLetStmt),
    /// An expression statement (a match, a pipeline, a template, a
    /// `result` block…).
    Expr(ExprId),
}

/// See [`Stmt::Try`].
#[derive(Debug)]
pub struct TryStmt {
    /// Span = `try <expr>` (the propagation, not the whole declaration).
    pub node: NodeId,
    /// Complete source statement used by whole-owner target lowering.
    pub owner: NodeId,
    /// The declaration form's binding text (`const <binding> =`), when
    /// there is one — kept as a node whose span is the user's binding
    /// bytes.
    pub binding: Option<BindingText>,
    /// The propagated expression.
    pub expr: ExprId,
}

/// A source binding copied into generated TypeScript, with its declaration
/// mode already classified by lowering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BindingText {
    /// Span = the binding bytes, excluding the declaration keyword.
    pub node: NodeId,
    /// Which declaration keyword introduced the binding.
    pub mode: BindingMode,
}

/// Declaration mode shared by `try`, let-else, and `result` propagation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingMode {
    /// `const`.
    Const,
    /// `let`.
    Let,
    /// `var`.
    Var,
}

/// See [`Stmt::LetElse`]. The pattern half lives in the site; the statement
/// keeps only what is not pattern-shaped: the diverging `else`.
#[derive(Debug)]
pub struct LetElseStmt {
    /// Span = the statement head (keyword through the bound expression).
    pub node: NodeId,
    /// The pattern half — a one-arm site with no body of its own.
    pub site: PatternSiteId,
    /// Declaration mode of the binding introduced after the decision.
    pub binding_mode: BindingMode,
    /// The `else { ... }` block's statements.
    pub else_body: BodyId,
    /// The parser's syntactic divergence hint (last statement starts with
    /// `return`/`throw`/`break`/`continue`). The flow pass (Phase 5)
    /// upgrades this to a real CFG answer.
    pub else_diverges_hint: bool,
}

/// See [`Stmt::IfLet`].
#[derive(Debug)]
pub struct IfLetStmt {
    /// Span = the statement head (`if let <pattern> = <expr>`).
    pub node: NodeId,
    /// The site's one arm carries the pattern and the then-body.
    pub site: PatternSiteId,
    /// The `else` continuation, if any.
    pub else_part: Option<IfLetElse>,
}

/// The `else` continuation of an [`IfLetStmt`].
#[derive(Debug)]
pub enum IfLetElse {
    /// `else { ... }`.
    Block(BodyId),
    /// `else if let ...` — chained.
    IfLet(Box<IfLetStmt>),
}

/// One expression.
#[derive(Debug)]
pub enum Expr {
    /// Passthrough TypeScript rl only points at — never re-parsed; its
    /// type is the backend's answer.
    OpaqueTs(NodeId),
    /// A nested statement stream in expression position (an arm's block
    /// body, a scrutinee that itself contains rl constructs).
    Seq {
        /// Span = the whole nested stream.
        node: NodeId,
        /// The lowered stream.
        body: BodyId,
    },
    /// A match expression — the pattern half is its site.
    Match {
        /// Span = `match (scrutinee)`.
        node: NodeId,
        /// The pattern site the match lowers to.
        site: PatternSiteId,
        /// Span = the complete match expression, including its closing `}`.
        extent: NodeId,
    },
    /// A pipeline (`head |> step |> ...`), or a `flow` composition when
    /// `head` is `None`.
    Pipe {
        /// Span = the head (`flow` for a composition).
        node: NodeId,
        /// The piped value, or `None` for a `flow` composition.
        head: Option<ExprId>,
        /// The steps, in source order. Never empty.
        steps: Vec<PipeStep>,
    },
    /// A `result { ... }` computation block. Each `<-` binding evaluates
    /// once and early-returns the `Err` — the same single-evaluation
    /// meaning as [`Stmt::Try`].
    ResultBlock {
        /// Span = `result` through the block body.
        node: NodeId,
        /// The body's items, in source order.
        items: Vec<ResultItem>,
        /// The trailing expression — the block's success value.
        value: ExprId,
    },
    /// A template literal; only the interpolations are lowered.
    Template {
        /// Span = the template's extent.
        node: NodeId,
        /// Raw source chunks and interpolation expressions in source order.
        chunks: Vec<TemplatePart>,
    },
}

/// One `|>` step of a pipeline.
#[derive(Debug)]
pub struct PipeStep {
    /// Span = the step text.
    pub node: NodeId,
    /// A method step (`|> .m(...)`) chains as postfix text.
    pub postfix: bool,
    /// The step's expression.
    pub body: ExprId,
}

/// One template component. Keeping raw chunks in HIR lets backend lowering
/// consume HIR alone rather than consulting the parser AST again.
#[derive(Debug)]
pub enum TemplatePart {
    /// Raw template bytes, including delimiters where present.
    Raw(NodeId),
    /// A `${ ... }` expression.
    Interp(ExprId),
}

/// One item of a `result` block's body.
#[derive(Debug)]
pub enum ResultItem {
    /// A run of ordinary statements.
    Stmts(BodyId),
    /// A `const|let|var <binding> <- <expr>;` binding.
    Bind {
        /// Span = the binding through its expression.
        node: NodeId,
        /// The binding text's node (span = the user's binding bytes).
        binding: BindingText,
        /// The expression after `<-`.
        expr: ExprId,
    },
}

/// The construct-neutral pattern site: every pattern-carrying syntax —
/// `match`, tuple match, `if let`, let-else, and any future refutable
/// binding — lowers to one of these, so every analysis (resolution,
/// usefulness, exhaustiveness, completion) has exactly one shape to
/// consume (`docs/design/compiler-core.md` §6).
#[derive(Debug)]
pub struct PatternSite {
    /// Span = the construct's head (`match (scrutinee)`, the `if let`
    /// head, the let-else head) — where a diagnostic about the site as a
    /// whole belongs.
    pub node: NodeId,
    /// Which construct this site lowered from.
    pub kind: SiteKind,
    /// The subject expression(s): one for a single match / `if let` /
    /// let-else, one per position for a tuple match.
    pub subjects: Vec<ExprId>,
    /// The arms, in source order. `if let` and let-else are one-arm sites.
    pub arms: Vec<SiteArm>,
}

/// Which construct a site lowered from — analysis rules that differ by
/// construct (an `if let` has no exhaustiveness question) key off this,
/// nothing else does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SiteKind {
    /// A `match` expression.
    Match,
    /// A tuple match (`match (a, b) { ... }`).
    TupleMatch,
    /// An `if let` statement.
    IfLet,
    /// A let-else statement.
    LetElse,
}

/// One arm of a site: a pattern, its guard, and the body that runs on a
/// match. A let-else arm has no body of its own (its bindings flow into
/// the statements after it) and never a guard.
#[derive(Debug)]
pub struct SiteArm {
    /// Span = the arm's pattern (its extent for a match arm).
    pub node: NodeId,
    /// The arm's pattern.
    pub pattern: PatternId,
    /// The `if <cond>` guard's condition, when there is one.
    pub guard: Option<ExprId>,
    /// The statements that run on a match. `None` for let-else, whose
    /// bindings flow into the statements after the construct.
    pub body: Option<BodyId>,
    /// Whether the arm body yields an expression or executes a block.
    pub body_kind: Option<ArmBodyKind>,
}

/// The two source forms of a pattern arm body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArmBodyKind {
    /// `pattern => expression`.
    Expression,
    /// `pattern => { statements }`.
    Block,
}

/// One pattern node. Or-patterns are a node with alternatives; nested
/// payload patterns recurse through [`FieldPat::Nested`]; names are
/// [`UnresolvedPath`]s until the resolver (Phase 2) binds them.
#[derive(Debug)]
pub enum Pat {
    /// `_` — covers everything.
    Wildcard,
    /// `A | B(x)` — one pattern with alternatives, not several patterns.
    Or(Vec<PatternId>),
    /// One element per tuple-match position.
    Tuple(Vec<PatternId>),
    /// `Tag` / `Tag(fields...)` — a constructor use. `fields: None` means
    /// no parens were written (matches the case without destructuring).
    Constructor {
        /// The written tag, unresolved until Phase 2.
        path: UnresolvedPath,
        /// The destructured fields; `None` when no parens were written.
        fields: Option<Vec<FieldPat>>,
    },
    /// `"north"`, `200`, `true`, `1n`.
    Literal(LitValue),
}

/// A name written in a pattern, not yet resolved to a declaration. The
/// resolver replaces "compare tags by string" with "compare definitions by
/// ID" — this node is where it starts.
#[derive(Debug)]
pub struct UnresolvedPath {
    /// The name's node (span = the name as written).
    pub node: NodeId,
    /// The name as written.
    pub name: String,
}

/// One field inside a constructor pattern's parens.
#[derive(Debug)]
pub struct FieldPat {
    /// The field name's node (span = the name as written).
    pub node: NodeId,
    /// The payload field being matched — an unresolved name, like the tag.
    pub name: String,
    /// What the pattern does with the field's value.
    pub binding: FieldBinding,
}

/// What a [`FieldPat`] does with the field's value.
#[derive(Debug)]
pub enum FieldBinding {
    /// Bind it: under the field's own name, or an alias (`field: alias`).
    /// The alias node's span is the alias as written.
    Named {
        /// The alias and its node, when one was written.
        alias: Option<(String, NodeId)>,
    },
    /// Match it against a nested pattern (`field: Tag(...)`).
    Nested(PatternId),
}

/// A literal pattern's value — same normalization as the AST's (two
/// spellings of one JavaScript value compare equal).
#[derive(Debug, Clone, PartialEq)]
pub enum LitValue {
    /// A string literal, escapes decoded.
    Str(String),
    /// A number literal as JavaScript sees it (`-0` normalized to `0`).
    Num(f64),
    /// A BigInt literal, as its decimal digits.
    BigInt(String),
    /// `true` / `false`.
    Bool(bool),
}
