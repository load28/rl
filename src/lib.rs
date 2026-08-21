//! rl — a tiny preprocessor language that compiles to TypeScript.
//!
//! Every valid TypeScript file is a valid `.rl` file and compiles to itself
//! byte for byte; the compiler only rewrites the constructs rl adds —
//! Rust-style `enum` declarations (plain TypeScript enums pass through
//! untouched), `match` expressions (literal, tuple and nested patterns
//! included), `try` statements (Rust-`?`-style error propagation over
//! `Result`), let-else and `if let` statements, and the pipeline operator
//! `|>` — plus the `val` binding modifier, which is erased, and relative
//! `.rl` import specifiers, which are rewritten to a consumable form (see
//! [`ImportRewrite`]). rl-level errors — duplicate cases, non-exhaustive
//! matches, bad field types, misplaced `try`, mutation through a `val`
//! binding — are rlc compile errors with exact positions; the emitted
//! output is plain TypeScript.
//!
//! The core public API is [`compile`] plus its [`Options`] (with
//! [`ImportRewrite`]) and error type [`CompileError`] — code, or the first
//! error. The multi-diagnostic forms are [`analyze`] (every rl-level
//! [`Diagnostic`], in source order) and [`compile_report`] (the same, plus
//! the emission when one is possible); the standard library source is
//! [`STD_SOURCE`] (`Option`/`Result` with functional combinators, written
//! out by `rlc --emit-std`). The `rlc` binary in this crate is a thin CLI
//! over it.
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

mod analysis;
mod ast;
mod codegen;
mod diagnostics;
pub mod engine;
mod error;
pub mod hir;
mod lexer;
mod parser;
mod probe;
pub mod resolve;
mod scanner;
mod sema;
mod sidecar;
mod stdlib;
pub(crate) mod typescript;
mod val;
mod verify;

pub use analysis::{
    AnalyzedArm, BodyBinding, Coverage, CoveredEnum, MatchAnalysis, MatchConstructor, MatchSubject,
    NameKind, Origin, PatternAnalyses, PatternBinding, PatternSite, PayloadField, SiteKind,
    UnresolvedName, pattern_analyses,
};
pub use diagnostics::{Diagnostic, DiagnosticCode, Severity};
pub use error::CompileError;
pub use probe::{
    Literal, LiteralMatch, PayloadProbe, TagMatch, literal_matches, payload_probes, tag_matches,
};
pub use sidecar::{Sidecar, build_sidecar};
pub use stdlib::{STD_SOURCE, STD_SPECIFIER};
pub use val::{Mutation, ValBinding, ValFn, ValParam, ValPass, ValProbes, is_builtin_mutator_name};

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
    scan_module(source).imports
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
    scan_module(source).imports_std
}

/// A source file's module-level facts, gathered in a **single** parse:
/// its static relative `.rl` imports ([`rl_imports`]) and whether it
/// imports the standard library ([`imports_std`]).
///
/// A build tool walking a whole project needs both of a file, and parsing
/// is the compiler's most expensive non-tsc phase — asking the two
/// single-fact helpers back to back parses the file twice. The `rlc` CLI
/// scans every input through this.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModuleScan {
    /// The file's static relative `.rl` imports and re-exports, in source
    /// order — see [`rl_imports`].
    pub imports: Vec<RlImport>,
    /// Whether the file imports [`STD_SPECIFIER`] — see [`imports_std`].
    pub imports_std: bool,
}

