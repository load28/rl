//! rl — a tiny preprocessor language that compiles to TypeScript.
//!
//! Every valid TypeScript file is a valid `.rl` file and compiles to itself
//! byte for byte; the compiler only rewrites the two constructs rl adds:
//! Rust-style `enum` declarations (plain TypeScript enums pass through
//! untouched) and `match` expressions. rl-level errors — duplicate cases,
//! non-exhaustive matches, bad field types — are rlc compile errors with
//! exact positions; the emitted output is plain TypeScript. See
//! `docs/design/` for the architecture and decisions.

mod error;
mod scanner;
mod transform;
mod verify;

pub use error::CompileError;

use error::{line_col, RlError};
use transform::Ctx;

#[derive(Debug, Clone)]
pub struct Options<'a> {
    /// Reported in error messages.
    pub filename: Option<&'a str>,
    /// Validate enum field types and the generated output with swc.
    pub verify: bool,
}

impl Default for Options<'_> {
    fn default() -> Self {
        Options { filename: None, verify: true }
    }
}

/// Compile rl source text to TypeScript source text.
pub fn compile(source: &str, options: &Options) -> Result<String, CompileError> {
    let to_compile_error = |e: RlError| {
        let (line, col) = match e.offset {
            Some(off) => line_col(source, off),
            None => (0, 0),
        };
        CompileError {
            message: e.message,
            filename: options.filename.map(String::from),
            line,
            col,
        }
    };

    let ctx = Ctx::new(source, options.verify);
    let code =
        transform::transform(&ctx, 0, source.len()).map_err(to_compile_error)?;

    // rlc owns exhaustiveness: wildcard-free matches over enums declared in
    // this file must cover every case. This is an rl-level error, checked
    // here — never delegated to tsc.
    transform::check_exhaustiveness(&ctx).map_err(to_compile_error)?;

    if options.verify
        && let Err(message) = verify::verify_output(&code) {
            return Err(CompileError {
                message,
                filename: options.filename.map(String::from),
                line: 0,
                col: 0,
            });
        }
    Ok(code)
}
