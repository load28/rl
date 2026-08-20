//! Exhaustiveness by usefulness — the algorithm, in rl's terms.
//!
//! This is Maranget's *usefulness* check, the one rustc's pattern analysis
//! is built on: a pattern row `q` is **useful** with respect to a matrix of
//! rows `P` when some value matches `q` and no row of `P`. Two questions
//! rl asks are the same question:
//!
//! - **Exhaustiveness** — is the all-wildcard row useful against the arms?
//!   Every value it finds is a value nothing handles, and the algorithm
//!   hands back that value as a **witness** (`Wrap(inner: Yes)`), not just
//!   the name of a missing case.
//! - **Reachability** — is an arm useful against the arms before it? A row
//!   that is not is dead code.
//!
//! Why this replaced counting tags: the old rule multiplied tag sets, so it
//! could only see one level. An arm carrying a nested pattern had to be
//! treated as covering *nothing* — which rejected genuinely exhaustive
//! matches (`docs/tasks/TASK-103-usefulness-exhaustiveness.md`). The
//! recursion here descends into payloads instead, so a nested pattern
//! covers exactly what it covers.
//!
//! What rl brings that a Rust matrix does not: patterns bind fields **by
//! name and by subset** (`Circle(radius)` and `Circle()` both match every
//! `Circle`). Specialization normalizes that — a constructor's row becomes
//! one column per *declared* field, in declaration order, with a wildcard
//! wherever the pattern said nothing.
//!
//! What rlc still does not know is types. A column whose constructor set
//! cannot be identified is [`ColTy::Opaque`]: only a wildcard covers it,
//! so the answer stays conservative rather than confident and wrong.

use super::{Entry, MatchConstructor, Table};
use crate::ast::TagPattern;

/// What the algorithm resolves column types against: the declaration
/// table, plus the alphabets a type checker named for payload columns.
///
/// The override is keyed by `(constructor, field)` — a constructor's field
/// has one declared type, so the pair names one column wherever it is
/// written.
pub(super) struct Alphabets<'a> {
    pub table: &'a Table,
    pub payloads: Vec<((String, String), Entry)>,
}

impl<'a> Alphabets<'a> {
    pub(super) fn of(table: &'a Table) -> Alphabets<'a> {
        Alphabets {
            table,
            payloads: Vec::new(),
        }
    }

    fn payload(&self, tag: &str, field: &str) -> Option<&Entry> {
        self.payloads
            .iter()
            .find(|((t, f), _)| t == tag && f == field)
            .map(|(_, entry)| entry)
    }
}

/// One cell of a pattern row: the source pattern, or a wildcard.
///
/// Rows borrow the AST rather than lowering it first, because how a
/// pattern *decomposes* depends on the column's constructor — which the
/// algorithm only knows as it descends.
#[derive(Clone, Copy)]
pub(super) enum Cell<'a> {
    Wild,
    Tag(&'a TagPattern),
}

/// What a column ranges over.
#[derive(Clone, Copy)]
pub(super) enum ColTy<'a> {
    /// A known enum: its constructors are the column's alphabet, so the
    /// column can be *completed* and a missing constructor named.
    Enum(&'a Entry),
    /// Nothing was written here at all — a tuple position every arm left
    /// as `_`. It constrains nothing, and saying so (`_`) is a complete
    /// answer, not a gap in one.
    Unconstrained,
    /// Constructors were written here, but which alphabet they belong to
    /// could not be identified — a hand-written union, a payload whose
    /// declared type names no enum. Only a wildcard covers it, and any
    /// witness that passes through is a **guess**: rl does not know what
    /// else the column admits.
    Unknown,
}

/// A value the arms do not handle, in rl pattern syntax.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Witness {
    /// Anything, and rl knows that is the whole answer.
    Wild,
    /// Anything, but only because the column's alphabet is unknown — the
    /// witness containing it is not certain (see [`Witness::certain`]).
    Unknown,
    Ctor {
        tag: String,
        /// Every declared field, in order, with what the witness needs it
        /// to be. Rendering drops the wildcards.
        args: Vec<(String, Witness)>,
    },
}