/// Scans a source file's module-level facts in one parse — see
/// [`ModuleScan`].
///
/// ```
/// let scan = rlc::scan_module(
///     "import { Option } from \"@rl/std\";\nimport { T } from \"./t.rl\";\n",
/// );
/// assert!(scan.imports_std);
/// assert_eq!(scan.imports[0].specifier, "./t.rl");
/// ```
pub fn scan_module(source: &str) -> ModuleScan {
    let program = parser::parse(source);
    let mut scan = ModuleScan::default();
    for segment in &program.segments {
        let ast::Segment::RlImport(decl) = segment else {
            continue;
        };
        match decl.kind {
            // The standard library is not a project module — nothing to
            // resolve or collect declarations from.
            ast::RlSpecifier::Std => scan.imports_std = true,
            ast::RlSpecifier::Relative => scan.imports.push(RlImport {
                specifier: source[decl.spec.start + 1..decl.spec.end - 1].to_string(),
                names: match &decl.names {
                    ast::RlImportNames::Namespace(ns) => RlImportNames::Namespace(ns.clone()),
                    ast::RlImportNames::Named(entries) => RlImportNames::Named(entries.clone()),
                    ast::RlImportNames::None => RlImportNames::None,
                },
            }),
        }
    }
    scan
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
    /// Byte offset of the name in the source (see [`line_col`]).
    pub offset: usize,
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
                                    offset: f.name_off,
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

/// One chunk of emitted output copied verbatim from the source: `len` bytes
/// starting at byte `src` of the source appear at byte `out` of the output.
/// Produced by [`emit_mapped`]; chunks are non-overlapping in both
/// coordinate spaces. Compiler-written glue (IIFE scaffolding,
/// destructurings, enum emissions) has no mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmitMapping {
    /// Byte offset of the chunk in the source.
    pub src: usize,
    /// Byte offset of the chunk in the emitted output.
    pub out: usize,
    /// Length of the chunk in bytes (identical in both spaces).
    pub len: usize,
}

/// Where a `match` put its scrutinee.
///
/// Every `match` evaluates its scrutinee once, into a temporary the emitted
/// switch discriminates on. That temporary is the only place a type checker
/// can be *asked* about the scrutinee's type: asking at the scrutinee's own
/// text answers about the text — for `match (getShape())` that is the type
/// of `getShape`, a function, not the `Shape` the match is over. So the
/// emitter records where it wrote the name, and typed exhaustiveness
/// ([`tag_matches`], [`literal_matches`]) asks there.
///
/// ```
/// let source = "const v = match (f()) { Circle(r) => r };\n";
/// let emit = rlc::emit_mapped(source);
/// let temp = emit.scrutinee_temps[0];
/// assert_eq!(&source[temp.src..temp.src + 5], "match");
/// assert!(emit.code[temp.out..].starts_with("$rl_m = (f())"));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrutineeTemp {
    /// Byte offset of the `match` keyword in the source — the same offset
    /// [`probe::TagMatch::offset`] and [`probe::LiteralMatch::offset`] carry,
    /// so a probe and its temporary are joined by it.
    pub src: usize,
    /// Byte offset of the temporary's name in the emitted output.
    pub out: usize,
}

/// Which rl construct a stretch of compiler-written glue belongs to.
///
/// The kind is half of what turns a TypeScript diagnostic on that glue into
/// an rl one — the other half is the error code (see
/// `docs/design/rust-parity-analysis.md` §10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorKind {
    /// A `match` expression's switch or if-chain.
    Match,
    /// A `try` statement's test, early return and binding.
    Try,
    /// A let-else statement's test and destructuring.
    LetElse,
    /// An `if let` statement's test and destructuring.
    IfLet,
    /// One `<-` binding of a `result` block.
    ResultBind,
    /// A pipeline's apply helper (`$rl_ap`) or composition helper
    /// (`$rl_fl`).
    Pipe,
}

/// A stretch of emitted output that rlc wrote itself, and the construct it
/// wrote it for.
///
/// [`EmitMapping`] answers "which source bytes are these output bytes?" and
/// exists only where the answer is *these exact bytes*. Glue has no such
/// answer — it is text no one wrote — but it always has an **origin**, and
/// that is what an anchor records. It is deliberately one-way and for
/// diagnostics only: navigation and rename must never resolve into glue
/// (an edit there would corrupt the program), while a diagnostic there is
/// worth reporting at the construct that produced it.
///
/// Anchors nest, and are ordered so that an inner one comes before the
/// outer one that contains it — a consumer takes the first match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmitAnchor {
    /// Byte offset in the emitted output where the construct's glue starts.
    pub out: usize,
    /// Byte offset just past its end.
    pub end: usize,
    /// Byte offset in the source of the construct's keyword — where a
    /// diagnostic about this glue belongs.
    pub src: usize,
    /// Byte offset just past the construct's own source text — the end of
    /// what a diagnostic about this glue should underline. The range it
    /// closes (`src..src_end`) is the construct as the user wrote it, not
    /// the whole statement: for `try` it is `try <expr>`, for a `match` the
    /// keyword and its scrutinee.
    pub src_end: usize,
    /// What kind of construct wrote it.
    pub kind: AnchorKind,
}

