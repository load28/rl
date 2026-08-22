//! TypeScript backend.
//!
//! Semantic lowering produces a validated [`crate::core_ir::CoreFile`].
//! Target lowering consumes only that Core IR, the resolved HIR held by
//! [`crate::analysis::SemanticFile`], and opaque source spans. The printer
//! then flattens the mapping-aware structured writer into TypeScript text.
//! Parser AST types are deliberately absent from this boundary.

mod core;
mod rope;

pub(crate) use core::emit_with_map;
