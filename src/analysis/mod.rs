//! Typed match analysis — one normalized, typed view of every `match`.
//!
//! The pipeline's other phases each look at a `match` through their own
//! keyhole: sema checks rules over raw patterns, codegen emits shapes,
//! and the engine's language surface asks TypeScript about whatever byte
//! the editor points at. What none of them had was a shared, *typed*
//! description of the match itself — which constructors the scrutinee can
//! be, what each pattern binding's payload type is, what an arm body sees.
//! This module is that description, in the mold of rustc's typed pattern
//! representation (surface pattern → analysis with types attached), sized
//! to rl's contract: rlc does not grow a TypeScript type system, so the
//! types here are the *declared* field types from enum declarations.
//!
//! Layering follows the compiler's existing seams exactly:
//!
//! - This module is a pure pipeline phase like [`crate::probe`] and
//!   [`crate::sema`]: text (plus the same imported-declaration input sema
//!   takes) in, analysis out. No file system, no TypeScript.
//! - The *authoritative* types are still TypeScript's. The engine's
//!   language surface asks the checker first — through the same service
//!   seam every other editor answer travels — and falls back to this
//!   analysis when the checker cannot be asked (an or-pattern binding span
//!   has no emitted counterpart; the toolchain may be absent entirely).
//!   That priority is the module's design contract, not an accident: see
//!   `docs/design/match-analysis.md`.
//! - Exhaustiveness is *computed* here and *reported* by [`crate::sema`]:
//!   the declaration table (local > imported > built-in), the covering
//!   rule, and the tuple product all live in this module, and sema turns
//!   the resulting [`Coverage`] into positioned errors. One rule, one
//!   implementation.
//!
//! The two maps the analysis keeps apart are the point of the model:
//!
//! - **Pattern bindings** ([`PatternBinding`]) — one entry per binding
//!   *occurrence*, keyed by its span. In `A(x) | B(x)` the two `x`
//!   occurrences are two entries with two different payload types.
//! - **Body bindings** ([`BodyBinding`]) — one entry per bound *name*, the
//!   alternatives' types merged (`A`'s payload `| B`'s payload), which is
//!   what the arm body actually sees.
//!
//! Merging these early is exactly the bug this model exists to prevent.

mod usefulness;

use crate::ast::*;
use crate::{EnumSymbol, ExternEnum};
use usefulness::{Alphabets, Cell, ColTy};

/// The typed analysis of every pattern in one source file, nested ones
/// included, in source order. Produced by [`pattern_analyses`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PatternAnalyses {
    /// Every `match`, in source order (outer before nested).
    pub matches: Vec<MatchAnalysis>,
    /// Every pattern site that is not a match arm — a let-else, an
    /// `if let` — in source order. A match arm's site lives inside its
    /// [`MatchAnalysis`]; these are the constructs that carry exactly one
    /// pattern and no arms.
    pub sites: Vec<PatternSite>,
    /// The names in patterns that did not resolve to a declaration *and*
    /// that the analysis can name a replacement for — the resolution
    /// phase's answer, sorted by position. Reported by [`crate::sema`],
    /// exactly as [`Coverage`] is: one rule, one implementation.
    pub unresolved: Vec<UnresolvedName>,
    /// Every name in a pattern that **did** resolve, with the span it was
    /// written at — the other half of the resolution phase's answer.
    ///
    /// This is what an editor asks for: a case tag and a payload field name
    /// exist nowhere in the emitted TypeScript (a tag becomes a string
    /// literal, a field a destructuring key), so the checker cannot be
    /// asked about them and rl has to answer. Sorted by position.
    pub resolved: Vec<ResolvedName>,
    /// The declaration table the analysis resolved against — local enums
    /// first, then imported ones, then the built-ins, each name once.
    ///
    /// Exposed because the same table answers questions no single pattern
    /// does: what a case's payload looks like (hover), which cases exist
    /// (completion).
    pub declarations: Vec<DeclaredEnum>,
}

/// One payload column's alphabet as a type checker named it: which
/// `(constructor, field)` column, and the `kind` literals its type admits.
///
/// This is the one thing the declaration table cannot work out — a field's
/// declared type text may be a type parameter, or name a union no rl
/// declaration describes (`docs/design/rust-parity-analysis.md` §10.3).
pub(crate) type PayloadAlphabet = ((String, String), Vec<String>);

/// One enum of the analysis' declaration table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredEnum {
    /// The enum's name in the analyzed file's scope (an import alias, or
    /// `ns.Name` for a namespace import).
    pub name: String,
    /// Where the declaration came from.
    pub origin: Origin,
    /// The constructors, in declaration order.
    pub constructors: Vec<MatchConstructor>,
}

/// One name in a pattern that resolved to a declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedName {
    /// Whether it was written in tag position or as a field.
    pub kind: NameKind,
    /// The name as written.
    pub name: String,
    /// Byte span of the name as written.
    pub start: usize,
    /// End of the name's span.
    pub end: usize,
    /// The enum it resolved in.
    pub enum_name: String,
    /// Where that enum was declared.
    pub origin: Origin,
    /// For a field, the case whose payload it belongs to.
    pub tag: Option<String>,
}

/// Which construct a pattern was written in — the wording an error uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SiteKind {
    /// A let-else statement's pattern.
    LetElse,
    /// An `if let` statement's pattern (each link of an `else if let`
    /// chain is its own site).
    IfLet,
}

/// One pattern written outside a `match`: a let-else, an `if let`. The
/// same two questions a match arm answers — what the pattern is over, and
/// what each binding's declared type is — asked of the constructs that
/// carry a single pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternSite {
    /// Which construct this is.
    pub kind: SiteKind,
    /// Byte offset of the construct's keyword (the declaration keyword for
    /// a let-else, `if` for an `if let`) — the site's identity.
    pub keyword_off: usize,
    /// What the pattern is over, when its tag names an enum the analysis
    /// knows. `None` is not an error: a tag pattern also matches any
    /// hand-written tagged union (`language.md` §3.2).
    pub subject: Option<MatchSubject>,
    /// Every binding occurrence in the pattern, nested leaves included —
    /// the same span-keyed map a match arm keeps.
    pub pattern_bindings: Vec<PatternBinding>,
}

/// A name in a pattern that names no declaration, together with the name
/// it was probably meant to be.
///
/// The suggestion is not decoration: rlc does not know the scrutinee's
/// type, so "this tag resolves to nothing" is *not by itself* an error —
/// a tag pattern legitimately matches hand-written tagged unions whose
/// tags no declaration table holds. What licenses the report is that the
/// analysis can also say what to write instead
/// (`docs/design/rust-parity-analysis.md`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedName {
    /// Whether the name was written in tag position or as a field.
    pub kind: NameKind,
    /// The name as written.
    pub name: String,
    /// Byte span of the name as written.
    pub start: usize,
    /// End of the name's span.
    pub end: usize,
    /// The enum the name was resolved against.
    pub enum_name: String,
    /// Where that enum was declared — an error names the origin.
    pub origin: Origin,
    /// For a field, the case whose payload it was looked up in.
    pub tag: Option<String>,
    /// The declared name it looks like a misspelling of.
    pub suggestion: String,
}

/// What kind of name an [`UnresolvedName`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameKind {
    /// A case tag (`Circel` in `Circel(r)`).
    Case,
    /// A payload field name (`radiuz` in `Circle(radiuz)`).
    Field,
}

/// One `match`, normalized: its subject(s), its arms with their typed
/// bindings, and its coverage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchAnalysis {
    /// Byte offset of the `match` keyword — the analysis' identity, shared
    /// with [`crate::TagMatch::offset`] and the emitted scrutinee temporary
    /// ([`crate::ScrutineeTemp::src`]).
    pub keyword_off: usize,
    /// Byte offset just past the match's head — `match (scrutinee)`, arms
    /// excluded. The head is what a diagnostic about the match as a whole
    /// (a hole in its coverage) is drawn over.
    pub head_end: usize,
    /// One subject per scrutinee position: one entry for a single match,
    /// one per position for a tuple match. `None` when the position's arm
    /// tags belong to no known enum — the match still analyzes, its
    /// declared types are simply unknown (TypeScript may still know).
    pub subjects: Vec<Option<MatchSubject>>,
    /// The arms, in source order.
    pub arms: Vec<AnalyzedArm>,
    /// The exhaustiveness answer — for a single match over its subject's
    /// tags, for a tuple match over the product of its positions. `None`
    /// when the question does not arise: a wildcard arm covers everything,
    /// the tags identify no known enum, or the match is a literal one
    /// (whose exhaustiveness is a question about a TypeScript type —
    /// [`crate::literal_matches`]). This is what sema reports on; there is
    /// no second implementation of the rule.
    pub coverage: Option<Coverage>,
    /// Whether resolution failed for a name written in *this* match's
    /// patterns ([`PatternAnalyses::unresolved`] holds the entries). The
    /// reporter uses it as the recovery boundary: a match with a typo has
    /// no exhaustiveness question worth asking, but its neighbour still
    /// does — suppression is per match, not per file (TASK-120).
    pub has_unresolved: bool,
}

/// What a match is over: the resolved enum and its constructors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchSubject {
    /// The enum's name in this file's scope (alias or `ns.Name` for an
    /// imported one).
    pub enum_name: String,
    /// The enum's constructors, in declaration order.
    pub constructors: Vec<MatchConstructor>,
}

/// One constructor (case) of a subject enum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchConstructor {
    /// The case tag.
    pub tag: String,
    /// `None` for a unit case; `Some` for a case with a (possibly empty)
    /// field list.
    pub fields: Option<Vec<PayloadField>>,
}

/// One field of a payload-carrying constructor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadField {
    /// The field name.
    pub name: String,
    /// Whether the field is optional (`name?: T`) — a destructured binding
    /// then sees `T | undefined`.
    pub optional: bool,
    /// The verbatim declared type text.
    pub ty: String,
}

