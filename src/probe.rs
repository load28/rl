//! Typed exhaustiveness probes for literal matches.
//!
//! rlc resolves tag exhaustiveness itself, from the enum declarations it can
//! see. A literal match has no such registry: whether `"north" | "south"`
//! covers the scrutinee is a fact about a *TypeScript type*, and the design
//! contract (`docs/design/match-literal-patterns.md`) is that rlc must not
//! grow a partial TypeScript type system to answer it.
//!
//! So the default compile path emits a runtime guard and says nothing, and
//! the `--types` path — which already runs the real checker over the emitted
//! modules — answers the question there. This module produces the input for
//! that: every wildcard-free literal match of a file, with the literals its
//! unguarded arms cover and the two byte offsets the pipeline needs (the
//! `match` keyword to report at, the scrutinee to ask the checker about).

use crate::ast::*;

/// A wildcard-free literal `match` — one typed exhaustiveness question.
/// Produced by [`crate::literal_matches`]; consumed by `rlc --types`.
#[derive(Debug, Clone, PartialEq)]
pub struct LiteralMatch {
    /// Byte offset of the `match` keyword — where a missing-literal
    /// diagnostic is reported (see [`crate::line_col`]).
    pub offset: usize,
    /// Byte offset of the scrutinee's first non-whitespace byte. The
    /// scrutinee is copied verbatim into the output, so this maps through
    /// [`crate::EmitMapping`]s to the range the checker is asked about.
    pub scrutinee: usize,
    /// Byte offset just past the scrutinee's last non-whitespace byte.
    pub scrutinee_end: usize,
    /// The literals the match's unguarded arms cover. A guarded arm may
    /// fall through, so — exactly like a guarded tag arm — it covers
    /// nothing.
    pub covered: Vec<Literal>,
}

/// One literal a [`LiteralMatch`] covers, normalized to the value
/// JavaScript compares with `===`.
#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    /// A string literal, escapes decoded.
    String(String),
    /// A number literal, as the `f64` JavaScript compares (`-0` is `0`).
    Number(f64),
    /// Decimal digits. A BigInt is never a member of a finite literal union
    /// TypeScript would report, so a match covering one is left unchecked.
    BigInt(String),
    /// `true` / `false`.
    Boolean(bool),
}

/// A wildcard-free tag `match` — one typed exhaustiveness question about an
/// enum. Produced by [`crate::tag_matches`].
///
/// rlc can answer this one from its own declaration table, and does when no
/// checker is available. A checker answers it *better*: the scrutinee's type
/// at the `match` is the narrowed one, so a case an earlier guard already
/// removed is not demanded back, and an enum declared in another module
/// needs no declaration collecting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagMatch {
    /// Byte offset of the `match` keyword — where a missing-case diagnostic
    /// is reported (see [`crate::line_col`]).
    pub offset: usize,
    /// Byte offset of the scrutinee's first non-whitespace byte.
    pub scrutinee: usize,
    /// Byte offset just past the scrutinee's last non-whitespace byte.
    pub scrutinee_end: usize,
    /// The case tags the match's unguarded arms cover. A guarded arm may
    /// fall through, so it covers nothing.
    pub covered: Vec<String>,
}

/// Collects every wildcard-free tag `match` of a source file, nested ones
/// included, in source order.
///
/// ```
/// let probes = rlc::tag_matches("const v = match (s) { Circle(r) => r, Point => 0 };\n");
/// assert_eq!(probes.len(), 1);
/// assert_eq!(probes[0].covered, ["Circle", "Point"]);
/// ```
///
/// A match with a `_` arm is already exhaustive and is not collected:
///
/// ```
/// assert!(rlc::tag_matches("const v = match (s) { Circle(r) => r, _ => 0 };\n").is_empty());
/// ```
pub fn tag_matches(source: &str) -> Vec<TagMatch> {
    let program = crate::parser::parse(source);
    let mut out = Probes::default();
    walk(&program, source, &mut out);
    out.tags
}

/// Collects every wildcard-free literal `match` of a source file, nested
/// ones included, in source order.
///
/// ```
/// let probes = rlc::literal_matches("const s = match (d) { \"n\" => 1, \"s\" => 2 };\n");
/// assert_eq!(probes.len(), 1);
/// assert_eq!(probes[0].covered, [
///     rlc::Literal::String("n".to_string()),
///     rlc::Literal::String("s".to_string()),
/// ]);
/// ```
///
/// A match with a `_` arm is already exhaustive and is not collected:
///
/// ```
/// assert!(rlc::literal_matches("const s = match (d) { \"n\" => 1, _ => 2 };\n").is_empty());
/// ```
pub fn literal_matches(source: &str) -> Vec<LiteralMatch> {
    let program = crate::parser::parse(source);
    let mut out = Probes::default();
    walk(&program, source, &mut out);
    out.literals
}

