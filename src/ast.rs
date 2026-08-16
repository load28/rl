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
    /// A template literal; its interpolations are recursively parsed.
    Template(Template),
}

/// A structurally parsed rl `enum` declaration.
#[derive(Debug)]
pub(crate) struct EnumDecl {
    pub name: String,
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

/// One `pattern => body` arm of a match.
#[derive(Debug)]
pub(crate) struct Arm {
    pub pattern: Pattern,
    /// Byte offset of the pattern, for error reporting.
    pub pattern_off: usize,
    /// Raw span of the body (used for `await` detection).
    pub body_span: Span,
    /// The body, recursively parsed. For block bodies the span excludes the
    /// surrounding braces.
    pub body: Program,
    /// True for a `{ ... }` block body, false for an expression body.
    pub block: bool,
}

/// A match arm's pattern.
#[derive(Debug)]
pub(crate) enum Pattern {
    /// The final `_` arm.
    Wildcard,
    /// `Tag` or `Tag(bindings...)`. `bindings` is `None` when there are no
    /// parens at all.
    Tag {
        tag: String,
        bindings: Option<Vec<Binding>>,
    },
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
