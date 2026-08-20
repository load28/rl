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
//! - Exhaustiveness stays sema's to report. [`Coverage`] here is the same
//!   resolution (local > imported > built-in, first enum containing every
//!   arm tag) exposed as data, so consumers share one subject model.
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

use crate::EnumSymbol;
use crate::ast::*;

/// The typed analysis of every `match` in one source file, nested ones
/// included, in source order. Produced by [`match_analyses`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MatchAnalyses {
    /// The analyses, in source order (outer before nested).
    pub matches: Vec<MatchAnalysis>,
}

/// One `match`, normalized: its subject(s), its arms with their typed
/// bindings, and its coverage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchAnalysis {
    /// Byte offset of the `match` keyword — the analysis' identity, shared
    /// with [`crate::TagMatch::offset`] and the emitted scrutinee temporary
    /// ([`crate::ScrutineeTemp::src`]).
    pub keyword_off: usize,
    /// One subject per scrutinee position: one entry for a single match,
    /// one per position for a tuple match. `None` when the position's arm
    /// tags belong to no known enum — the match still analyzes, its
    /// declared types are simply unknown (TypeScript may still know).
    pub subjects: Vec<Option<MatchSubject>>,
    /// The arms, in source order.
    pub arms: Vec<AnalyzedArm>,
    /// Tag coverage, for a single wildcard-free tag match with a resolved
    /// subject. `None` otherwise: a wildcard covers everything, a literal
    /// match's coverage is a question about a TypeScript type
    /// ([`crate::literal_matches`]), and tuple coverage is a product sema
    /// still owns whole.
    pub coverage: Option<Coverage>,
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

/// Tag coverage of a single wildcard-free match with a resolved subject —
/// the same rule sema enforces, exposed as data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Coverage {
    /// Tags covered by unguarded, nested-free arms (a guard may be false
    /// and a nested pattern may mismatch, so those arms cover nothing).
    pub covered: Vec<String>,
    /// The subject's tags no covering arm handles.
    pub missing: Vec<String>,
}

impl MatchAnalyses {
    /// The pattern binding whose bound-name span contains `offset` (end
    /// inclusive, matching how the language surface treats a chunk's end).
    ///
    /// ```
    /// let src = "enum E { A(x: string), B(x: number) }\nconst v = match (e) { A(x) | B(x) => x };\n";
    /// let analyses = rlc::match_analyses(src, &[]);
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
    /// [`MatchAnalyses::body_definitions`].
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
/// let analyses = rlc::match_analyses(src, &[]);
/// let subject = analyses.matches[0].subjects[0].as_ref().unwrap();
/// assert_eq!(subject.enum_name, "E");
/// assert_eq!(analyses.matches[0].arms[0].body_bindings[0].ty.as_deref(), Some("string"));
/// ```
pub fn match_analyses(source: &str, externs: &[EnumSymbol]) -> MatchAnalyses {
    let program = crate::parser::parse(source);
    let table = Table::build(&program, externs);
    let mut analyses = MatchAnalyses::default();
    walk(&program, &table, &mut analyses);
    analyses
}

/// The candidate enums a match's subject can resolve to, in shadowing
/// order — the analysis' declaration table.
struct Table {
    /// `(in-scope name, constructors)`, local > imported > built-in, each
    /// name once (the nearer origin wins, as in sema).
    entries: Vec<(String, Vec<MatchConstructor>)>,
}

impl Table {
    fn build(program: &Program, externs: &[EnumSymbol]) -> Table {
        let mut entries: Vec<(String, Vec<MatchConstructor>)> = Vec::new();
        collect_local_enums(program, &mut entries);
        for e in externs {
            if entries.iter().any(|(name, _)| *name == e.name) {
                continue;
            }
            entries.push((
                e.name.clone(),
                e.cases
                    .iter()
                    .map(|c| MatchConstructor {
                        tag: c.tag.clone(),
                        fields: c.fields.as_ref().map(|fields| {
                            fields
                                .iter()
                                .map(|f| PayloadField {
                                    name: f.name.clone(),
                                    optional: f.optional,
                                    ty: f.ty.clone(),
                                })
                                .collect()
                        }),
                    })
                    .collect(),
            ));
        }
        for (name, constructors) in builtin_enums() {
            if entries.iter().any(|(n, _)| *n == name) {
                continue;
            }
            entries.push((name, constructors));
        }
        Table { entries }
    }