/// One analyzed arm: where its body is, and the two binding maps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyzedArm {
    /// Byte offset of the arm's pattern — where the arm begins, and what a
    /// consumer points at when it has something to say about the arm
    /// itself rather than about its body.
    pub pattern_start: usize,
    /// Byte span of the arm's body (braces excluded for block bodies).
    pub body_start: usize,
    /// End of the body span.
    pub body_end: usize,
    /// Every binding occurrence in the arm's pattern, alternatives kept
    /// apart — the span-keyed map.
    pub pattern_bindings: Vec<PatternBinding>,
    /// Every bound name the body sees, alternatives merged — the name-keyed
    /// map.
    pub body_bindings: Vec<BodyBinding>,
}

/// One binding occurrence inside a pattern: `x` in `A(x)`, `alias` in
/// `A(field: alias)`, a leaf of a nested pattern. In an or-pattern each
/// alternative contributes its own occurrences with its own constructor's
/// payload type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternBinding {
    /// The name the pattern binds (the alias when the binding is aliased).
    pub name: String,
    /// Byte span of the bound name as written.
    pub start: usize,
    /// End of the bound name's span.
    pub end: usize,
    /// The constructor whose payload this occurrence destructures — the
    /// innermost one for a nested pattern's leaf.
    pub tag: String,
    /// The declared type of the destructured field (`| undefined` applied
    /// for an optional field). `None` when the subject — or, for a nested
    /// leaf, the field's enum — is unknown, or the constructor has no such
    /// field.
    pub ty: Option<String>,
    /// The enum [`PatternBinding::ty`] was read from, when it is known.
    pub enum_name: Option<String>,
    /// Byte span of the whole alternative list this occurrence belongs to
    /// (`A(x) | B(x)` for either `x`) — what a consumer replaces when it
    /// isolates one alternative.
    pub group_start: usize,
    /// End of the alternative list's span.
    pub group_end: usize,
    /// Byte span of this occurrence's own top-level alternative (`A(x)` or
    /// `B(x)`) — what the isolated group is replaced *with*.
    pub alt_start: usize,
    /// End of the alternative's span.
    pub alt_end: usize,
    /// How many alternatives the group has. `1` means the emitted
    /// destructuring maps this span already; more means it does not (the
    /// destructuring speaks for every alternative at once), which is when
    /// a consumer needs [`PatternBinding::group_start`]..[`PatternBinding::alt_end`].
    pub alternatives: usize,
}

/// One bound name as an arm's body sees it: the alternatives' payload
/// types merged in source order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyBinding {
    /// The bound name.
    pub name: String,
    /// The merged declared type — a single type when every alternative
    /// agrees, a `A | B` union otherwise. `None` when any alternative's
    /// type is unknown (a partial union would claim more than is known).
    pub ty: Option<String>,
}

/// The exhaustiveness answer for one `match` — the single source sema's
/// error and every other consumer read (`docs/design/match-analysis.md` §5).
///
/// A single match is the arity-1 case: one position, one tag per row. A
/// tuple match enumerates the cartesian product of its positions, so a row
/// is a combination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Coverage {
    /// One entry per scrutinee position: the enum whose cases that position
    /// enumerates. `None` for a *universal* position of a tuple match —
    /// every arm writes `_` there, so it constrains nothing.
    pub positions: Vec<Option<CoveredEnum>>,
    /// Tags an arm covers outright — unguarded and nested-free, since a
    /// guard may be false and a nested pattern reaches further in. Single
    /// matches only. This is not what exhaustiveness is computed from (the
    /// algorithm descends into payloads); it is the flat summary a
    /// consumer wants when it asks "which cases are already written here?"
    pub covered: Vec<String>,
    /// **Witnesses**: the values the arms leave unhandled. Empty when the
    /// match is exhaustive; bounded, so a wide product does not build a
    /// list nobody can read.
    pub missing: Vec<Uncovered>,
    /// Indices of arms that match nothing an earlier arm has not already
    /// matched — dead code, in source order.
    ///
    /// Nothing reports these yet: an unreachable arm is a *lint* in Rust,
    /// and rl has only errors, so turning it into one would reject
    /// programs that compile today. It is computed here because the same
    /// recursion answers it, and because the editor is where a hint
    /// belongs (TASK-101 §P3). The narrower duplicate-arm rule sema
    /// enforces is unchanged.
    pub unreachable: Vec<usize>,
}

/// One value a match leaves unhandled, as the rl pattern that would cover
/// it (`Wrap(inner: Yes)`, `_` at a universal position of a tuple match) —
/// one entry per scrutinee position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uncovered {
    /// The pattern text, one entry per scrutinee position.
    pub pattern: Vec<String>,
    /// Whether every position was decided from a **known** constructor
    /// set.
    ///
    /// `false` means some column's alphabet was not identifiable — a
    /// hand-written union, a payload whose declared type names no enum —
    /// so this witness is rl's best guess rather than its knowledge. The
    /// default compile path reports it anyway (a conservative "you may
    /// have missed something" is the only answer available without types);
    /// the typed path does not, because there the honest move is to ask
    /// the checker (`docs/design/rust-parity-analysis.md` §10.3).
    pub certain: bool,
}

impl Coverage {
    /// The arity-1 view of [`Coverage::missing`] — the patterns a single
    /// match leaves uncovered. Empty for a tuple match's coverage.
    pub fn missing_tags(&self) -> Vec<&str> {
        if self.positions.len() != 1 {
            return Vec::new();
        }
        self.missing
            .iter()
            .filter_map(|row| row.pattern.first().map(String::as_str))
            .collect()
    }
}

/// An enum a [`Coverage`] position enumerates, with where it was declared —
/// the origin an error message names ("enum E", "built-in enum Option",
/// "enum T (imported from \"./token.rl\")").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoveredEnum {
    /// The enum's name in this file's scope.
    pub name: String,
    /// Where the declaration came from.
    pub origin: Origin,
}

/// Where a declaration in the analysis' table came from. Resolution runs
/// local > imported > built-in, so a nearer origin shadows a farther one of
/// the same name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// Declared in the file being analyzed.
    Local,
    /// Imported from another module.
    Imported {
        /// The specifier as written (`./token.rl`), when the collector
        /// recorded it — an error message quotes it to say *which* enum.
        from: Option<String>,
    },
    /// A built-in enum (`Option`, `Result`).
    Builtin,
}

impl PatternAnalyses {
    /// The pattern binding whose bound-name span contains `offset` (end
    /// inclusive, matching how the language surface treats a chunk's end).
    ///
    /// ```
    /// let src = "enum E { A(x: string), B(x: number) }\nconst v = match (e) { A(x) | B(x) => x };\n";
    /// let analyses = rlc::pattern_analyses(src, &[]);
    /// let a_x = analyses.binding_at(src.find("A(x)").unwrap() + 2).unwrap();
    /// assert_eq!(a_x.ty.as_deref(), Some("string"));
    /// let b_x = analyses.binding_at(src.find("B(x)").unwrap() + 2).unwrap();
    /// assert_eq!(b_x.ty.as_deref(), Some("number"));
    /// ```
    pub fn binding_at(&self, offset: usize) -> Option<&PatternBinding> {
        self.matches
            .iter()
            .flat_map(|m| &m.arms)
            .flat_map(|a| &a.pattern_bindings)
            .chain(self.sites.iter().flat_map(|s| &s.pattern_bindings))
            .find(|b| b.start <= offset && offset <= b.end)
    }

    /// Where the name under `offset` — a reference inside an arm's body —
    /// was bound: the pattern-binding spans of that name in the innermost
    /// enclosing arm. Empty when `offset` is not on such a reference.
    ///
    /// This is a *fallback* answer by design: it does not model shadowing
    /// inside the body, so a consumer asks it only when the checker had no
    /// answer of its own (the or-pattern destructuring is compiler glue,
    /// which navigation never lands in).
    pub fn body_definitions(&self, source: &str, offset: usize) -> Vec<(usize, usize)> {
        let Some((start, end)) = identifier_at(source, offset) else {
            return Vec::new();
        };
        let name = &source[start..end];
        for arm in self.enclosing_arms(offset) {
            let spans: Vec<(usize, usize)> = arm
                .pattern_bindings
                .iter()
                .filter(|b| b.name == name)
                .map(|b| (b.start, b.end))
                .collect();
            if !spans.is_empty() {
                return spans;
            }
        }
        Vec::new()
    }

    /// The body binding the name under `offset` refers to, with the
    /// identifier's span — the name-keyed lookup, same fallback contract as
    /// [`PatternAnalyses::body_definitions`].
    pub fn body_binding_at(
        &self,
        source: &str,
        offset: usize,
    ) -> Option<(&BodyBinding, (usize, usize))> {
        let (start, end) = identifier_at(source, offset)?;
        let name = &source[start..end];
        for arm in self.enclosing_arms(offset) {
            if let Some(binding) = arm.body_bindings.iter().find(|b| b.name == name) {
                return Some((binding, (start, end)));
            }
        }
        None
    }

    /// The arms whose body contains `offset`, innermost first: a nested
    /// match's arm body sits inside the outer arm's, and the nearer binding
    /// is the one a name resolves to.
    fn enclosing_arms(&self, offset: usize) -> Vec<&AnalyzedArm> {
        let mut arms: Vec<&AnalyzedArm> = self
            .matches
            .iter()
            .flat_map(|m| &m.arms)
            .filter(|a| a.body_start <= offset && offset < a.body_end)
            .collect();
        arms.sort_by_key(|a| a.body_end - a.body_start);
        arms
    }
}

/// Analyzes every `match` of a source file, nested ones included, in
/// source order. `externs` are imported enum declarations under their
/// in-scope names — the same input [`crate::Options::extern_enums`] gives
/// sema, carried as [`EnumSymbol`]s because the analysis needs field
/// types, not just tags. Subjects resolve local > imported > built-in
/// (`Option`, `Result`), exactly as exhaustiveness does.
///
/// ```
/// let src = "enum E { A(x: string), B(x: number) }\nconst v = match (e) { A(x) => x, B(x) => x };\n";
/// let analyses = rlc::pattern_analyses(src, &[]);
/// let subject = analyses.matches[0].subjects[0].as_ref().unwrap();
/// assert_eq!(subject.enum_name, "E");
/// assert_eq!(analyses.matches[0].arms[0].body_bindings[0].ty.as_deref(), Some("string"));
/// ```
///
/// let-else and `if let` are analyzed the same way, as [`PatternSite`]s:
///
/// ```
/// let src = "enum E { A(x: string) }\nif let A(x) = e { use(x); }\n";
/// let analyses = rlc::pattern_analyses(src, &[]);
/// assert_eq!(analyses.sites[0].pattern_bindings[0].ty.as_deref(), Some("string"));
/// ```
pub fn pattern_analyses(source: &str, externs: &[EnumSymbol]) -> PatternAnalyses {
    let program = crate::parser::parse(source);
    let decls: Vec<crate::resolve::ExternDecl> = externs.iter().map(Into::into).collect();
    analyses_over(&program, &decls, Depth::Full)
}

