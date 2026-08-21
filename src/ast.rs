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
    /// Structurally identified rl syntax that cannot be emitted as written.
    /// Normal compilation still copies it verbatim; the project engine uses
    /// these nodes to build a type-checkable recovery projection without
    /// hiding independent diagnostics elsewhere in the file.
    pub recoveries: Vec<RecoveryNode>,
    /// Candidates committed to rl syntax but malformed. Unlike a failed
    /// lookalike parse, these cannot be valid TypeScript passthrough.
    pub malformed: Vec<crate::error::RlError>,
    /// Byte offsets of `|>` tokens that could not be claimed as a pipeline.
    /// `|>` cannot occur in valid TypeScript, so leaving one in the output
    /// would fail the self-check without a position — the semantic phase
    /// reports these as rl errors instead (the parser stays infallible).
    pub stray_pipes: Vec<usize>,
    /// Byte offsets of `if let` sequences that could not be claimed as an
    /// `if let` statement — same reporting story as [`Self::stray_pipes`]
    /// (an undotted `if` followed by `let` is never valid TypeScript).
    pub stray_if_lets: Vec<usize>,
    /// Byte offsets of `result { ... }` blocks that hold a Result binding
    /// (`const x <- ...;`) but could not be claimed — same reporting story
    /// as [`Self::stray_pipes`]: a declaration keyword followed by `<-`
    /// instead of `=` is never valid TypeScript, so the text cannot be
    /// passed through either.
    pub stray_results: Vec<usize>,
    /// Byte offsets of `result` bindings written without a declaration
    /// keyword (`b <- f();`), reported by the semantic phase.
    pub result_missing_kw: Vec<usize>,
    /// Byte offsets of `result` bindings written **below** a block's top
    /// level (inside an `if` body, a loop, a function written in the
    /// block) — a binding early-returns the block's IIFE, and only a
    /// top-level statement can (`docs/reference/language.md` §8). Same
    /// reporting story as [`Self::stray_pipes`]: the shape is never valid
    /// TypeScript, so it cannot pass through either.
    pub result_nested_binds: Vec<usize>,
}

/// A parser-owned error node used only by the typed projection.
///
/// This mirrors a compiler AST error expression: the diagnostic remains the
/// source of truth, while the node records the complete syntactic region and
/// the category of placeholder that keeps later phases running.
#[derive(Debug, Clone)]
pub(crate) struct RecoveryNode {
    pub span: Span,
    pub kind: RecoveryKind,
}

#[derive(Debug, Clone)]
pub(crate) enum RecoveryKind {
    /// Replace an invalid expression with `undefined`.
    Expression,
    /// Replace an invalid statement with an empty statement.
    Statement,
    /// Replace an invalid TypeScript type fragment with a literal type.
    Type,
    /// Replace an invalid enum declaration with a value-and-type placeholder.
    EnumDecl { name: String, exported: bool },
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
    /// An rl tuple match expression (`match (a, b) { (P, Q) => ... }`).
    TupleMatch(TupleMatchExpr),
    /// An rl `try` statement (Rust-style error propagation).
    Try(TryStmt),
    /// An rl let-else statement (Rust-style refutable binding).
    LetElse(LetElseStmt),
    /// An rl `if let` statement (conditional refutable binding).
    IfLet(IfLetStmt),
    /// A static import declaration or `export ... from` re-export whose
    /// specifier is a relative path ending in `.rl`. Only the specifier
    /// string is lifted out of the byte stream — the rest of the statement
    /// stays verbatim; codegen rewrites the extension per
    /// [`crate::ImportRewrite`]. The clause's imported names are recorded
    /// for the declaration-collection API ([`crate::rl_imports`]).
    RlImport(RlImportDecl),
    /// A lifted `val` binding modifier (the keyword plus the spaces after
    /// it). `val` is a compile-time-only modifier — codegen emits nothing
    /// for this segment, so `val const x = 1;` becomes `const x = 1;`.
    /// Which occurrences of the identifier `val` are modifiers is decided
    /// structurally by [`crate::val::modifier_at`]; every other one stays
    /// verbatim.
    ValModifier(Span),
    /// A template literal; its interpolations are recursively parsed.
    Template(Template),
    /// An rl pipeline expression (`head |> step |> ...`).
    Pipe(PipeExpr),
    /// An rl `result { ... }` computation block.
    ResultBlock(ResultBlock),
}