    /// The first enum whose cases contain every tag — `None` for an empty
    /// tag set (nothing identifies an enum) or when no candidate fits.
    fn resolve(&self, tags: &[&str]) -> Option<(&str, &[MatchConstructor])> {
        if tags.is_empty() {
            return None;
        }
        self.entries
            .iter()
            .find(|(_, cases)| tags.iter().all(|t| cases.iter().any(|c| c.tag == *t)))
            .map(|(name, cases)| (name.as_str(), cases.as_slice()))
    }

    /// The enum a declared type text names: a bare (possibly dotted)
    /// identifier, optionally with type arguments — `Shape`,
    /// `Option<number>`, `ns.Token` — and nothing else. Type arguments are
    /// not substituted (rlc has no type system); the constructor's declared
    /// field text answers as written.
    fn resolve_type(&self, ty: &str) -> Option<(&str, &[MatchConstructor])> {
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
        self.entries
            .iter()
            .find(|(name, _)| name == base)
            .map(|(name, cases)| (name.as_str(), cases.as_slice()))
    }
}

fn collect_local_enums(program: &Program, entries: &mut Vec<(String, Vec<MatchConstructor>)>) {
    for segment in &program.segments {
        if let Segment::Enum(decl) = segment {
            let constructors = decl
                .cases
                .iter()
                .map(|c| MatchConstructor {
                    tag: c.tag.clone(),
                    fields: c.fields.as_ref().map(|fields| {
                        fields
                            .iter()
                            .map(|f| PayloadField {
                                name: f.name.clone(),
                                optional: f.optional,
                                ty: f.ty.clone(),
                            })
                            .collect()
                    }),
                })
                .collect();
            // Later declarations win, as in sema's registry.
            if let Some(entry) = entries.iter_mut().find(|(name, _)| *name == decl.name) {
                entry.1 = constructors;
            } else {
                entries.push((decl.name.clone(), constructors));
            }
        }
    }
}

/// `Option`/`Result` as the analysis sees them, matching
/// [`crate::stdlib::BUILTIN_ENUMS`] (tags) and the standard library module
/// (field names). Payload types are the declarations' type parameters —
/// the honest declared answer; instantiation is the checker's.
fn builtin_enums() -> Vec<(String, Vec<MatchConstructor>)> {
    let field = |name: &str, ty: &str| PayloadField {
        name: name.to_string(),
        optional: false,
        ty: ty.to_string(),
    };
    vec![
        (
            "Option".to_string(),
            vec![
                MatchConstructor {
                    tag: "Some".to_string(),
                    fields: Some(vec![field("value", "T")]),
                },
                MatchConstructor {
                    tag: "None".to_string(),
                    fields: None,
                },
            ],
        ),
        (
            "Result".to_string(),
            vec![
                MatchConstructor {
                    tag: "Ok".to_string(),
                    fields: Some(vec![field("value", "T")]),
                },
                MatchConstructor {
                    tag: "Err".to_string(),
                    fields: Some(vec![field("error", "E")]),
                },
            ],
        ),
    ]
}

fn walk(program: &Program, table: &Table, out: &mut MatchAnalyses) {
    for segment in &program.segments {
        match segment {
            Segment::Verbatim(_)
            | Segment::RlImport(_)
            | Segment::Enum(_)
            | Segment::ValModifier(_) => {}
            Segment::Match(expr) => {
                out.matches.push(analyze_match(expr, table));
                walk(&expr.scrutinee, table, out);
                for arm in &expr.arms {
                    if let Some(guard) = &arm.guard {
                        walk(&guard.expr, table, out);
                    }
                    walk(&arm.body, table, out);
                }
            }
            Segment::TupleMatch(expr) => {
                out.matches.push(analyze_tuple_match(expr, table));
                for (_, scrutinee) in &expr.scrutinees {
                    walk(scrutinee, table, out);
                }
                for arm in &expr.arms {
                    if let Some(guard) = &arm.guard {
                        walk(&guard.expr, table, out);
                    }
                    walk(&arm.body, table, out);
                }
            }
            Segment::Try(stmt) => walk(&stmt.expr, table, out),
            Segment::LetElse(stmt) => {
                walk(&stmt.expr, table, out);
                walk(&stmt.else_body, table, out);
            }
            Segment::IfLet(stmt) => walk_if_let(stmt, table, out),
            Segment::Pipe(pipe) => {
                if let Some(head) = &pipe.head {
                    walk(head, table, out);
                }
                for step in &pipe.steps {
                    walk(&step.body, table, out);
                }
            }
            Segment::ResultBlock(block) => {
                for item in &block.items {
                    match item {
                        ResultItem::Stmts(stmts) => walk(stmts, table, out),
                        ResultItem::Bind(bind) => walk(&bind.expr, table, out),
                    }
                }
                walk(&block.value, table, out);
            }
            Segment::Template(template) => {
                for chunk in &template.chunks {
                    if let TemplateChunk::Interp(interp) = chunk {
                        walk(interp, table, out);
                    }
                }
            }
        }
    }
}

fn walk_if_let(stmt: &IfLetStmt, table: &Table, out: &mut MatchAnalyses) {
    walk(&stmt.expr, table, out);
    walk(&stmt.body, table, out);
    match &stmt.else_part {
        Some(IfLetElse::Block(block)) => walk(block, table, out),
        Some(IfLetElse::IfLet(inner)) => walk_if_let(inner, table, out),
        None => {}
    }
}

fn analyze_match(expr: &MatchExpr, table: &Table) -> MatchAnalysis {
    // The subject is identified from *every* arm's tags, guarded or not —
    // the same identification sema uses.
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
                body_start: arm.body_span.start,
                body_end: arm.body_span.end,
                pattern_bindings: Vec::new(),
                body_bindings: Vec::new(),
            };
            if let Pattern::Tags(alts) = &arm.pattern {
                analyze_group(alts, subject, table, &mut analyzed);
            }
            analyzed
        })
        .collect();

    let coverage = coverage_of(expr, subject);
    MatchAnalysis {
        keyword_off: expr.keyword_off,
        subjects: vec![subject.map(to_subject)],
        arms,
        coverage,
    }
}