/// The coverage-only analysis of an already-parsed program — sema's input,
/// so exhaustiveness and the editor's model answer from one implementation
/// of the rule.
///
/// `externs` are the imported declarations the CLI collects for sema
/// ([`crate::Options::extern_enums`]); they carry tags without field types,
/// which is all coverage needs. Pattern bindings are therefore not analyzed
/// and every arm comes back empty — [`pattern_analyses`] is the entry point
/// for those.
pub(crate) fn coverage_analyses(program: &Program, externs: &[ExternEnum]) -> PatternAnalyses {
    let decls: Vec<crate::resolve::ExternDecl> = externs.iter().map(Into::into).collect();
    analyses_over(program, &decls, Depth::CoverageOnly)
}

/// The one pipeline both entry points run: lower, resolve, build the
/// declaration table **from the resolver's world** (one construction of
/// the rules — local later-wins, imports shadowed by locals, built-ins by
/// both), analyze, then attach the resolver's name answers.
fn analyses_over(
    program: &Program,
    externs: &[crate::resolve::ExternDecl],
    depth: Depth,
) -> PatternAnalyses {
    let mut hir = crate::hir::lower_program(crate::hir::FileId(0), program);
    let resolution = crate::resolve::resolve_file(&mut hir, externs);
    let table = Table::from_resolution(&resolution);
    let mut analyses = analyze(program, &table, depth);
    attach_resolution(&mut analyses, &hir, &resolution);
    analyses
}

/// Copies the resolver's answers into the analysis' vocabulary:
/// [`PatternAnalyses::unresolved`], [`PatternAnalyses::resolved`], and each
/// match's [`MatchAnalysis::has_unresolved`]. Name resolution has **one**
/// implementation — [`crate::resolve`] — and this is where its answers
/// enter the surface sema and the editor already consume.
fn attach_resolution(
    analyses: &mut PatternAnalyses,
    hir: &crate::hir::HirFile,
    resolution: &crate::resolve::Resolution,
) {
    use crate::resolve as res;

    let span_of = |node: crate::hir::NodeId| {
        hir.source_map
            .node_span(node)
            .map_or((0, 0), |s| (s.start, s.end))
    };
    let enum_origin = |def: crate::hir::DefId| match &resolution.defs[def].kind {
        res::DefKind::Enum(data) => match &data.origin {
            res::DeclOrigin::Local(_) => Origin::Local,
            res::DeclOrigin::Imported { from } => Origin::Imported { from: from.clone() },
            res::DeclOrigin::Builtin => Origin::Builtin,
        },
        res::DefKind::EnumValue { .. } => Origin::Local,
    };

    for miss in &resolution.unresolved {
        let (start, end) = span_of(miss.node);
        analyses.unresolved.push(UnresolvedName {
            kind: match miss.kind {
                res::UseKind::Case => NameKind::Case,
                res::UseKind::Field => NameKind::Field,
            },
            name: miss.name.clone(),
            start,
            end,
            enum_name: resolution.defs[miss.against].name.clone(),
            origin: enum_origin(miss.against),
            tag: miss.tag.clone(),
            suggestion: miss.suggestion.clone(),
        });
    }
    for (&node, answer) in &resolution.uses {
        let (start, end) = span_of(node);
        match answer {
            res::Res::Variant(v) => analyses.resolved.push(ResolvedName {
                kind: NameKind::Case,
                name: resolution
                    .variant(*v)
                    .map(|d| d.name.clone())
                    .unwrap_or_default(),
                start,
                end,
                enum_name: resolution.defs[v.enum_def].name.clone(),
                origin: enum_origin(v.enum_def),
                tag: None,
            }),
            res::Res::Field(f) => analyses.resolved.push(ResolvedName {
                kind: NameKind::Field,
                name: resolution
                    .field(*f)
                    .map(|d| d.name.clone())
                    .unwrap_or_default(),
                start,
                end,
                enum_name: resolution.defs[f.variant.enum_def].name.clone(),
                origin: enum_origin(f.variant.enum_def),
                tag: resolution.variant(f.variant).map(|d| d.name.clone()),
            }),
            _ => {}
        }
    }
    // Source order, so the error a file reports does not depend on which
    // construct the resolver happened to reach first.
    analyses.unresolved.sort_by_key(|u| u.start);
    analyses.resolved.sort_by_key(|r| r.start);

    // The recovery boundary: a match holding an unresolved name keeps its
    // own coverage question suppressed. Sites and matches are joined by
    // the construct's keyword offset — the head span's start on both sides.
    let mut with_miss: Vec<usize> = resolution
        .unresolved
        .iter()
        .filter_map(|miss| {
            let site = &hir.sites[miss.site];
            hir.source_map.node_span(site.node).map(|s| s.start)
        })
        .collect();
    with_miss.sort_unstable();
    for analysis in &mut analyses.matches {
        analysis.has_unresolved = with_miss.binary_search(&analysis.keyword_off).is_ok();
    }
}

/// How much of each match to analyze — bindings cost work no coverage
/// consumer would read.
#[derive(Clone, Copy, PartialEq)]
enum Depth {
    Full,
    CoverageOnly,
}

fn analyze(program: &Program, table: &Table, depth: Depth) -> PatternAnalyses {
    let mut analyses = PatternAnalyses::default();
    walk(program, table, depth, &mut analyses);
    analyses.declarations = table
        .entries
        .iter()
        .map(|e| DeclaredEnum {
            name: e.name.clone(),
            origin: e.origin.clone(),
            constructors: e.constructors.clone(),
        })
        .collect();
    analyses
}

/// One candidate enum of the analysis' declaration table.
struct Entry {
    /// The enum's name in the analyzed file's scope.
    name: String,
    /// Where it was declared — carried into [`Coverage`] so a consumer can
    /// name the origin without a table of its own.
    origin: Origin,
    /// The constructors, in declaration order. An imported declaration that
    /// only carried tags ([`ExternEnum`], sema's input) has `fields: None`
    /// throughout — enough for coverage, which is all that path asks.
    constructors: Vec<MatchConstructor>,
}

/// The candidate enums a match's subject can resolve to, in shadowing
/// order — the analysis' declaration table.
struct Table {
    /// Local declarations first (in source order), then imported ones, then
    /// the built-ins; each name appears once, so the nearer origin wins.
    entries: Vec<Entry>,
}

impl Table {
    /// The table, derived from the resolver's world — **one** construction
    /// of the visibility rules (local later-wins, imports shadowed by
    /// locals, built-ins by both), owned by [`crate::resolve`]. What stays
    /// here is only the coverage/typed-model *view* of it: names, origins
    /// and constructors with declared field text.
    fn from_resolution(resolution: &crate::resolve::Resolution) -> Table {
        use crate::resolve::{DeclOrigin, DefKind};
        let entries = resolution
            .defs
            .iter()
            .filter_map(|(id, def)| {
                let DefKind::Enum(data) = &def.kind else {
                    return None;
                };
                // Only the winner of each name is a candidate.
                if resolution.type_ns.get(&def.name) != Some(&id) {
                    return None;
                }
                Some(Entry {
                    name: def.name.clone(),
                    origin: match &data.origin {
                        DeclOrigin::Local(_) => Origin::Local,
                        DeclOrigin::Imported { from } => Origin::Imported { from: from.clone() },
                        DeclOrigin::Builtin => Origin::Builtin,
                    },
                    constructors: data
                        .variants
                        .iter()
                        .map(|variant| MatchConstructor {
                            tag: variant.name.clone(),
                            fields: variant.fields.as_ref().map(|fields| {
                                fields
                                    .iter()
                                    .map(|field| PayloadField {
                                        name: field.name.clone(),
                                        optional: field.optional,
                                        ty: field.ty_text.clone(),
                                    })
                                    .collect()
                            }),
                        })
                        .collect(),
                })
            })
            .collect();
        Table { entries }
    }

    /// The first enum whose cases contain every tag — `None` for an empty
    /// tag set (nothing identifies an enum) or when no candidate fits.
    fn resolve(&self, tags: &[&str]) -> Option<(&str, &[MatchConstructor])> {
        if tags.is_empty() {
            return None;
        }
        self.candidates(tags)
            .first()
            .map(|entry| (entry.name.as_str(), entry.constructors.as_slice()))
    }

    /// Every candidate for a tag set, in shadowing order.
    fn candidates(&self, tags: &[&str]) -> Vec<&Entry> {
        self.entries
            .iter()
            .filter(|entry| {
                tags.iter()
                    .all(|tag| entry.constructors.iter().any(|c| c.tag == *tag))
            })
            .collect()
    }

    /// A subject the *checker* named: its constituents' `kind` literals,
    /// in the type's own order.
    ///
    /// The payload of each tag is filled from this table when some
    /// declaration has that tag, so a nested pattern under it still
    /// analyzes; a tag no declaration knows becomes a constructor with no
    /// field list, which specializes to nothing and so covers only itself.
    /// The entry has no name — the checker answers with a *type*, not a
    /// declaration, which is why the typed path's message names no enum.
    fn entry_of_members(&self, tags: &[String]) -> Entry {
        Entry {
            name: String::new(),
            origin: Origin::Local,
            constructors: tags
                .iter()
                .map(|tag| MatchConstructor {
                    tag: tag.clone(),
                    fields: self
                        .entries
                        .iter()
                        .find_map(|e| e.constructors.iter().find(|c| c.tag == *tag))
                        .and_then(|c| c.fields.clone()),
                })
                .collect(),
        }
    }

    /// The enum a declared type text names: a bare (possibly dotted)
    /// identifier, optionally with type arguments — `Shape`,
    /// `Option<number>`, `ns.Token` — and nothing else. Type arguments are
    /// not substituted (rlc has no type system); the constructor's declared
    /// field text answers as written.
    fn resolve_type(&self, ty: &str) -> Option<(&str, &[MatchConstructor])> {
        self.entry_of_type(ty)
            .map(|e| (e.name.as_str(), e.constructors.as_slice()))
    }

