//! Structured diagnostics — the compiler's report vocabulary.
//!
//! Every rl-level problem is reported as a [`Diagnostic`]: a stable
//! [`DiagnosticCode`], a [`Severity`], a message, and a byte span in the
//! source. One file can carry many of them — the semantic passes accumulate
//! and keep checking the next independent node instead of stopping at the
//! first violation (`docs/design/compiler-core.md` §8, TASK-117).
//!
//! The code is the diagnostic's identity across every consumer: the CLI,
//! `--server`, the engine and the editor all see the same code for the same
//! rule, and the typed and untyped pipelines share one message renderer
//! ([`non_exhaustive_message`]) so their wording cannot drift apart.

use crate::error::{RlError, line_col};

/// How serious a [`Diagnostic`] is.
///
/// Every current rl rule is an error; the variant space leaves room for
/// warnings (e.g. unreachable arms, today an editor hint) without another
/// migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Severity {
    /// The program violates an rl rule; compilation does not produce output.
    Error,
    /// Suspicious but not fatal; compilation proceeds.
    Warning,
}

/// The stable identity of an rl rule — what makes the same violation the
/// same diagnostic in every pipeline and every consumer.
///
/// Codes are per *rule*, not per message: the wording may name the enum or
/// the binding, the code says which rule fired. [`DiagnosticCode::as_str`]
/// is the wire form (`--server`, editors).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DiagnosticCode {
    /// A `|>` the parser could not claim — the surrounding text is not
    /// emittable TypeScript.
    StrayPipe,
    /// An `if let` the parser could not claim.
    StrayIfLet,
    /// A `result` block the parser could not claim.
    StrayResult,
    /// A `result` binding missing its declaration keyword.
    ResultMissingKeyword,
    /// A `result` binding below the block's top level — it cannot
    /// early-return the block.
    ResultNestedBinding,
    /// A `flow` composition whose first step is a method step.
    FlowFirstStepMethod,
    /// A `try` outside the top-level statement stream.
    TryPlacement,
    /// A let-else outside the top-level statement stream.
    LetElsePlacement,
    /// A let-else whose `else` block does not end in a diverging statement.
    LetElseNotDiverging,
    /// An `if let` in expression position.
    IfLetPlacement,
    /// An enum declaring the same case tag twice.
    EnumDuplicateCase,
    /// An enum field whose type annotation does not parse as TypeScript.
    EnumInvalidFieldType,
    /// A pattern binding the same name twice.
    PatternDuplicateBinding,
    /// A match mixing tag patterns and literal patterns.
    MatchMixedPatterns,
    /// A wildcard `_` arm that is not the last arm.
    MatchWildcardNotLast,
    /// Or-pattern alternatives of different literal kinds.
    MatchOrLiteralKindMismatch,
    /// An arm repeating a tag or literal an earlier arm already covers.
    MatchDuplicateArm,
    /// A nested pattern inside an or-pattern.
    MatchNestedInOrPattern,
    /// Or-pattern alternatives that do not bind the same names.
    MatchOrBindingMismatch,
    /// A tuple pattern whose element count differs from the scrutinees'.
    MatchTupleArity,
    /// A case tag that resolves to no declaration (with a suggestion).
    UnknownCase,
    /// A payload field that resolves to no declaration (with a suggestion).
    UnknownField,
    /// A match that does not cover every case of its subject.
    MatchNotExhaustive,
    /// A mutation through a `val` binding.
    ValMutation,
    /// A `val` binding passed to a parameter not declared `val`.
    ValPass,
    /// The emitted output failed the TypeScript self-check.
    VerifyFailed,
    /// A diagnostic no specific rule claims. Reporting sites should not
    /// produce this — it exists so an unclassified error still has a code.
    Other,
}

