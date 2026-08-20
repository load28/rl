//! The rl language engine — one authoritative project state for every
//! consumer.
//!
//! rlc's batch build compiles files one at a time and forgets them; that
//! mode needs no engine (the same way `tsc` runs without tsserver). This
//! module is for everything else: type checking, watching, editors, and
//! external tooling — the consumers that need a **live** project whose
//! answers stay consistent while files change.
//!
//! The shape follows typescript-go's project service, sized to rl:
//!
//! - an [`Engine`] discovers the toolchain and opens projects;
//! - a [`Project`] is the long-lived, mutable state of one workspace —
//!   documents (disk and unsaved overlays), cached projections, and the
//!   running TypeScript session;
//! - a [`Snapshot`] is the project at one moment, immutable; every semantic
//!   request runs against a snapshot, so a request started before an edit
//!   still answers about a consistent state;
//! - the projection ([`ProjectedDocument`]) — rl source, generated
//!   TypeScript, byte-exact mappings, probes — is the engine's own layer,
//!   the one thing rl has that a plain TypeScript engine does not.
//!
//! Results come back in rl's own vocabulary ([`Diagnostic`], [`Checked`]):
//! TypeScript's unstable API surface stays behind
//! [`crate::typescript::backend`] and never reaches a consumer.
//!
//! ```no_run
//! use rlc::engine::{CheckRequest, Engine, ProjectOptions};
//!
//! let engine = Engine::new(None);
//! let mut project = engine
//!     .open_project(&["src".to_string()], &ProjectOptions::default())
//!     .unwrap();
//! let files = project.initial_files();
//! let snapshot = project.update(&files).unwrap();
//! let checked = project.check(&snapshot, &CheckRequest::default()).unwrap();
//! for d in &checked.diagnostics {
//!     eprintln!("{}: {}", d.path.display(), d.message);
//! }
//! ```

mod language;
mod project;
mod projection;
mod semantics;
mod snapshot;
mod tokens;

pub use language::{
    CompletionAnswer, CompletionDetail, CompletionItem, HoverInfo, Location, Position,
    RENAME_PLACEHOLDER, Range, Reference, RenameEdit, ServiceDiagnostic, Signature, SignatureHelp,
    SignatureParameter,
};
pub use project::{Blocked, CheckRequest, Project, collect_sources};
pub use projection::ProjectedDocument;
pub use semantics::{Checked, Declarations, Diagnostic, ModuleDeclaration};
pub use snapshot::Snapshot;
pub use tokens::{SemanticToken, SemanticTokenKind, semantic_tokens};

use std::path::PathBuf;

use crate::typescript::native::NativeBackend;

/// Process-wide entry point: toolchain discovery and project creation.
#[derive(Debug, Default)]
pub struct Engine {
    /// The `node` binary that runs the TypeScript host, or `None` for the
    /// `node` on PATH.
    node: Option<PathBuf>,
}

/// How a project is opened, beside its inputs.
#[derive(Debug, Default)]
pub struct ProjectOptions {
    /// The `tsconfig.json` to check against, when named (`--project`).
    /// Otherwise the nearest one at or above the inputs decides, and a
    /// workspace without one still works — the compiler infers a project.
    pub tsconfig: Option<PathBuf>,
    /// A directory of generated output (the sidecar tree) that scans must
    /// not descend into.
    pub out_dir: Option<PathBuf>,
}

impl Engine {
    /// An engine that runs its TypeScript host with `node` (or the PATH's
    /// `node` when `None`). Nothing is started until a project's first
    /// question.
    pub fn new(node: Option<PathBuf>) -> Engine {
        Engine { node }
    }

    /// Opens the project `inputs` belong to.
    ///
    /// The project's own configuration is what the checker runs with: the
    /// named `tsconfig` or the nearest one above the inputs. The graph is
    /// the project's, not the input list's — every `.rl` file under the
    /// root joins it, because a named input may import one that was not
    /// named. What was named decides only what an emitting pass writes.
    ///
    /// The error is a ready-to-print sentence: nothing collected, an
    /// unreadable input, or no TypeScript toolchain.
    pub fn open_project(
        &self,
        inputs: &[String],
        options: &ProjectOptions,
    ) -> Result<Project, String> {
        let collected = match project::collect_rl(inputs) {
            Ok(files) if files.is_empty() => return Err("no .rl sources found".to_string()),
            Ok(files) => files,
            Err(e) => return Err(e.to_string()),
        };
        let (tsconfig, root) = identity_of(&collected, options);
        // The graph is the project's, not the command line's: every `.rl`
        // file under the project root joins the first pass, because a named
        // input may import one that was not named.
        let initial = match project::project_sources(&root, options.out_dir.as_deref(), &["rl"]) {
            Ok(all) if !all.is_empty() => all,
            Ok(_) => collected.clone(),
            Err(e) => return Err(e.to_string()),
        };
        let backend = NativeBackend::new(self.node.clone(), &root)?;
        // With a configuration the project's own `include` decides which
        // hand-written files are in the program; without one, they have to
        // be listed or a `.ts` nothing imports is never checked.
        let sources = match tsconfig {
            Some(_) => Vec::new(),
            None => {
                project::project_sources(&root, options.out_dir.as_deref(), &["ts", "mts", "cts"])
                    .unwrap_or_default()
            }
        };
        Ok(Project::new(
            root,
            tsconfig,
            options.out_dir.clone(),
            collected,
            initial,
            sources,
            backend,
        ))
    }

    /// The identity `inputs` resolve to — the `(tsconfig, root)` pair a
    /// project is opened as. Two input sets with the same identity describe
    /// the same project, which is what a server keys its live sessions by.
    pub fn project_identity(
        inputs: &[String],
        options: &ProjectOptions,
    ) -> Result<(Option<PathBuf>, PathBuf), String> {
        let collected = match project::collect_rl(inputs) {
            Ok(files) if files.is_empty() => return Err("no .rl sources found".to_string()),
            Ok(files) => files,
            Err(e) => return Err(e.to_string()),
        };
        Ok(identity_of(&collected, options))
    }
}

/// The `(tsconfig, root)` the collected inputs belong to.
fn identity_of(collected: &[PathBuf], options: &ProjectOptions) -> (Option<PathBuf>, PathBuf) {
    let tsconfig = options
        .tsconfig
        .clone()
        .or_else(|| project::find_tsconfig(collected))
        .map(|path| path.canonicalize().unwrap_or(path));
    let root = match &tsconfig {
        Some(path) => path
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .to_path_buf(),
        // No configuration: the sources' own directories are the project.
        None => collected
            .first()
            .and_then(|f| f.parent())
            .unwrap_or(std::path::Path::new("."))
            .to_path_buf(),
    };
    (tsconfig, root)
}