    /// [`Table::resolve_type`]'s answer as the table entry itself — what
    /// resolution needs, since it reports the enum's origin too.
    fn entry_of_type(&self, ty: &str) -> Option<&Entry> {
        let trimmed = ty.trim();
        let base_len = trimmed
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'$' | b'.'))
            .count();
        if base_len == 0 {
            return None;
        }
        let rest = trimmed[base_len..].trim_start();
        let type_args = rest.starts_with('<') && rest.ends_with('>');
        if !rest.is_empty() && !type_args {
            return None; // a union, intersection, array, ... — not one enum
        }
        let base = &trimmed[..base_len];
        self.entries.iter().find(|e| e.name == base)
    }
}

impl Entry {
    fn covered_enum(&self) -> CoveredEnum {
        CoveredEnum {
            name: self.name.clone(),
            origin: self.origin.clone(),
        }
    }
}

fn walk(program: &Program, table: &Table, depth: Depth, out: &mut PatternAnalyses) {
    for segment in &program.segments {
        match segment {
            Segment::Verbatim(_)
            | Segment::RlImport(_)
            | Segment::Enum(_)
            | Segment::ValModifier(_) => {}
            Segment::Match(expr) => {
                let analysis = analyze_match(expr, table, depth);
                out.matches.push(analysis);
                walk(&expr.scrutinee, table, depth, out);
                for arm in &expr.arms {
                    if let Some(guard) = &arm.guard {
                        walk(&guard.expr, table, depth, out);
                    }
                    walk(&arm.body, table, depth, out);
                }
            }
            Segment::TupleMatch(expr) => {
                let analysis = analyze_tuple_match(expr, table, depth);
                out.matches.push(analysis);
                for (_, scrutinee) in &expr.scrutinees {
                    walk(scrutinee, table, depth, out);
                }
                for arm in &expr.arms {
                    if let Some(guard) = &arm.guard {
                        walk(&guard.expr, table, depth, out);
                    }
                    walk(&arm.body, table, depth, out);
                }
            }
            Segment::Try(stmt) => walk(&stmt.expr, table, depth, out),
            Segment::LetElse(stmt) => {
                let site = analyze_let_else(stmt, table, depth);
                out.sites.push(site);
                walk(&stmt.expr, table, depth, out);
                walk(&stmt.else_body, table, depth, out);
            }
            Segment::IfLet(stmt) => walk_if_let(stmt, table, depth, out),
            Segment::Pipe(pipe) => {
                if let Some(head) = &pipe.head {
                    walk(head, table, depth, out);
                }
                for step in &pipe.steps {
                    walk(&step.body, table, depth, out);
                }
            }
            Segment::ResultBlock(block) => {
                for item in &block.items {
                    match item {
                        ResultItem::Stmts(stmts) => walk(stmts, table, depth, out),
                        ResultItem::Bind(bind) => walk(&bind.expr, table, depth, out),
                    }
                }
                walk(&block.value, table, depth, out);
            }
            Segment::Template(template) => {
                for chunk in &template.chunks {
                    if let TemplateChunk::Interp(interp) = chunk {
                        walk(interp, table, depth, out);
                    }
                }
            }
        }
    }
}

fn walk_if_let(stmt: &IfLetStmt, table: &Table, depth: Depth, out: &mut PatternAnalyses) {
    let site = analyze_if_let(stmt, table, depth);
    out.sites.push(site);
    walk(&stmt.expr, table, depth, out);
    walk(&stmt.body, table, depth, out);
    match &stmt.else_part {
        Some(IfLetElse::Block(block)) => walk(block, table, depth, out),
        Some(IfLetElse::IfLet(inner)) => walk_if_let(inner, table, depth, out),
        None => {}
    }
}

fn analyze_match(expr: &MatchExpr, table: &Table, depth: Depth) -> MatchAnalysis {
    // The subject is read from *every* arm's tags, guarded or not — the
    // type-reading counterpart of the resolver's identification (name
    // resolution itself is [`crate::resolve`]'s and attached afterwards).
    let tags: Vec<&str> = expr
        .arms
        .iter()
        .flat_map(|a| match &a.pattern {
            Pattern::Tags(alts) => alts.iter().map(|t| t.tag.as_str()).collect::<Vec<_>>(),
            Pattern::Wildcard | Pattern::Literals(_) => Vec::new(),
        })
        .collect();
    let subject = table.resolve(&tags);

    let arms = expr
        .arms
        .iter()
        .map(|arm| {
            let mut analyzed = AnalyzedArm {
                pattern_start: arm.pattern_off,
                body_start: arm.body_span.start,
                body_end: arm.body_span.end,
                pattern_bindings: Vec::new(),
                body_bindings: Vec::new(),
            };
            if let (Depth::Full, Pattern::Tags(alts)) = (depth, &arm.pattern) {
                analyze_group(alts, subject, table, &mut analyzed);
            }
            analyzed
        })
        .collect();

    let coverage = coverage_of(expr, table);
    MatchAnalysis {
        keyword_off: expr.keyword_off,
        head_end: expr.scrutinee_span.end + 1,
        subjects: vec![subject.map(to_subject)],
        arms,
        coverage,
        // Filled by `attach_resolution` — the resolver owns the answer.
        has_unresolved: false,
    }
}

fn analyze_tuple_match(expr: &TupleMatchExpr, table: &Table, depth: Depth) -> MatchAnalysis {
    let arity = expr.scrutinees.len();
    // Each position reads its subject independently, from the tags every
    // arm uses there.
    let subjects: Vec<Option<(&str, &[MatchConstructor])>> = (0..arity)
        .map(|p| {
            let tags: Vec<&str> = expr
                .arms
                .iter()
                .flat_map(|a| match &a.pattern {
                    TuplePattern::Elems(elems) => match elems.get(p) {
                        Some(Pattern::Tags(alts)) => {
                            alts.iter().map(|t| t.tag.as_str()).collect::<Vec<_>>()
                        }
                        _ => Vec::new(),
                    },
                    TuplePattern::Wildcard => Vec::new(),
                })
                .collect();
            table.resolve(&tags)
        })
        .collect();

    let arms = expr
        .arms
        .iter()
        .map(|arm| {
            let mut analyzed = AnalyzedArm {
                pattern_start: arm.pattern_off,
                body_start: arm.body_span.start,
                body_end: arm.body_span.end,
                pattern_bindings: Vec::new(),
                body_bindings: Vec::new(),
            };
            if let (Depth::Full, TuplePattern::Elems(elems)) = (depth, &arm.pattern) {
                for (p, elem) in elems.iter().enumerate() {
                    if let Pattern::Tags(alts) = elem {
                        analyze_group(
                            alts,
                            subjects.get(p).copied().flatten(),
                            table,
                            &mut analyzed,
                        );
                    }
                }
            }
            analyzed
        })
        .collect();

    MatchAnalysis {
        keyword_off: expr.keyword_off,
        head_end: expr
            .scrutinees
            .last()
            .map_or(expr.keyword_off, |(span, _)| span.end + 1),
        subjects: subjects.into_iter().map(|s| s.map(to_subject)).collect(),
        arms,
        coverage: tuple_coverage_of(expr, table),
        // Filled by `attach_resolution` — the resolver owns the answer.
        has_unresolved: false,
    }
}

fn to_subject((name, constructors): (&str, &[MatchConstructor])) -> MatchSubject {
    MatchSubject {
        enum_name: name.to_string(),
        constructors: constructors.to_vec(),
    }
}

/// Analyzes a let-else's pattern as a [`PatternSite`] — one or more
/// alias-only alternatives, no nested patterns.
fn analyze_let_else(stmt: &LetElseStmt, table: &Table, depth: Depth) -> PatternSite {
    analyze_alt_site(
        SiteKind::LetElse,
        stmt.keyword_off,
        &stmt.alternatives,
        table,
        depth,
    )
}

/// Analyzes one `if let` link as a [`PatternSite`]. Chained `else if let`s
/// are separate sites, recorded by the walk.
fn analyze_if_let(stmt: &IfLetStmt, table: &Table, depth: Depth) -> PatternSite {
    analyze_alt_site(
        SiteKind::IfLet,
        stmt.keyword_off,
        &stmt.alternatives,
        table,
        depth,
    )
}

/// The body both statement pattern sites share: identify the subject from
/// every alternative's tag — the same evidence rule a match arm list uses
/// — and record each alternative's bindings with their declared types,
/// occurrence spans kept apart exactly as [`analyze_group`] keeps a match
/// or-arm's. (Resolving the names — and the near-miss report when they do
/// not resolve — is [`crate::resolve`]'s, attached afterwards.)
fn analyze_alt_site(
    kind: SiteKind,
    keyword_off: usize,
    alts: &[TagPattern],
    table: &Table,
    depth: Depth,
) -> PatternSite {
    let tags: Vec<&str> = alts.iter().map(|alt| alt.tag.as_str()).collect();
    let subject = table.resolve(&tags);
    let group = (alts[0].tag_off, alts.last().expect("non-empty").end);

    let mut pattern_bindings = Vec::new();
    if depth == Depth::Full {
        for alt in alts {
            let constructor = subject
                .and_then(|(_, cases)| cases.iter().find(|c| c.tag == alt.tag))
                .map(|c| (subject.expect("just matched").0, c));
            let mut leaves = Vec::new();
            collect_bindings(
                alt.bindings.as_deref().unwrap_or_default(),
                constructor,
                &alt.tag,
                table,
                &mut leaves,
            );
            for leaf in leaves {
                pattern_bindings.push(PatternBinding {
                    group_start: group.0,
                    group_end: group.1,
                    alt_start: alt.tag_off,
                    alt_end: alt.end,
                    alternatives: alts.len(),
                    ..leaf
                });
            }
        }
    }

    PatternSite {
        kind,
        keyword_off,
        subject: subject.map(to_subject),
        pattern_bindings,
    }
}

