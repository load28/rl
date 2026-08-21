//! Semantic results in rl's own vocabulary.
//!
//! The checker's answers come back as [`crate::typescript::backend::Answers`]
//! — coordinates in emitted modules, symbol ids, raw diagnostics. This module
//! turns them into what a consumer of the engine actually wants: diagnostics
//! at positions in the `.rl` source, with the exact wording the CLI has
//! always printed, and declarations matched back to the files they were
//! emitted for. Nothing TypeScript-shaped leaves the engine.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use super::projection::{self, Probes, ProjectedDocument};
use super::snapshot::Snapshot;
use crate::AnchorKind;
use crate::analysis::{DeclaredEnum, PayloadAlphabet};
use crate::typescript::backend::{Answers, Diagnostic as TsDiagnostic, Resolution};

/// One reported problem, at a position in a file the user can open.
///
/// The message carries its full wording — including the `ts(CODE):` prefix
/// of a type diagnostic — so a consumer prints or displays it verbatim and
/// two consumers can never drift apart on phrasing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// The file the problem is in — an `.rl` source, or a hand-written
    /// TypeScript file when the checker reported one of those.
    pub path: PathBuf,
    /// 1-based line and column (columns count UTF-8 code points), or `None`
    /// when the diagnostic has no position in a file the user wrote.
    pub position: Option<(usize, usize)>,
    /// Where the diagnostic's range ends — 1-based line and column, past
    /// the last character it covers. `None` when only a position is known;
    /// a consumer that draws a range then decides its own width (the
    /// editor underlines the word at the position).
    ///
    /// This is what makes a diagnostic point at the *construct*: `try
    /// parse(text)`, `match (shape)`, the mutated binding — the same span
    /// Rust underlines, rather than the first character of the statement.
    pub end: Option<(usize, usize)>,
    /// The full message, as it is shown.
    pub message: String,
    /// The diagnostic's stable identity: an rl rule's code
    /// ([`crate::DiagnosticCode::as_str`], e.g. `match-not-exhaustive`) or
    /// a TypeScript code (`ts2322`). `None` only where no rule is known.
    /// The same rule carries the same code on every path — CLI, server,
    /// editor, typed or untyped.
    pub code: Option<String>,
}

/// What one checked snapshot came back with.
#[derive(Debug, Default)]
pub struct Checked {
    /// Every diagnostic of the pass, in report order: the type layer first
    /// (unless the request was rl-only), then literal exhaustiveness, tag
    /// exhaustiveness, `val` mutations, and `val` passes.
    pub diagnostics: Vec<Diagnostic>,
    /// The declarations the compiler emitted, when they were requested.
    pub declarations: Declarations,
}

/// The declarations of one emitting pass, matched back to their sources.
#[derive(Debug, Default)]
pub struct Declarations {
    /// The standard library's own declarations, when `@rl/std` is in the
    /// graph — the `rl.d.ts` a consumer's `paths` points at.
    pub std: Option<String>,
    /// One entry per requested `.rl` file the compiler emitted for.
    pub modules: Vec<ModuleDeclaration>,
}

/// One lowered module's declarations, paired with the file they belong to.
#[derive(Debug)]
pub struct ModuleDeclaration {
    /// The projected file the declarations were emitted for.
    pub file: Arc<ProjectedDocument>,
    /// The declaration text — the compiler's own; only the sidecar map is
    /// rlc's to build.
    pub text: String,
}

/// One match's alphabets as the checker named them: where the `match`
/// keyword is, and each scrutinee position's constituents in position
/// order (a single match has one).
type MatchAlphabets = (usize, Vec<Vec<String>>);