impl Witness {
    /// The witness as an rl pattern — **what a user can paste in as an
    /// arm**. That is the whole contract of this rendering, and it is why
    /// the two positions differ:
    ///
    /// - At the top of an arm, a constructor with nothing to constrain is
    ///   the bare tag (`Wrap`), which matches every `Wrap`.
    /// - Nested, the same thing must be written `Wrap(inner: No())`:
    ///   inside a pattern, `field: Name` **binds** the field to `Name`
    ///   rather than matching a case of that name (`language.md` §3.2), so
    ///   dropping the parens would render a pattern that compiles and
    ///   means something else.
    pub(super) fn render(&self) -> String {
        self.write(false)
    }

    fn write(&self, nested: bool) -> String {
        match self {
            Witness::Wild | Witness::Unknown => "_".to_string(),
            Witness::Ctor { tag, args } => {
                let constrained: Vec<String> = args
                    .iter()
                    .filter(|(_, w)| !matches!(w, Witness::Wild | Witness::Unknown))
                    .map(|(name, w)| format!("{name}: {}", w.write(true)))
                    .collect();
                match (constrained.is_empty(), nested) {
                    (true, false) => tag.clone(),
                    (true, true) => format!("{tag}()"),
                    (false, _) => format!("{tag}({})", constrained.join(", ")),
                }
            }
        }
    }
}

impl Witness {
    /// Whether every position of this witness was decided from a known
    /// constructor set. A witness that is not certain names a value rl
    /// *guessed* at, which a consumer with a real type checker refuses to
    /// report (TASK-108).
    pub(super) fn certain(&self) -> bool {
        match self {
            Witness::Unknown => false,
            Witness::Wild => true,
            Witness::Ctor { args, .. } => args.iter().all(|(_, w)| w.certain()),
        }
    }
}

/// How many witnesses to build before giving up on completeness. A
/// message showing more than a handful is unreadable anyway, and the
/// product of several columns can grow fast.
const WITNESS_BUDGET: usize = 40;

