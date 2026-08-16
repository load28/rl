//! rl — a tiny preprocessor language that compiles to TypeScript.
//!
//! Every valid TypeScript file is a valid `.rl` file and compiles to itself
//! byte for byte; the compiler only rewrites the four constructs rl adds —
//! Rust-style `enum` declarations (plain TypeScript enums pass through
//! untouched), `match` expressions, `try` statements (Rust-`?`-style error
//! propagation over `Result`), and let-else statements — plus relative
//! `.rl` import specifiers, which are rewritten to a consumable form (see
//! [`ImportRewrite`]). rl-level errors — duplicate cases, non-exhaustive
//! matches, bad field types, misplaced `try` — are rlc compile errors with
//! exact positions; the emitted output is plain TypeScript.
//!
//! The whole public API is [`compile`] plus its [`Options`] (with
//! [`ImportRewrite`]), error type
//! [`CompileError`], and the standard library source [`STD_SOURCE`]
//! (`Option`/`Result` with functional combinators, written out by
//! `rlc --emit-std`). The `rlc` binary in this crate is a thin CLI over it.
//!
//! # Example
//!
//! ```
//! use rlc::{compile, Options};
//!
//! let source = r#"
//! export enum Shape {
//!   Circle(radius: number),
//!   Point,
//! }
//!
//! const area = match (Shape.Circle(2)) {
//!   Circle(radius) => Math.PI * radius * radius,
//!   Point => 0,
//! };
//! "#;
//!
//! let ts = compile(source, &Options::default())?;
//! assert!(ts.contains(r#"{ kind: "Circle"; radius: number }"#));
//! assert!(ts.contains("switch ($rl_m.kind)"));
//! # Ok::<(), rlc::CompileError>(())
//! ```
//!
//! # Documentation
//!
//! - `docs/reference/language.md` — normative language reference (grammar,
//!   enum/TS-enum disambiguation, emitted code shapes, exhaustiveness).
//! - `docs/reference/cli.md` / `docs/reference/errors.md` — CLI and
//!   diagnostics reference.
//! - `docs/design/` — architecture and design decisions.

mod ast;
mod codegen;
mod error;
mod parser;
mod scanner;
mod sema;
mod stdlib;
mod verify;

pub use error::CompileError;
pub use stdlib::STD_SOURCE;

use error::{RlError, line_col};

/// How relative `.rl` import specifiers are rewritten in the emitted
/// TypeScript. Applies to static `import` declarations and
/// `export ... from` re-exports whose specifier is a relative path ending
/// in `.rl`; every other specifier — and dynamic `import(...)` — passes
/// through untouched. Corresponds to the CLI's `--rewrite-imports` flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImportRewrite {
    /// `"./x.rl"` → `"./x.js"` — works under both `moduleResolution:
    /// nodenext` (Node ESM requires the extension) and `bundler` (tsc maps
    /// `.js` to `.ts`). The default.
    #[default]
    Js,
    /// `"./x.rl"` → `"./x"` — for bundler setups that prefer
    /// extensionless specifiers.
    Bare,
    /// Leave `.rl` specifiers untouched (byte-for-byte passthrough).
    Off,
}

/// Compilation options for [`compile`].
///
/// The default is no filename, verification enabled, and `.rl` import
/// specifiers rewritten to `.js`:
///
/// ```
/// let opts = rlc::Options::default();
/// assert_eq!(opts.filename, None);
/// assert!(opts.verify);
/// assert_eq!(opts.rewrite_imports, rlc::ImportRewrite::Js);
/// ```
#[derive(Debug, Clone)]
pub struct Options<'a> {
    /// Filename reported in [`CompileError`]s (and their `Display` output).
    /// `None` renders as `<input>`.
    pub filename: Option<&'a str>,
    /// Validate enum field types and the generated output with swc.
    /// Corresponds to the CLI's `--no-verify` escape hatch when `false`;
    /// disabling it lets syntactically bad field types flow into the output
    /// (where tsc will report them) and skips the emitted-code self-check.
    pub verify: bool,
    /// How relative `.rl` import specifiers are rewritten in the output.
    pub rewrite_imports: ImportRewrite,
}

impl Default for Options<'_> {
    fn default() -> Self {
        Options {
            filename: None,
            verify: true,
            rewrite_imports: ImportRewrite::default(),
        }
    }
}

/// Compile rl source text to TypeScript source text.
///
/// Only rl constructs (`enum` declarations, `match` expressions, `try` and
/// let-else statements) and relative `.rl` import specifiers (per
/// [`Options::rewrite_imports`]) are rewritten; everything else — including
/// all plain TypeScript `enum` forms — passes through byte for byte. A
/// candidate construct that does not fully parse as rl syntax is passed
/// through untouched rather than reported as an error.
/// The output has no generated banner comment (that is added by the CLI).
///
/// # Errors
///
/// Returns a [`CompileError`] with a 1-based position in `source` for every
/// rl-level rule violation: duplicate enum cases, invalid field types,
/// duplicate or misplaced `match` arms, and non-exhaustive matches over enums
/// declared in this source. With [`Options::verify`] enabled, a final
/// self-check that the generated output parses as TypeScript can also fail
/// (reported without a position). See `docs/reference/errors.md` for the
/// full catalogue.
///
/// ```
/// use rlc::{compile, Options};
///
/// let source = "enum E { A(x: number), B }\nconst v = match (E.A(1)) { A(x) => x };";
/// let options = Options { filename: Some("demo.rl"), ..Options::default() };
/// let err = compile(source, &options).unwrap_err();
/// assert_eq!((err.line, err.col), (2, 11));
/// assert!(err.message.contains(r#"not exhaustive: missing "B""#));
/// assert!(err.to_string().starts_with("demo.rl:2:11: "));
/// ```
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

    // The swc-style pipeline: structural parse (infallible; anything that is
    // not fully rl syntax stays a verbatim byte range) → semantic checks
    // (every rl-level error, including exhaustiveness — never delegated to
    // tsc) → code emission (infallible).
    let program = parser::parse(source);
    sema::check(&program, options.verify).map_err(to_compile_error)?;
    let code = codegen::emit(&program, source, options.rewrite_imports);

    if options.verify
        && let Err(message) = verify::verify_output(&code)
    {
        return Err(CompileError {
            message,
            filename: options.filename.map(String::from),
            line: 0,
            col: 0,
        });
    }
    Ok(code)
}
