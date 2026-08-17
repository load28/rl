//! The rl AST — the contract between the compiler's phases.
//!
//! A parsed file is a [`Program`]: an ordered list of [`Segment`]s covering
//! the whole source. Anything that is not an rl construct stays a
//! [`Segment::Verbatim`] byte range of the original source, which is how the
//! "every valid TypeScript file compiles to itself byte for byte" contract is
//! carried through the pipeline: the parser only lifts fully-parsed rl
//! constructs out of the byte stream, and codegen copies every verbatim span
//! back unchanged.
//!
//! Nested code (a match scrutinee, an arm body, a template interpolation) is
//! itself a `Program`, so the tree is uniformly recursive. All spans and
//! offsets are absolute byte positions into the original source; they are what
//! ties semantic errors back to exact `file:line:col` positions.

/// A half-open byte range `[start, end)` into the original source.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Span {
    pub start: usize,
    pub end: usize,
}

/// A parsed source range: rl constructs plus untouched byte ranges.
#[derive(Debug)]
pub(crate) struct Program {
    pub segments: Vec<Segment>,
}

/// One top-level piece of a [`Program`], in source order.
#[derive(Debug)]
pub(crate) enum Segment {
    /// Bytes copied to the output unchanged.
    Verbatim(Span),
    /// An rl `enum` declaration (plain TypeScript enums never get here).
    Enum(EnumDecl),
    /// An rl `match` expression.
    Match(MatchExpr),
    /// An rl `try` statement (Rust-style error propagation).
    Try(TryStmt),
    /// An rl let-else statement (Rust-style refutable binding).
    LetElse(LetElseStmt),
    /// A static import declaration or `export ... from` re-export whose
    /// specifier is a relative path ending in `.rl`. Only the specifier
    /// string is lifted out of the byte stream — the rest of the statement
    /// stays verbatim; codegen rewrites the extension per
    /// [`crate::ImportRewrite`]. The clause's imported names are recorded
    /// for the declaration-collection API ([`crate::rl_imports`]).
    RlImport(RlImportDecl),
    /// A template literal; its interpolations are recursively parsed.
    Template(Template),
}

/// See [`Segment::RlImport`].
#[derive(Debug)]
pub(crate) struct RlImportDecl {
    /// Span of the specifier string, including quotes.
    pub spec: Span,
    /// What the statement brings into local scope.
    pub names: RlImportNames,
}

/// The bindings a lifted `.rl` import brings into local scope. Collection
/// is best-effort and never affects whether the specifier is lifted: an
/// exotic clause entry (e.g. a string import name) is simply skipped, which
/// only means no exhaustiveness information for that binding.
#[derive(Debug)]
pub(crate) enum RlImportNames {
    /// `import * as ns from ...` — every export, namespace-qualified.
    Namespace(String),
    /// `import { a, b as c, type d } from ...` — (exported name, alias).
    /// A default binding is not recorded (rl enums are named exports).
    Named(Vec<(String, Option<String>)>),
    /// A side-effect import or a re-export — nothing enters local scope.
    None,
}

/// A structurally parsed rl let-else statement:
/// `const|let|var Tag(bindings...) = <expr> else { ... };`. Like
/// [`TryStmt`] it compiles to statements in the enclosing function scope:
/// evaluate once, run the (diverging) `else` block unless the value's
/// `kind` is the pattern's tag, then destructure the bindings.
#[derive(Debug)]
pub(crate) struct LetElseStmt {
    /// Byte offset of the declaration keyword, for error reporting.
    pub keyword_off: usize,
    /// The declaration keyword: `const`, `let`, or `var`.
    pub kw: String,
    /// The pattern's case tag.
    pub tag: String,
    /// The pattern's bindings. Possibly empty — the parens are mandatory
    /// (`const Tag() = ... else ...;` checks the case without binding).
    pub bindings: Vec<Binding>,
    /// The expression after `=`, recursively parsed.
    pub expr: Program,
    /// The `else { ... }` block body, recursively parsed (braces excluded).
    pub else_body: Program,
    /// Byte offset of the `else` keyword, for error reporting.
    pub else_off: usize,
    /// Whether the else block's last top-level statement starts with
    /// `return`, `throw`, `break`, or `continue` — the syntactic stand-in
    /// for Rust's "the else block must diverge" rule. Computed by the
    /// parser (which stays infallible), enforced by sema.
    pub diverges: bool,
}

