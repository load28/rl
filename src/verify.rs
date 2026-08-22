//! swc-based validation.
//!
//! rl syntax is not valid TypeScript, so swc cannot parse a `.rl` file as a
//! whole — construct detection stays in the hand-rolled scanner. swc is used
//! where real TypeScript exists:
//!
//! 1. `check_type_fragment` — variant field types are pure TS type syntax;
//!    parsing them at compile time rejects bad annotations with an exact
//!    position in the `.rl` file.
//! 2. `verify_output` — the fully generated TypeScript module is parsed as a
//!    self-check that the compiler emitted valid code (and that passthrough
//!    code was valid TS to begin with). Disabled with `--no-verify`.
//!
//! This SWC check is intentionally in the syntax pipeline. The compiler
//! already uses a whole-program SWC AST to model TypeScript owners and
//! evaluation contexts (`crate::program_syntax`), so parsing the final module
//! here checks the target against the same in-process syntax boundary. It does
//! not ask or approximate any type-semantic question.
//!
//! The TypeScript 7 backend remains the authority for inferred types,
//! narrowing, resolution, diagnostics, and declaration emit. Even when that
//! backend is a required toolchain component, using it for this self-check
//! would broaden an external semantic adapter into the compiler's syntax
//! layer, add a process/protocol dependency to a local invariant, and repeat
//! work before the typed pass. A valid TypeScript form accepted by TypeScript
//! but rejected here is therefore an SWC compatibility/configuration bug to
//! reproduce and fix at this boundary, not evidence that syntax verification
//! belongs to the type backend.

use swc_common::input::StringInput;
use swc_common::sync::Lrc;
use swc_common::{FileName, SourceMap, Spanned};
use swc_ecma_parser::lexer::Lexer;
use swc_ecma_parser::{Parser, Syntax, TsSyntax};

fn ts_syntax() -> Syntax {
    Syntax::Typescript(TsSyntax {
        tsx: false,
        decorators: true,
        ..Default::default()
    })
}

/// Parses `code` as a TypeScript module; returns the first syntax error as
/// `(message, line, col)` (1-based, positions in `code`).
fn parse_ts_module(code: &str) -> Result<(), (String, usize, usize)> {
    let cm: Lrc<SourceMap> = Default::default();
    let fm = cm.new_source_file(Lrc::new(FileName::Anon), code.to_string());
    let lexer = Lexer::new(
        ts_syntax(),
        Default::default(),
        StringInput::from(&*fm),
        None,
    );
    let mut parser = Parser::new_from(lexer);
    let result = parser.parse_module();
    let mut errors = parser.take_errors();
    if let Err(e) = result {
        errors.push(e);
    }
    match errors.into_iter().next() {
        None => Ok(()),
        Some(e) => {
            let pos = cm.lookup_char_pos(e.span().lo());
            let msg = e.into_kind().msg().to_string();
            Err((msg, pos.line, pos.col_display + 1))
        }
    }
}

/// Validates a variant field's type annotation. Returns a plain message on error.
pub(crate) fn check_type_fragment(ty: &str) -> Result<(), String> {
    let wrapped = format!("type __Rl = {};", ty);
    parse_ts_module(&wrapped).map_err(|(msg, _, _)| msg)
}

/// A failed self-check: swc's message and where it stopped in the
/// *generated* module (1-based line and column).
pub(crate) struct Failure {
    pub message: String,
    pub line: usize,
    pub col: usize,
}

/// Validates the final generated TypeScript.
pub(crate) fn verify_output(code: &str) -> Result<(), Failure> {
    parse_ts_module(code).map_err(|(message, line, col)| Failure { message, line, col })
}