/// Turns a TypeScript diagnostic that landed on rlc's own glue into an rl
/// one — said in rl's words, about rl's construct.
///
/// This is the third layer of `docs/design/rust-parity-analysis.md` §10 and
/// the point of [`crate::EmitAnchor`]. The error-layer contract asks that a
/// user never meet a TypeScript error caused by code rlc wrote; until now
/// that state was not reached — the diagnostic simply arrived wearing the
/// generated code's face. Translating it reaches it.
///
/// The pair `(construct, error code)` is the whole key. It is deliberately
/// a **whitelist**: a diagnostic this table does not recognize is passed
/// through unchanged rather than guessed at, because silently restating an
/// error as something it is not is worse than an ugly message. The original
/// text rides along in every translation for the same reason.
///
/// `declarations` is the file's declaration table ([`crate::pattern_analyses`]).
/// It is what lets the translation say `Test.OutOfRange` where TypeScript
/// wrote `{ kind: "OutOfRange"; value: number; }` — see [`name_types`].
pub(crate) fn translate(
    kind: AnchorKind,
    code: u32,
    message: &str,
    declarations: &[DeclaredEnum],
) -> Option<String> {
    let said = match (kind, code) {
        // `.kind` / `.value` reached for on something that is not a Result.
        (AnchorKind::Try, 2339 | 2551 | 2571) => {
            "`try` needs a `Result` — this expression is not one".to_string()
        }
        (AnchorKind::ResultBind, 2339 | 2551 | 2571) => {
            "`<-` needs a `Result` — this expression is not one".to_string()
        }
        // The propagated `Err` reaching a return type that cannot hold it.
        (AnchorKind::Try, 2322 | 2345) => "the `Err` this `try` propagates does not fit the \
             enclosing function's return type — rl has no automatic conversion, so widen the \
             return type or convert the error"
            .to_string(),
        (AnchorKind::LetElse, 2339 | 2571) => {
            "let-else needs a value with a `kind` discriminant — this expression has none"
                .to_string()
        }
        (AnchorKind::IfLet, 2339 | 2571) => {
            "`if let` needs a value with a `kind` discriminant — this expression has none"
                .to_string()
        }
        (AnchorKind::Match, 2339 | 2571) => {
            "match on a tag pattern needs a value with a `kind` discriminant — this scrutinee \
             has none (a plain TypeScript `enum` is not one)"
                .to_string()
        }
        // A tag compared against a type that cannot hold it. rlc reports
        // the ones that look like misspellings itself; this is the rest.
        (AnchorKind::Match, 2678) | (AnchorKind::LetElse | AnchorKind::IfLet, 2367) => {
            "this pattern's case is not one the value can be".to_string()
        }
        _ => return None,
    };
    let message = message.trim();
    match name_types(message, declarations) {
        Some(named) => Some(format!(
            "{said} (in rl's names: {named}) (ts{code}: {message})"
        )),
        None => Some(format!("{said} (ts{code}: {message})")),
    }
}

/// The message again, with every structural case type the declaration table
/// **uniquely** recognizes written as the rl name it lowers from — `None`
/// when it recognizes none of them and the restatement would be the message
/// itself.
///
/// TypeScript has no word for an rl case: `Test.OutOfRange` lowers to a
/// member of a union type, so a diagnostic about one prints the member —
/// `{ kind: "OutOfRange"; value: number; }` — and the reader is left to
/// match it back against a declaration by eye. rl can do that matching: the
/// tag and the payload's field names name a case, and the table says which
/// enum declares it.
///
/// Two rules keep the restatement honest, and both are the whitelist rule
/// of [`translate`] again:
///
/// - A tag two enums in scope declare is **not** named. The point is to say
///   which declaration this is, and a guess between two says nothing.
/// - A member whose field names are not exactly the case's is not named
///   either — it is some other type that happens to carry the same tag.
///
/// A union of members that covers every case of one enum is the enum
/// itself (`ParseError`), which is how the return type in the motivating
/// example comes back to its declared name; a partial union stays a union
/// of named cases (`ParseError.NotANumber | ParseError.Overflow`).
///
/// The restatement is a reading aid and never replaces the original — the
/// caller carries TypeScript's own text along with it, because a name rl
/// got wrong has to be checkable against what the checker actually said.
fn name_types(message: &str, declarations: &[DeclaredEnum]) -> Option<String> {
    let members = case_members(message, declarations);
    if members.is_empty() {
        return None;
    }
    let mut out = String::new();
    let mut at = 0;
    for run in union_runs(message, &members) {
        let (first, last) = (&members[run.start], &members[run.end - 1]);
        out.push_str(&message[at..first.start]);
        match collapsed(&members[run.start..run.end], declarations) {
            Some(name) => out.push_str(&name),
            None => {
                let named: Vec<String> = members[run.start..run.end]
                    .iter()
                    .map(|m| format!("{}.{}", m.enum_name, m.tag))
                    .collect();
                out.push_str(&named.join(" | "));
            }
        }
        at = last.end;
    }
    out.push_str(&message[at..]);
    Some(out)
}

