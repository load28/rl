use std::collections::HashMap;
use std::path::Path;

use rlc::EmitMapping;

/// Where a virtual `.ts` module came from, kept so TypeScript diagnostics can
/// be reported against the original `.rl` file instead of generated code.
pub(crate) struct TypeOrigin<'a> {
    pub(crate) file: &'a Path,
    pub(crate) source: &'a str,
    pub(crate) code: &'a str,
    pub(crate) mappings: &'a [EmitMapping],
}

/// One TypeScript diagnostic as the host reports it. Positions are 1-based
/// line and 1-based UTF-16 column over the file the checker saw.
pub(crate) struct TypeDiagnostic {
    pub(crate) file: Option<String>,
    pub(crate) line: usize,
    pub(crate) col: usize,
    pub(crate) message: String,
}

impl TypeDiagnostic {
    /// The `file:line:col: message` line rlc prints. A diagnostic over a
    /// virtual module is moved onto the `.rl` file it was compiled from; a
    /// hand-written `.ts` file already has real coordinates and is left alone.
    pub(crate) fn render(&self, origins: &HashMap<&str, TypeOrigin<'_>>) -> String {
        let Some(file) = &self.file else {
            return self.message.clone();
        };
        let key = file.replace('\\', "/");
        let Some(origin) = origins.get(key.as_str()) else {
            return format!("{file}:{}:{}: {}", self.line, self.col, self.message);
        };
        let out = utf16_offset(origin.code, self.line, self.col);
        let src = source_offset(origin.mappings, out);
        let (line, col) = rlc::line_col(origin.source, src);
        format!("{}:{line}:{col}: {}", slash_path(origin.file), self.message)
    }
}

/// Byte offset in `text` of a 1-based `(line, col)` whose column counts
/// UTF-16 code units. Positions past the end of a line, or of the text, clamp
/// to it.
pub(crate) fn utf16_offset(text: &str, line: usize, col: usize) -> usize {
    let mut at = 0;
    for _ in 1..line.max(1) {
        match text[at..].find('\n') {
            Some(newline) => at += newline + 1,
            None => return text.len(),
        }
    }
    let rest = &text[at..];
    let line_text = &rest[..rest.find('\n').unwrap_or(rest.len())];
    let mut units = 0;
    for (offset, ch) in line_text.char_indices() {
        if units >= col.saturating_sub(1) {
            return at + offset;
        }
        units += ch.len_utf16();
    }
    at + line_text.len()
}

/// The source byte offset an emitted byte offset came from, given the
/// mappings of the verbatim-copied chunks ordered by output offset.
pub(crate) fn source_offset(mappings: &[EmitMapping], out: usize) -> usize {
    let after = mappings.partition_point(|m| m.out <= out);
    if let Some(m) = after.checked_sub(1).map(|i| &mappings[i])
        && out < m.out + m.len
    {
        return m.src + (out - m.out);
    }
    mappings
        .get(after)
        .map(|m| m.src)
        .or_else(|| mappings.last().map(|m| m.src + m.len))
        .unwrap_or(0)
}

fn slash_path(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}
