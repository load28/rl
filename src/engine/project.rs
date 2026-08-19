//! The project — the authoritative, long-lived state of one workspace.
//!
//! A [`Project`] owns everything a semantic answer depends on: which files
//! are in the graph, what each one's current text is (disk or overlay), each
//! file's cached projection, and the running TypeScript session. Consumers
//! never talk to the compiler behind it — they take a [`Snapshot`] and ask
//! about that.
//!
//! The lifecycle mirrors typescript-go's project service, sized to rl:
//! mutation happens on the project (documents open, change, close; disk
//! moves), and [`Project::update`] is the single funnel that turns the
//! current state into an immutable [`Snapshot`]. A file whose text is
//! unchanged between two snapshots keeps its projection — that is the
//! engine's incrementality, and it composes with the session's own (the
//! compiler process stays up and only changed modules are re-served).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::projection::{self, ProjectedDocument};
use super::semantics::{self, Checked};
use super::snapshot::Snapshot;
use crate::CompileError;
use crate::typescript::backend::TypeScriptBackend;
use crate::typescript::native::NativeBackend;

/// What counts as an rl source, and what counts as hand-written TypeScript.
const RL_EXTENSIONS: &[&str] = &["rl"];
const TS_EXTENSIONS: &[&str] = &["ts", "mts", "cts"];

/// A snapshot could not be taken: an rl-level error left a file impossible
/// to lower, so there is nothing to ask the checker about. Distinct from "a
/// pass ran and reported", because a caller holding a previous result (an
/// editor showing the last good sidecar) keeps it here and replaces it
/// there.
#[derive(Debug)]
pub struct Blocked {
    /// The file that failed to lower.
    pub path: PathBuf,
    /// Its rl-level error, with the file's own position.
    pub error: CompileError,
}

/// What one check is asked for, beside the snapshot.
#[derive(Debug, Clone, Copy, Default)]
pub struct CheckRequest {
    /// Emit declarations and return them (`--types`). A plain check does not.
    pub emit_declarations: bool,
    /// Report only the rl layer. The type layer is TypeScript's answer about
    /// the user's own code, and a caller that already has it from somewhere
    /// else (an editor with a live language server) would show it twice.
    pub rl_only: bool,
}

/// One workspace's compiler state: documents, projections, and the session.
#[derive(Debug)]
pub struct Project {
    pub(crate) root: PathBuf,
    tsconfig: Option<PathBuf>,
    /// The output tree a scan must not descend into (`--types`'s sidecar
    /// directory).
    out_dir: Option<PathBuf>,
    /// The inputs' `.rl` files — what a `--types` run writes. The graph is
    /// always the whole project; this only narrows emission.
    requested: HashSet<PathBuf>,
    /// The file set the first pass runs over, fixed at open: the project
    /// scan, or the inputs themselves when the scan found nothing.
    initial: Vec<PathBuf>,
    /// The project's hand-written TypeScript, listed only when there is no
    /// `tsconfig.json` to decide the program's files — see
    /// [`crate::typescript::backend::Query::sources`].
    sources: Vec<PathBuf>,
    /// Unsaved text standing in for files on disk, keyed by canonical path.
    pub(crate) overlays: HashMap<PathBuf, String>,
    /// Projections by path, kept across snapshots. An entry is reused when
    /// the file's current text equals the projected text.
    cache: HashMap<PathBuf, Arc<ProjectedDocument>>,
    backend: NativeBackend,
    next_snapshot: u64,
    /// The language-service half — the running `tsgo --lsp` conversation —
    /// started by the first editor question ([`crate::engine::language`]).
    pub(crate) service: Option<super::language::ServiceSession>,
}

impl Project {
    pub(crate) fn new(
        root: PathBuf,
        tsconfig: Option<PathBuf>,
        out_dir: Option<PathBuf>,
        collected: Vec<PathBuf>,
        initial: Vec<PathBuf>,
        sources: Vec<PathBuf>,
        backend: NativeBackend,
    ) -> Project {
        Project {
            root,
            tsconfig,
            out_dir,
            requested: collected.into_iter().collect(),
            initial,
            sources,
            overlays: HashMap::new(),
            cache: HashMap::new(),
            backend,
            next_snapshot: 0,
            service: None,
        }
    }

    /// The project root — the directory the compiler runs in.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The inputs' own `.rl` files: what an emitting pass writes for.
    pub fn requested(&self) -> &HashSet<PathBuf> {
        &self.requested
    }

    /// Substitutes `text` for `path`'s contents on disk, keyed by the
    /// canonical path. This is how an editor has the buffer it is showing
    /// checked as part of the project it belongs to: the module keeps its
    /// real path — so its imports, and the imports that name it, resolve
    /// exactly as they do on disk — and only its text is the unsaved one.
    pub fn open_document(&mut self, path: PathBuf, text: String) {
        self.overlays.insert(path, text);
    }

    /// Replaces an open document's text. The next [`Project::update`] sees
    /// the new text; snapshots already taken keep the old one.
    pub fn update_document(&mut self, path: PathBuf, text: String) {
        self.overlays.insert(path, text);
    }

    /// Closes an open document: the file's text is the disk's again.
    pub fn close_document(&mut self, path: &Path) {
        self.overlays.remove(path);
    }

    /// Every `.rl` file of the project, as sorted absolute paths — the
    /// compiler resolves modules by absolute path, and so must the modules
    /// rlc adds. Scanned fresh so a file created since the last call is
    /// seen.
    pub fn scan(&self) -> std::io::Result<Vec<PathBuf>> {
        project_sources(&self.root, self.out_dir.as_deref(), RL_EXTENSIONS)
    }