impl DiagnosticCode {
    /// The code's stable wire form, e.g. `"match-not-exhaustive"`.
    pub fn as_str(self) -> &'static str {
        match self {
            DiagnosticCode::StrayPipe => "stray-pipe",
            DiagnosticCode::StrayIfLet => "stray-if-let",
            DiagnosticCode::StrayResult => "stray-result",
            DiagnosticCode::ResultMissingKeyword => "result-missing-keyword",
            DiagnosticCode::ResultNestedBinding => "result-nested-binding",
            DiagnosticCode::FlowFirstStepMethod => "flow-first-step-method",
            DiagnosticCode::TryPlacement => "try-placement",
            DiagnosticCode::LetElsePlacement => "let-else-placement",
            DiagnosticCode::LetElseNotDiverging => "let-else-not-diverging",
            DiagnosticCode::IfLetPlacement => "if-let-placement",
            DiagnosticCode::EnumDuplicateCase => "enum-duplicate-case",
            DiagnosticCode::EnumInvalidFieldType => "enum-invalid-field-type",
            DiagnosticCode::PatternDuplicateBinding => "pattern-duplicate-binding",
            DiagnosticCode::MatchMixedPatterns => "match-mixed-patterns",
            DiagnosticCode::MatchWildcardNotLast => "match-wildcard-not-last",
            DiagnosticCode::MatchOrLiteralKindMismatch => "match-or-literal-kind-mismatch",
            DiagnosticCode::MatchDuplicateArm => "match-duplicate-arm",
            DiagnosticCode::MatchNestedInOrPattern => "match-nested-in-or-pattern",
            DiagnosticCode::MatchOrBindingMismatch => "match-or-binding-mismatch",
            DiagnosticCode::MatchTupleArity => "match-tuple-arity",
            DiagnosticCode::UnknownCase => "unknown-case",
            DiagnosticCode::UnknownField => "unknown-field",
            DiagnosticCode::MatchNotExhaustive => "match-not-exhaustive",
            DiagnosticCode::ValMutation => "val-mutation",
            DiagnosticCode::ValPass => "val-pass",
            DiagnosticCode::VerifyFailed => "verify-failed",
            DiagnosticCode::Other => "other",
        }
    }

    /// Whether a diagnostic with this code leaves the file impossible to
    /// project into the TypeScript program.
    ///
    /// Most rl errors are *recoverable*: the emitter still produces plain
    /// TypeScript for the file (codegen is infallible), so the typed pass
    /// can run and its diagnostics are reported **alongside** the rl ones —
    /// a duplicate arm no longer hides the file's other exhaustiveness
    /// holes, type errors and `val` violations (TASK-117 symptom 3).
    ///
    /// The exceptions are the diagnostics whose presence means the output
    /// cannot be valid TypeScript at all: text the parser could not claim
    /// (a stray `|>` passes through verbatim and is not TS), a field type
    /// that would be emitted verbatim into a type position, and the output
    /// self-check itself.
    pub fn blocks_projection(self) -> bool {
        matches!(
            self,
            DiagnosticCode::StrayPipe
                | DiagnosticCode::StrayIfLet
                | DiagnosticCode::StrayResult
                | DiagnosticCode::ResultMissingKeyword
                | DiagnosticCode::ResultNestedBinding
                | DiagnosticCode::EnumInvalidFieldType
                | DiagnosticCode::VerifyFailed
        )
    }
}

/// One reported problem, with a byte span in the source it was found in.
///
/// This is the multi-diagnostic counterpart of [`crate::CompileError`]:
/// positions are byte offsets (convert with [`crate::line_col`], or
/// [`Diagnostic::to_compile_error`] for the CLI's line/column form), and a
/// file produces a `Vec` of them in source order rather than the first one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Which rule fired.
    pub code: DiagnosticCode,
    /// How serious it is.
    pub severity: Severity,
    /// The full message, as it is shown.
    pub message: String,
    /// Byte offset of the diagnostic's position in the source, or `None`
    /// for positionless diagnostics (a failed output self-check).
    pub start: Option<usize>,
    /// Byte offset just past the range the diagnostic covers — the
    /// construct as written. `None` leaves the width to the consumer.
    pub end: Option<usize>,
}

