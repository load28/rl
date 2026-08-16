use std::fmt;

/// A compile error with a position in the original `.rl` source.
#[derive(Debug, Clone)]
pub struct CompileError {
    pub message: String,
    pub filename: Option<String>,
    /// 1-based line, 1-based column. (0, 0) means "no position".
    pub line: usize,
    pub col: usize,
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
}

impl RlError {
    pub fn at(offset: usize, message: impl Into<String>) -> Self {
        RlError { message: message.into(), offset: Some(offset) }
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