/// One structural case type found in a message, resolved to its
/// declaration.
struct CaseMember {
    /// Byte span of the `{ ... }` in the message.
    start: usize,
    end: usize,
    /// The enum that declares the case, under the name the analyzed file
    /// calls it by.
    enum_name: String,
    /// The case's tag.
    tag: String,
}

/// A `[start, end)` range of members that a message writes as one union.
struct Run {
    start: usize,
    end: usize,
}

/// Every structural case type in `message`, in order, that the table names.
///
/// The scan is quote-aware — a `{` inside a string literal opens nothing —
/// and it descends into an object type it does not recognize, so a case
/// nested in some other type is still found.
fn case_members(message: &str, declarations: &[DeclaredEnum]) -> Vec<CaseMember> {
    let bytes = message.as_bytes();
    let mut out = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'"' => at = string_end(bytes, at),
            b'{' => match object_end(bytes, at) {
                Some(end) => match recognize(&message[at..end], declarations) {
                    Some((enum_name, tag)) => {
                        out.push(CaseMember {
                            start: at,
                            end,
                            enum_name,
                            tag,
                        });
                        at = end;
                    }
                    // Not a case of its own — but one may be written
                    // inside it.
                    None => at += 1,
                },
                None => at += 1,
            },
            _ => at += 1,
        }
    }
    out
}

/// The members grouped into the unions the message writes them as: members
/// separated by exactly `" | "` belong to one union.
fn union_runs(message: &str, members: &[CaseMember]) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    for (i, member) in members.iter().enumerate() {
        match runs.last_mut() {
            Some(run) if &message[members[i - 1].end..member.start] == " | " => run.end = i + 1,
            _ => runs.push(Run {
                start: i,
                end: i + 1,
            }),
        }
    }
    runs
}

/// The enum name a whole union collapses to — `Some` only when its members
/// are the cases of one enum, each once, all of them.
fn collapsed(run: &[CaseMember], declarations: &[DeclaredEnum]) -> Option<String> {
    let first = run.first()?;
    if run.iter().any(|m| m.enum_name != first.enum_name) {
        return None;
    }
    let declared = declarations.iter().find(|d| d.name == first.enum_name)?;
    if declared.constructors.len() != run.len() {
        return None;
    }
    declared
        .constructors
        .iter()
        .all(|c| run.iter().any(|m| m.tag == c.tag))
        .then(|| first.enum_name.clone())
}

/// The case an object type names — `Some((enum, tag))` when exactly one
/// declaration in scope declares the tag *and* its payload is exactly the
/// fields written here.
fn recognize(text: &str, declarations: &[DeclaredEnum]) -> Option<(String, String)> {
    let (tag, mut fields) = object_fields(text)?;
    fields.sort_unstable();
    let mut candidates = declarations
        .iter()
        .filter(|d| d.constructors.iter().any(|c| c.tag == tag));
    let declared = candidates.next()?;
    if candidates.next().is_some() {
        return None; // two declarations answer to the tag — neither is *the* one
    }
    let constructor = declared.constructors.iter().find(|c| c.tag == tag)?;
    let mut declared_fields: Vec<&str> = constructor
        .fields
        .iter()
        .flatten()
        .map(|f| f.name.as_str())
        .collect();
    declared_fields.sort_unstable();
    (declared_fields == fields).then(|| (declared.name.clone(), tag))
}

/// An object type's `kind` tag and the names of its other members — `None`
/// when it has no string-literal `kind`, or is written in a shape this
/// reader does not fully understand (a method, an index signature).
fn object_fields(text: &str) -> Option<(String, Vec<&str>)> {
    let inner = text.strip_prefix('{')?.strip_suffix('}')?;
    let mut tag = None;
    let mut fields = Vec::new();
    for entry in split_top(inner, b';') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let colon = split_top(entry, b':').next()?.len();
        if colon >= entry.len() {
            return None; // no `name: type` — not a property
        }
        let name = entry[..colon].trim().trim_end_matches('?');
        let value = entry[colon + 1..].trim();
        if !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'$')
        {
            return None; // an index signature, a quoted name, a call
        }
        if name == "kind" {
            let literal = value.strip_prefix('"')?.strip_suffix('"')?;
            if literal.contains(['"', '|']) {
                return None; // a union of tags, not one case
            }
            tag = Some(literal.to_string());
        } else {
            fields.push(name);
        }
    }
    Some((tag?, fields))
}

