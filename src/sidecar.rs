//! Editor sidecars: `x.rl.d.ts` + `x.rl.d.ts.map`.
//!
//! A `.ts` file that imports `"./x.rl"` gets `TS2307` from tsserver, which
//! does not know the extension. TypeScript's own escape hatch is in that
//! error text — "or its corresponding type declarations" — so placing a
//! declaration file next to the module resolves it.
//!
//! The declaration *body* needs type inference, which is tsc's job (run it
//! with `--emitDeclarationOnly` over rlc's output). What tsc cannot know is
//! where each declaration lives in the original `.rl`; this module supplies
//! that as a declaration map whose `sources` points at the `.rl` file, which
//! is what sends "go to definition" to the original instead of the `.d.ts`.

use crate::enum_symbols;

/// The two files that make up a module's editor sidecar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sidecar {
    /// Contents of `<name>.d.ts` — the declarations plus a
    /// `sourceMappingURL` comment.
    pub declarations: String,
    /// Contents of `<name>.d.ts.map` — a source map v3 document whose
    /// `sources` is the original `.rl` file.
    pub map: String,
}

/// Builds the sidecar for one module.
///
/// `source` is the original `.rl` text, `declarations` is what tsc emitted
/// for rlc's output of that module, and `rl_path` is the path to the `.rl`
/// file **relative to where the sidecar will be written** — `"notice.rl"`
/// when the two sit together, `"../src/notice.rl"` when declarations live
/// in their own tree (which TypeScript merges back with `rootDirs`). It
/// becomes the map's `sources`, and its file name becomes the stem of the
/// written files (`notice.rl.d.ts`).
///
/// Every exported declaration that can be located in the source gets two
/// mapping segments: one at column 0 and one at the column where its name
/// starts. The second is the one that matters — "go to definition" asks
/// about the name's position, and without a segment there the editor stops
/// at the `.d.ts`.
pub fn build_sidecar(source: &str, declarations: &str, rl_path: &str) -> Sidecar {
    let rl_file_name = rl_path
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(rl_path);
    let source_lines: Vec<&str> = source.lines().collect();
    let enums = enum_symbols(source);

    let decl_lines: Vec<&str> = declarations
        .lines()
        .filter(|line| !line.trim_start().starts_with("//# sourceMappingURL="))
        .collect();

    let hits: Vec<Option<Hit>> = decl_lines
        .iter()
        .map(|line| {
            let name = declared_name(line)?;
            let position = locate(source, &source_lines, &enums, name)?;
            Some(Hit {
                generated_column: utf16_column(line, line.find(name)?),
                line: position.0,
                column: position.1,
            })
        })
        .collect();

    let map_name = format!("{rl_file_name}.d.ts.map");
    let body = decl_lines.join("\n");
    // The banner costs one generated line, so the mappings are shifted by
    // one (the leading `;` below).
    let declarations = format!(
        "// @generated from {rl_file_name} by rlc --sidecar — do not edit.\n{}\n//# sourceMappingURL={}\n",
        body.trim_end(),
        map_name.as_str()
    );
    // A leading `;` skips the banner line the declarations open with.
    let mappings = format!(";{}", encode_mappings(&hits));
    let map = format!(
        "{{\"version\":3,\"file\":{},\"sourceRoot\":\"\",\"sources\":[{}],\"names\":[],\"mappings\":\"{}\"}}\n",
        json_string(&format!("{rl_file_name}.d.ts")),
        json_string(rl_path),
        mappings,
    );

    Sidecar { declarations, map }
}

struct Hit {
    generated_column: usize,
    line: usize,
    column: usize,
}

/// The name declared by a `.d.ts` line, if it declares one.
fn declared_name(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix("export ")?.trim_start();
    let rest = rest.strip_prefix("declare ").unwrap_or(rest).trim_start();
    let rest = rest.strip_prefix("abstract ").unwrap_or(rest).trim_start();
    for keyword in [
        "function ",
        "const ",
        "let ",
        "var ",
        "class ",
        "interface ",
        "type ",
        "enum ",
    ] {
        if let Some(after) = rest.strip_prefix(keyword) {
            let name = identifier_prefix(after.trim_start());
            return (!name.is_empty()).then_some(name);
        }
    }
    None
}