/// A structurally parsed rl pipeline expression: `head ("|>" step)+`.
/// `|>` cannot occur anywhere in valid TypeScript (after a `|` an
/// expression or type must follow, and `>` starts neither), so claiming it
/// never affects the passthrough contract. Compiles to nested calls of a
/// per-file two-argument apply helper (`$rl_ap`) — argument position gives
/// each step contextual typing, which is what keeps curried combinator
/// steps fully inferred by tsc; method steps chain as plain postfix text.
///
/// With the head keyword `flow` the same step chain composes *functions*
/// instead of flowing a value ([`PipeExpr::head`] is then `None`); it
/// compiles to nested calls of the per-file composition helper (`$rl_fl`).
#[derive(Debug)]
pub(crate) struct PipeExpr {
    /// Raw span of the head expression — the `flow` keyword for a
    /// composition — for error reporting.
    pub head_span: Span,
    /// The head expression, recursively parsed. `None` for a `flow`
    /// composition, which has no value head.
    pub head: Option<Program>,
    /// The pipeline's steps, in source order. Never empty.
    pub steps: Vec<PipeStep>,
}

/// One `|> step` of a [`PipeExpr`].
#[derive(Debug)]
pub(crate) struct PipeStep {
    /// Raw span of the step text (for a method step, including the leading
    /// `.`).
    pub span: Span,
    /// True for a method step (`|> .m(...)`) — the step text is appended
    /// to the piped value as a postfix chain instead of applied via the
    /// helper.
    pub postfix: bool,
    /// The step text, recursively parsed.
    pub body: Program,
}

/// A structurally parsed rl `result { ... }` computation block: a chain of
/// `Result` bindings written as ordinary statements, with the block's last
/// expression as its success value.
///
/// Contract safety rests on the bindings: the parser only claims a block
/// that carries at least one `const|let|var <binding> <- <expr>;` at its
/// top level, and a declaration keyword followed by `<-` (rather than `=`)
/// cannot occur in valid TypeScript. Without that requirement `result`
/// followed by a block would be ambiguous with an expression statement
/// naming a variable `result` plus a block statement on the next line.
///
/// Compiles to an IIFE of plain statements — each binding evaluates once
/// and early-`return`s the `Err`, exactly like a [`TryStmt`] — so tsc
/// narrows every step on its own and the block's type (including the union
/// of the steps' error types) is inferred with no type-level tricks.
#[derive(Debug)]
pub(crate) struct ResultBlock {
    /// Byte offset of the `result` keyword, for error reporting.
    pub keyword_off: usize,
    /// Raw span of the block body, braces excluded (for `await` detection).
    pub body_span: Span,
    /// The block's statements, in source order. Contains at least one
    /// [`ResultItem::Bind`].
    pub items: Vec<ResultItem>,
    /// The trailing expression — the block's success value, recursively
    /// parsed. Never empty.
    pub value: Program,
}

/// One item of a [`ResultBlock`] body, in source order. Every byte of the
/// body up to the trailing expression belongs to exactly one item — the
/// trivia before a binding is the (possibly token-less) [`ResultItem::Stmts`]
/// run in front of it — so emission stays byte-faithful.
#[derive(Debug)]
pub(crate) enum ResultItem {
    /// A run of ordinary statements, recursively parsed.
    Stmts(Program),
    /// A Result binding: `const|let|var <binding> <- <expr>;`.
    Bind(ResultBind),
}

/// See [`ResultItem::Bind`].
#[derive(Debug)]
pub(crate) struct ResultBind {
    /// The declaration keyword: `const`, `let`, or `var`.
    pub kw: String,
    /// Span of the verbatim text between the keyword and `<-` (identifier
    /// or destructuring pattern, optionally type-annotated), trimmed of the
    /// whitespace around it. Carried as a span, not a copy, so the emitted
    /// declaration maps back to the name the user wrote.
    pub binding_span: Span,
    /// Raw span of the expression after `<-`, `;` excluded — with
    /// [`Self::binding_span`] it bounds the binding a diagnostic belongs on
    /// (`crate::EmitAnchor`).
    pub expr_span: Span,
    /// The expression after `<-`, recursively parsed.
    pub expr: Program,
}

