//! rl — a tiny preprocessor language that compiles to TypeScript.
//!
//! Every valid TypeScript file is a valid `.rl` file and compiles to itself
//! byte for byte; the compiler only rewrites the six constructs rl adds —
//! Rust-style `enum` declarations (plain TypeScript enums pass through
//! untouched), `match` expressions (tuple matches and nested patterns
//! included), `try` statements (Rust-`?`-style error propagation over
//! `Result`), let-else and `if let` statements, and the pipeline operator
//! `|>` — plus relative `.rl` import specifiers, which are rewritten to a
//! consumable form (see [`ImportRewrite`]). rl-level errors — duplicate
//! cases, non-exhaustive matches, bad field types, misplaced `try` — are
//! rlc compile errors with exact positions; the emitted output is plain
//! TypeScript.
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
mod lexer;
mod parser;
mod scanner;
mod sema;
mod sidecar;
mod stdlib;
mod verify;

pub use error::CompileError;
pub use sidecar::{Sidecar, build_sidecar};
pub use stdlib::{STD_SOURCE, STD_SPECIFIER};

use error::RlError;

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
    /// `"./x.rl"` → `"./x.ts"` — points at the file rlc actually emits.
    /// Requires the consumer to enable TypeScript's
    /// `allowImportingTsExtensions` *and* `rewriteRelativeImportExtensions`
    /// (TypeScript 5.7+), which turn `.ts` specifiers into `.js` on emit.
    Ts,
    /// Leave `.rl` specifiers untouched (byte-for-byte passthrough).
    Off,
}

/// An enum declaration from another module, made available to [`compile`]'s
/// exhaustiveness checking via [`Options::extern_enums`].
///
/// Collected by build tools (the `rlc` CLI does this for direct relative
/// `.rl` imports) with [`exported_enums`] over the imported file's source,
/// filtered through the importing file's clause ([`rl_imports`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternEnum {
    /// The enum's name in the *importing* file's scope (import aliases
    /// applied; `ns.Name` for a namespace import). A local declaration of
    /// the same name shadows it; it shadows a built-in of the same name.
    pub name: String,
    /// The enum's case tags.
    pub tags: Vec<String>,
    /// Where the declaration came from, quoted in error messages —
    /// typically the import specifier as written (e.g. `./token.rl`).
    /// [`exported_enums`] leaves it `None`; the collector fills it in.
    pub from: Option<String>,
}

/// One static relative `.rl` import (or re-export) of a source file, in
/// source order — the file's outgoing module-graph edges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RlImport {
    /// The specifier as written, without quotes (e.g. `./token.rl`).
    pub specifier: String,
    /// What the statement brings into local scope.
    pub names: RlImportNames,
}

/// The bindings an [`RlImport`] brings into local scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RlImportNames {
    /// `import * as ns from ...` — every export, namespace-qualified.
    Namespace(String),
    /// `import { a, b as c, type d } from ...` — (exported name, alias).
    Named(Vec<(String, Option<String>)>),
    /// A side-effect import or a re-export — nothing enters local scope.
    None,
}

/// Extracts the exported rl enum declarations (name + case tags) of a
/// source file, without compiling it — the declaration-table half of
/// project-wide exhaustiveness checking. Non-exported enums and plain
/// TypeScript enums are not included. The returned entries have
/// [`ExternEnum::from`] set to `None`.
///
/// ```
/// let decls = rlc::exported_enums(
///     "export enum Token { Num(value: number), Eof }\nenum Private { A() }\n",
/// );
/// assert_eq!(decls.len(), 1);
/// assert_eq!(decls[0].name, "Token");
/// assert_eq!(decls[0].tags, ["Num", "Eof"]);
/// ```
pub fn exported_enums(source: &str) -> Vec<ExternEnum> {
    let program = parser::parse(source);
    program
        .segments
        .iter()
        .filter_map(|segment| match segment {
            ast::Segment::Enum(decl) if decl.exported => Some(ExternEnum {
                name: decl.name.clone(),
                tags: decl.cases.iter().map(|c| c.tag.clone()).collect(),
                from: None,
            }),
            _ => None,
        })
        .collect()
}

/// Lists a source file's static relative `.rl` imports and re-exports, in
/// source order — the edges a build tool follows to collect declarations
/// with [`exported_enums`].
///
/// ```
/// let imports = rlc::rl_imports("import { Token as T } from \"./token.rl\";\n");
/// assert_eq!(imports[0].specifier, "./token.rl");
/// assert_eq!(
///     imports[0].names,
///     rlc::RlImportNames::Named(vec![("Token".into(), Some("T".into()))]),
/// );
/// ```
pub fn rl_imports(source: &str) -> Vec<RlImport> {
    let program = parser::parse(source);
    program
        .segments
        .iter()
        .filter_map(|segment| match segment {
            // The standard library is not a project module — nothing to
            // resolve or collect declarations from.
            ast::Segment::RlImport(decl) if decl.kind == ast::RlSpecifier::Relative => {
                Some(RlImport {
                    specifier: source[decl.spec.start + 1..decl.spec.end - 1].to_string(),
                    names: match &decl.names {
                        ast::RlImportNames::Namespace(ns) => RlImportNames::Namespace(ns.clone()),
                        ast::RlImportNames::Named(entries) => RlImportNames::Named(entries.clone()),
                        ast::RlImportNames::None => RlImportNames::None,
                    },
                })
            }
            _ => None,
        })
        .collect()
}

