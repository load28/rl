//! The rl standard library: `Option`/`Result` as a plain TypeScript module.
//!
//! rl itself adds no runtime — the standard library is a single TypeScript
//! module that users write out once (`rlc --emit-std <file>`) and import
//! like any other module; the import passes through the compiler untouched,
//! so the passthrough contract is unaffected. The declarations inside are
//! byte-identical to what the corresponding rl `enum`s would compile to
//! (guarded by `tests/stdlib.rs`), which is what makes `match` — and the
//! built-in exhaustiveness check below — work on these values.

/// TypeScript source of the rl standard library module: `Option<T>`,
/// `Result<T, E>` and their functional combinators (`map`, `andThen`,
/// `unwrapOr`, ...). Written out by `rlc --emit-std <file>`; see
/// `docs/reference/std.md` for the module's API.
pub const STD_SOURCE: &str = include_str!("stdlib/rl_std.ts");

/// Enums the exhaustiveness check knows about without a declaration in the
/// file: name → case tags. A file-local rl enum with the same name shadows
/// the built-in one (see [`crate::sema`]).
pub(crate) const BUILTIN_ENUMS: &[(&str, &[&str])] =
    &[("Option", &["Some", "None"]), ("Result", &["Ok", "Err"])];