/// See [`Segment::RlImport`].
#[derive(Debug)]
pub(crate) struct RlImportDecl {
    /// Span of the specifier string, including quotes.
    pub spec: Span,
    /// Which kind of rl specifier this is.
    pub kind: RlSpecifier,
    /// What the statement brings into local scope.
    pub names: RlImportNames,
}

/// The two specifiers rlc understands beyond plain passthrough.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RlSpecifier {
    /// A relative path ending in `.rl` — another module of this project.
    Relative,
    /// The standard library (`@rl/std`), which rlc supplies itself.
    Std,
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
/// `const|let|var Tag(bindings...) (| Tag(bindings...))* = <expr> else
/// { ... };`. Like [`TryStmt`] it compiles to statements in the enclosing
/// function scope: evaluate once, run the (diverging) `else` block unless
/// the value's `kind` is one of the pattern's tags, then destructure the
/// bindings (shared across alternatives — sema enforces the same
/// (field, name) set, exactly as in a match or-arm).
#[derive(Debug)]
pub(crate) struct LetElseStmt {
    /// Byte offset of the declaration keyword, for error reporting.
    pub keyword_off: usize,
    /// The statement's head: the declaration keyword through the last byte
    /// of the bound expression, the `else` block excluded — the span a
    /// diagnostic about the binding belongs on (`crate::EmitAnchor`).
    pub head_span: Span,
    /// The declaration keyword: `const`, `let`, or `var`.
    pub kw: String,
    /// The `|`-separated pattern alternatives, non-empty. The first always
    /// carries parens (that is what claims the construct); later ones may
    /// be bare tags. Bindings are alias-only (no nested patterns).
    pub alternatives: Vec<TagPattern>,
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
    /// Whether the statement sits inside a function body written in the
    /// same parse region — same fact as [`TryStmt::in_function`]. Inside
    /// an rl construct's statement region it decides placement: the
    /// `else`'s exits must not leave the construct's IIFE, so without a
    /// function written there the statement is rejected.
    pub in_function: bool,
}

/// A structurally parsed rl `if let` statement:
/// `if let Tag(bindings...) = <expr> { ... } else ...`. let-else's
/// non-diverging sibling: evaluate once, run the body with the bindings
/// when the pattern matches, the `else` part (a block or another `if let`)
/// otherwise. Contract safety: in valid TypeScript `if` is always followed
/// by `(`, never by `let`. Compiles to a self-contained block statement —
/// no IIFE and no `return` of its own — so it is valid in any statement
/// position; the bindings materialize as `const`s, which keeps their
/// narrowed types inside closures.
#[derive(Debug)]
pub(crate) struct IfLetStmt {
    /// Byte offset of the `if` keyword, for error reporting.
    pub keyword_off: usize,
    /// The statement's head: `if` through the last byte of the scrutinee
    /// expression, the then-block excluded — the span a diagnostic about
    /// the binding belongs on (`crate::EmitAnchor`).
    pub head_span: Span,
    /// The `|`-separated pattern alternatives, non-empty — the same
    /// pattern grammar as a match arm's: nested patterns allowed in a
    /// single alternative, and (sema-enforced) not combinable with
    /// or-patterns; the first alternative's parens are mandatory, later
    /// ones may be bare tags.
    pub alternatives: Vec<TagPattern>,
    /// The expression after `=`, recursively parsed.
    pub expr: Program,
    /// The then-block body, recursively parsed (braces excluded).
    pub body: Program,
    /// The `else` continuation, if any.
    pub else_part: Option<IfLetElse>,
    /// Whether the statement sits inside a function body written in the
    /// same parse region — same fact as [`TryStmt::in_function`]. An
    /// `if let` emits a block statement (no `return` of its own), so the
    /// fact only matters in expression regions: a function written there
    /// provides the statement position the construct needs.
    pub in_function: bool,
}