/// `text` split on `sep` where nothing encloses it — a `;` inside a nested
/// object, a bracket or a string separates nothing.
fn split_top(text: &str, sep: u8) -> impl Iterator<Item = &str> {
    let bytes = text.as_bytes();
    let mut cuts = Vec::new();
    let (mut at, mut depth, mut start) = (0, 0usize, 0);
    while at < bytes.len() {
        match bytes[at] {
            b'"' => {
                at = string_end(bytes, at);
                continue;
            }
            b'{' | b'(' | b'[' => depth += 1,
            b'}' | b')' | b']' => depth = depth.saturating_sub(1),
            b if b == sep && depth == 0 => {
                cuts.push(&text[start..at]);
                start = at + 1;
            }
            _ => {}
        }
        at += 1;
    }
    cuts.push(&text[start..]);
    cuts.into_iter()
}

/// The byte just past the `}` that closes the object type opening at
/// `at` — `None` when nothing closes it.
fn object_end(bytes: &[u8], at: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = at;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                i = string_end(bytes, i);
                continue;
            }
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// The byte just past the string literal opening at `at`.
fn string_end(bytes: &[u8], at: usize) -> usize {
    let quote = bytes[at];
    let mut i = at + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b if b == quote => return i + 1,
            _ => i += 1,
        }
    }
    bytes.len()
}

