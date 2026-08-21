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

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::projection::{self, ProjectedDocument};
use super::semantics::{self, Checked, FileSemantics};
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
    /// The TypeScript backend — or why there is none (no toolchain found).
    /// A project without one still opens and still answers the rl layer;
    /// only the typed facts degrade to unknown ([`Project::check`]).
    backend: Result<NativeBackend, String>,
    /// Cross-snapshot semantic cache, keyed per file by (content hash,
    /// imported declarations). A change to a dependency's body leaves an
    /// importer's entry valid; a change to its exported declarations
    /// invalidates exactly the importers — the invalidation boundary of
    /// `docs/design/compiler-core.md` §11.
    semantics_cache: RefCell<HashMap<PathBuf, CachedSemantics>>,
    /// How many per-file semantic computations the cache answered without
    /// recomputing, over the project's lifetime — observability for the
    /// invalidation contract (and its tests).
    semantic_cache_hits: Cell<usize>,
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
        backend: Result<NativeBackend, String>,
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
            semantics_cache: RefCell::new(HashMap::new()),
            semantic_cache_hits: Cell::new(0),
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
                            end_line: 0,
                            end_col: 0,
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

    /// How many per-file semantic computations the cross-snapshot cache
    /// answered without recomputing. A dependency's body-only change keeps
    /// its importers' entries; an exported-declaration change invalidates
    /// them — this counter is how that contract is observed and tested.
    pub fn semantic_cache_hits(&self) -> usize {
        self.semantic_cache_hits.get()
    }

    /// The per-file semantics of a snapshot, served from the
    /// cross-snapshot cache where the key — (content, imported
    /// declarations) — still matches.
    fn file_semantics(&self, snapshot: &Snapshot) -> HashMap<PathBuf, Arc<FileSemantics>> {
        let files = snapshot.files();
        let mut out = HashMap::with_capacity(files.len());
        let mut cache = self.semantics_cache.borrow_mut();
        for file in files {
            let externs = semantics::externs_of(files, file);
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            file.source.hash(&mut hasher);
            let source_hash = hasher.finish();
            if let Some(cached) = cache.get(&file.source_path)
                && cached.source_hash == source_hash
                && cached.value.externs == externs
            {
                self.semantic_cache_hits
                    .set(self.semantic_cache_hits.get() + 1);
                out.insert(file.source_path.clone(), cached.value.clone());
                continue;
            }
            let analyses = crate::pattern_analyses(&file.source, &externs);
            let value = Arc::new(FileSemantics { externs, analyses });
            cache.insert(
                file.source_path.clone(),
                CachedSemantics {
                    source_hash,
                    value: value.clone(),
                },
            );
            out.insert(file.source_path.clone(), value);
        }
        out
    }

    /// Checks a snapshot: asks the running compiler about it and returns
    /// diagnostics at `.rl` positions — and the emitted declarations, when
    /// the request wants them. The session persists across calls; only what
    /// changed since the last ask travels.
    pub fn check(&self, snapshot: &Snapshot, request: &CheckRequest) -> Result<Checked, String> {
        let semantics = self.file_semantics(snapshot);
        let (mut query, probes) = projection::assemble(snapshot.files(), &self.root, &self.sources);
        query.emit_declarations = request.emit_declarations;
        // A backend that cannot run removes the typed facts, not the pass:
        // every typed answer degrades to unknown and the rl layer still
        // reports in full (`docs/design/compiler-core.md` §7).
        let (answers, backend_error) = match &self.backend {
            Ok(backend) => match backend.ask(self.tsconfig.as_deref(), &self.root, &query) {
                Ok(answers) => (answers, None),
                Err(error) => (Default::default(), Some(error)),
            },
            Err(missing) => (Default::default(), Some(missing.clone())),
        };
        let declarations = if request.emit_declarations && backend_error.is_none() {
            semantics::match_declarations(snapshot, &answers, &self.root, &self.requested)
        } else {
            Default::default()
        };
        Ok(Checked {
            diagnostics: semantics::report(
                snapshot,
                &answers,
                &probes,
                request.rl_only,
                &semantics,
            ),
            declarations,
            backend_error,
        })
    }
}

/// One cached [`FileSemantics`] with the half of its key the value does
/// not carry (the content hash; the externs are compared on the value).
#[derive(Debug)]
struct CachedSemantics {
    source_hash: u64,
    value: Arc<FileSemantics>,
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