/// Analyzes one alternative list (`A(x) | B(x)`): every alternative
/// independently against the subject, occurrences recorded apart, body
/// bindings merged at the end — never the other way around.
fn analyze_group(
    alts: &[TagPattern],
    subject: Option<(&str, &[MatchConstructor])>,
    table: &Table,
    arm: &mut AnalyzedArm,
) {
    let group = (alts[0].tag_off, alts.last().expect("non-empty").end);
    // Bound name → the type each alternative gives it, in source order.
    let mut merged: Vec<(String, Vec<Option<String>>)> = Vec::new();
    for alt in alts {
        let constructor = subject
            .and_then(|(_, cases)| cases.iter().find(|c| c.tag == alt.tag))
            .map(|c| (subject.expect("just matched").0, c));
        let mut leaves = Vec::new();
        collect_bindings(
            alt.bindings.as_deref().unwrap_or_default(),
            constructor,
            &alt.tag,
            table,
            &mut leaves,
        );
        for leaf in leaves {
            let binding = PatternBinding {
                group_start: group.0,
                group_end: group.1,
                alt_start: alt.tag_off,
                alt_end: alt.end,
                alternatives: alts.len(),
                ..leaf
            };
            match merged.iter_mut().find(|(name, _)| *name == binding.name) {
                Some((_, types)) => types.push(binding.ty.clone()),
                None => merged.push((binding.name.clone(), vec![binding.ty.clone()])),
            }
            arm.pattern_bindings.push(binding);
        }
    }
    for (name, types) in merged {
        arm.body_bindings.push(BodyBinding {
            ty: merge_types(&types),
            name,
        });
    }
}

/// Walks one alternative's bindings, nested patterns included, recording a
/// [`PatternBinding`] per leaf. `constructor` is `(enum name, constructor)`
/// when the expected type is known; group fields are filled by the caller.
fn collect_bindings(
    bindings: &[Binding],
    constructor: Option<(&str, &MatchConstructor)>,
    tag: &str,
    table: &Table,
    out: &mut Vec<PatternBinding>,
) {
    for b in bindings {
        let field = constructor.and_then(|(_, c)| {
            c.fields
                .as_deref()
                .unwrap_or_default()
                .iter()
                .find(|f| f.name == b.name)
        });
        match &b.nested {
            Some(inner) => {
                // The field's declared type is the nested pattern's
                // expected type; resolve it to an enum and recurse.
                let nested_constructor =
                    field
                        .and_then(|f| table.resolve_type(&f.ty))
                        .and_then(|(name, cases)| {
                            cases.iter().find(|c| c.tag == inner.tag).map(|c| (name, c))
                        });
                collect_bindings(
                    inner.bindings.as_deref().unwrap_or_default(),
                    nested_constructor,
                    &inner.tag,
                    table,
                    out,
                );
            }
            None => {
                let (name, span) = match (&b.alias, b.alias_span) {
                    (Some(alias), Some(span)) => (alias.clone(), span),
                    _ => (b.name.clone(), b.name_span),
                };
                out.push(PatternBinding {
                    name,
                    start: span.start,
                    end: span.end,
                    tag: tag.to_string(),
                    ty: field.map(field_type),
                    enum_name: field
                        .and(constructor)
                        .map(|(enum_name, _)| enum_name.to_string()),
                    // Filled by the caller with the top-level group.
                    group_start: 0,
                    group_end: 0,
                    alt_start: 0,
                    alt_end: 0,
                    alternatives: 0,
                });
            }
        }
    }
}

/// The type a destructured binding sees: the declared text, `| undefined`
/// for an optional field (exactly what the emitted destructuring yields).
fn field_type(field: &PayloadField) -> String {
    if field.optional {
        format!("{} | undefined", field.ty)
    } else {
        field.ty.clone()
    }
}

/// Merges one bound name's per-alternative types into what the body sees:
/// duplicates collapse, distinct types union in source order, and any
/// unknown makes the whole answer unknown — a partial union would claim
/// more than is known.
fn merge_types(types: &[Option<String>]) -> Option<String> {
    let mut distinct: Vec<&str> = Vec::new();
    for ty in types {
        let ty = ty.as_deref()?;
        if !distinct.contains(&ty) {
            distinct.push(ty);
        }
    }
    match distinct.len() {
        0 => None,
        1 => Some(distinct[0].to_string()),
        _ => Some(distinct.join(" | ")),
    }
}

/// True when the alternative carries a nested pattern — like a guard, such
/// an arm may mismatch at runtime, so it identifies the enum but covers
/// nothing (sema's rule, and now the only copy of it).
pub(crate) fn has_nested(alt: &TagPattern) -> bool {
    alt.bindings
        .as_deref()
        .unwrap_or_default()
        .iter()
        .any(|b| b.nested.is_some())
}

/// Whether an arm covers what it matches: guarded arms and arms with a
/// nested pattern identify the subject but cover nothing.
fn covers(guard: &Option<GuardExpr>, alts: &[TagPattern]) -> bool {
    guard.is_none() && !alts.iter().any(has_nested)
}

/// [`Coverage`] of a single match, when the question means something: a tag
/// match with no wildcard arm whose tags identify a known enum.
///
/// The arms become a one-column matrix and the algorithm answers
/// ([`usefulness`]). Guarded arms stay out of it — a guard may be false —
/// but arms carrying nested patterns are now *in*: the recursion descends
/// into the payload, so such an arm covers exactly what it covers instead
/// of being written off.
fn coverage_of(expr: &MatchExpr, table: &Table) -> Option<Coverage> {
    let rows = match_rows(expr)?;
    // Several enums can hold every tag. The one the arms *satisfy* is the
    // subject if there is one; otherwise the one they leave least of —
    // the rule sema has always reported, now measured in witnesses.
    let cx = Alphabets::of(table);
    let mut best: Option<(&Entry, Vec<Uncovered>)> = None;
    for entry in table.candidates(&rows.tags) {
        let types = [ColTy::Enum(entry)];
        let missing = render_witnesses(&usefulness::missing(&rows.rows, &types, &cx));
        if missing.is_empty() {
            best = Some((entry, missing));
            break;
        }
        if best.as_ref().is_none_or(|(_, m)| missing.len() < m.len()) {
            best = Some((entry, missing));
        }
    }
    let (entry, missing) = best?;
    Some(Coverage {
        positions: vec![Some(entry.covered_enum())],
        covered: rows.covered,
        missing,
        unreachable: unreachable_arms(&rows.arm_rows, &[ColTy::Enum(entry)], &cx),
    })
}

/// The same answer for a subject the caller names — the typed path, where
/// the checker says which constituents the scrutinee's type still has and
/// rl runs its own algorithm over that alphabet. One algorithm, a better
/// oracle for the one column the checker can speak about.
pub(crate) fn checked_coverage(
    source: &str,
    externs: &[EnumSymbol],
    members: &[(usize, Vec<Vec<String>>)],
    payloads: &[PayloadAlphabet],
) -> Vec<(usize, Coverage)> {
    let program = crate::parser::parse(source);
    let decls: Vec<crate::resolve::ExternDecl> = externs.iter().map(Into::into).collect();
    let mut hir = crate::hir::lower_program(crate::hir::FileId(0), &program);
    let resolution = crate::resolve::resolve_file(&mut hir, &decls);
    let table = Table::from_resolution(&resolution);
    let mut found = Vec::new();
    let mut matches = Vec::new();
    let mut tuples = Vec::new();
    collect_matches(&program, &mut matches, &mut tuples);
    for expr in matches {
        let Some((_, positions)) = members.iter().find(|(at, _)| *at == expr.keyword_off) else {
            continue;
        };
        let Some(tags) = positions.first() else {
            continue;
        };
        let entry = table.entry_of_members(tags);
        let Some(rows) = match_rows(expr) else {
            continue;
        };
        let cx = Alphabets {
            table: &table,
            payloads: payloads
                .iter()
                .map(|((tag, field), members)| {
                    (
                        (tag.clone(), field.clone()),
                        table.entry_of_members(members),
                    )
                })
                .collect(),
        };
        let types = [ColTy::Enum(&entry)];
        let missing = render_witnesses(&usefulness::missing(&rows.rows, &types, &cx));
        found.push((
            expr.keyword_off,
            Coverage {
                positions: vec![None],
                covered: rows.covered,
                missing,
                unreachable: unreachable_arms(&rows.arm_rows, &types, &cx),
            },
        ));
    }

    // A tuple match asks one question per position and enumerates the
    // product, exactly as the default path does — the only difference is
    // where each column's alphabet came from.
    for expr in tuples {
        let Some((_, positions)) = members.iter().find(|(at, _)| *at == expr.keyword_off) else {
            continue;
        };
        let arity = expr.scrutinees.len();
        if positions.len() != arity {
            continue;
        }
        let written = tuple_position_tags(expr, arity);
        let entries: Vec<Option<Entry>> = positions
            .iter()
            .enumerate()
            .map(|(index, tags)| {
                // A position no arm writes a tag at constrains nothing, and
                // saying `_` there is a shorter true answer than
                // enumerating a column nobody asked about.
                (!written[index].is_empty()).then(|| table.entry_of_members(tags))
            })
            .collect();
        let types: Vec<ColTy> = entries
            .iter()
            .map(|entry| match entry {
                Some(entry) => ColTy::Enum(entry),
                None => ColTy::Unconstrained,
            })
            .collect();
        let mut rows: Vec<Vec<Cell>> = Vec::new();
        let mut arm_rows: Vec<(usize, Vec<Vec<Cell>>)> = Vec::new();
        for (index, arm) in expr.arms.iter().enumerate() {
            let TuplePattern::Elems(elems) = &arm.pattern else {
                continue;
            };
            if elems.len() != arity || arm.guard.is_some() {
                continue;
            }
            let Some(this) = tuple_rows(elems) else {
                continue;
            };
            arm_rows.push((index, this.clone()));
            rows.extend(this);
        }
        let cx = Alphabets {
            table: &table,
            payloads: payloads
                .iter()
                .map(|((tag, field), members)| {
                    (
                        (tag.clone(), field.clone()),
                        table.entry_of_members(members),
                    )
                })
                .collect(),
        };
        found.push((
            expr.keyword_off,
            Coverage {
                positions: vec![None; arity],
                covered: Vec::new(),
                missing: render_witnesses(&usefulness::missing(&rows, &types, &cx)),
                unreachable: unreachable_arms(&arm_rows, &types, &cx),
            },
        ));
    }
    found
}