/// Builds the pass's diagnostics from the checker's answers, in the exact
/// order and wording the report has always had.
pub(crate) fn report(
    snapshot: &Snapshot,
    answers: &Answers,
    probes: &Probes,
    rl_only: bool,
) -> Vec<Diagnostic> {
    let files = snapshot.files();
    let mut out = Vec::new();
    // A file's declaration table, built the first time a translation on
    // that file asks for it: it costs a parse of the file and of what it
    // imports, and most passes translate nothing at all.
    let mut declarations: HashMap<PathBuf, Vec<DeclaredEnum>> = HashMap::new();

    // The rl layer first: the diagnostics each file's projection found on
    // its own (duplicate arms, unknown cases, misplaced constructs). They
    // are rl's answers about rl's constructs, so they are reported on the
    // rl-only path too — and they no longer gate the rest of this report
    // (TASK-117 symptom 3): the typed answers below follow either way.
    for file in files {
        for d in &file.rl_diagnostics {
            out.push(Diagnostic {
                path: file.source_path.clone(),
                position: d.start.map(|at| crate::line_col(&file.source, at)),
                end: d.end.map(|at| crate::line_col(&file.source, at)),
                message: d.message.clone(),
                code: Some(d.code.as_str().to_string()),
            });
        }
    }

    // TypeScript's own diagnostics, at the position in the `.rl` file the
    // offending code was written at.
    let type_diagnostics: &[TsDiagnostic] = if rl_only { &[] } else { &answers.diagnostics };
    for diagnostic in type_diagnostics {
        let Some(file) = files.iter().find(|f| f.module_path == diagnostic.file) else {
            // A hand-written file: TypeScript's own coordinates already name
            // a file the user can open, so they are used as they are.
            out.push(Diagnostic {
                path: diagnostic.file.clone(),
                position: None,
                end: None,
                message: format!("ts({}): {}", diagnostic.code, diagnostic.message),
                code: Some(format!("ts{}", diagnostic.code)),
            });
            continue;
        };
        // Glue is not the user's code. When rlc can say what the construct
        // meant, it says that — over the construct's own text. The
        // declaration table its wording names types from is built only for
        // a file that has a diagnostic on glue at all, and once for it.
        if projection::glue_anchor(file, diagnostic.start).is_some() {
            let declared = declarations
                .entry(file.source_path.clone())
                .or_insert_with(|| {
                    crate::pattern_analyses(&file.source, &externs_of(files, file)).declarations
                });
            if let Some((anchor, said)) = projection::translate_on_glue(file, diagnostic, declared)
            {
                let entry = Diagnostic {
                    path: file.source_path.clone(),
                    position: Some(crate::line_col(&file.source, anchor.src)),
                    end: Some(crate::line_col(&file.source, anchor.src_end)),
                    message: said,
                    code: Some(format!("ts{}", diagnostic.code)),
                };
                // One construct's glue can draw several TypeScript errors
                // that all mean the same rl thing (`$rl_t.kind` and
                // `$rl_t.value`).
                if !out.contains(&entry) {
                    out.push(entry);
                }
                continue;
            }
        }
        match projection::diagnostic_source_offset(file, diagnostic.start) {
            Some((nearest, exact)) => {
                // Exact: the user's own text, so the diagnostic covers
                // exactly what TypeScript underlined. On glue: the
                // construct that wrote it, which is the honest extent —
                // and better than the nearest byte before the glue.
                let anchor = projection::glue_anchor(file, diagnostic.start);
                let (offset, end) = match (exact, anchor) {
                    (true, _) => (
                        nearest,
                        projection::diagnostic_source_offset(file, diagnostic.end)
                            .filter(|(at, exact)| *exact && *at >= nearest)
                            .map(|(at, _)| at),
                    ),
                    (false, Some(anchor)) => (anchor.src, Some(anchor.src_end)),
                    (false, None) => (nearest, None),
                };
                out.push(Diagnostic {
                    path: file.source_path.clone(),
                    position: Some(crate::line_col(&file.source, offset)),
                    end: end.map(|at| crate::line_col(&file.source, at)),
                    message: format!(
                        "ts({}): {}{}",
                        diagnostic.code,
                        diagnostic.message,
                        // By the error-layer contract rlc's output must not
                        // draw type errors, so say where it came from rather
                        // than pinning it on the line the position landed
                        // near.
                        if exact {
                            ""
                        } else {
                            " (in code rlc generated for this construct)"
                        },
                    ),
                    code: Some(format!("ts{}", diagnostic.code)),
                });
            }
            None => out.push(Diagnostic {
                path: file.source_path.clone(),
                position: None,
                end: None,
                message: format!("ts({}): {}", diagnostic.code, diagnostic.message),
                code: Some(format!("ts{}", diagnostic.code)),
            }),
        }
    }

    // Literal-match exhaustiveness, decided by the type TypeScript computes
    // at the scrutinee — narrowing included.
    for missing in &answers.literal_missing {
        let Some(anchor) = probes.literals.get(missing.index) else {
            continue;
        };
        let Some(file) = files.iter().find(|f| f.source_path == anchor.source_path) else {
            continue;
        };
        out.push(Diagnostic {
            path: file.source_path.clone(),
            position: Some(crate::line_col(&file.source, anchor.offset)),
            end: Some(crate::line_col(&file.source, anchor.end)),
            message: crate::diagnostics::non_exhaustive_message(
                Some("literal union"),
                &missing
                    .missing
                    .iter()
                    .map(display_literal)
                    .collect::<Vec<_>>(),
                false,
            ),
            code: Some(
                crate::DiagnosticCode::MatchNotExhaustive
                    .as_str()
                    .to_string(),
            ),
        });
    }

    // Tag exhaustiveness. The checker names the constituents the
    // scrutinee's type still has — narrowing included — and rl runs its
    // own algorithm over that alphabet, which is what sees a hole *inside*
    // a payload as well as a missing case (TASK-108).
    //
    // A witness rl is not certain of is dropped here: the default path
    // reports those because it has nothing better, but on this path the
    // honest answer for an unidentifiable column is to ask the checker,
    // and that question is not asked yet.
    // Per file, per match: the alphabet of each scrutinee position, in
    // position order (a single match has one).
    let mut by_file: HashMap<PathBuf, Vec<MatchAlphabets>> = HashMap::new();
    // The payload answers ride in the same list, after the match ones —
    // they name the alphabet of a `(constructor, field)` column, which is
    // the one thing rl cannot work out from declarations alone.
    let mut payloads: HashMap<PathBuf, Vec<PayloadAlphabet>> = HashMap::new();
    // `(file, match keyword) -> end of `match (scrutinee)``.
    let mut match_ends: HashMap<(PathBuf, usize), usize> = HashMap::new();
    for members in &answers.tag_members {
        if let Some(anchor) = probes.tags.get(members.index) {
            let per_match = by_file.entry(anchor.source_path.clone()).or_default();
            match per_match.iter_mut().find(|(at, _)| *at == anchor.offset) {
                Some((_, positions)) => positions.push(members.tags.clone()),
                None => per_match.push((anchor.offset, vec![members.tags.clone()])),
            }
            // The keyword offset keys the alphabets; the range it opens is
            // what the diagnostic underlines.
            match_ends.insert((anchor.source_path.clone(), anchor.offset), anchor.end);
            continue;
        }
        let Some(anchor) = probes
            .payloads
            .get(members.index.wrapping_sub(probes.tags.len()))
        else {
            continue;
        };
        payloads
            .entry(anchor.source_path.clone())
            .or_default()
            .push((
                (anchor.tag.clone(), anchor.field.clone()),
                members.tags.clone(),
            ));
    }
    for file in files {
        let Some(asked) = by_file.get(&file.source_path) else {
            continue;
        };
        // The nested columns are resolved from declarations, so the
        // imported ones have to be collected — otherwise a payload whose
        // type is an imported enum reads as an unknown alphabet and its
        // holes go unreported.
        let externs = externs_of(files, file);
        let asked_payloads = payloads
            .get(&file.source_path)
            .map_or(&[][..], Vec::as_slice);
        for (offset, coverage) in
            crate::analysis::checked_coverage(&file.source, &externs, asked, asked_payloads)
        {
            // A single match's witness is one pattern, quoted the way the
            // default path quotes one; a tuple match's is a combination of
            // positions, written as one `(a, b)` and left unquoted — the
            // quotes would read as part of the pattern.
            let uncovered: Vec<String> = coverage
                .missing
                .iter()
                .filter(|m| m.certain)
                .map(|m| {
                    if m.pattern.len() > 1 {
                        format!("({})", m.pattern.join(", "))
                    } else {
                        format!("{:?}", m.pattern.first().cloned().unwrap_or_default())
                    }
                })
                .collect();
            if uncovered.is_empty() {
                continue;
            }
            // The typed pass knows the alphabet but not the declaration,
            // so the shared renderer gets no subject — one renderer, one
            // wording, on both pipelines (TASK-120).
            let tuple = coverage.positions.len() > 1;
            out.push(Diagnostic {
                path: file.source_path.clone(),
                position: Some(crate::line_col(&file.source, offset)),
                end: match_ends
                    .get(&(file.source_path.clone(), offset))
                    .map(|at| crate::line_col(&file.source, *at)),
                message: crate::diagnostics::non_exhaustive_message(None, &uncovered, tuple),
                code: Some(
                    crate::DiagnosticCode::MatchNotExhaustive
                        .as_str()
                        .to_string(),
                ),
            });
        }
    }

    // `val`: two resolutions decide, and rlc guesses neither of them.
    //
    // 1. Which binding a path is rooted at — the root identifier and the
    //    binding's declaration are the same binding when they are the same
    //    symbol. Shadowing, redeclaration and destructuring come out right
    //    because this is TypeScript's own resolution, not a model of it.
    // 2. For a method call, whether the method is a built-in — declared in
    //    TypeScript's own lib files. A user-defined method that shares the
    //    name is not, and anything unresolved is left alone.
    let symbols: HashMap<usize, &Resolution> =
        answers.resolutions.iter().map(|r| (r.index, r)).collect();
    let val_symbols: HashSet<i64> = probes
        .val_bindings
        .iter()
        .filter_map(|i| symbols.get(i).map(|r| r.id))
        .collect();

    for mutation in &probes.mutations {
        let Some(root) = symbols.get(&mutation.root) else {
            continue; // unresolved — never a verdict
        };
        if !val_symbols.contains(&root.id) {
            continue; // not this binding, whatever it is called
        }
        if let Some(method) = mutation.method {
            match symbols.get(&method) {
                // Two halves make the verdict: the checker's — the
                // method is one of TypeScript's own — and rl's policy —
                // that method is one of the mutating ones. A built-in
                // `get` fails the second; a user-defined `set`, or a
                // method the checker could not resolve, fails the first.
                Some(resolution)
                    if resolution.builtin && crate::is_builtin_mutator_name(&resolution.name) => {}
                _ => continue,
            }
        }
        let Some(file) = files
            .iter()
            .find(|f| f.source_path == mutation.anchor.source_path)
        else {
            continue;
        };
        let message = match &mutation.method_name {
            // The built-in itself is not named: the compiler answered
            // "this method is one of TypeScript's own", which is the
            // verdict — not which interface declares it.
            Some(method) => format!(
                "cannot call mutating method `{}` through val binding `{}` \
                 (the binding is declared with `val`, so every access path from it is \
                 read-only)",
                method, mutation.name,
            ),
            None => format!(
                "cannot mutate through val binding `{}` \
                 (the binding is declared with `val`, so every access path \
                 from it is read-only)",
                mutation.name,
            ),
        };
        out.push(Diagnostic {
            path: file.source_path.clone(),
            position: Some(crate::line_col(&file.source, mutation.anchor.offset)),
            end: Some(crate::line_col(&file.source, mutation.anchor.end)),
            message,
            code: Some(crate::DiagnosticCode::ValMutation.as_str().to_string()),
        });
    }

    // The callee table: a declaration's symbol names its parameter
    // list. One symbol carrying declarations with *different* lists
    // (TypeScript overloads, `var` merging) makes that callee
    // ambiguous, and an ambiguous callee is not judged — the same
    // caution the name-keyed table of the untyped path takes, here at
    // symbol granularity, so two functions merely sharing a name stay
    // two callees.
    let mut callees: HashMap<i64, Option<&[crate::ValParam]>> = HashMap::new();
    for function in &probes.functions {
        let Some(resolution) = symbols.get(&function.root) else {
            continue;
        };
        match callees.get(&resolution.id) {
            Some(Some(prev)) if *prev == function.params.as_slice() => {}
            Some(_) => {
                callees.insert(resolution.id, None);
            }
            None => {
                callees.insert(resolution.id, Some(&function.params));
            }
        }
    }

    // The function boundary: a `val` binding may only be handed to a
    // parameter that is itself `val`. Which binding the argument names,
    // and which declaration the call names, are the same symbol
    // question the mutations above ask — an unresolved callee, or one
    // no collected declaration matches (an import, a method), is never
    // a verdict.
    for pass in &probes.passes {
        let Some(root) = symbols.get(&pass.root) else {
            continue;
        };
        if !val_symbols.contains(&root.id) {
            continue;
        }
        let Some(callee) = symbols.get(&pass.callee_symbol) else {
            continue;
        };
        let Some(Some(params)) = callees.get(&callee.id) else {
            continue;
        };
        let Some(param) = params.get(pass.arg_index) else {
            continue;
        };
        if param.is_val {
            continue;
        }
        let described = match &param.name {
            Some(name) => format!("`{name}`"),
            None => format!("#{}", pass.arg_index + 1),
        };
        let Some(file) = files
            .iter()
            .find(|f| f.source_path == pass.anchor.source_path)
        else {
            continue;
        };
        out.push(Diagnostic {
            path: file.source_path.clone(),
            position: Some(crate::line_col(&file.source, pass.anchor.offset)),
            end: Some(crate::line_col(&file.source, pass.anchor.end)),
            message: format!(
                "cannot pass val binding `{}` to mutable parameter {} of \
                 `{}` (the parameter is not declared with `val`, so the function may mutate \
                 through it)",
                pass.name, described, pass.callee,
            ),
            code: Some(crate::DiagnosticCode::ValPass.as_str().to_string()),
        });
    }

    out
}