/// Whether a source file imports the standard library ([`STD_SPECIFIER`]).
///
/// Build tools use this to decide whether the module has to be written out
/// (the `rlc` CLI does it automatically) and where the importing file
/// should point — see [`Options::std_import`].
///
/// ```
/// assert!(rlc::imports_std("import { Option } from \"@rl/std\";\n"));
/// assert!(!rlc::imports_std("import { Option } from \"./rl.js\";\n"));
/// ```
pub fn imports_std(source: &str) -> bool {
    parser::parse(source).segments.iter().any(|segment| {
        matches!(
            segment,
            ast::Segment::RlImport(decl) if decl.kind == ast::RlSpecifier::Std
        )
    })
}

/// An rl enum declaration with source positions — the symbol-interface
/// counterpart of [`ExternEnum`], produced by [`enum_symbols`] and emitted
/// as JSON by `rlc --symbols` for language tooling (go-to-definition,
/// completion, hover).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumSymbol {
    /// The enum's declared name.
    pub name: String,
    /// Byte offset of the name in the source (see [`line_col`]).
    pub offset: usize,
    /// Whether the declaration has an `export` modifier.
    pub exported: bool,
    /// The verbatim `<...>` generic parameter list, or `""`.
    pub generics: String,
    /// The enum's cases, in declaration order.
    pub cases: Vec<CaseSymbol>,
}

/// One case of an [`EnumSymbol`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseSymbol {
    /// The case tag.
    pub tag: String,
    /// Byte offset of the tag in the source.
    pub offset: usize,
    /// `None` for a unit case without parens; `Some` (possibly empty) for
    /// a case with a field list.
    pub fields: Option<Vec<FieldSymbol>>,
}

/// One field of a payload-carrying case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldSymbol {
    /// The field name.
    pub name: String,
    /// Whether the field is optional (`name?: T`).
    pub optional: bool,
    /// The verbatim type annotation text.
    pub ty: String,
}

/// Extracts every rl enum declaration of a source file with positions —
/// exported or not, flagged by [`EnumSymbol::exported`]. Plain TypeScript
/// enums are not rl enums and are not included.
///
/// ```
/// let syms = rlc::enum_symbols("export enum Token { Num(value: number), Eof }\n");
/// assert_eq!(syms[0].name, "Token");
/// assert_eq!(rlc::line_col(
///     "export enum Token { Num(value: number), Eof }\n", syms[0].offset), (1, 13));
/// assert_eq!(syms[0].cases[1].tag, "Eof");
/// assert_eq!(syms[0].cases[1].fields, None);
/// ```
pub fn enum_symbols(source: &str) -> Vec<EnumSymbol> {
    let program = parser::parse(source);
    program
        .segments
        .iter()
        .filter_map(|segment| match segment {
            ast::Segment::Enum(decl) => Some(EnumSymbol {
                name: decl.name.clone(),
                offset: decl.name_off,
                exported: decl.exported,
                generics: decl.generics.clone(),
                cases: decl
                    .cases
                    .iter()
                    .map(|c| CaseSymbol {
                        tag: c.tag.clone(),
                        offset: c.tag_off,
                        fields: c.fields.as_ref().map(|fields| {
                            fields
                                .iter()
                                .map(|f| FieldSymbol {
                                    name: f.name.clone(),
                                    optional: f.optional,
                                    ty: f.ty.clone(),
                                })
                                .collect()
                        }),
                    })
                    .collect(),
            }),
            _ => None,
        })
        .collect()
}

/// Converts a byte offset into `source` to a 1-based `(line, column)` —
/// the same mapping [`CompileError`] positions use (column counted in
/// UTF-8 code points). Offsets past the end clamp to the last position.
pub fn line_col(source: &str, offset: usize) -> (usize, usize) {
    error::line_col(source, offset)
}

/// Compilation options for [`compile`].
///
/// The default is no filename, verification enabled, `.rl` import
/// specifiers rewritten to `.js`, and no imported declarations:
///
/// ```
/// let opts = rlc::Options::default();
/// assert_eq!(opts.filename, None);
/// assert!(opts.verify);
/// assert_eq!(opts.rewrite_imports, rlc::ImportRewrite::Js);
/// assert!(opts.extern_enums.is_empty());
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
    /// Enum declarations imported from other modules, included in
    /// exhaustiveness checking (shadowed by local declarations; shadowing
    /// built-ins of the same name). The `rlc` CLI fills this from the
    /// file's direct relative `.rl` imports.
    pub extern_enums: &'a [ExternEnum],
    /// What `"@rl/std"` ([`STD_SPECIFIER`]) is rewritten to on the way out
    /// — the path of the standard library module this output will sit
    /// next to (`"./rl.js"`, `"../rl.ts"`, ...). `None` leaves the bare
    /// specifier untouched, which is what a bundler plugin wants: it
    /// resolves the module itself.
    pub std_import: Option<&'a str>,
}

impl Default for Options<'_> {
    fn default() -> Self {
        Options {
            filename: None,
            verify: true,
            rewrite_imports: ImportRewrite::default(),
            extern_enums: &[],
            std_import: None,
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
    sema::check(&program, options.verify, options.extern_enums).map_err(to_compile_error)?;
    let code = codegen::emit(
        &program,
        source,
        options.rewrite_imports,
        options.std_import,
    );

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
