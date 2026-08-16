//! rl — a tiny preprocessor language that compiles to TypeScript.
//!
//! Every valid TypeScript file is a valid `.rl` file and compiles to itself
//! byte for byte; the compiler only rewrites the two constructs rl adds:
//! Rust-style `variant` declarations and `match` expressions. See
//! `docs/design/rust-rewrite.md` for the architecture.

mod error;
mod scanner;
mod transform;
mod verify;

pub use error::CompileError;

use error::line_col;
use transform::Ctx;

#[derive(Debug, Clone)]
pub struct Options<'a> {
    /// Reported in error messages.
    pub filename: Option<&'a str>,
    /// Validate variant field types and the generated output with swc.
    pub verify: bool,
}

impl Default for Options<'_> {
    fn default() -> Self {
        Options { filename: None, verify: true }
    }
}

/// Compile rl source text to TypeScript source text.
pub fn compile(source: &str, options: &Options) -> Result<String, CompileError> {
    let ctx = Ctx {
        src: source,
        bytes: source.as_bytes(),
        verify: options.verify,
    };
    let code = transform::transform(&ctx, 0, source.len()).map_err(|e| {
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
    })?;

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