/// The self-check's failure as an error in the `.rl` file the user has
/// open.
///
/// swc stopped at a byte of the *generated* module, which is a file no one
/// wrote — reporting that position as-is gives the user nothing to look
/// at, and an editor nowhere to put the squiggle but line 1. The position
/// travels back the way every other diagnostic does: through the emit
/// mappings to the source byte it was copied from, and — when it landed on
/// glue instead — to the construct that wrote the glue
/// ([`crate::EmitAnchor`]), whose own text is then the error's span.
///
/// The common cause is the documented trap: a construct that *almost*
/// parsed as rl (a missing `;` after `try`, a `match` without its
/// scrutinee parens) is passed through verbatim by contract, and then it
/// is rl syntax sitting in a TypeScript module. That is exactly what the
/// mapping points at, so the message says so.
pub(crate) fn at_source(
    source: &str,
    mappings: &[crate::EmitMapping],
    anchors: &[crate::EmitAnchor],
    code: &str,
    failure: &Failure,
) -> crate::error::RlError {
    let out = byte_of(code, failure.line, failure.col);
    let generic = || {
        format!(
            "generated TypeScript failed to parse: {}. This is either invalid TypeScript passed \
             through from the source or an rlc bug; use --no-verify to bypass.",
            failure.message,
        )
    };
    let (message, span) = match crate::typescript::mapper::to_source(mappings, out) {
        // Copied from the source: the offending text is the user's own,
        // and a construct that failed to parse as rl may be sitting in it.
        Some(src) => match rl_construct_at(source, src) {
            Some((word, at)) => (
                format!(
                    "`{word}` here did not parse as an rl `{word}`, so it was passed through as \
                     TypeScript and the generated module no longer parses: {}",
                    failure.message,
                ),
                Some((at, at + word.len())),
            ),
            None => (generic(), Some((src, src))),
        },
        // Glue: rlc's own output, and the construct that wrote it is the
        // only thing the user can act on.
        None => (
            generic(),
            anchors
                .iter()
                .find(|a| a.out <= out && out < a.end)
                .map(|a| (a.src, a.src_end)),
        ),
    };
    let code = crate::DiagnosticCode::VerifyFailed;
    match span {
        Some((start, end)) if end > start => {
            crate::error::RlError::span(start, end, message).code(code)
        }
        Some((start, _)) => crate::error::RlError::at(start, message).code(code),
        None => crate::error::RlError {
            message,
            offset: None,
            end: None,
            code,
            owner: None,
        },
    }
}

/// The byte offset of a 1-based line/column pair in `text`. swc counts
/// columns in characters, so the walk does too.
fn byte_of(text: &str, line: usize, col: usize) -> usize {
    let mut at = 0;
    for (n, text) in text.split_inclusive('\n').enumerate() {
        if n + 1 == line {
            return at
                + text
                    .char_indices()
                    .nth(col.saturating_sub(1))
                    .map_or(text.len(), |(byte, _)| byte);
        }
        at += text.len();
    }
    text.len()
}

/// The rl keyword the failing text belongs to, when the source around
/// `at` still holds one: an rl construct that did not parse is passed
/// through by contract, and it is never valid TypeScript.
///
/// Only the line the failure landed on is considered, and only a keyword
/// standing on its own — a `match` in `dispatch.match(x)` or a string is
/// not one. Nothing found means the text is ordinary TypeScript that
/// simply does not parse, and the message stays as it is.
fn rl_construct_at(source: &str, at: usize) -> Option<(&'static str, usize)> {
    let at = at.min(source.len());
    let line_start = source[..at].rfind('\n').map_or(0, |p| p + 1);
    let line_end = source[at..].find('\n').map_or(source.len(), |p| at + p);
    let line = &source[line_start..line_end];
    let mut best: Option<(&'static str, usize)> = None;
    for word in ["match", "try", "result", "flow"] {
        let mut from = 0;
        while let Some(found) = line[from..].find(word) {
            let start = from + found;
            let end = start + word.len();
            let before = line[..start].chars().next_back();
            let after = line[end..].chars().next();
            let standalone = before.is_none_or(|c| !is_ident_byte(c) && c != '.')
                && after.is_none_or(|c| !is_ident_byte(c));
            if standalone && best.is_none_or(|(_, best_at)| line_start + start < best_at) {
                best = Some((word, line_start + start));
            }
            from = end;
        }
    }
    best
}

fn is_ident_byte(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '$'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_column_pair_becomes_the_byte_it_names() {
        let text = "const a = 1;\nconst 한글 = 2;\nconst c = 3;\n";
        assert_eq!(byte_of(text, 1, 1), 0);
        assert_eq!(byte_of(text, 2, 7), text.find('한').unwrap());
        // Columns are counted in characters, as swc reports them.
        assert_eq!(byte_of(text, 2, 10), text.find("= 2").unwrap());
        // Past the end clamps rather than panicking.
        assert_eq!(byte_of(text, 99, 1), text.len());
    }

    #[test]
    fn an_rl_keyword_on_the_failing_line_is_named() {
        let src = "function f() {\n  return match shape {\n  };\n}\n";
        let at = src.find("shape {").unwrap();
        assert_eq!(
            rl_construct_at(src, at),
            Some(("match", src.find("match").unwrap()))
        );
    }

    #[test]
    fn a_keyword_that_is_part_of_a_name_is_not_one() {
        // `dispatch.match(x)` is a method call, and `matches` is a name.
        let src = "const n = dispatch.match(x) + matches.length;\n";
        assert_eq!(rl_construct_at(src, 10), None);
    }

    #[test]
    fn a_line_without_rl_syntax_is_left_to_the_generic_message() {
        let src = "const n = (1 + ;\n";
        assert_eq!(rl_construct_at(src, 12), None);
    }
}