/// The tags any arm writes at each position — what says whether a position
/// constrains anything at all.
fn tuple_position_tags(expr: &TupleMatchExpr, arity: usize) -> Vec<Vec<&str>> {
    let mut out: Vec<Vec<&str>> = vec![Vec::new(); arity];
    for arm in &expr.arms {
        let TuplePattern::Elems(elems) = &arm.pattern else {
            continue;
        };
        if elems.len() != arity {
            continue;
        }
        for (position, elem) in elems.iter().enumerate() {
            if let Pattern::Tags(alts) = elem {
                for alt in alts {
                    if !out[position].contains(&alt.tag.as_str()) {
                        out[position].push(&alt.tag);
                    }
                }
            }
        }
    }
    out
}

/// Every single `match` of a program, nested ones included, in source
/// order.
fn collect_matches<'a>(
    program: &'a Program,
    out: &mut Vec<&'a MatchExpr>,
    tuples: &mut Vec<&'a TupleMatchExpr>,
) {
    for segment in &program.segments {
        match segment {
            Segment::Match(expr) => {
                out.push(expr);
                collect_matches(&expr.scrutinee, out, tuples);
                for arm in &expr.arms {
                    if let Some(guard) = &arm.guard {
                        collect_matches(&guard.expr, out, tuples);
                    }
                    collect_matches(&arm.body, out, tuples);
                }
            }
            Segment::TupleMatch(expr) => {
                tuples.push(expr);
                for (_, scrutinee) in &expr.scrutinees {
                    collect_matches(scrutinee, out, tuples);
                }
                for arm in &expr.arms {
                    if let Some(guard) = &arm.guard {
                        collect_matches(&guard.expr, out, tuples);
                    }
                    collect_matches(&arm.body, out, tuples);
                }
            }
            Segment::Try(stmt) => collect_matches(&stmt.expr, out, tuples),
            Segment::LetElse(stmt) => {
                collect_matches(&stmt.expr, out, tuples);
                collect_matches(&stmt.else_body, out, tuples);
            }
            Segment::IfLet(stmt) => collect_if_let_matches(stmt, out, tuples),
            Segment::Pipe(pipe) => {
                if let Some(head) = &pipe.head {
                    collect_matches(head, out, tuples);
                }
                for step in &pipe.steps {
                    collect_matches(&step.body, out, tuples);
                }
            }
            Segment::ResultBlock(block) => {
                for item in &block.items {
                    match item {
                        ResultItem::Stmts(stmts) => collect_matches(stmts, out, tuples),
                        ResultItem::Bind(bind) => collect_matches(&bind.expr, out, tuples),
                    }
                }
                collect_matches(&block.value, out, tuples);
            }
            Segment::Template(template) => {
                for chunk in &template.chunks {
                    if let TemplateChunk::Interp(interp) = chunk {
                        collect_matches(interp, out, tuples);
                    }
                }
            }
            Segment::Verbatim(_)
            | Segment::RlImport(_)
            | Segment::Enum(_)
            | Segment::ValModifier(_) => {}
        }
    }
}

fn collect_if_let_matches<'a>(
    stmt: &'a IfLetStmt,
    out: &mut Vec<&'a MatchExpr>,
    tuples: &mut Vec<&'a TupleMatchExpr>,
) {
    collect_matches(&stmt.expr, out, tuples);
    collect_matches(&stmt.body, out, tuples);
    match &stmt.else_part {
        Some(IfLetElse::Block(block)) => collect_matches(block, out, tuples),
        Some(IfLetElse::IfLet(inner)) => collect_if_let_matches(inner, out, tuples),
        None => {}
    }
}

/// One match's arms as the algorithm's input: the tags they name, the tags
/// they cover outright, the matrix, and the per-arm rows reachability
/// needs. `None` when the question does not arise — a wildcard arm covers
/// everything, or no arm carries a tag pattern.
struct MatchRows<'a> {
    tags: Vec<&'a str>,
    covered: Vec<String>,
    rows: Vec<Vec<Cell<'a>>>,
    arm_rows: Vec<(usize, Vec<Vec<Cell<'a>>>)>,
}

fn match_rows(expr: &MatchExpr) -> Option<MatchRows<'_>> {
    if expr
        .arms
        .iter()
        .any(|a| matches!(a.pattern, Pattern::Wildcard))
    {
        return None;
    }
    // Identification uses every arm's tags, guarded ones included.
    let mut tags: Vec<&str> = Vec::new();
    let mut covered: Vec<String> = Vec::new();
    let mut rows: Vec<Vec<Cell>> = Vec::new();
    let mut arm_rows: Vec<(usize, Vec<Vec<Cell>>)> = Vec::new();
    for (index, arm) in expr.arms.iter().enumerate() {
        let Pattern::Tags(alts) = &arm.pattern else {
            continue;
        };
        for alt in alts {
            if !tags.contains(&alt.tag.as_str()) {
                tags.push(&alt.tag);
            }
        }
        if arm.guard.is_some() {
            continue;
        }
        // An or-pattern is several rows: each alternative stands alone.
        let this: Vec<Vec<Cell>> = alts.iter().map(|alt| vec![Cell::Tag(alt)]).collect();
        arm_rows.push((index, this.clone()));
        rows.extend(this);
        if covers(&arm.guard, alts) {
            for alt in alts {
                if !covered.contains(&alt.tag) {
                    covered.push(alt.tag.clone());
                }
            }
        }
    }
    if tags.is_empty() {
        return None;
    }
    Some(MatchRows {
        tags,
        covered,
        rows,
        arm_rows,
    })
}

/// [`Coverage`] of a tuple match: the same algorithm over as many columns
/// as there are scrutinees. `None` when a bare `_` arm covers everything,
/// when a tagged position resolves to no known enum, or when no position
/// is tagged at all (nothing to enumerate).
fn tuple_coverage_of(expr: &TupleMatchExpr, table: &Table) -> Option<Coverage> {
    let arity = expr.scrutinees.len();
    if expr
        .arms
        .iter()
        .any(|a| matches!(a.pattern, TuplePattern::Wildcard))
    {
        return None;
    }

    // Per position, the tags any arm writes there — identification, as in
    // a single match but one column at a time.
    let mut position_tags: Vec<Vec<&str>> = vec![Vec::new(); arity];
    for arm in &expr.arms {
        let TuplePattern::Elems(elems) = &arm.pattern else {
            continue;
        };
        if elems.len() != arity {
            continue; // sema reports the arity mismatch
        }
        for (position, elem) in elems.iter().enumerate() {
            if let Pattern::Tags(alts) = elem {
                for alt in alts {
                    if !position_tags[position].contains(&alt.tag.as_str()) {
                        position_tags[position].push(&alt.tag);
                    }
                }
            }
        }
    }

    let mut positions: Vec<Option<CoveredEnum>> = Vec::with_capacity(arity);
    let mut types: Vec<ColTy> = Vec::with_capacity(arity);
    for tags in &position_tags {
        if tags.is_empty() {
            // Universal position: only `_` was written here.
            positions.push(None);
            types.push(ColTy::Unconstrained);
            continue;
        }
        // A position whose tags name no enum makes the whole question
        // unanswerable — the same conservatism as before.
        let entry = *table.candidates(tags).first()?;
        positions.push(Some(entry.covered_enum()));
        types.push(ColTy::Enum(entry));
    }
    if positions.iter().all(Option::is_none) {
        return None;
    }

    let mut rows: Vec<Vec<Cell>> = Vec::new();
    let mut arm_rows: Vec<(usize, Vec<Vec<Cell>>)> = Vec::new();
    for (index, arm) in expr.arms.iter().enumerate() {
        let TuplePattern::Elems(elems) = &arm.pattern else {
            continue;
        };
        if elems.len() != arity || arm.guard.is_some() {
            continue;
        }
        let Some(this) = tuple_rows(elems) else {
            continue;
        };
        arm_rows.push((index, this.clone()));
        rows.extend(this);
    }

    let cx = Alphabets::of(table);
    Some(Coverage {
        positions,
        covered: Vec::new(),
        missing: render_witnesses(&usefulness::missing(&rows, &types, &cx)),
        unreachable: unreachable_arms(&arm_rows, &types, &cx),
    })
}

/// One tuple arm as rows: the cartesian product of its elements'
/// alternatives, since `(A | B, C)` matches two combinations. `None` when
/// the arm can match no combination of tags at all (a literal element),
/// which is the same as contributing no row.
fn tuple_rows<'a>(elems: &'a [Pattern]) -> Option<Vec<Vec<Cell<'a>>>> {
    let mut rows: Vec<Vec<Cell>> = vec![Vec::new()];
    for elem in elems {
        let cells: Vec<Cell> = match elem {
            Pattern::Wildcard => vec![Cell::Wild],
            Pattern::Literals(_) => return None,
            Pattern::Tags(alts) => alts.iter().map(Cell::Tag).collect(),
        };
        rows = rows
            .into_iter()
            .flat_map(|row| {
                cells.iter().map(move |cell| {
                    let mut next = row.clone();
                    next.push(*cell);
                    next
                })
            })
            .collect();
    }
    Some(rows)
}