/// Matches the compiler's emitted declarations back to the snapshot's files.
/// Only `requested` files are kept — the rest of the project is in the graph
/// so that it resolves, not so that it is emitted.
pub(crate) fn match_declarations(
    snapshot: &Snapshot,
    answers: &Answers,
    root: &std::path::Path,
    requested: &HashSet<PathBuf>,
) -> Declarations {
    let mut out = Declarations::default();
    // The standard library's own declarations, so a consumer running plain
    // tsc can point `@rl/std` at them (`paths`). It is a module of the
    // project like any other, so the compiler emitted it too — it just has
    // no `.rl` source to sit beside.
    let std_declaration = root.join(projection::STD_MODULE).with_extension("d.ts");
    for declaration in &answers.declarations {
        if declaration.path == std_declaration {
            out.std = Some(declaration.text.clone());
            continue;
        }
        let Some(file) = snapshot
            .files()
            .iter()
            .find(|f| projection::declaration_path_of(f) == declaration.path)
            .filter(|f| requested.contains(&f.source_path))
        else {
            continue;
        };
        out.modules.push(ModuleDeclaration {
            file: file.clone(),
            text: declaration.text.clone(),
        });
    }
    out
}

/// The enum declarations one file's direct `.rl` imports bring into scope,
/// preferring the snapshot's own text for a file it holds — the same
/// 1-hop collection every other surface does, so an imported enum is known
/// under the name the import gave it.
fn externs_of(
    files: &[Arc<ProjectedDocument>],
    file: &ProjectedDocument,
) -> Vec<crate::EnumSymbol> {
    super::language::externs_of(&file.source_path, &file.source, &|target| {
        files
            .iter()
            .find(|f| f.source_path == target)
            .map(|f| f.source.clone())
            .or_else(|| std::fs::read_to_string(target).ok())
    })
}