/// Both kinds of typed exhaustiveness question, gathered in one walk.
#[derive(Default)]
struct Probes {
    literals: Vec<LiteralMatch>,
    tags: Vec<TagMatch>,
}

/// What one match's arms turned out to be.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// No arm carried a pattern worth a question.
    None,
    Literal,
    Tag,
}

fn walk(program: &Program, src: &str, out: &mut Probes) {
    for segment in &program.segments {
        match segment {
            Segment::Verbatim(_)
            | Segment::RlImport(_)
            | Segment::Enum(_)
            | Segment::ValModifier(_) => {}
            Segment::Match(expr) => {
                collect(expr, src, out);
                walk(&expr.scrutinee, src, out);
                for arm in &expr.arms {
                    if let Some(guard) = &arm.guard {
                        walk(&guard.expr, src, out);
                    }
                    walk(&arm.body, src, out);
                }
            }
            Segment::TupleMatch(expr) => {
                for (_, scrutinee) in &expr.scrutinees {
                    walk(scrutinee, src, out);
                }
                for arm in &expr.arms {
                    if let Some(guard) = &arm.guard {
                        walk(&guard.expr, src, out);
                    }
                    walk(&arm.body, src, out);
                }
            }
            Segment::Try(stmt) => walk(&stmt.expr, src, out),
            Segment::LetElse(stmt) => {
                walk(&stmt.expr, src, out);
                walk(&stmt.else_body, src, out);
            }
            Segment::IfLet(stmt) => walk_if_let(stmt, src, out),
            Segment::Pipe(pipe) => {
                if let Some(head) = &pipe.head {
                    walk(head, src, out);
                }
                for step in &pipe.steps {
                    walk(&step.body, src, out);
                }
            }
            Segment::ResultBlock(block) => {
                for item in &block.items {
                    match item {
                        ResultItem::Stmts(stmts) => walk(stmts, src, out),
                        ResultItem::Bind(bind) => walk(&bind.expr, src, out),
                    }
                }
                walk(&block.value, src, out);
            }
            Segment::Template(template) => {
                for chunk in &template.chunks {
                    if let TemplateChunk::Interp(interp) = chunk {
                        walk(interp, src, out);
                    }
                }
            }
        }
    }
}

fn walk_if_let(stmt: &IfLetStmt, src: &str, out: &mut Probes) {
    walk(&stmt.expr, src, out);
    walk(&stmt.body, src, out);
    match &stmt.else_part {
        Some(IfLetElse::Block(block)) => walk(block, src, out),
        Some(IfLetElse::IfLet(inner)) => walk_if_let(inner, src, out),
        None => {}
    }
}

/// Records one match if it is a wildcard-free literal match.
fn collect(expr: &MatchExpr, src: &str, out: &mut Probes) {
    if expr
        .arms
        .iter()
        .any(|a| matches!(a.pattern, Pattern::Wildcard))
    {
        return;
    }
    let Some((scrutinee, scrutinee_end)) = trimmed_scrutinee(expr, src) else {
        return;
    };

    // Literal and tag patterns never mix in one match, so at most one of
    // these fires.
    let mut literals = Vec::new();
    let mut tags = Vec::new();
    let mut kind = Kind::None;
    for arm in &expr.arms {
        // A guarded arm may fall through, so it covers nothing — but it
        // still tells us which kind of match this is.
        match &arm.pattern {
            Pattern::Literals(alts) => {
                if arm.guard.is_none() {
                    literals.extend(alts.iter().map(|alt| match &alt.value {
                        LiteralValue::Str(s) => Literal::String(s.clone()),
                        LiteralValue::Num(n) => Literal::Number(*n),
                        LiteralValue::BigInt(d) => Literal::BigInt(d.clone()),
                        LiteralValue::Bool(b) => Literal::Boolean(*b),
                    }));
                }
                kind = Kind::Literal;
            }
            Pattern::Tags(alts) => {
                if arm.guard.is_none() {
                    tags.extend(alts.iter().map(|alt| alt.tag.clone()));
                }
                kind = Kind::Tag;
            }
            Pattern::Wildcard => {}
        }
    }
    match kind {
        Kind::Literal => out.literals.push(LiteralMatch {
            offset: expr.keyword_off,
            scrutinee,
            scrutinee_end,
            covered: literals,
        }),
        Kind::Tag => out.tags.push(TagMatch {
            offset: expr.keyword_off,
            scrutinee,
            scrutinee_end,
            covered: tags,
        }),
        Kind::None => {}
    }
}

/// The scrutinee span with surrounding whitespace trimmed, or `None` when it
/// is empty.
fn trimmed_scrutinee(expr: &MatchExpr, src: &str) -> Option<(usize, usize)> {
    let bytes = src.as_bytes();
    let mut start = expr.scrutinee_span.start;
    while start < expr.scrutinee_span.end && bytes[start].is_ascii_whitespace() {
        start += 1;
    }
    let mut end = expr.scrutinee_span.end;
    while end > start && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    (start < end).then_some((start, end))
}
