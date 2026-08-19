use std::path::PathBuf;

/// One module handed to the host: the text a `.rl` file compiles to, keyed by
/// the path it would occupy on disk.
pub(crate) struct VirtualModule {
    /// Path relative to the project root (`src/engine.ts`).
    pub(crate) path: String,
    pub(crate) text: String,
    /// The `.rl` file it came from; `None` for the standard library.
    pub(crate) source: Option<PathBuf>,
}