/// A covered literal as it reads in a message.
fn display_literal(literal: &crate::Literal) -> String {
    match literal {
        crate::Literal::String(s) => format!("{s:?}"),
        crate::Literal::Number(n) => n.to_string(),
        crate::Literal::BigInt(d) => format!("{d}n"),
        crate::Literal::Boolean(b) => b.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The declaration table of a source, as a translation sees it.
    fn table(source: &str) -> Vec<DeclaredEnum> {
        crate::pattern_analyses(source, &[]).declarations
    }

    /// The motivating example of TASK-118: the propagated `Err` and the
    /// return type it does not fit, both said as the user declared them.
    #[test]
    fn a_case_is_named_and_a_full_union_is_its_enum() {
        let declarations = table(
            "enum Test { OutOfRange(value: number), Empty }\n\
             enum ParseError { NotANumber(text: string) }\n",
        );
        let said = translate(
            AnchorKind::Try,
            2322,
            "Type 'Err<{ kind: \"OutOfRange\"; value: number; }>' is not assignable to type \
             'Result<string, { kind: \"NotANumber\"; text: string; }>'.",
            &declarations,
        )
        .expect("translated");
        assert!(
            said.contains(
                "(in rl's names: Type 'Err<Test.OutOfRange>' is not assignable to type \
                 'Result<string, ParseError>'.)"
            ),
            "{said}"
        );
        // The original rides along, unchanged — the names are checkable.
        assert!(
            said.contains("(ts2322: Type 'Err<{ kind: \"OutOfRange\""),
            "{said}"
        );
    }

    #[test]
    fn a_partial_union_stays_a_union_of_named_cases() {
        let declarations = table("enum E { A(x: number), B, C }\n");
        let named = name_types(
            "Type '{ kind: \"A\"; x: number; } | { kind: \"B\"; }' is not assignable to type 'E'.",
            &declarations,
        )
        .expect("named");
        assert_eq!(
            named,
            "Type 'E.A | E.B' is not assignable to type 'E'.".to_string()
        );
    }

    #[test]
    fn a_tag_two_declarations_answer_to_is_not_named() {
        let declarations = table("enum A { Empty }\nenum B { Empty }\n");
        assert_eq!(
            name_types("Type '{ kind: \"Empty\"; }'.", &declarations),
            None
        );
    }

    #[test]
    fn a_type_that_only_shares_a_tag_is_not_named() {
        // Same tag, different payload — some other type, not this case.
        let declarations = table("enum E { A(x: number) }\n");
        assert_eq!(
            name_types("Type '{ kind: \"A\"; y: string; }'.", &declarations),
            None
        );
    }

    #[test]
    fn a_message_with_no_structural_case_is_left_alone() {
        let declarations = table("enum E { A(x: number), B }\n");
        assert_eq!(
            name_types(
                "Property 'kind' does not exist on type 'number'.",
                &declarations
            ),
            None,
        );
        // ... and a translation of it carries no naming clause.
        let said = translate(
            AnchorKind::Try,
            2339,
            "Property 'kind' does not exist.",
            &declarations,
        )
        .expect("translated");
        assert!(!said.contains("in rl's names"), "{said}");
    }

    #[test]
    fn a_case_nested_in_another_type_is_named_where_it_stands() {
        let declarations = table("enum E { A(x: number), B }\n");
        let named = name_types(
            "Type 'Array<{ kind: \"A\"; x: number; }>' is not assignable.",
            &declarations,
        )
        .expect("named");
        assert_eq!(named, "Type 'Array<E.A>' is not assignable.".to_string());
    }

    #[test]
    fn a_tag_union_names_nothing() {
        // `{ kind: "A" | "B" }` is not one case, so it is not one name.
        let declarations = table("enum E { A(x: number), B }\n");
        assert_eq!(
            name_types("Type '{ kind: \"A\" | \"B\"; }'.", &declarations),
            None
        );
    }
}
