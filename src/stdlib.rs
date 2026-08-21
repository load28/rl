//! The rl standard library: `Option`/`Result` as a plain TypeScript module.
//!
//! rl itself adds no runtime — the standard library is a single TypeScript
//! module that users write out once (`rlc --emit-std <file>`) and import
//! like any other module; the import passes through the compiler untouched,
//! so the passthrough contract is unaffected. The values inside are
//! byte-identical to what the corresponding rl `enum`s would compile to
//! (guarded by `tests/stdlib.rs`), which is what makes `match` — and the
//! built-in exhaustiveness check below — work on them. `Result`'s two
//! constructors are the one deliberate deviation: they are typed by the
//! variant each builds (`Ok<T>` / `Err<E>`) rather than by the whole
//! `Result<T, E>`, so a function with several `try`s infers a union of the
//! real error types instead of `unknown`.

/// TypeScript source of the rl standard library module: `Option<T>`,
/// `Result<T, E>` and their functional combinators (`map`, `andThen`,
/// `unwrapOr`, ...). Written out by `rlc --emit-std <file>`; run
/// `rlc help std` for the module's user-facing API.
pub const STD_SOURCE: &str = include_str!("stdlib/rl_std.ts");

/// The bare specifier a `.rl` file uses to reach [`STD_SOURCE`].
///
/// It is bare rather than relative on purpose: a relative path would have
/// to name a file that only exists after generation, and TypeScript's
/// `paths` — the mapping an editor needs — does not apply to relative
/// specifiers. The `rlc` CLI writes the module into the output tree and
/// rewrites this specifier to point at it; a bundler plugin can serve it
/// as a virtual module instead.
pub const STD_SPECIFIER: &str = "@rl/std";

// The built-in enums a file gets without declaring them (`Option`,
// `Result`) live in [`crate::analysis`], with their payload fields: one
// declaration table serves both exhaustiveness and the editor's types, so
// there is no tag-only copy of them here.