/// Where a nested pattern's **receiver** landed in the emitted output.
///
/// `Ok(value: Some(v))` lowers to a condition chain whose second link
/// reads `$rl_m.value.kind === "Some"`. That `$rl_m.value` is the only
/// place a type checker can be asked what the *payload* admits — rlc knows
/// the field's declared type text, but a text is not a type, and a type
/// parameter or a hand-written union names no declaration rlc holds. The
/// emitter records where it wrote the receiver, and the typed
/// exhaustiveness pass asks there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PayloadTemp {
    /// Byte offset of the nested pattern's tag in the source — the
    /// occurrence this receiver was written for.
    pub src: usize,
    /// Byte offset of the receiver expression in the emitted output.
    pub out: usize,
}

/// The result of [`emit_mapped`]: the emitted TypeScript and the
/// source↔output mappings of every verbatim-copied chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappedEmit {
    /// The emitted TypeScript.
    pub code: String,
    /// Source↔output mappings, ordered by output offset.
    pub mappings: Vec<EmitMapping>,
    /// Where each `match` bound its scrutinee, ordered by output offset.
    pub scrutinee_temps: Vec<ScrutineeTemp>,
    /// Where each nested pattern's receiver was written, ordered by output
    /// offset.
    pub payload_temps: Vec<PayloadTemp>,
    /// The glue each construct wrote, innermost first — the origin of a
    /// diagnostic that lands where no mapping reaches.
    pub anchors: Vec<EmitAnchor>,
}

impl MappedEmit {
    /// The construct that wrote the glue at output byte `out`, innermost
    /// first. `None` when the byte is not in any construct's glue.
    pub fn anchor_at(&self, out: usize) -> Option<&EmitAnchor> {
        self.anchors.iter().find(|a| a.out <= out && out < a.end)
    }
}

/// Emits `source` for language tooling: structural parse + code emission
/// only, with source↔output byte mappings for every chunk copied verbatim
/// (passthrough segments, match scrutinees and arm bodies, `try`/let-else/
/// `if let` expressions, pipeline steps, template chunks).
///
/// Unlike [`compile`] this is **infallible**: semantic checks and output
/// verification are skipped, so a buffer mid-edit (with, say, a
/// non-exhaustive match) still emits — diagnostics remain [`compile`]/`rlc
/// --check`'s job. Relative `.rl` import specifiers and `"@rl/std"` are
/// left untouched ([`ImportRewrite::Off`] semantics): the consumer — an
/// editor serving the output as a virtual TypeScript document — resolves
/// them itself. Corresponds to the CLI's `--emit-map`.
///
/// ```
/// let m = rlc::emit_mapped("const n: number = 1;\n");
/// assert_eq!(m.code, "const n: number = 1;\n");
/// assert_eq!(m.mappings, [rlc::EmitMapping { src: 0, out: 0, len: 21 }]);
/// ```
pub fn emit_mapped(source: &str) -> MappedEmit {
    let program = parser::parse(source);
    let flat = codegen::emit_with_map(&program, source, ImportRewrite::Off, None);
    MappedEmit {
        code: flat.code,
        mappings: flat.mappings,
        scrutinee_temps: flat.scrutinee_temps,
        payload_temps: flat.payload_temps,
        anchors: flat.anchors,
    }
}

