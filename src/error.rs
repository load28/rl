use std::fmt;

/// A compile error with a position in the original `.rl` source.
///
/// `Display` renders the CLI's diagnostic format, `file:line:col: message`
/// (position omitted when there is none):
///
/// ```
/// let err = rlc::CompileError {
///     message: "match: duplicate arm \"Circle\"".to_string(),
///     filename: Some("shapes.rl".to_string()),
///     line: 3,
///     col: 7,
///     end_line: 3,
///     end_col: 13,
/// };
/// assert_eq!(err.to_string(), "shapes.rl:3:7: match: duplicate arm \"Circle\"");
/// ```
#[derive(Debug, Clone)]
pub struct CompileError {
    /// Human-readable description of the error.
    pub message: String,
    /// The filename passed via [`crate::Options::filename`], if any.
    pub filename: Option<String>,
    /// 1-based line, 1-based column. (0, 0) means "no position".
    pub line: usize,
    /// See [`CompileError::line`].
    pub col: usize,
    /// End of the range the error covers, past its last character —
    /// 1-based line and column, `(0, 0)` when the error is a position
    /// only. `Display` never prints it: it is what a consumer that draws a
    /// range (an editor) underlines, so the squiggle covers `try
    /// parse(text)` rather than one character of it.
    pub end_line: usize,
    /// See [`CompileError::end_line`].
    pub end_col: usize,
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = self.filename.as_deref().unwrap_or("<input>");
        if self.line > 0 {
            write!(f, "{}:{}:{}: {}", name, self.line, self.col, self.message)
        } else {
            write!(f, "{}: {}", name, self.message)
        }
    }
}

impl std::error::Error for CompileError {}

/// Internal error type carrying a byte offset into the source; converted to
/// line/column at the `compile()` boundary.
#[derive(Debug, Clone)]
pub(crate) struct RlError {
    pub message: String,
    /// Byte offset into the original source, or None for positionless errors.
    pub offset: Option<usize>,
    /// Byte offset just past the last byte the error covers, when the
    /// reporting site knows the construct's extent. `None` leaves the
    /// width to the consumer (the editor underlines the word at the
    /// position); `Some` is what makes the squiggle cover the construct.
    pub end: Option<usize>,
    /// Which rule fired — the diagnostic's stable identity
    /// ([`crate::DiagnosticCode`]). Reporting sites set it with
    /// [`RlError::code`]; a site that forgets still compiles, as
    /// [`DiagnosticCode::Other`], which is why sema's tests pin the codes.
    pub code: DiagnosticCode,
    /// Complete syntax node that owns consequences of this cause.
    pub owner: Option<DiagnosticOwner>,
}

use crate::diagnostics::{DiagnosticCode, DiagnosticOwner};

impl RlError {
    /// An error at one byte, its width left to the consumer.
    pub fn at(offset: usize, message: impl Into<String>) -> Self {
        RlError {
            message: message.into(),
            offset: Some(offset),
            end: None,
            code: DiagnosticCode::Other,
            owner: None,
        }
    }

    /// An error over a byte range — the construct it is about, as the user
    /// wrote it. Reported at `start`, underlined to `end`.
    pub fn span(start: usize, end: usize, message: impl Into<String>) -> Self {
        RlError {
            message: message.into(),
            offset: Some(start),
            end: Some(end.max(start)),
            code: DiagnosticCode::Other,
            owner: None,
        }
    }

    /// The same error, carrying its rule's code.
    #[must_use]
    pub fn code(mut self, code: DiagnosticCode) -> Self {
        self.code = code;
        self
    }

    /// Associates this cause with the complete syntax node it invalidates.
    #[must_use]
    pub fn owner(mut self, start: usize, end: usize) -> Self {
        self.owner = Some(DiagnosticOwner { start, end });
        self
    }
}

/// Convert a byte offset to (1-based line, 1-based column in UTF-8 code points).
pub(crate) fn line_col(src: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(src.len());
    let before = &src[..offset];
    let line = before.bytes().filter(|&b| b == b'\n').count() + 1;
    let line_start = before.rfind('\n').map(|p| p + 1).unwrap_or(0);
    let col = before[line_start..].chars().count() + 1;
    (line, col)
}