/// The `else` continuation of an [`IfLetStmt`].
#[derive(Debug)]
pub(crate) enum IfLetElse {
    /// `else { ... }` (braces excluded).
    Block(Program),
    /// `else if let ...` — chained.
    IfLet(Box<IfLetStmt>),
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
    /// The propagation itself: the `try` keyword through the last byte of
    /// the expression it propagates, `;` excluded. This is the span a
    /// diagnostic about the propagation belongs on — Rust underlines the
    /// `?`, not the whole `let` statement (`crate::EmitAnchor`).
    pub span: Span,
    /// `Some((decl_keyword, binding_span))` for the declaration form, where
    /// `binding_span` covers the (trimmed) bytes between the keyword and
    /// `=` — an identifier or destructuring pattern, optionally
    /// type-annotated. codegen copies those bytes from the source rather
    /// than rebuilding them, so the emitted declaration carries a mapping
    /// back to the name the user wrote. `None` for the bare `try <expr>;`
    /// form.
    pub decl: Option<(String, Span)>,
    /// The expression after `try`, recursively parsed.
    pub expr: Program,
    /// Whether the statement sits inside a function body **written in the
    /// same parse region** (`crate::flow::in_function_body`) — the
    /// placement fact sema judges: the emitted `return` must have a
    /// user-written function to exit. At a module's top level there is
    /// none; inside an rl construct's own statement region (which compiles
    /// into an IIFE) the region boundary is the construct, so only a
    /// function the user wrote *inside* it counts.
    pub in_function: bool,
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
    /// Byte offset of the field name, for error reporting and for the
    /// symbol API (an editor navigates to the declaration by it).
    pub name_off: usize,
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
    /// Byte offset of the body's opening `{`.
    pub body_open: usize,
    /// Byte offset of the body's closing `}` — where an editor inserts a
    /// missing arm ([`crate::engine::rl_declarations`]).
    pub body_close: usize,
    /// Raw span of the scrutinee (used for `await` detection).
    pub scrutinee_span: Span,
    /// The scrutinee, recursively parsed.
    pub scrutinee: Program,
    pub arms: Vec<Arm>,
}

/// A structurally parsed rl tuple match: two or more comma-separated
/// scrutinees matched jointly against tuple patterns. Disambiguation from a
/// comma-expression scrutinee is arm-driven: the arms decide — every arm
/// must be a parenthesized tuple pattern (or a final bare `_`), otherwise
/// the whole thing parses as a single match over a comma expression, so
/// existing programs keep their meaning. Exhaustiveness is checked over the
/// cartesian product of the per-position enums.
#[derive(Debug)]
pub(crate) struct TupleMatchExpr {
    /// Byte offset of the `match` keyword, for error reporting.
    pub keyword_off: usize,
    /// Byte offset of the body's opening `{`.
    pub body_open: usize,
    /// Byte offset of the body's closing `}` — same role as
    /// [`MatchExpr::body_close`].
    pub body_close: usize,
    /// The scrutinees, in source order (always two or more): raw span for
    /// `await` detection plus the recursively parsed expression.
    pub scrutinees: Vec<(Span, Program)>,
    pub arms: Vec<TupleArm>,
}

/// One arm of a [`TupleMatchExpr`].
#[derive(Debug)]
pub(crate) struct TupleArm {
    /// Byte offset of the pattern, for error reporting.
    pub pattern_off: usize,
    pub pattern: TuplePattern,
    /// `Some` for a guarded arm; never attached to a bare `_` arm.
    pub guard: Option<GuardExpr>,
    /// Raw span of the body (used for `await` detection).
    pub body_span: Span,
    /// The body, recursively parsed (braces excluded for block bodies).
    pub body: Program,
    /// True for a `{ ... }` block body.
    pub block: bool,
}

/// A tuple arm's pattern.
#[derive(Debug)]
pub(crate) enum TuplePattern {
    /// The final bare `_` arm — covers every combination.
    Wildcard,
    /// `(elem, elem, ...)` — one [`Pattern`] per scrutinee position (the
    /// arity match is a semantic check). Each element is a tag pattern
    /// (or-patterns and bindings included) or `_`.
    Elems(Vec<Pattern>),
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
    /// One or more `|`-separated literal alternatives: `"north"`,
    /// `200 | 201`, `true`. Non-empty, same as [`Pattern::Tags`]. Literal
    /// and tag patterns never mix in one match (the emitted discriminant
    /// differs: `$rl_m` vs `$rl_m.kind`) — that is a semantic check.
    Literals(Vec<LiteralPattern>),
}