/// A structurally parsed rl `try` statement: `try <expr>;` or
/// `const|let|var <binding> = try <expr>;`. Compiles to statements in the
/// enclosing function scope — an early `return` of the `Err` value — so it is
/// only valid where the parser sees the top-level statement stream (enforced
/// by [`crate::sema`], which rejects it inside match expressions, template
/// interpolations, and other try expressions).
#[derive(Debug)]
pub(crate) struct TryStmt {
    /// Byte offset of the statement start (the declaration keyword, or `try`
    /// for the bare form), for error reporting.
    pub keyword_off: usize,
    /// `Some((decl_keyword, binding_text))` for the declaration form, where
    /// `binding_text` is the verbatim text between the keyword and `=`
    /// (identifier or destructuring pattern, optionally type-annotated).
    /// `None` for the bare `try <expr>;` form.
    pub decl: Option<(String, String)>,
    /// The expression after `try`, recursively parsed.
    pub expr: Program,
}

/// A structurally parsed rl `enum` declaration.
#[derive(Debug)]
pub(crate) struct EnumDecl {
    pub name: String,
    /// Byte offset of the name, for error reporting and the symbol API.
    pub name_off: usize,
    pub exported: bool,
    /// The verbatim `<...>` generic parameter list, or `""`.
    pub generics: String,
    pub cases: Vec<EnumCase>,
}

/// One case of an rl enum.
#[derive(Debug)]
pub(crate) struct EnumCase {
    pub tag: String,
    /// Byte offset of the tag, for error reporting.
    pub tag_off: usize,
    /// `None` = unit case (no parens); `Some(vec)` = case with a field list.
    pub fields: Option<Vec<Field>>,
}

/// One field of a payload-carrying enum case.
#[derive(Debug)]
pub(crate) struct Field {
    pub name: String,
    pub optional: bool,
    /// The verbatim type annotation text.
    pub ty: String,
    /// Byte offset of the type annotation, for error reporting.
    pub ty_off: usize,
}

/// A structurally parsed rl `match` expression.
#[derive(Debug)]
pub(crate) struct MatchExpr {
    /// Byte offset of the `match` keyword, for error reporting.
    pub keyword_off: usize,
    /// Raw span of the scrutinee (used for `await` detection).
    pub scrutinee_span: Span,
    /// The scrutinee, recursively parsed.
    pub scrutinee: Program,
    pub arms: Vec<Arm>,
}

/// One `pattern (if guard)? => body` arm of a match.
#[derive(Debug)]
pub(crate) struct Arm {
    pub pattern: Pattern,
    /// Byte offset of the pattern, for error reporting.
    pub pattern_off: usize,
    /// `Some` for a guarded arm (`pattern if <cond> => body`). The parser
    /// never attaches a guard to a wildcard pattern (`_ if` fails the parse).
    pub guard: Option<GuardExpr>,
    /// Raw span of the body (used for `await` detection).
    pub body_span: Span,
    /// The body, recursively parsed. For block bodies the span excludes the
    /// surrounding braces.
    pub body: Program,
    /// True for a `{ ... }` block body, false for an expression body.
    pub block: bool,
}

/// The `if <cond>` guard of a match arm.
#[derive(Debug)]
pub(crate) struct GuardExpr {
    /// Raw span of the condition (used for `await` detection).
    pub span: Span,
    /// The condition, recursively parsed.
    pub expr: Program,
}

/// A match arm's pattern.
#[derive(Debug)]
pub(crate) enum Pattern {
    /// The final `_` arm.
    Wildcard,
    /// One or more `|`-separated tag alternatives: `Tag`, `Tag(bindings...)`,
    /// `A | B(x)`. The parser guarantees the list is non-empty; a plain tag
    /// pattern is a single-element list. The semantic phase guarantees every
    /// alternative binds the same (field, name) set, so codegen can emit one
    /// shared destructuring from the first alternative.
    Tags(Vec<TagPattern>),
}

/// One tag alternative inside a pattern.
#[derive(Debug)]
pub(crate) struct TagPattern {
    pub tag: String,
    /// Byte offset of the tag, for error reporting.
    pub tag_off: usize,
    /// `None` = no parens at all; `Some(vec)` = a (possibly empty) binding list.
    pub bindings: Option<Vec<Binding>>,
}

/// One binding inside a pattern's parens: `name` or `name: alias`.
#[derive(Debug)]
pub(crate) struct Binding {
    pub name: String,
    pub alias: Option<String>,
}

/// A template literal split into raw text and recursively parsed
/// interpolations. Raw chunks include the surrounding backticks and the
/// literal text; codegen re-emits `${` and `}` around each interpolation.
#[derive(Debug)]
pub(crate) struct Template {
    pub chunks: Vec<TemplateChunk>,
}

/// One piece of a [`Template`], in source order.
#[derive(Debug)]
pub(crate) enum TemplateChunk {
    /// Raw template text, copied unchanged.
    Raw(Span),
    /// A `${ ... }` interpolation body.
    Interp(Program),
}