fn analyze_tuple_match(expr: &TupleMatchExpr, table: &Table) -> MatchAnalysis {
    let arity = expr.scrutinees.len();
    // Each position resolves independently, from the tags every arm uses
    // there — sema's tuple identification.
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
                body_start: arm.body_span.start,
                body_end: arm.body_span.end,
                pattern_bindings: Vec::new(),
                body_bindings: Vec::new(),
            };
            if let TuplePattern::Elems(elems) = &arm.pattern {
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
        subjects: subjects.into_iter().map(|s| s.map(to_subject)).collect(),
        arms,
        coverage: None,
    }
}

fn to_subject((name, constructors): (&str, &[MatchConstructor])) -> MatchSubject {
    MatchSubject {
        enum_name: name.to_string(),
        constructors: constructors.to_vec(),
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
            alt,
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
    alt: &TagPattern,
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
                    inner,
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
                    tag: alt.tag.clone(),
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

/// [`Coverage`] of a single match, when it means something: a tag match
/// with a resolved subject and no wildcard arm — sema's covering rule
/// (guarded arms and nested patterns cover nothing) over the subject's
/// cases.
fn coverage_of(expr: &MatchExpr, subject: Option<(&str, &[MatchConstructor])>) -> Option<Coverage> {
    let (_, cases) = subject?;
    if expr
        .arms
        .iter()
        .any(|a| matches!(a.pattern, Pattern::Wildcard))
    {
        return None;
    }
    let mut covered: Vec<String> = Vec::new();
    for arm in &expr.arms {
        let Pattern::Tags(alts) = &arm.pattern else {
            continue;
        };
        let nested = alts.iter().any(|alt| {
            alt.bindings
                .as_deref()
                .unwrap_or_default()
                .iter()
                .any(|b| b.nested.is_some())
        });
        if arm.guard.is_some() || nested {
            continue;
        }
        for alt in alts {
            if !covered.contains(&alt.tag) {
                covered.push(alt.tag.clone());
            }
        }
    }
    let missing = cases
        .iter()
        .filter(|c| !covered.contains(&c.tag))
        .map(|c| c.tag.clone())
        .collect();
    Some(Coverage { covered, missing })
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

    fn at(src: &str, needle: &str, delta: usize) -> usize {
        src.find(needle).expect("needle") + delta
    }

    /// `A(x)`-style lookup: the binding two bytes into the needle.
    fn binding<'a>(
        analyses: &'a MatchAnalyses,
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
        let analyses = match_analyses(src, &[]);
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
        let analyses = match_analyses(src, &[]);
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
        let analyses = match_analyses(src, &[]);
        assert_eq!(
            analyses.matches[0].arms[0].body_bindings[0].ty.as_deref(),
            Some("string")
        );
    }

    #[test]
    fn aliases_bind_the_alias_span_and_optional_fields_widen() {
        let src = "enum E { A(v?: string), B(v: number) }\nconst r = match (e) { A(v: x) | B(v: x) => x };\n";
        let analyses = match_analyses(src, &[]);
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
        let analyses = match_analyses(src, &[]);
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
        let analyses = match_analyses(src, &[]);
        let x = binding(&analyses, src, "Some(value)", 5);
        // Declared, not instantiated: the checker's answer supersedes this.
        assert_eq!(x.ty.as_deref(), Some("T"));
        assert_eq!(x.enum_name.as_deref(), Some("Option"));
    }

    #[test]
    fn tuple_elements_resolve_per_position() {
        let src = "enum L { A(x: string), B }\nenum R { C(y: number), D }\nconst v = match (l, r) { (A(x) | B, C(y) | D) => 0, _ => 1 };\n";
        let analyses = match_analyses(src, &[]);
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
        let analyses = match_analyses(src, &[]);
        let value = binding(&analyses, src, "Some(value)", 5);
        assert_eq!(value.ty.as_deref(), Some("T"));
        assert_eq!(value.enum_name.as_deref(), Some("Option"));

        let shadowed = "enum Option { Some(value: string), None }\nconst v = match (o) { Some(value) => value, None => 0 };\n";
        let analyses = match_analyses(shadowed, &[]);
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
        let analyses = match_analyses(src, &externs);
        let value = binding(&analyses, src, "Num(value)", 4);
        assert_eq!(value.ty.as_deref(), Some("number"));
        assert_eq!(value.enum_name.as_deref(), Some("T"));
    }

    #[test]
    fn an_unresolved_subject_keeps_spans_but_knows_no_types() {
        let src = "const v = match (e) { What(x) | Ever(x) => x };\n";
        let analyses = match_analyses(src, &[]);
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
        let analyses = match_analyses(src, &[]);
        let coverage = analyses.matches[0].coverage.as_ref().unwrap();
        assert_eq!(coverage.covered, ["A"]);
        // The guarded `B` arm identifies the enum but covers nothing.
        assert_eq!(coverage.missing, ["B", "C"]);

        let with_wildcard = "enum E { A, B }\nconst v = match (e) { A => 0, _ => 1 };\n";
        assert_eq!(match_analyses(with_wildcard, &[]).matches[0].coverage, None);
    }

    #[test]
    fn body_definitions_answer_the_innermost_arm() {
        let src =
            "enum E { A(x: string), B(x: number) }\nconst v = match (e) { A(x) | B(x) => x };\n";
        let analyses = match_analyses(src, &[]);
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
        let analyses = match_analyses(src, &[]);
        let body_x = src.rfind("=> x").unwrap() + 3;
        let (b, span) = analyses.body_binding_at(src, body_x).unwrap();
        assert_eq!(b.ty.as_deref(), Some("string | number"));
        assert_eq!(&src[span.0..span.1], "x");
        assert!(analyses.body_binding_at(src, 0).is_none());
    }

    #[test]
    fn nested_matches_are_all_collected() {
        let src = "enum E { A(x: string), B }\nconst v = match (e) { A(x) => match (e) { A(x: y) | B => 0 }, B => 1 };\n";
        let analyses = match_analyses(src, &[]);
        assert_eq!(analyses.matches.len(), 2);
        let y = binding(&analyses, src, "A(x: y)", 5);
        assert_eq!(y.name, "y");
        assert_eq!(y.ty.as_deref(), Some("string"));
    }
}