/// The values `rows` leaves unhandled, over columns of the given types.
/// Empty means exhaustive.
pub(super) fn missing<'a>(
    rows: &[Vec<Cell<'a>>],
    types: &[ColTy<'a>],
    cx: &'a Alphabets<'a>,
) -> Vec<Vec<Witness>> {
    let query: Vec<Cell> = vec![Cell::Wild; types.len()];
    usefulness(rows, &query, types, cx)
}

/// Whether `row` matches anything `rows` does not — reachability, asked of
/// one arm against the arms before it.
pub(super) fn is_useful<'a>(
    rows: &[Vec<Cell<'a>>],
    row: &[Cell<'a>],
    types: &[ColTy<'a>],
    cx: &'a Alphabets<'a>,
) -> bool {
    !usefulness(rows, row, types, cx).is_empty()
}

/// `U(P, q)` with witnesses: the values matching `q` that no row of `rows`
/// matches. Empty means `q` is not useful.
fn usefulness<'a>(
    rows: &[Vec<Cell<'a>>],
    query: &[Cell<'a>],
    types: &[ColTy<'a>],
    cx: &'a Alphabets<'a>,
) -> Vec<Vec<Witness>> {
    let Some(column) = types.first() else {
        // No columns left: the query is useful exactly when nothing
        // reached this far — the base case that makes the recursion an
        // emptiness test.
        return if rows.is_empty() {
            vec![Vec::new()]
        } else {
            Vec::new()
        };
    };

    match query[0] {
        // A constructor query asks only about that constructor.
        Cell::Tag(pattern) => {
            let ColTy::Enum(entry) = column else {
                // The column's alphabet is unknown, so nothing can be
                // proven redundant here.
                return vec![vec![Witness::Unknown; types.len()]];
            };
            let Some(constructor) = entry.constructors.iter().find(|c| c.tag == pattern.tag) else {
                return vec![vec![Witness::Unknown; types.len()]];
            };
            let specialized = specialize(rows, constructor);
            let mut sub_query = expand(pattern, constructor);
            sub_query.extend_from_slice(&query[1..]);
            let sub_types = descend(&specialized, constructor, types, cx);
            let found = usefulness(&specialized, &sub_query, &sub_types, cx);
            rebuild(found, constructor)
        }
        Cell::Wild => {
            let used = used_tags(rows);
            let complete = match column {
                ColTy::Enum(entry) => entry.constructors.iter().all(|c| used.contains(&c.tag)),
                ColTy::Unconstrained | ColTy::Unknown => false,
            };
            if complete {
                let ColTy::Enum(entry) = column else {
                    unreachable!("only an enum column can be complete")
                };
                // Every constructor is written somewhere, so the query
                // splits into one branch per constructor.
                let mut out: Vec<Vec<Witness>> = Vec::new();
                for constructor in &entry.constructors {
                    let specialized = specialize(rows, constructor);
                    let mut sub_query = vec![Cell::Wild; arity(constructor)];
                    sub_query.extend_from_slice(&query[1..]);
                    let sub_types = descend(&specialized, constructor, types, cx);
                    let found = usefulness(&specialized, &sub_query, &sub_types, cx);
                    out.extend(rebuild(found, constructor));
                    if out.len() >= WITNESS_BUDGET {
                        break;
                    }
                }
                out
            } else {
                // Some constructor is missing (or the alphabet is
                // unknown): what the rest of the row needs is decided by
                // the rows that say nothing about this column.
                let defaulted = default(rows);
                let rest = usefulness(&defaulted, &query[1..], &types[1..], cx);
                if rest.is_empty() {
                    return Vec::new();
                }
                let heads = missing_heads(column, &used);
                let mut out = Vec::new();
                for head in heads {
                    for tail in &rest {
                        let mut witness = vec![head.clone()];
                        witness.extend(tail.iter().cloned());
                        out.push(witness);
                        if out.len() >= WITNESS_BUDGET {
                            return out;
                        }
                    }
                }
                out
            }
        }
    }
}

/// The witness heads for a column no row completes: each missing
/// constructor by name, or a bare wildcard when there is nothing to name
/// (an opaque column — its alphabet is unknown, so `_` is the honest
/// answer). An enum column no arm mentioned at all is missing *every*
/// constructor, and naming them beats printing `_` at the one place the
/// user can act on.
fn missing_heads(column: &ColTy<'_>, used: &[String]) -> Vec<Witness> {
    match column {
        ColTy::Enum(entry) => entry
            .constructors
            .iter()
            .filter(|c| !used.contains(&c.tag))
            .map(|c| Witness::Ctor {
                tag: c.tag.clone(),
                args: fields_of(c)
                    .iter()
                    .map(|f| (f.clone(), Witness::Wild))
                    .collect(),
            })
            .collect(),
        ColTy::Unconstrained => vec![Witness::Wild],
        ColTy::Unknown => vec![Witness::Unknown],
    }
}

/// `S(c, P)`: the rows that can match `c`, with the column replaced by one
/// column per declared field.
fn specialize<'a>(rows: &[Vec<Cell<'a>>], constructor: &MatchConstructor) -> Vec<Vec<Cell<'a>>> {
    let mut out = Vec::new();
    for row in rows {
        let mut next: Vec<Cell> = match row[0] {
            Cell::Wild => vec![Cell::Wild; arity(constructor)],
            Cell::Tag(pattern) if pattern.tag == constructor.tag => expand(pattern, constructor),
            Cell::Tag(_) => continue,
        };
        next.extend_from_slice(&row[1..]);
        out.push(next);
    }
    out
}

/// `D(P)`: the rows that say nothing about the first column, with it
/// dropped.
fn default<'a>(rows: &[Vec<Cell<'a>>]) -> Vec<Vec<Cell<'a>>> {
    rows.iter()
        .filter(|row| matches!(row[0], Cell::Wild))
        .map(|row| row[1..].to_vec())
        .collect()
}