/// Collects a file's `val` bindings and its mutations, **unpaired** — the
/// delegated form of `val`'s analysis.
///
/// [`compile`] pairs the two itself, with a lexical scope model of its own.
/// A caller that has a TypeScript checker does better: the binding a
/// mutation belongs to is the one whose declaration shares its *symbol*, and
/// symbol identity is not an approximation of scope — it is scope, as
/// TypeScript resolved it. `rlc --check-types` pairs them that way; see
/// `docs/reference/language.md` §10 for what `val` means either way.
///
/// ```
/// let probes = rlc::val_probes("val const xs = [];\nxs.push(1);\nys.push(2);\n");
/// assert_eq!(probes.bindings.len(), 1);
/// assert_eq!(probes.bindings[0].name, "xs");
/// // Both calls are collected: which one is rooted at the `val` binding is
/// // not decided here.
/// assert_eq!(probes.mutations.len(), 2);
/// assert_eq!(probes.mutations[1].name, "ys");
/// ```
///
/// Method calls are collected whatever the method is called — whether one
/// mutates is the verdict's half (the checker's built-in answer plus
/// [`is_builtin_mutator_name`]), not collection's:
///
/// ```
/// let probes = rlc::val_probes("val const d = mk();\nd.at(0);\n");
/// assert_eq!(probes.mutations.len(), 1);
/// assert_eq!(probes.mutations[0].method.as_ref().unwrap().0, "at");
/// ```
pub fn val_probes(source: &str) -> ValProbes {
    let tokens = lexer::lex(source, 0, source.len());
    val::probes(source, &tokens)
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
    /// Leave the two judgments a TypeScript checker makes better to a
    /// TypeScript checker: match exhaustiveness, and which binding a
    /// mutation path is rooted at (`val`).
    ///
    /// rlc answers both on its own, from its enum declarations and a lexical
    /// scope model of its own, and those answers are what [`compile`]
    /// reports by default. Both are approximations of TypeScript's:
    /// exhaustiveness is the *declared* type's answer, so a case an earlier
    /// guard already removed is still demanded and an enum from another
    /// module has to be collected ([`Options::extern_enums`]); `val`'s
    /// pairing is a scope model, so shadowing and redeclaration are rlc's
    /// reading rather than TypeScript's. A caller with a checker asks it
    /// instead — the narrowed type at each `match`, and symbol identity for
    /// each binding — and reports what it says. `rlc --check-types` does
    /// exactly that ([`tag_matches`], [`literal_matches`], [`val_probes`]).
    ///
    /// Every other rl-level check runs either way: duplicate cases,
    /// misplaced wildcards, bad field types, `val`'s call-capability rule.
    pub defer_to_checker: bool,
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
            defer_to_checker: false,
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
    compile_mapped(source, options).map(|emit| emit.code)
}

/// [`compile`], also returning the source↔output byte mappings of every
/// chunk copied verbatim from the source — the same mappings
/// [`emit_mapped`] produces, but from a fully checked compilation.
///
/// Callers that report a tsc diagnostic over the emitted TypeScript use
/// these to name the position in the `.rl` source instead of one in a file
/// that was never written (`rlc --types`, `docs/reference/cli.md` §types).
///
/// ```
/// use rlc::{compile_mapped, Options};
///
/// let emit = compile_mapped("const n = 1;\n", &Options::default()).unwrap();
/// assert_eq!(emit.code, "const n = 1;\n");
/// assert_eq!(emit.mappings, [rlc::EmitMapping { src: 0, out: 0, len: 13 }]);
/// ```
///
/// # Errors
///
/// Identical to [`compile`].
pub fn compile_mapped(source: &str, options: &Options) -> Result<MappedEmit, CompileError> {
    // The swc-style pipeline: structural parse (infallible; anything that is
    // not fully rl syntax stays a verbatim byte range) → semantic checks
    // (every rl-level error, including exhaustiveness — never delegated to
    // tsc; `val`'s binding analysis reads the token stream the parse
    // already produced) → code emission (infallible).
    //
    // The checks accumulate everything ([`analyze`] is the API that returns
    // it all); this entry point keeps its historical contract — code, or
    // the first error in source order — and skips emission when the checks
    // already failed.
    let (program, tokens) = parser::lex_and_parse(source);
    if let Some(first) = rl_errors(source, &program, &tokens, options)
        .into_iter()
        .next()
    {
        return Err(
            diagnostics::Diagnostic::from_rl(first).to_compile_error(source, options.filename)
        );
    }
    let flat = codegen::emit_with_map(
        &program,
        source,
        options.rewrite_imports,
        options.std_import,
    );
    if options.verify
        && let Err(failure) = verify::verify_output(&flat.code)
    {
        // The self-check reads the *generated* module, but the user only
        // has the `.rl` file open. A position in a file no one wrote is
        // not a position, so it is carried back through the mappings to
        // the source — and where the failure fell in a construct's glue,
        // that construct is named. (Without this the error arrives with no
        // position at all and an editor pins it to line 1.)
        let failure =
            verify::at_source(source, &flat.mappings, &flat.anchors, &flat.code, &failure);
        return Err(
            diagnostics::Diagnostic::from_rl(failure).to_compile_error(source, options.filename)
        );
    }
    Ok(MappedEmit {
        code: flat.code,
        mappings: flat.mappings,
        scrutinee_temps: flat.scrutinee_temps,
        payload_temps: flat.payload_temps,
        anchors: flat.anchors,
    })
}

