//! The rl standard library as tree-shakeable TypeScript modules.
//!
//! rl itself adds no runtime — the standard library is three TypeScript
//! modules that the CLI materializes as needed and bundler adapters expose
//! virtually; imports otherwise pass through the compiler untouched,
//! so the passthrough contract is unaffected. The values inside are
//! byte-identical to what the corresponding rl `enum`s would compile to
//! (guarded by `tests/stdlib.rs`), which is what makes `match` — and the
//! built-in exhaustiveness check below — work on them. `Result`'s two
//! constructors are the one deliberate deviation: they are typed by the
//! variant each builds (`Ok<T>` / `Err<E>`) rather than by the whole
//! `TResult<T, E>`, so a function with several `try`s infers a union of the
//! real error types instead of `unknown`.

/// TypeScript source of the `@rl/std` type-only entry point.
pub const STD_TYPES_SOURCE: &str = include_str!("stdlib/types.ts");

/// TypeScript source of the `@rl/std/option` runtime module.
pub const STD_OPTION_SOURCE: &str = include_str!("stdlib/option.ts");

/// TypeScript source of the `@rl/std/result` runtime module.
pub const STD_RESULT_SOURCE: &str = include_str!("stdlib/result.ts");

/// The bare specifier a `.rl` file uses for standard-library types.
///
/// It is bare rather than relative on purpose: a relative path would have
/// to name a file that only exists after generation, and TypeScript's
/// `paths` — the mapping an editor needs — does not apply to relative
/// specifiers. The `rlc` CLI writes the module into the output tree and
/// rewrites this specifier to point at it; a bundler plugin can serve it
/// as a virtual module instead.
pub const STD_SPECIFIER: &str = "@rl/std";

/// One physical module of the standard-library package.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StdModule {
    /// `@rl/std`, the type-only entry point.
    Types,
    /// `@rl/std/option`, the Option constructors and combinators.
    Option,
    /// `@rl/std/result`, the Result constructors and combinators.
    Result,
}

impl StdModule {
    /// All modules in deterministic materialization order.
    pub const ALL: [StdModule; 3] = [StdModule::Types, StdModule::Option, StdModule::Result];

    /// The bare module specifier users write.
    pub const fn specifier(self) -> &'static str {
        match self {
            StdModule::Types => STD_SPECIFIER,
            StdModule::Option => "@rl/std/option",
            StdModule::Result => "@rl/std/result",
        }
    }

    /// The module's file name inside the generated `rl/` directory.
    pub const fn file_name(self) -> &'static str {
        match self {
            StdModule::Types => "index.ts",
            StdModule::Option => "option.ts",
            StdModule::Result => "result.ts",
        }
    }

    /// The module's embedded TypeScript source.
    pub const fn source(self) -> &'static str {
        match self {
            StdModule::Types => STD_TYPES_SOURCE,
            StdModule::Option => STD_OPTION_SOURCE,
            StdModule::Result => STD_RESULT_SOURCE,
        }
    }

    pub(crate) fn from_specifier(specifier: &[u8]) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|module| specifier == module.specifier().as_bytes())
    }
}

/// Per-module standard-library rewrites supplied by a build adapter.
#[derive(Debug, Clone, Copy, Default)]
pub struct StdImports<'a> {
    /// Replacement for `@rl/std`.
    pub types: Option<&'a str>,
    /// Replacement for `@rl/std/option`.
    pub option: Option<&'a str>,
    /// Replacement for `@rl/std/result`.
    pub result: Option<&'a str>,
}

impl<'a> StdImports<'a> {
    pub(crate) const fn get(self, module: StdModule) -> Option<&'a str> {
        match module {
            StdModule::Types => self.types,
            StdModule::Option => self.option,
            StdModule::Result => self.result,
        }
    }
}

// The built-in enums a file gets without declaring them (`Option`,
// `Result`) live in [`crate::analysis`], with their payload fields: one
// declaration table serves both exhaustiveness and the editor's types, so
// there is no tag-only copy of them here.
