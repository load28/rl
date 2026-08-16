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

/// Validates the final generated TypeScript. Returns a formatted message on error.
pub(crate) fn verify_output(code: &str) -> Result<(), String> {
    parse_ts_module(code).map_err(|(msg, line, col)| {
        format!(
            "generated TypeScript failed to parse: {} (line {}, col {} of the generated output). \
             This is either invalid TypeScript passed through from the source or an rlc bug; \
             use --no-verify to bypass.",
            msg, line, col
        )
    })
}
