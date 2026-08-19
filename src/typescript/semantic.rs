use std::path::{Path, PathBuf};

use rlc::{EmitMapping, Literal, ValMethodCall};

/// One wildcard-free literal match handed to the host for typed
/// exhaustiveness.
pub(crate) struct LiteralCheck {
    /// Virtual module path the checker sees.
    pub(crate) module: String,
    /// UTF-16 offsets of the scrutinee expression in that module.
    pub(crate) start: usize,
    pub(crate) end: usize,
    /// The literals the match's unguarded arms cover.
    pub(crate) covered: Vec<Literal>,
    /// The `.rl` file, and the byte offset of its `match` keyword.
    pub(crate) file: PathBuf,
    pub(crate) offset: usize,
    /// Index into the loaded sources, for the source text to position in.
    pub(crate) source_index: usize,
}

/// One method call made through a `val` binding, handed to the host so the
/// checker can say what the receiver actually is.
pub(crate) struct ValCheck {
    /// Virtual module path the checker sees.
    pub(crate) module: String,
    /// UTF-16 offsets of the method name in that module.
    pub(crate) start: usize,
    pub(crate) end: usize,
    /// The method name and the `val` binding the path is rooted at.
    pub(crate) method: String,
    pub(crate) binding: String,
    /// The `.rl` file, and the byte offset of the path's root identifier.
    pub(crate) file: PathBuf,
    pub(crate) offset: usize,
    /// Index into the loaded sources, for the source text to position in.
    pub(crate) source_index: usize,
}

/// The probes of one compiled module, dropped when the scrutinee did not
/// survive into the output as verbatim text or when a covered literal is a
/// BigInt, which no TypeScript literal union holds.
pub(crate) fn literal_checks(
    source: &str,
    module: &str,
    code: &str,
    mappings: &[EmitMapping],
    file: &Path,
    source_index: usize,
) -> Vec<LiteralCheck> {
    rlc::literal_matches(source)
        .into_iter()
        .filter(|probe| {
            !probe
                .covered
                .iter()
                .any(|l| matches!(l, Literal::BigInt(_)))
        })
        .filter_map(|probe| {
            let start = output_offset(mappings, probe.scrutinee)?;
            let end = output_offset(mappings, probe.scrutinee_end.checked_sub(1)?)? + 1;
            Some(LiteralCheck {
                module: module.to_string(),
                start: utf16_position(code, start),
                end: utf16_position(code, end),
                covered: probe.covered,
                file: file.to_path_buf(),
                offset: probe.offset,
                source_index,
            })
        })
        .collect()
}

/// The `val` probes of one compiled module, dropped when the method name did
/// not survive into the output as verbatim text.
pub(crate) fn val_checks(
    source: &str,
    module: &str,
    code: &str,
    mappings: &[EmitMapping],
    file: &Path,
    source_index: usize,
) -> Vec<ValCheck> {
    rlc::val_method_calls(source)
        .into_iter()
        .filter_map(|call: ValMethodCall| {
            let start = output_offset(mappings, call.name)?;
            let end = output_offset(mappings, call.name_end.checked_sub(1)?)? + 1;
            Some(ValCheck {
                module: module.to_string(),
                start: utf16_position(code, start),
                end: utf16_position(code, end),
                method: call.method,
                binding: call.binding,
                file: file.to_path_buf(),
                offset: call.offset,
                source_index,
            })
        })
        .collect()
}

/// The emitted byte offset a source byte was copied to, or None when the byte
/// belongs to compiler-written glue rather than a verbatim chunk.
fn output_offset(mappings: &[EmitMapping], src: usize) -> Option<usize> {
    mappings
        .iter()
        .find(|m| src >= m.src && src < m.src + m.len)
        .map(|m| m.out + (src - m.src))
}

/// Offset of `byte` in `text` counted in UTF-16 code units.
fn utf16_position(text: &str, byte: usize) -> usize {
    text.get(..byte)
        .map_or(0, |prefix| prefix.encode_utf16().count())
}
