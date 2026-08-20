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

use crate::ast::*;
use crate::{EnumSymbol, ExternEnum};

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
    /// The exhaustiveness answer — for a single match over its subject's
    /// tags, for a tuple match over the product of its positions. `None`
    /// when the question does not arise: a wildcard arm covers everything,
    /// the tags identify no known enum, or the match is a literal one
    /// (whose exhaustiveness is a question about a TypeScript type —
    /// [`crate::literal_matches`]). This is what sema reports on; there is
    /// no second implementation of the rule.
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
    /// Tags covered by covering arms — unguarded and nested-free, since a
    /// guard may be false and a nested pattern may mismatch. Single matches
    /// only: a tuple arm covers a *combination*, not a tag, so this is
    /// empty there and [`Coverage::missing`] carries the whole answer.
    pub covered: Vec<String>,
    /// The combinations no covering arm handles, in the subject's case
    /// order: one row per uncovered combination, one entry per position
    /// (`None` at a universal position). Empty when the match is
    /// exhaustive.
    pub missing: Vec<Vec<Option<String>>>,
}

impl Coverage {
    /// The arity-1 view of [`Coverage::missing`] — the tags a single match
    /// leaves uncovered. Empty for a tuple match's coverage.
    pub fn missing_tags(&self) -> Vec<&str> {
        if self.positions.len() != 1 {
            return Vec::new();
        }
        self.missing
            .iter()
            .filter_map(|row| row.first().and_then(Option::as_deref))
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
    analyze(&program, &table, Depth::Full)
}

/// The coverage-only analysis of an already-parsed program — sema's input,
/// so exhaustiveness and the editor's model answer from one implementation
/// of the rule.
///
/// `externs` are the imported declarations the CLI collects for sema
/// ([`crate::Options::extern_enums`]); they carry tags without field types,
/// which is all coverage needs. Pattern bindings are therefore not analyzed
/// and every arm comes back empty — [`match_analyses`] is the entry point
/// for those.
pub(crate) fn coverage_analyses(program: &Program, externs: &[ExternEnum]) -> MatchAnalyses {
    let table = Table::build_from_tags(program, externs);
    analyze(program, &table, Depth::CoverageOnly)
}

/// How much of each match to analyze — bindings cost work no coverage
/// consumer would read.
#[derive(Clone, Copy, PartialEq)]
enum Depth {
    Full,
    CoverageOnly,
}

fn analyze(program: &Program, table: &Table, depth: Depth) -> MatchAnalyses {
    let mut analyses = MatchAnalyses::default();
    walk(program, table, depth, &mut analyses);
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
    /// The table an editor-side analysis uses: imported declarations with
    /// their field types ([`EnumSymbol`]), so pattern bindings get types.
    fn build(program: &Program, externs: &[EnumSymbol]) -> Table {
        Table::assemble(
            program,
            externs
                .iter()
                .map(|e| Entry {
                    name: e.name.clone(),
                    origin: Origin::Imported { from: None },
                    constructors: e
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
                        .collect(),
                })
                .collect(),
        )
    }

    /// The table the compiler's own passes use: imported declarations as
    /// the CLI collects them for sema ([`ExternEnum`] — tags and the
    /// specifier they came from, no field types).
    fn build_from_tags(program: &Program, externs: &[ExternEnum]) -> Table {
        Table::assemble(
            program,
            externs
                .iter()
                .map(|e| Entry {
                    name: e.name.clone(),
                    origin: Origin::Imported {
                        from: e.from.clone(),
                    },
                    constructors: e
                        .tags
                        .iter()
                        .map(|tag| MatchConstructor {
                            tag: tag.clone(),
                            fields: None,
                        })
                        .collect(),
                })
                .collect(),
        )
    }

    fn assemble(program: &Program, externs: Vec<Entry>) -> Table {
        let mut entries: Vec<Entry> = Vec::new();
        collect_local_enums(program, &mut entries);
        for e in externs {
            if entries.iter().any(|entry| entry.name == e.name) {
                continue;
            }
            entries.push(e);
        }
        for (name, constructors) in builtin_enums() {
            if entries.iter().any(|entry| entry.name == name) {
                continue;
            }
            entries.push(Entry {
                name,
                origin: Origin::Builtin,
                constructors,
            });
        }
        Table { entries }
    }

    /// The first enum whose cases contain every tag — `None` for an empty
    /// tag set (nothing identifies an enum) or when no candidate fits.
    ///
    /// This is the *type* answer: which declaration a pattern binding reads
    /// its field type from. Exhaustiveness asks a different question and
    /// uses [`Table::resolve_coverage`].
    fn resolve(&self, tags: &[&str]) -> Option<(&str, &[MatchConstructor])> {
        if tags.is_empty() {
            return None;
        }
        self.candidates(tags)
            .first()
            .map(|e| (e.name.as_str(), e.constructors.as_slice()))
    }

    /// Every candidate for a tag set, in shadowing order: the enums whose
    /// cases contain every tag the arms use.
    fn candidates(&self, tags: &[&str]) -> Vec<&Entry> {
        self.entries
            .iter()
            .filter(|e| {
                tags.iter()
                    .all(|t| e.constructors.iter().any(|c| c.tag == *t))
            })
            .collect()
    }

    /// The exhaustiveness answer for one tag set: the candidate the covering
    /// arms satisfy if there is one, otherwise the candidate they leave
    /// fewest cases of — the rule sema has always reported, now with one
    /// implementation. `None` when no candidate fits the tags at all: rlc
    /// has no type information for such a match, so it is not checked.
    fn resolve_coverage(
        &self,
        tags: &[&str],
        covered: &[String],
    ) -> Option<(CoveredEnum, Vec<String>)> {
        if tags.is_empty() {
            return None;
        }
        let mut best: Option<(&Entry, Vec<String>)> = None;
        for entry in self.candidates(tags) {
            let missing: Vec<String> = entry
                .constructors
                .iter()
                .filter(|c| !covered.contains(&c.tag))
                .map(|c| c.tag.clone())
                .collect();
            if missing.is_empty() {
                return Some((entry.covered_enum(), Vec::new()));
            }
            if best.as_ref().is_none_or(|(_, m)| missing.len() < m.len()) {
                best = Some((entry, missing));
            }
        }
        best.map(|(entry, missing)| (entry.covered_enum(), missing))
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
            .find(|e| e.name == base)
            .map(|e| (e.name.as_str(), e.constructors.as_slice()))
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

fn collect_local_enums(program: &Program, entries: &mut Vec<Entry>) {
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
            if let Some(entry) = entries.iter_mut().find(|e| e.name == decl.name) {
                entry.constructors = constructors;
            } else {
                entries.push(Entry {
                    name: decl.name.clone(),
                    origin: Origin::Local,
                    constructors,
                });
            }
        }
    }
}

/// `Option`/`Result` as every consumer sees them: the enums a file gets
/// without declaring them. Tags and field names match the standard library
/// module (`src/stdlib/rl_std.ts`); payload types are the declarations'
/// type parameters — the honest declared answer, since instantiation is the
/// checker's. This is the only table of the built-ins in the compiler.
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

fn walk(program: &Program, table: &Table, depth: Depth, out: &mut MatchAnalyses) {
    for segment in &program.segments {
        match segment {
            Segment::Verbatim(_)
            | Segment::RlImport(_)
            | Segment::Enum(_)
            | Segment::ValModifier(_) => {}
            Segment::Match(expr) => {
                out.matches.push(analyze_match(expr, table, depth));
                walk(&expr.scrutinee, table, depth, out);
                for arm in &expr.arms {
                    if let Some(guard) = &arm.guard {
                        walk(&guard.expr, table, depth, out);
                    }
                    walk(&arm.body, table, depth, out);
                }
            }
            Segment::TupleMatch(expr) => {
                out.matches.push(analyze_tuple_match(expr, table, depth));
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

fn walk_if_let(stmt: &IfLetStmt, table: &Table, depth: Depth, out: &mut MatchAnalyses) {
    walk(&stmt.expr, table, depth, out);
    walk(&stmt.body, table, depth, out);
    match &stmt.else_part {
        Some(IfLetElse::Block(block)) => walk(block, table, depth, out),
        Some(IfLetElse::IfLet(inner)) => walk_if_let(inner, table, depth, out),
        None => {}
    }
}

fn analyze_match(expr: &MatchExpr, table: &Table, depth: Depth) -> MatchAnalysis {
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
            if let (Depth::Full, Pattern::Tags(alts)) = (depth, &arm.pattern) {
                analyze_group(alts, subject, table, &mut analyzed);
            }
            analyzed
        })
        .collect();

    let coverage = coverage_of(expr, table);
    MatchAnalysis {
        keyword_off: expr.keyword_off,
        subjects: vec![subject.map(to_subject)],
        arms,
        coverage,
    }
}

fn analyze_tuple_match(expr: &TupleMatchExpr, table: &Table, depth: Depth) -> MatchAnalysis {
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
        subjects: subjects.into_iter().map(|s| s.map(to_subject)).collect(),
        arms,
        coverage: tuple_coverage_of(expr, table),
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
fn coverage_of(expr: &MatchExpr, table: &Table) -> Option<Coverage> {
    if expr
        .arms
        .iter()
        .any(|a| matches!(a.pattern, Pattern::Wildcard))
    {
        return None;
    }
    // Identification uses every arm's tags; covering uses only the arms
    // that cannot fall through.
    let mut tags: Vec<&str> = Vec::new();
    let mut covered: Vec<String> = Vec::new();
    for arm in &expr.arms {
        let Pattern::Tags(alts) = &arm.pattern else {
            continue;
        };
        for alt in alts {
            if !tags.contains(&alt.tag.as_str()) {
                tags.push(&alt.tag);
            }
        }
        if !covers(&arm.guard, alts) {
            continue;
        }
        for alt in alts {
            if !covered.contains(&alt.tag) {
                covered.push(alt.tag.clone());
            }
        }
    }
    let (subject, missing) = table.resolve_coverage(&tags, &covered)?;
    Some(Coverage {
        positions: vec![Some(subject)],
        covered,
        missing: missing.into_iter().map(|tag| vec![Some(tag)]).collect(),
    })
}

/// [`Coverage`] of a tuple match: the cartesian product of its positions'
/// case sets, minus every combination a covering arm handles. `None` when
/// a bare `_` arm covers everything, when any tagged position resolves to
/// no known enum, or when no position is tagged at all (nothing to
/// enumerate).
fn tuple_coverage_of(expr: &TupleMatchExpr, table: &Table) -> Option<Coverage> {
    let arity = expr.scrutinees.len();
    if expr
        .arms
        .iter()
        .any(|a| matches!(a.pattern, TuplePattern::Wildcard))
    {
        return None;
    }

    // Per position, the tags any arm uses there (identification); per
    // covering arm, what it covers at each position (`None` = `_`).
    let mut position_tags: Vec<Vec<String>> = vec![Vec::new(); arity];
    let mut rows: Vec<Vec<Option<Vec<String>>>> = Vec::new();
    for arm in &expr.arms {
        let TuplePattern::Elems(elems) = &arm.pattern else {
            continue;
        };
        if elems.len() != arity {
            continue; // sema reports the arity mismatch; nothing to enumerate here
        }
        let mut row: Vec<Option<Vec<String>>> = Vec::with_capacity(arity);
        let mut nested = false;
        for (p, elem) in elems.iter().enumerate() {
            match elem {
                Pattern::Wildcard => row.push(None),
                // A literal element covers no tag combination.
                Pattern::Literals(_) => row.push(Some(Vec::new())),
                Pattern::Tags(alts) => {
                    nested |= alts.iter().any(has_nested);
                    let tags: Vec<String> = alts.iter().map(|t| t.tag.clone()).collect();
                    for tag in &tags {
                        if !position_tags[p].contains(tag) {
                            position_tags[p].push(tag.clone());
                        }
                    }
                    row.push(Some(tags));
                }
            }
        }
        if arm.guard.is_none() && !nested {
            rows.push(row);
        }
    }

    // Each position resolves independently, to the first candidate whose
    // cases contain every tag used there.
    let mut positions: Vec<Option<CoveredEnum>> = Vec::with_capacity(arity);
    let mut cases: Vec<Vec<String>> = Vec::with_capacity(arity);
    for tags in &position_tags {
        if tags.is_empty() {
            positions.push(None); // universal position: only `_` written here
            cases.push(Vec::new());
            continue;
        }
        let refs: Vec<&str> = tags.iter().map(String::as_str).collect();
        let entry = *table.candidates(&refs).first()?;
        positions.push(Some(entry.covered_enum()));
        cases.push(entry.constructors.iter().map(|c| c.tag.clone()).collect());
    }

    let tagged: Vec<usize> = (0..arity).filter(|&p| !cases[p].is_empty()).collect();
    if tagged.is_empty() {
        return None;
    }

    // Odometer over the tagged positions, rightmost fastest.
    let mut missing: Vec<Vec<Option<String>>> = Vec::new();
    let mut idx = vec![0usize; tagged.len()];
    loop {
        let handled = rows.iter().any(|row| {
            tagged.iter().enumerate().all(|(ti, &p)| match &row[p] {
                None => true,
                Some(tags) => tags.iter().any(|t| *t == cases[p][idx[ti]]),
            })
        });
        if !handled {
            let mut combination: Vec<Option<String>> = vec![None; arity];
            for (ti, &p) in tagged.iter().enumerate() {
                combination[p] = Some(cases[p][idx[ti]].clone());
            }
            missing.push(combination);
        }
        let mut ti = tagged.len();
        let mut wrapped = true;
        while ti > 0 {
            ti -= 1;
            idx[ti] += 1;
            if idx[ti] < cases[tagged[ti]].len() {
                wrapped = false;
                break;
            }
            idx[ti] = 0;
        }
        if wrapped {
            break;
        }
    }

    Some(Coverage {
        positions,
        covered: Vec::new(),
        missing,
    })
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
        assert_eq!(coverage.missing_tags(), ["B", "C"]);
        assert_eq!(
            coverage.positions[0].as_ref().map(|e| (&e.name, &e.origin)),
            Some((&"E".to_string(), &Origin::Local))
        );

        let with_wildcard = "enum E { A, B }\nconst v = match (e) { A => 0, _ => 1 };\n";
        assert_eq!(match_analyses(with_wildcard, &[]).matches[0].coverage, None);
    }

    #[test]
    fn coverage_prefers_the_candidate_the_arms_satisfy() {
        // Both enums contain every arm tag; `Small` is fully covered, so the
        // match is exhaustive even though `Big` is missing a case. This is
        // the rule sema has always reported — an arm set that satisfies
        // *some* candidate is not a missing-case error.
        let src = "enum Big { A(s: string), B, C }\nenum Small { A(s: string), B }\nconst v = match (e) { A(x) => x, B => 1 };\n";
        let coverage = match_analyses(src, &[]).matches[0]
            .coverage
            .clone()
            .expect("resolved");
        assert!(coverage.missing.is_empty());
        assert_eq!(coverage.positions[0].as_ref().unwrap().name, "Small");

        // With no satisfied candidate, the one left fewest cases is named.
        let unsatisfied = "enum Big { A(s: string), B, C, D }\nenum Small { A(s: string), B, C }\nconst v = match (e) { A(x) => x, B => 1 };\n";
        let coverage = match_analyses(unsatisfied, &[]).matches[0]
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
        let coverage = match_analyses(src, &[]).matches[0]
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
        assert_eq!(
            coverage.missing,
            [vec![Some("X".to_string()), Some("Q".to_string())]]
        );
        // A tuple arm covers a combination, not a tag.
        assert!(coverage.covered.is_empty());
        assert!(coverage.missing_tags().is_empty());
    }

    #[test]
    fn a_universal_tuple_position_shows_as_a_hole() {
        // Nothing is ever written at position 1, so it constrains nothing.
        let src = "enum A { X(v: number), Y }\nconst v = match (a, b) { (X, _) => 0 };\n";
        let coverage = match_analyses(src, &[]).matches[0]
            .coverage
            .clone()
            .expect("resolved");
        assert_eq!(coverage.positions[1], None);
        assert_eq!(coverage.missing, [vec![Some("Y".to_string()), None]]);

        // A bare `_` arm covers everything; there is nothing to enumerate.
        let bare = "enum A { X(v: number), Y }\nconst v = match (a, b) { (X, _) => 0, _ => 1 };\n";
        assert_eq!(match_analyses(bare, &[]).matches[0].coverage, None);
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
