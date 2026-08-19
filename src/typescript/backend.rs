//! The seam: what rl asks TypeScript, in rl's terms.
//!
//! Nothing here mentions a process, a protocol, or a compiler version — that
//! is [`super::native`]'s business. rl features build a [`Query`] and read an
//! [`Answers`]; swapping how the compiler is reached changes neither.
//!
//! A query is a **batch**. One round trip carries every question about a
//! project, because the transport is a real IPC channel and asking a hundred
//! questions one at a time costs a hundred round trips.

use std::path::PathBuf;

/// One module of the project as TypeScript should see it: the ordinary
/// TypeScript an `.rl` file lowers to, at the path that `.rl` file occupies
/// with a `.ts` extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Module {
    /// Absolute path, `.ts` extension — the name TypeScript resolves.
    pub path: PathBuf,
    /// The emitted TypeScript.
    pub text: String,
}

/// "Which of these literals does the scrutinee's type still allow?" — the
/// typed half of literal-`match` exhaustiveness.
///
/// The position names the scrutinee **as it appears in the emitted module**,
/// so the answer reflects the type TypeScript computes at that point,
/// narrowing included. rlc never reconstructs the declared type.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LiteralQuery {
    /// The module the scrutinee lives in.
    pub module: PathBuf,
    /// UTF-16 offset of the scrutinee in that module.
    pub position: usize,
    /// The literals the match's unguarded arms cover.
    pub covered: Vec<rlc::Literal>,
}

/// "What does this method call resolve to?" — the typed half of `val`.
///
/// rlc does not decide from the method's name whether a call mutates; it
/// names the call and lets the checker say where the method is declared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValQuery {
    /// The module the call lives in.
    pub module: PathBuf,
    /// UTF-16 offset of the method name in that module.
    pub position: usize,
}

/// Everything asked of one project graph, in one round trip.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Query {
    /// The lowered `.rl` modules. Hand-written `.ts` files are not listed:
    /// the compiler reads those from disk, where they already are.
    pub modules: Vec<Module>,
    pub literals: Vec<LiteralQuery>,
    pub vals: Vec<ValQuery>,
}

/// One TypeScript diagnostic, in TypeScript's coordinates. Mapping it back
/// to a position in the `.rl` source is [`super::mapper`]'s job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Diagnostic {
    /// The file TypeScript reported it in.
    pub file: PathBuf,
    /// UTF-16 offsets in that file.
    pub start: usize,
    pub end: usize,
    /// TypeScript's error code (`2322` for `TS2322`).
    pub code: u32,
    pub message: String,
}

/// The literals a [`LiteralQuery`]'s arms fail to cover. Present only when
/// the checker found a **definite** finite union of literals; an indefinite
/// type produces no answer at all rather than a guess.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LiteralMissing {
    /// Index into [`Query::literals`].
    pub index: usize,
    pub missing: Vec<rlc::Literal>,
}

/// What a [`ValQuery`]'s method resolved to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValResolution {
    /// Index into [`Query::vals`].
    pub index: usize,
    /// The resolved method's name.
    pub method: String,
    /// The files the method is declared in. TypeScript's own lib files carry
    /// the `bundled:///libs/` prefix, which is what makes a call a built-in
    /// mutation rather than a user-defined method that shares a name.
    pub declared_in: Vec<String>,
}

/// The answers to one [`Query`].
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Answers {
    pub diagnostics: Vec<Diagnostic>,
    pub literal_missing: Vec<LiteralMissing>,
    pub val_resolutions: Vec<ValResolution>,
}

/// A source of TypeScript semantics for one project.
///
/// The project is named by its `tsconfig.json`: the compiler owns
/// configuration, module resolution and the file list, and rlc only adds the
/// modules it lowered.
pub(crate) trait TypeScriptBackend {
    /// Answers every question of `query` against the project rooted at
    /// `tsconfig`. Returns a human-readable message when the backend itself
    /// could not run — never when the *code* has errors, which are
    /// [`Answers::diagnostics`].
    fn ask(&self, tsconfig: &std::path::Path, query: &Query) -> Result<Answers, String>;
}