/// One pattern's payload as columns: a cell per declared field, in
/// declaration order. A field the pattern does not mention — or binds
/// plainly, without a nested pattern — constrains nothing, so it is a
/// wildcard. This is where rl's bind-by-name-and-subset patterns become a
/// fixed-arity matrix.
fn expand<'a>(pattern: &'a TagPattern, constructor: &MatchConstructor) -> Vec<Cell<'a>> {
    let bindings = pattern.bindings.as_deref().unwrap_or_default();
    constructor
        .fields
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|field| {
            bindings
                .iter()
                .find(|b| b.name == field.name)
                .and_then(|b| b.nested.as_ref())
                .map_or(Cell::Wild, Cell::Tag)
        })
        .collect()
}

/// The column types a constructor's payload introduces, decided the same
/// way the top-level subject is: by what the patterns written there name.
///
/// The declared field type is consulted first — it is the precise answer
/// when it names an enum that has every tag used in the column — and the
/// patterns themselves decide otherwise. That fallback is what makes a
/// generic payload (`Ok(value: T)`) analyzable at all: rlc does not
/// substitute type arguments, but `Some`/`None` written in that column
/// name `Option` just as arm tags name a match's subject.
fn descend<'a>(
    specialized: &[Vec<Cell<'a>>],
    constructor: &MatchConstructor,
    types: &[ColTy<'a>],
    cx: &'a Alphabets<'a>,
) -> Vec<ColTy<'a>> {
    let width = arity(constructor);
    let declared = constructor.fields.as_deref().unwrap_or_default();
    let mut out: Vec<ColTy> = Vec::with_capacity(width + types.len() - 1);
    for index in 0..width {
        let tags: Vec<&str> = specialized
            .iter()
            .filter_map(|row| match row[index] {
                Cell::Tag(pattern) => Some(pattern.tag.as_str()),
                Cell::Wild => None,
            })
            .collect();
        if tags.is_empty() {
            out.push(ColTy::Unconstrained);
            continue;
        }
        // A checker's answer for this column wins: it is the type, not a
        // guess about which declaration the field's type text names.
        let asked = declared
            .get(index)
            .and_then(|field| cx.payload(&constructor.tag, &field.name));
        let by_type = declared
            .get(index)
            .and_then(|field| cx.table.entry_of_type(&field.ty))
            .filter(|e| {
                tags.iter()
                    .all(|t| e.constructors.iter().any(|c| c.tag == *t))
            });
        let resolved = asked
            .or(by_type)
            .or_else(|| cx.table.candidates(&tags).first().copied());
        out.push(resolved.map_or(ColTy::Unknown, ColTy::Enum));
    }
    out.extend_from_slice(&types[1..]);
    out
}

/// Folds a constructor's payload columns back into one witness cell.
fn rebuild(found: Vec<Vec<Witness>>, constructor: &MatchConstructor) -> Vec<Vec<Witness>> {
    let width = arity(constructor);
    let names = fields_of(constructor);
    found
        .into_iter()
        .map(|witness| {
            let mut rest = witness;
            let payload: Vec<Witness> = rest.drain(..width).collect();
            let mut out = vec![Witness::Ctor {
                tag: constructor.tag.clone(),
                args: names.iter().cloned().zip(payload).collect(),
            }];
            out.extend(rest);
            out
        })
        .collect()
}

/// The tags any row writes in the first column.
fn used_tags(rows: &[Vec<Cell<'_>>]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for row in rows {
        if let Cell::Tag(pattern) = row[0]
            && !out.contains(&pattern.tag)
        {
            out.push(pattern.tag.clone());
        }
    }
    out
}

fn arity(constructor: &MatchConstructor) -> usize {
    constructor.fields.as_deref().unwrap_or_default().len()
}

fn fields_of(constructor: &MatchConstructor) -> Vec<String> {
    constructor
        .fields
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|f| f.name.clone())
        .collect()
}