/// Every rl-level violation of `source`, in source order — the semantic
/// passes over an already-built parse. What [`analyze`] and
/// [`compile_report`] share.
fn rl_errors(
    source: &str,
    program: &ast::Program,
    tokens: &[lexer::Token],
    options: &Options,
) -> Vec<RlError> {
    let mut errors = sema::check_all(
        program,
        options.verify,
        options.extern_enums,
        options.defer_to_checker,
    );
    if !options.defer_to_checker {
        errors.extend(val::check_all(source, tokens));
    }
    // One order for every producer: where the reader's eye goes, top to
    // bottom. Stable, so equal positions keep their category order.
    errors.sort_by_key(|e| e.offset.unwrap_or(usize::MAX));
    errors
}

/// Checks `source` and returns **every** rl-level diagnostic, in source
/// order — nothing is emitted and nothing stops at the first violation.
///
/// This is the multi-diagnostic form of [`compile`]'s error half: the CLI's
/// `--check`, the `--server`, and the engine all report from it, so one
/// broken match no longer hides the file's other problems (TASK-117).
/// Positions are byte offsets ([`Diagnostic::to_compile_error`] converts to
/// the CLI's line/column form). The output self-check needs an emission and
/// is [`compile_report`]'s half.
///
/// ```
/// let source = "enum E { A(x: number), B }\n\
///     const a = match (E.A(1)) { A(x) => x };\n\
///     const b = match (E.B) { B => 0 };\n";
/// let diagnostics = rlc::analyze(source, &rlc::Options::default());
/// assert_eq!(diagnostics.len(), 2);
/// assert!(diagnostics.iter().all(|d| d.code == rlc::DiagnosticCode::MatchNotExhaustive));
/// ```
pub fn analyze(source: &str, options: &Options) -> Vec<Diagnostic> {
    let (program, tokens) = parser::lex_and_parse(source);
    rl_errors(source, &program, &tokens, options)
        .into_iter()
        .map(diagnostics::Diagnostic::from_rl)
        .collect()
}

/// A full compilation's answer: everything found, and the emission when one
/// was possible.
///
/// Unlike [`compile`], recoverable rl errors do not withhold the emission:
/// codegen is infallible, so a file with a duplicate arm still lowers to
/// plain TypeScript — which is what lets a typed pass run and report its
/// diagnostics *alongside* the rl ones instead of losing them
/// ([`DiagnosticCode::blocks_projection`], TASK-117 symptom 3). `emit` is
/// `None` only when a diagnostic blocks projection: text the parser could
/// not claim, a bad field type, or a failed output self-check.
#[derive(Debug, Clone)]
pub struct CompileReport {
    /// The emitted TypeScript with its mappings, unless a diagnostic made
    /// emission impossible.
    pub emit: Option<MappedEmit>,
    /// Every rl-level diagnostic, in source order.
    pub diagnostics: Vec<Diagnostic>,
}

/// Compiles `source` and reports everything — the multi-diagnostic,
/// still-emitting form of [`compile_mapped`]. See [`CompileReport`].
pub fn compile_report(source: &str, options: &Options) -> CompileReport {
    let (program, tokens) = parser::lex_and_parse(source);
    let mut errors = rl_errors(source, &program, &tokens, options);
    if errors.iter().any(|e| e.code.blocks_projection()) {
        return CompileReport {
            emit: None,
            diagnostics: errors
                .into_iter()
                .map(diagnostics::Diagnostic::from_rl)
                .collect(),
        };
    }
    let flat = codegen::emit_with_map(
        &program,
        source,
        options.rewrite_imports,
        options.std_import,
    );
    let mut emit = Some(MappedEmit {
        code: flat.code,
        mappings: flat.mappings,
        scrutinee_temps: flat.scrutinee_temps,
        payload_temps: flat.payload_temps,
        anchors: flat.anchors,
    });
    if options.verify
        && let Some(flat) = &emit
        && let Err(failure) = verify::verify_output(&flat.code)
    {
        errors.push(verify::at_source(
            source,
            &flat.mappings,
            &flat.anchors,
            &flat.code,
            &failure,
        ));
        emit = None;
    }
    CompileReport {
        emit,
        diagnostics: errors
            .into_iter()
            .map(diagnostics::Diagnostic::from_rl)
            .collect(),
    }
}