impl Diagnostic {
    /// The diagnostic in [`crate::CompileError`]'s form: 1-based line and
    /// column in `source`, carrying `filename` — what the CLI prints.
    pub fn to_compile_error(&self, source: &str, filename: Option<&str>) -> crate::CompileError {
        let (line, col) = match self.start {
            Some(offset) => line_col(source, offset),
            None => (0, 0),
        };
        let (end_line, end_col) = match self.end {
            Some(offset) => line_col(source, offset),
            None => (0, 0),
        };
        crate::CompileError {
            message: self.message.clone(),
            filename: filename.map(String::from),
            line,
            col,
            end_line,
            end_col,
        }
    }

    pub(crate) fn from_rl(error: RlError) -> Diagnostic {
        Diagnostic {
            code: error.code,
            severity: Severity::Error,
            message: error.message,
            start: error.offset,
            end: error.end,
        }
    }
}

/// The one wording of "this match has holes", shared by the untyped pass
/// (sema, over the declaration table) and the typed pass (the engine, over
/// the checker's alphabet) — one renderer, so the two pipelines cannot
/// drift apart on phrasing (TASK-117 symptom 2).
///
/// `subject` names what the match is over when the reporter knows it
/// (`enum Shape`, `(Token, _)`, `literal union`); the typed pass, which
/// knows the alphabet but not the declaration, passes `None`. `missing`
/// entries arrive fully rendered (quoted tags, `(a, b)` combinations);
/// `tuple` picks the truncation unit.
pub(crate) fn non_exhaustive_message(
    subject: Option<&str>,
    missing: &[String],
    tuple: bool,
) -> String {
    let shown = if missing.len() > 4 {
        let unit = if tuple {
            "combinations in total"
        } else {
            "in total"
        };
        format!("{}, … ({} {unit})", missing[..3].join(", "), missing.len())
    } else {
        missing.join(", ")
    };
    let on = match subject {
        Some(subject) => format!(" on {subject}"),
        None => String::new(),
    };
    format!(
        "match{on} is not exhaustive: missing {shown} \
         (add the missing arms or a final `_` arm)"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_typed_and_untyped_wordings_are_one_renderer() {
        let missing = vec!["\"Square\"".to_string(), "\"Tri\"".to_string()];
        assert_eq!(
            non_exhaustive_message(Some("enum Shape"), &missing, false),
            "match on enum Shape is not exhaustive: missing \"Square\", \"Tri\" \
             (add the missing arms or a final `_` arm)",
        );
        assert_eq!(
            non_exhaustive_message(None, &missing, false),
            "match is not exhaustive: missing \"Square\", \"Tri\" \
             (add the missing arms or a final `_` arm)",
        );
    }

    #[test]
    fn long_lists_truncate_the_same_way_on_both_paths() {
        let missing: Vec<String> = (0..6).map(|i| format!("\"C{i}\"")).collect();
        let said = non_exhaustive_message(None, &missing, false);
        assert!(
            said.contains("\"C0\", \"C1\", \"C2\", … (6 in total)"),
            "{said}"
        );
        let combos: Vec<String> = (0..6).map(|i| format!("(A, B{i})")).collect();
        let said = non_exhaustive_message(None, &combos, true);
        assert!(said.contains("… (6 combinations in total)"), "{said}");
    }

    #[test]
    fn a_diagnostic_converts_to_the_cli_error_form() {
        let d = Diagnostic {
            code: DiagnosticCode::MatchDuplicateArm,
            severity: Severity::Error,
            message: "match: duplicate arm \"A\"".to_string(),
            start: Some(5),
            end: Some(6),
        };
        let e = d.to_compile_error("abc\ndef\n", Some("x.rl"));
        assert_eq!((e.line, e.col), (2, 2));
        assert_eq!(e.to_string(), "x.rl:2:2: match: duplicate arm \"A\"");
    }
}