    /// The file set the first pass runs over, decided when the project was
    /// opened: the project scan, or — when that found nothing (inputs
    /// outside the root) — the inputs themselves.
    pub fn initial_files(&self) -> Vec<PathBuf> {
        self.initial.clone()
    }

    /// Takes a snapshot of `files` as they are now: overlay text where a
    /// document is open, disk text otherwise. A file whose text is unchanged
    /// since the last snapshot keeps its projection; the rest are
    /// re-projected. The first file that fails an rl-level check blocks the
    /// snapshot — rl diagnostics come first and are never delegated.
    pub fn update(&mut self, files: &[PathBuf]) -> Result<Snapshot, Box<Blocked>> {
        let mut projected = Vec::with_capacity(files.len());
        let mut cache = HashMap::with_capacity(files.len());
        for file in files {
            let text = match self.overlays.get(file) {
                Some(text) => text.clone(),
                None => std::fs::read_to_string(file).map_err(|e| {
                    Box::new(Blocked {
                        path: file.clone(),
                        error: CompileError {
                            message: format!("cannot read: {e}"),
                            filename: Some(file.display().to_string()),
                            line: 0,
                            col: 0,
                        },
                    })
                })?,
            };
            let doc = match self.cache.get(file) {
                Some(cached) if cached.source == text => cached.clone(),
                _ => Arc::new(ProjectedDocument::project(file, text).map_err(|error| {
                    Box::new(Blocked {
                        path: file.clone(),
                        error,
                    })
                })?),
            };
            cache.insert(file.clone(), doc.clone());
            projected.push(doc);
        }
        // Entries for files that left the project go with the old map; a
        // blocked update above leaves the previous cache intact instead, so
        // the files that were fine keep their projections.
        self.cache = cache;
        self.next_snapshot += 1;
        Ok(Snapshot {
            id: self.next_snapshot,
            files: projected,
        })
    }

    /// Checks a snapshot: asks the running compiler about it and returns
    /// diagnostics at `.rl` positions — and the emitted declarations, when
    /// the request wants them. The session persists across calls; only what
    /// changed since the last ask travels.
    pub fn check(&self, snapshot: &Snapshot, request: &CheckRequest) -> Result<Checked, String> {
        let (mut query, probes) = projection::assemble(snapshot.files(), &self.root, &self.sources);
        query.emit_declarations = request.emit_declarations;
        let answers = self
            .backend
            .ask(self.tsconfig.as_deref(), &self.root, &query)?;
        let declarations = if request.emit_declarations {
            semantics::match_declarations(snapshot, &answers, &self.root, &self.requested)
        } else {
            Default::default()
        };
        Ok(Checked {
            diagnostics: semantics::report(snapshot, &answers, &probes, request.rl_only),
            declarations,
        })
    }
}

/// Every file of the project with one of `extensions`, as absolute paths.
/// `node_modules`, dot directories and the output tree are skipped — nothing
/// there is a source.
pub(crate) fn project_sources(
    root: &Path,
    out_dir: Option<&Path>,
    extensions: &[&str],
) -> std::io::Result<Vec<PathBuf>> {
    let out = out_dir.and_then(|d| d.canonicalize().ok());
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if Some(&dir) == out.as_ref() {
            continue;
        }
        for entry in std::fs::read_dir(&dir)? {
            let path = entry?.path();
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if name.starts_with('.') || name == "node_modules" {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
            } else if path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| extensions.contains(&e))
            {
                files.push(path.canonicalize()?);
            }
        }
    }
    files.sort();
    Ok(files)
}

/// The nearest `tsconfig.json` at or above the inputs' common directory.
pub(crate) fn find_tsconfig(files: &[PathBuf]) -> Option<PathBuf> {
    let mut dir = files.first()?.parent()?.to_path_buf();
    loop {
        let candidate = dir.join("tsconfig.json");
        if candidate.is_file() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Collects sources under `entry` the way every rlc mode does: a file is
/// taken as it is; a directory is walked recursively, skipping
/// dot-directories and `node_modules`, taking `.rl` — and, when
/// `include_ts` is set, hand-written TypeScript (`.ts`/`.mts`/`.cts`) too.
pub fn collect_sources(
    entry: &Path,
    include_ts: bool,
    out: &mut Vec<PathBuf>,
) -> std::io::Result<()> {
    let meta = std::fs::metadata(entry)?;
    if meta.is_file() {
        out.push(entry.to_path_buf());
        return Ok(());
    }
    if meta.is_dir() {
        let mut children: Vec<PathBuf> = std::fs::read_dir(entry)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .collect();
        children.sort();
        for child in children {
            let meta = std::fs::metadata(&child)?;
            if meta.is_dir() {
                // Dot-directories (.git, .rl-build, .rl-types, ...) and
                // node_modules are never sources; descending into them
                // would pull generated or vendored TypeScript into the
                // build — or the cache tree into itself.
                let skip = child.file_name().is_some_and(|name| {
                    let name = name.to_string_lossy();
                    name.starts_with('.') || name == "node_modules"
                });
                if !skip {
                    collect_sources(&child, include_ts, out)?;
                }
            } else if meta.is_file()
                && child.extension().is_some_and(|e| {
                    e == "rl" || (include_ts && TS_EXTENSIONS.iter().any(|ts| *ts == e))
                })
            {
                out.push(child);
            }
        }
    }
    Ok(())
}

/// The `.rl` files of `inputs`, as absolute paths.
pub(crate) fn collect_rl(inputs: &[String]) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for input in inputs {
        collect_sources(Path::new(input), false, &mut files)?;
    }
    files
        .into_iter()
        .filter(|f| f.extension().is_some_and(|e| e == "rl"))
        .map(|f| f.canonicalize())
        .collect()
}