/// The leading ASCII identifier of `text`.
fn identifier_prefix(text: &str) -> &str {
    let end = text
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '$'))
        .unwrap_or(text.len());
    &text[..end]
}

/// Where `name` is declared in the source, as a zero-based (line, column).
///
/// rl enums come from the parsed declarations, so their positions are exact.
/// Everything else lives in a passthrough region and is found by scanning
/// for its declaration keyword; the first match wins.
fn locate(
    source: &str,
    source_lines: &[&str],
    enums: &[crate::EnumSymbol],
    name: &str,
) -> Option<(usize, usize)> {
    if let Some(symbol) = enums.iter().find(|e| e.name == name) {
        let (line, column) = crate::line_col(source, symbol.offset);
        let index = line.checked_sub(1)?;
        // `offset` points at the declaration keyword; move to the name.
        let text = source_lines.get(index)?;
        let column = text
            .find(name)
            .map_or(column.saturating_sub(1), |byte| utf16_column(text, byte));
        return Some((index, column));
    }

    source_lines.iter().enumerate().find_map(|(index, text)| {
        if !declares(text, name) {
            return None;
        }
        let byte = text.find(name)?;
        Some((index, utf16_column(text, byte)))
    })
}

/// Whether a source line declares `name` at the top of a statement.
fn declares(line: &str, name: &str) -> bool {
    let rest = line.trim_start();
    let rest = rest.strip_prefix("export ").unwrap_or(rest).trim_start();
    let rest = rest.strip_prefix("declare ").unwrap_or(rest).trim_start();
    let rest = rest.strip_prefix("abstract ").unwrap_or(rest).trim_start();
    for keyword in [
        "function ",
        "const ",
        "let ",
        "var ",
        "class ",
        "interface ",
        "type ",
        "enum ",
    ] {
        if let Some(after) = rest.strip_prefix(keyword) {
            return identifier_prefix(after.trim_start()) == name;
        }
    }
    false
}

/// Column of `byte` in `text`, counted in UTF-16 code units (what source
/// maps and editors use).
fn utf16_column(text: &str, byte: usize) -> usize {
    text.get(..byte)
        .map_or(0, |prefix| prefix.encode_utf16().count())
}

/// Encodes one segment per located declaration into a source map v3
/// `mappings` string.
fn encode_mappings(hits: &[Option<Hit>]) -> String {
    let mut previous_line: i64 = 0;
    let mut previous_column: i64 = 0;
    let mut lines: Vec<String> = Vec::with_capacity(hits.len());

    for hit in hits {
        let Some(hit) = hit else {
            lines.push(String::new());
            continue;
        };
        let mut generated: Vec<usize> = vec![0];
        if hit.generated_column > 0 {
            generated.push(hit.generated_column);
        }

        let mut previous_generated: i64 = 0;
        let mut segments: Vec<String> = Vec::with_capacity(generated.len());
        for column in generated {
            let mut segment = String::new();
            vlq(column as i64 - previous_generated, &mut segment);
            vlq(0, &mut segment);
            vlq(hit.line as i64 - previous_line, &mut segment);
            vlq(hit.column as i64 - previous_column, &mut segment);
            previous_generated = column as i64;
            previous_line = hit.line as i64;
            previous_column = hit.column as i64;
            segments.push(segment);
        }
        lines.push(segments.join(","));
    }

    lines.join(";")
}

/// Base64 VLQ, the source map encoding for a signed delta.
fn vlq(value: i64, out: &mut String) {
    const DIGITS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut bits = if value < 0 {
        ((-value) << 1) | 1
    } else {
        value << 1
    };
    loop {
        let mut digit = (bits & 31) as usize;
        bits >>= 5;
        if bits > 0 {
            digit |= 32;
        }
        out.push(DIGITS[digit] as char);
        if bits == 0 {
            break;
        }
    }
}

/// Minimal JSON string literal (the map only ever carries file names).
fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
