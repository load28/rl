//! The TypeScript semantic backend.
//!
//! rl owns syntax and rl-only semantics; TypeScript owns TypeScript
//! semantics. Every question whose answer is a fact about a TypeScript type
//! — what a literal `match` scrutinee narrows to, what method a `val` path
//! resolves to, whether the emitted module type-checks — is asked here and
//! answered by the real compiler, never guessed by rlc.
//!
//! The layering is deliberate:
//!
//! - [`backend::TypeScriptBackend`] is the seam. rl features speak to it in
//!   rl's own terms (queries and answers), never in the compiler's.
//! - [`native`] is the only module that knows how the compiler is reached
//!   (a `tsgo` API server, driven through the host script it was built
//!   with). Its instability is contained here.
//! - [`mapper`] converts between the three coordinate spaces involved:
//!   `.rl` source bytes, emitted TypeScript bytes, and the UTF-16 code-unit
//!   offsets TypeScript itself uses.
//! - [`project`] assembles the one project graph both hand-written `.ts`
//!   and lowered `.rl` modules live in.

pub(crate) mod backend;
pub(crate) mod check;
pub(crate) mod mapper;
pub(crate) mod native;
pub(crate) mod project;