/// One literal alternative inside a pattern.
#[derive(Debug)]
pub(crate) struct LiteralPattern {
    /// Byte span of the literal as written — emitted verbatim as the
    /// `case` label, so the source representation of a number (`0xff`,
    /// `1_000`, `1e3`) is preserved instead of round-tripped.
    pub span: Span,
    /// The literal's value, for duplicate detection and the typed
    /// exhaustiveness probe.
    pub value: LiteralValue,
}

/// A [`LiteralPattern`]'s value, normalized so that two spellings of the
/// same JavaScript value (`"a"` / `'a'`, `200` / `0xc8`) compare equal —
/// `switch` compares with `===`, so those really are the same case.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum LiteralValue {
    /// A string literal, with its escapes decoded.
    Str(String),
    /// A number literal, as the `f64` JavaScript would see (`-0` normalized
    /// to `0`, which is how `===` compares it).
    Num(f64),
    /// A BigInt literal (`1n`), as its decimal digits — never equal to a
    /// [`LiteralValue::Num`], exactly like `1n === 1` being `false`.
    BigInt(String),
    /// `true` / `false`.
    Bool(bool),
}

impl LiteralValue {
    /// The kind name used in diagnostics — or-pattern alternatives must all
    /// be of one kind.
    pub fn kind(&self) -> &'static str {
        match self {
            LiteralValue::Str(_) => "string",
            LiteralValue::Num(_) => "number",
            LiteralValue::BigInt(_) => "bigint",
            LiteralValue::Bool(_) => "boolean",
        }
    }

    /// The value as it appears in a diagnostic — canonical, not the
    /// spelling used in the source (`0xc8` renders as `200`, which is what
    /// makes a duplicate report legible).
    pub fn render(&self) -> String {
        match self {
            LiteralValue::Str(s) => {
                let mut out = String::with_capacity(s.len() + 2);
                out.push('"');
                for ch in s.chars() {
                    match ch {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        '\r' => out.push_str("\\r"),
                        '\t' => out.push_str("\\t"),
                        c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                        c => out.push(c),
                    }
                }
                out.push('"');
                out
            }
            LiteralValue::Num(n) => format!("{n}"),
            LiteralValue::BigInt(d) => format!("{d}n"),
            LiteralValue::Bool(b) => b.to_string(),
        }
    }
}

/// One tag alternative inside a pattern.
#[derive(Debug)]
pub(crate) struct TagPattern {
    pub tag: String,
    /// Byte offset of the tag, for error reporting.
    pub tag_off: usize,
    /// Byte just past the alternative — past the closing paren, or past the
    /// tag for a bare one. With [`TagPattern::tag_off`] this is the span the
    /// match analysis isolates when it asks about one alternative of an
    /// or-pattern ([`crate::engine::analysis`]).
    pub end: usize,
    /// `None` = no parens at all; `Some(vec)` = a (possibly empty) binding list.
    pub bindings: Option<Vec<Binding>>,
}

/// One binding inside a pattern's parens: `name`, `name: alias`, or —
/// in match patterns only — a nested tag pattern `name: Tag(...)`. The
/// nested form always carries parens (`name: None()` for a unit case), so
/// a plain `name: alias` never changes meaning. `alias` and `nested` are
/// mutually exclusive.
#[derive(Debug)]
pub(crate) struct Binding {
    pub name: String,
    /// Byte span of the field name as written — codegen copies it from the
    /// source so the emitted destructuring maps back to the pattern.
    pub name_span: Span,
    pub alias: Option<String>,
    /// Byte span of the alias as written, when there is one. Same purpose
    /// as [`Binding::name_span`].
    pub alias_span: Option<Span>,
    /// `name: Tag(...)` — match the field's value against a nested tag
    /// pattern instead of binding the field. Mismatch falls through to the
    /// next arm, like a failing guard. (Recursion bottoms out through the
    /// `Vec` inside [`TagPattern`].)
    pub nested: Option<TagPattern>,
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