/// The arms that match nothing an earlier arm has not: each arm's rows
/// against every row before it.
fn unreachable_arms<'a>(
    arm_rows: &[(usize, Vec<Vec<Cell<'a>>>)],
    types: &[ColTy<'a>],
    cx: &'a Alphabets<'a>,
) -> Vec<usize> {
    let mut seen: Vec<Vec<Cell>> = Vec::new();
    let mut out = Vec::new();
    for (index, rows) in arm_rows {
        let useful = rows
            .iter()
            .any(|row| usefulness::is_useful(&seen, row, types, cx));
        if !useful {
            out.push(*index);
        }
        seen.extend(rows.iter().cloned());
    }
    out
}

fn render_witnesses(found: &[Vec<usefulness::Witness>]) -> Vec<Uncovered> {
    found
        .iter()
        .map(|row| Uncovered {
            pattern: row.iter().map(usefulness::Witness::render).collect(),
            certain: row.iter().all(usefulness::Witness::certain),
        })
        .collect()
}

/// The identifier span containing `offset`, byte-based like the scanner:
/// ASCII identifier bytes plus opaque multi-byte UTF-8.
fn identifier_at(source: &str, offset: usize) -> Option<(usize, usize)> {
    let bytes = source.as_bytes();
    if offset >= bytes.len() {
        return None;
    }
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'$' || b >= 0x80;
    if !is_ident(bytes[offset]) {
        return None;
    }
    let mut start = offset;
    while start > 0 && is_ident(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = offset;
    while end < bytes.len() && is_ident(bytes[end]) {
        end += 1;
    }
    if bytes[start].is_ascii_digit() {
        return None; // a number, not a name
    }
    Some((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The witnesses of a coverage as plain rows, for comparison.
    fn patterns(coverage: &Coverage) -> Vec<Vec<&str>> {
        coverage
            .missing
            .iter()
            .map(|m| m.pattern.iter().map(String::as_str).collect())
            .collect()
    }

    fn at(src: &str, needle: &str, delta: usize) -> usize {
        src.find(needle).expect("needle") + delta
    }

    /// `A(x)`-style lookup: the binding two bytes into the needle.
    fn binding<'a>(
        analyses: &'a PatternAnalyses,
        src: &str,
        needle: &str,
        delta: usize,
    ) -> &'a PatternBinding {
        analyses
            .binding_at(at(src, needle, delta))
            .unwrap_or_else(|| panic!("no binding at {needle:?}+{delta}"))
    }

    #[test]
    fn single_constructor_binding_and_body_share_the_payload_type() {
        let src = "enum E { A(x: string), B }\nconst v = match (e) { A(x) => x, B => 0 };\n";
        let analyses = pattern_analyses(src, &[]);
        let b = binding(&analyses, src, "A(x)", 2);
        assert_eq!(b.ty.as_deref(), Some("string"));
        assert_eq!(b.tag, "A");
        assert_eq!(b.enum_name.as_deref(), Some("E"));
        assert_eq!(b.alternatives, 1);
        let arm = &analyses.matches[0].arms[0];
        assert_eq!(arm.body_bindings.len(), 1);
        assert_eq!(arm.body_bindings[0].name, "x");
        assert_eq!(arm.body_bindings[0].ty.as_deref(), Some("string"));
    }

    #[test]
    fn or_pattern_occurrences_keep_their_own_types_and_the_body_merges_them() {
        let src =
            "enum E { A(x: string), B(x: number) }\nconst v = match (e) { A(x) | B(x) => x };\n";
        let analyses = pattern_analyses(src, &[]);
        let a = binding(&analyses, src, "A(x)", 2);
        let b = binding(&analyses, src, "B(x)", 2);
        assert_eq!(a.ty.as_deref(), Some("string"));
        assert_eq!(b.ty.as_deref(), Some("number"));
        assert_eq!((a.alternatives, b.alternatives), (2, 2));
        // Both occurrences share the group span; each keeps its own
        // alternative span.
        assert_eq!(&src[a.group_start..a.group_end], "A(x) | B(x)");
        assert_eq!(&src[a.alt_start..a.alt_end], "A(x)");
        assert_eq!(&src[b.alt_start..b.alt_end], "B(x)");
        let body = &analyses.matches[0].arms[0].body_bindings;
        assert_eq!(body[0].ty.as_deref(), Some("string | number"));
    }

    #[test]
    fn agreeing_alternatives_merge_to_a_single_type() {
        let src =
            "enum E { A(x: string), B(x: string) }\nconst v = match (e) { A(x) | B(x) => x };\n";
        let analyses = pattern_analyses(src, &[]);
        assert_eq!(
            analyses.matches[0].arms[0].body_bindings[0].ty.as_deref(),
            Some("string")
        );
    }

    #[test]
    fn aliases_bind_the_alias_span_and_optional_fields_widen() {
        let src = "enum E { A(v?: string), B(v: number) }\nconst r = match (e) { A(v: x) | B(v: x) => x };\n";
        let analyses = pattern_analyses(src, &[]);
        let a = binding(&analyses, src, "A(v: x)", 5);
        assert_eq!(a.name, "x");
        assert_eq!(a.ty.as_deref(), Some("string | undefined"));
        // The field-name span is not a binding.
        assert!(analyses.binding_at(at(src, "A(v: x)", 2)).is_none());
        let body = &analyses.matches[0].arms[0].body_bindings;
        assert_eq!(body[0].ty.as_deref(), Some("string | undefined | number"));
    }

    #[test]
    fn nested_patterns_resolve_through_the_field_type() {
        let src = "enum Inner { Some(value: number), None }\nenum E { A(o: Inner), B(o: Inner) }\nconst v = match (e) { A(o: Some(value)) => value, B(o: None()) => 0 };\n";
        let analyses = pattern_analyses(src, &[]);
        let x = binding(&analyses, src, "Some(value)", 5);
        assert_eq!(x.ty.as_deref(), Some("number"));
        assert_eq!(x.tag, "Some");
        assert_eq!(x.enum_name.as_deref(), Some("Inner"));
        assert_eq!(
            analyses.matches[0].arms[0].body_bindings[0].ty.as_deref(),
            Some("number")
        );
    }

    #[test]
    fn generic_field_types_resolve_their_base_enum() {
        let src = "enum E { A(o: Option<number>) }\nconst v = match (e) { A(o: Some(value)) => value, _ => 0 };\n";
        let analyses = pattern_analyses(src, &[]);
        let x = binding(&analyses, src, "Some(value)", 5);
        // Declared, not instantiated: the checker's answer supersedes this.
        assert_eq!(x.ty.as_deref(), Some("T"));
        assert_eq!(x.enum_name.as_deref(), Some("Option"));
    }

    #[test]
    fn tuple_elements_resolve_per_position() {
        let src = "enum L { A(x: string), B }\nenum R { C(y: number), D }\nconst v = match (l, r) { (A(x) | B, C(y) | D) => 0, _ => 1 };\n";
        let analyses = pattern_analyses(src, &[]);
        let x = binding(&analyses, src, "A(x)", 2);
        let y = binding(&analyses, src, "C(y)", 2);
        assert_eq!(x.ty.as_deref(), Some("string"));
        assert_eq!(y.ty.as_deref(), Some("number"));
        assert_eq!(&src[x.group_start..x.group_end], "A(x) | B");
        let m = &analyses.matches[0];
        assert_eq!(m.subjects.len(), 2);
        assert_eq!(m.subjects[0].as_ref().unwrap().enum_name, "L");
        assert_eq!(m.subjects[1].as_ref().unwrap().enum_name, "R");
    }

    #[test]
    fn builtins_answer_and_locals_shadow_them() {
        let src = "const v = match (o) { Some(value) => value, None => 0 };\n";
        let analyses = pattern_analyses(src, &[]);
        let value = binding(&analyses, src, "Some(value)", 5);
        assert_eq!(value.ty.as_deref(), Some("T"));
        assert_eq!(value.enum_name.as_deref(), Some("Option"));

        let shadowed = "enum Option { Some(value: string), None }\nconst v = match (o) { Some(value) => value, None => 0 };\n";
        let analyses = pattern_analyses(shadowed, &[]);
        let value = binding(&analyses, shadowed, "Some(value)", 5);
        assert_eq!(value.ty.as_deref(), Some("string"));
    }

    #[test]
    fn extern_declarations_answer_under_their_in_scope_names() {
        let externs = vec![EnumSymbol {
            name: "T".to_string(),
            offset: 0,
            exported: true,
            generics: String::new(),
            cases: vec![
                crate::CaseSymbol {
                    tag: "Num".to_string(),
                    offset: 0,
                    fields: Some(vec![crate::FieldSymbol {
                        name: "value".to_string(),
                        offset: 0,
                        optional: false,
                        ty: "number".to_string(),
                    }]),
                },
                crate::CaseSymbol {
                    tag: "Eof".to_string(),
                    offset: 0,
                    fields: None,
                },
            ],
        }];
        let src = "const v = match (t) { Num(value) => value, Eof => 0 };\n";
        let analyses = pattern_analyses(src, &externs);
        let value = binding(&analyses, src, "Num(value)", 4);
        assert_eq!(value.ty.as_deref(), Some("number"));
        assert_eq!(value.enum_name.as_deref(), Some("T"));
    }

    #[test]
    fn an_unresolved_subject_keeps_spans_but_knows_no_types() {
        let src = "const v = match (e) { What(x) | Ever(x) => x };\n";
        let analyses = pattern_analyses(src, &[]);
        let x = binding(&analyses, src, "What(x)", 5);
        assert_eq!(x.ty, None);
        assert_eq!(x.alternatives, 2);
        assert_eq!(analyses.matches[0].subjects[0], None);
        assert_eq!(analyses.matches[0].arms[0].body_bindings[0].ty, None);
    }

    #[test]
    fn coverage_mirrors_the_exhaustiveness_rule() {
        let src =
            "enum E { A(s: string), B, C }\nconst v = match (e) { A(x) => x, B if f() => 1 };\n";
        let analyses = pattern_analyses(src, &[]);
        let coverage = analyses.matches[0].coverage.as_ref().unwrap();
        assert_eq!(coverage.covered, ["A"]);
        // The guarded `B` arm identifies the enum but covers nothing.
        assert_eq!(coverage.missing_tags(), ["B", "C"]);
        assert_eq!(
            coverage.positions[0].as_ref().map(|e| (&e.name, &e.origin)),
            Some((&"E".to_string(), &Origin::Local))
        );

        let with_wildcard = "enum E { A, B }\nconst v = match (e) { A => 0, _ => 1 };\n";
        assert_eq!(
            pattern_analyses(with_wildcard, &[]).matches[0].coverage,
            None
        );
    }

    #[test]
    fn coverage_prefers_the_candidate_the_arms_satisfy() {
        // Both enums contain every arm tag; `Small` is fully covered, so the
        // match is exhaustive even though `Big` is missing a case. This is
        // the rule sema has always reported — an arm set that satisfies
        // *some* candidate is not a missing-case error.
        let src = "enum Big { A(s: string), B, C }\nenum Small { A(s: string), B }\nconst v = match (e) { A(x) => x, B => 1 };\n";
        let coverage = pattern_analyses(src, &[]).matches[0]
            .coverage
            .clone()
            .expect("resolved");
        assert!(coverage.missing.is_empty());
        assert_eq!(coverage.positions[0].as_ref().unwrap().name, "Small");

        // With no satisfied candidate, the one left fewest cases is named.
        let unsatisfied = "enum Big { A(s: string), B, C, D }\nenum Small { A(s: string), B, C }\nconst v = match (e) { A(x) => x, B => 1 };\n";
        let coverage = pattern_analyses(unsatisfied, &[]).matches[0]
            .coverage
            .clone()
            .expect("resolved");
        assert_eq!(coverage.positions[0].as_ref().unwrap().name, "Small");
        assert_eq!(coverage.missing_tags(), ["C"]);
    }

    #[test]
    fn coverage_of_an_imported_enum_carries_its_specifier() {
        let src = "import { Token } from \"./token.rl\";\nconst v = match (t) { Word => 0 };\n";
        let externs = [ExternEnum {
            name: "Token".to_string(),
            tags: vec!["Word".to_string(), "Punct".to_string()],
            from: Some("./token.rl".to_string()),
        }];
        let program = crate::parser::parse(src);
        let analyses = coverage_analyses(&program, &externs);
        let coverage = analyses.matches[0].coverage.as_ref().unwrap();
        assert_eq!(coverage.missing_tags(), ["Punct"]);
        assert_eq!(
            coverage.positions[0].as_ref().unwrap().origin,
            Origin::Imported {
                from: Some("./token.rl".to_string())
            }
        );
        // Coverage-only analyses skip binding work entirely.
        assert!(analyses.matches[0].arms[0].pattern_bindings.is_empty());
    }

    #[test]
    fn tuple_coverage_is_the_product_of_its_positions() {
        let src = "enum A { X(v: number), Y }\nenum B { P(v: number), Q }\nconst v = match (a, b) { (X, P) => 0, (Y, _) => 1 };\n";
        let coverage = pattern_analyses(src, &[]).matches[0]
            .coverage
            .clone()
            .expect("resolved");
        let names: Vec<&str> = coverage
            .positions
            .iter()
            .map(|p| p.as_ref().map_or("_", |e| e.name.as_str()))
            .collect();
        assert_eq!(names, ["A", "B"]);
        // (X, Q) is the only combination no arm handles.
        assert_eq!(patterns(&coverage), [["X", "Q"]]);
        // A tuple arm covers a combination, not a tag.
        assert!(coverage.covered.is_empty());
        assert!(coverage.missing_tags().is_empty());
    }

    #[test]
    fn a_universal_tuple_position_shows_as_a_hole() {
        // Nothing is ever written at position 1, so it constrains nothing.
        let src = "enum A { X(v: number), Y }\nconst v = match (a, b) { (X, _) => 0 };\n";
        let coverage = pattern_analyses(src, &[]).matches[0]
            .coverage
            .clone()
            .expect("resolved");
        assert_eq!(coverage.positions[1], None);
        assert_eq!(patterns(&coverage), [["Y", "_"]]);

        // A bare `_` arm covers everything; there is nothing to enumerate.
        let bare = "enum A { X(v: number), Y }\nconst v = match (a, b) { (X, _) => 0, _ => 1 };\n";
        assert_eq!(pattern_analyses(bare, &[]).matches[0].coverage, None);
    }

    #[test]
    fn body_definitions_answer_the_innermost_arm() {
        let src =
            "enum E { A(x: string), B(x: number) }\nconst v = match (e) { A(x) | B(x) => x };\n";
        let analyses = pattern_analyses(src, &[]);
        let body_x = src.rfind("=> x").unwrap() + 3;
        let spans = analyses.body_definitions(src, body_x);
        assert_eq!(spans.len(), 2);
        assert_eq!(&src[spans[0].0..spans[0].1], "x");
        assert_eq!(&src[spans[1].0..spans[1].1], "x");
        assert!(spans[0].0 < spans[1].0);
        // A name the arm does not bind answers nothing.
        assert!(
            analyses
                .body_definitions(src, at(src, "match (e)", 7))
                .is_empty()
        );
    }

    #[test]
    fn body_binding_lookup_merges_like_the_body_map() {
        let src =
            "enum E { A(x: string), B(x: number) }\nconst v = match (e) { A(x) | B(x) => x };\n";
        let analyses = pattern_analyses(src, &[]);
        let body_x = src.rfind("=> x").unwrap() + 3;
        let (b, span) = analyses.body_binding_at(src, body_x).unwrap();
        assert_eq!(b.ty.as_deref(), Some("string | number"));
        assert_eq!(&src[span.0..span.1], "x");
        assert!(analyses.body_binding_at(src, 0).is_none());
    }

    #[test]
    fn let_else_and_if_let_are_pattern_sites_with_typed_bindings() {
        let src = "enum E { A(x: string), B }\n\
                   const A(x) = e else { throw 0; };\n\
                   if let A(x: y) = e { use(y); }\n";
        let analyses = pattern_analyses(src, &[]);
        assert_eq!(
            analyses.sites.iter().map(|s| s.kind).collect::<Vec<_>>(),
            [SiteKind::LetElse, SiteKind::IfLet]
        );
        assert_eq!(
            analyses.sites[0]
                .subject
                .as_ref()
                .map(|s| s.enum_name.as_str()),
            Some("E")
        );
        assert_eq!(
            binding(&analyses, src, "A(x)", 2).ty.as_deref(),
            Some("string")
        );
        assert_eq!(
            binding(&analyses, src, "A(x: y)", 5).ty.as_deref(),
            Some("string")
        );
    }

    #[test]
    fn each_link_of_an_if_let_chain_is_its_own_site() {
        let src = "enum E { A(x: string), B(y: number) }\n\
                   if let A(x) = e { use(x); } else if let B(y) = e { use(y); }\n";
        let analyses = pattern_analyses(src, &[]);
        assert_eq!(analyses.sites.len(), 2);
        assert_eq!(
            binding(&analyses, src, "B(y)", 2).ty.as_deref(),
            Some("number")
        );
    }

    #[test]
    fn resolution_reports_a_misspelling_and_stays_quiet_otherwise() {
        let typo = pattern_analyses(
            "enum E { Alpha(x: string), Beta }\nconst v = match (e) { Alhpa(x) => x, Beta => 0 };\n",
            &[],
        );
        assert_eq!(typo.unresolved.len(), 1);
        assert_eq!(typo.unresolved[0].kind, NameKind::Case);
        assert_eq!(typo.unresolved[0].suggestion, "Alpha");

        // A name that is nobody's misspelling is not an error: the pattern
        // may be over a hand-written tagged union.
        let union = pattern_analyses(
            "enum E { Alpha(x: string), Beta }\nconst v = match (m) { Beta => 0, Gamma(q) => q };\n",
            &[],
        );
        assert!(union.unresolved.is_empty());
    }

    #[test]
    fn an_ambiguous_tag_identifies_no_enum() {
        // Both enums contain `A`, so neither is *the* subject — and a
        // suggestion rlc cannot choose is no suggestion.
        let analyses = pattern_analyses(
            "enum L { A(x: string), Left(n: number) }\n\
             enum R { A(x: string), Righ(n: number) }\n\
             const v = match (e) { A(x) => x, Right(n) => n };\n",
            &[],
        );
        assert!(analyses.unresolved.is_empty());
    }

    #[test]
    fn unreachable_arms_are_computed_but_not_an_error() {
        // `A` is already covered, so the third arm matches nothing new.
        let src = "enum E { A(x: string), B(y: number) }\n\
                   const v = match (e) { A(x) => x, B(y) => y, A(x: z) => z };\n";
        let coverage = pattern_analyses(src, &[]).matches[0]
            .coverage
            .clone()
            .expect("resolved");
        assert_eq!(coverage.unreachable, [2]);
        assert!(coverage.missing.is_empty());
    }

    #[test]
    fn a_guarded_arm_leaves_its_case_uncovered() {
        let src = "enum E { A(x: string), B(y: number) }\n\
                   const v = match (e) { A(x) if ok => x, B(y) => y };\n";
        let coverage = pattern_analyses(src, &[]).matches[0]
            .coverage
            .clone()
            .expect("resolved");
        assert_eq!(patterns(&coverage), [["A"]]);
    }

    #[test]
    fn a_nested_pattern_is_a_column_of_its_own() {
        let src = "enum I { Y(n: number), N }\n\
                   enum O { W(i: I), B }\n\
                   const v = match (o) { W(i: Y(n)) => n, B => 0 };\n";
        let coverage = pattern_analyses(src, &[]).matches[0]
            .coverage
            .clone()
            .expect("resolved");
        assert_eq!(patterns(&coverage), [["W(i: N())"]]);
    }

    #[test]
    fn a_witness_from_an_unidentifiable_column_is_not_certain() {
        // `Inner` names no rl enum, so the payload column's alphabet is
        // unknown and the witness is a guess. The default path reports it
        // anyway (nothing better is available without types); a consumer
        // with a checker filters on this flag and asks instead.
        let src = "enum O { W(i: Inner), B }\n\
                   const v = match (o) { W(i: Yes(n)) => n, B => 0 };\n";
        let coverage = pattern_analyses(src, &[]).matches[0]
            .coverage
            .clone()
            .expect("resolved");
        assert_eq!(patterns(&coverage), [["W"]]);
        assert!(!coverage.missing[0].certain);

        // A column the table *can* name is certain.
        let src = "enum I { Y(n: number), N }\n\
                   enum O { W(i: I), B }\n\
                   const v = match (o) { W(i: Y(n)) => n, B => 0 };\n";
        let coverage = pattern_analyses(src, &[]).matches[0]
            .coverage
            .clone()
            .expect("resolved");
        assert!(coverage.missing[0].certain);
    }

    #[test]
    fn nested_matches_are_all_collected() {
        let src = "enum E { A(x: string), B }\nconst v = match (e) { A(x) => match (e) { A(x: y) | B => 0 }, B => 1 };\n";
        let analyses = pattern_analyses(src, &[]);
        assert_eq!(analyses.matches.len(), 2);
        let y = binding(&analyses, src, "A(x: y)", 5);
        assert_eq!(y.name, "y");
        assert_eq!(y.ty.as_deref(), Some("string"));
    }
}
