//! `rlc --check-types` / `rlc --types` — the whole pipeline over one
//! project graph.
//!
//! Lowers every `.rl` input to ordinary TypeScript, hands the lowered modules
//! to the real TypeScript compiler as part of the user's own project, and
//! reports what comes back **at positions in the `.rl` source**.
//!
//! The two diagnostic layers stay distinguishable. An rl-level error (a
//! duplicate case, a misplaced wildcard, a mutation through a `val` binding)
//! is rlc's and reads exactly as it does under `--check`; a type error is
//! TypeScript's and is marked `ts(CODE):`. Both name a place in the file the
//! user wrote, in rlc's own diagnostic form on stderr — stdout stays free
//! for the modes that pipe (`docs/reference/errors.md`).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use super::backend::{Resolution, TypeScriptBackend};
use super::native::NativeBackend;
use super::project;

/// What counts as an rl source, and what counts as hand-written TypeScript.
const RL_EXTENSIONS: &[&str] = &["rl"];
const TS_EXTENSIONS: &[&str] = &["ts", "mts", "cts"];

/// Runs the check. Returns failure when anything was reported, exactly like
/// `--check`.
pub(crate) fn run(
    inputs: &[String],
    project_arg: Option<&Path>,
    node: Option<&Path>,
    out_dir: Option<&Path>,
    emit: bool,
    watch: bool,
) -> ExitCode {
    let files = match collect(inputs) {
        Ok(files) if files.is_empty() => {
            eprintln!("rlc: no .rl sources found");
            return ExitCode::FAILURE;
        }
        Ok(files) => files,
        Err(e) => {
            eprintln!("rlc: {e}");
            return ExitCode::FAILURE;
        }
    };

    // The project's own configuration is what the checker runs with. A
    // workspace without one still works: the compiler infers a project for
    // the modules, exactly as an editor does for a loose file.
    let tsconfig = project_arg
        .map(PathBuf::from)
        .or_else(|| find_tsconfig(&files))
        .map(|path| path.canonicalize().unwrap_or(path));

    // The graph is the project's, not the command line's: every `.rl` file
    // under the project root is lowered, because a named input may import
    // one that was not named. What was named decides only what is written.
    let root = match &tsconfig {
        Some(path) => path.parent().unwrap_or(Path::new(".")).to_path_buf(),
        // No configuration: the sources' own directories are the project.
        None => files
            .first()
            .and_then(|f| f.parent())
            .unwrap_or(Path::new("."))
            .to_path_buf(),
    };
    let requested: std::collections::HashSet<PathBuf> = files.iter().cloned().collect();
    let files = match project_sources(&root, out_dir, RL_EXTENSIONS) {
        Ok(all) if !all.is_empty() => all,
        Ok(_) => files,
        Err(e) => {
            eprintln!("rlc: {e}");
            return ExitCode::FAILURE;
        }
    };
    let backend = match NativeBackend::new(node.map(PathBuf::from), &root) {
        Ok(backend) => backend,
        Err(e) => {
            eprintln!("rlc: {e}");
            return ExitCode::FAILURE;
        }
    };

    // With a configuration the project's own `include` decides which
    // hand-written files are in the program; without one, they have to be
    // named or a `.ts` nothing imports is never checked.
    let sources = match tsconfig {
        Some(_) => Vec::new(),
        None => project_sources(&root, out_dir, TS_EXTENSIONS).unwrap_or_default(),
    };

    let pass = Pass {
        backend: &backend,
        requested: &requested,
        inputs,
        tsconfig: tsconfig.as_deref(),
        root: &root,
        out_dir,
        emit,
        sources: &sources,
    };

    if watch {
        return watch_loop(&pass, &root, out_dir);
    }

    match pass.once(&files) {
        // The exit code answers "did the check pass?", in every mode — a
        // `--types` run still *writes* its sidecars when the code has type
        // errors (a stale sidecar is worse than one built from erroring
        // code), but it says so.
        //
        // 2 is the third answer: the check could not run, so nothing was
        // written. A caller holding a previous result — an editor showing
        // the last good sidecar — keeps it on 2 and replaces it on 1.
        Ok(report) if report.blocked => ExitCode::from(2),
        Ok(report) if report.reported == 0 => ExitCode::SUCCESS,
        Ok(_) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("rlc: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Everything one pass needs, so a watch can run many of them against one
/// running compiler.
struct Pass<'a> {
    backend: &'a NativeBackend,
    /// The inputs' `.rl` files — what gets written.
    requested: &'a std::collections::HashSet<PathBuf>,
    inputs: &'a [String],
    tsconfig: Option<&'a Path>,
    root: &'a Path,
    out_dir: Option<&'a Path>,
    emit: bool,
    /// The project's hand-written TypeScript, listed only when there is no
    /// `tsconfig.json` to decide the program's files — see [`Query::sources`].
    sources: &'a [PathBuf],
}

/// What one pass found.
struct Report {
    /// How many diagnostics were printed. Zero is the only passing result.
    reported: usize,
    /// Whether the pass could not run at all: an rl-level error left nothing
    /// to lower, so nothing was asked and nothing was written. Distinct from
    /// "ran and reported", because a caller holding a previous result should
    /// keep it in the first case and replace it in the second.
    blocked: bool,
}

impl Pass<'_> {
    /// Lowers `files`, asks the compiler about them and reports what comes
    /// back.
    fn once(&self, files: &[PathBuf]) -> Result<Report, String> {
        let lowered = match project::lower(files) {
            Ok(lowered) => lowered,
            Err((file, error)) => {
                eprintln!(
                    "rlc: {}:{}:{}: {}",
                    shown(&file),
                    error.line,
                    error.col,
                    error.message
                );
                return Ok(Report {
                    reported: 1,
                    blocked: true,
                });
            }
        };

        let (mut query, probes) = project::query(&lowered, self.root, self.sources);
        query.emit_declarations = self.emit;
        let answers = self.backend.ask(self.tsconfig, self.root, &query)?;

        // The declarations the compiler emitted for the lowered modules, laid
        // out under `-o` the way the sources are laid out under the project.
        if self.emit {
            write_declarations(
                &answers.declarations,
                &lowered,
                self.requested,
                self.inputs,
                self.out_dir,
                self.root,
            )
            .map_err(|e| e.to_string())?;
        }

        let mut reported = 0usize;

        // TypeScript's own diagnostics, at the position in the `.rl` file the
        // offending code was written at.
        for diagnostic in &answers.diagnostics {
            reported += 1;
            let Some(file) = lowered.iter().find(|f| f.module_path == diagnostic.file) else {
                // A hand-written file: TypeScript's own coordinates already name
                // a file the user can open, so they are used as they are.
                eprintln!(
                    "rlc: {}: ts({}): {}",
                    shown(&diagnostic.file),
                    diagnostic.code,
                    diagnostic.message,
                );
                continue;
            };
            match project::diagnostic_source_offset(file, diagnostic.start) {
                Some((offset, exact)) => {
                    let (line, col) = rlc::line_col(&file.source, offset);
                    eprintln!(
                        "rlc: {}:{}:{}: ts({}): {}{}",
                        shown(&file.source_path),
                        line,
                        col,
                        diagnostic.code,
                        diagnostic.message,
                        // Glue is not the user's code: by the error-layer
                        // contract rlc's output must not draw type errors, so
                        // say where it came from rather than pinning it on the
                        // line the position landed near.
                        if exact {
                            ""
                        } else {
                            " (in code rlc generated for this construct)"
                        },
                    );
                }
                None => eprintln!(
                    "rlc: {}: ts({}): {}",
                    shown(&file.source_path),
                    diagnostic.code,
                    diagnostic.message,
                ),
            }
        }

        // Literal-match exhaustiveness, decided by the type TypeScript computes
        // at the scrutinee — narrowing included.
        for missing in &answers.literal_missing {
            let Some(anchor) = probes.literals.get(missing.index) else {
                continue;
            };
            let Some(file) = lowered.iter().find(|f| f.source_path == anchor.source_path) else {
                continue;
            };
            let (line, col) = rlc::line_col(&file.source, anchor.offset);
            reported += 1;
            eprintln!(
                "rlc: {}:{}:{}: match on literal union is not exhaustive: missing {} \
                 (add the missing arms or a final `_` arm)",
                shown(&file.source_path),
                line,
                col,
                missing
                    .missing
                    .iter()
                    .map(display_literal)
                    .collect::<Vec<_>>()
                    .join(", "),
            );
        }

        // Tag exhaustiveness, from the same narrowed type.
        for missing in &answers.tag_missing {
            let Some(anchor) = probes.tags.get(missing.index) else {
                continue;
            };
            let Some(file) = lowered.iter().find(|f| f.source_path == anchor.source_path) else {
                continue;
            };
            let (line, col) = rlc::line_col(&file.source, anchor.offset);
            reported += 1;
            eprintln!(
                "rlc: {}:{}:{}: match is not exhaustive: missing {} \
                 (add the missing arms or a final `_` arm)",
                shown(&file.source_path),
                line,
                col,
                missing
                    .missing
                    .iter()
                    .map(|t| format!("{t:?}"))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
        }

        // `val`: two resolutions decide, and rlc guesses neither of them.
        //
        // 1. Which binding a path is rooted at — the root identifier and the
        //    binding's declaration are the same binding when they are the same
        //    symbol. Shadowing, redeclaration and destructuring come out right
        //    because this is TypeScript's own resolution, not a model of it.
        // 2. For a method call, whether the method is a built-in — declared in
        //    TypeScript's own lib files. A user-defined method that shares the
        //    name is not, and anything unresolved is left alone.
        let symbols: std::collections::HashMap<usize, &Resolution> =
            answers.resolutions.iter().map(|r| (r.index, r)).collect();
        let val_symbols: std::collections::HashSet<i64> = probes
            .val_bindings
            .iter()
            .filter_map(|i| symbols.get(i).map(|r| r.id))
            .collect();

        for mutation in &probes.mutations {
            let Some(root) = symbols.get(&mutation.root) else {
                continue; // unresolved — never a verdict
            };
            if !val_symbols.contains(&root.id) {
                continue; // not this binding, whatever it is called
            }
            if let Some(method) = mutation.method {
                match symbols.get(&method) {
                    Some(resolution) if resolution.builtin => {}
                    // A user-defined method, or one the checker could not
                    // resolve: rl says nothing.
                    _ => continue,
                }
            }
            let Some(file) = lowered
                .iter()
                .find(|f| f.source_path == mutation.anchor.source_path)
            else {
                continue;
            };
            let (line, col) = rlc::line_col(&file.source, mutation.anchor.offset);
            reported += 1;
            match &mutation.method_name {
                // The built-in itself is not named: the compiler answered
                // "this method is one of TypeScript's own", which is the
                // verdict — not which interface declares it.
                Some(method) => eprintln!(
                    "rlc: {}:{}:{}: cannot call mutating method `{}` through val binding `{}` \
                     (the binding is declared with `val`, so every access path from it is \
                     read-only)",
                    shown(&file.source_path),
                    line,
                    col,
                    method,
                    mutation.name,
                ),
                None => eprintln!(
                    "rlc: {}:{}:{}: cannot mutate through val binding `{}` \
                     (the binding is declared with `val`, so every access path \
                     from it is read-only)",
                    shown(&file.source_path),
                    line,
                    col,
                    mutation.name,
                ),
            }
        }

        // The function boundary: a `val` binding may only be handed to a
        // parameter that is itself `val`. Which binding the argument names
        // is the same symbol question the mutations above ask.
        for pass in &probes.passes {
            let Some(root) = symbols.get(&pass.root) else {
                continue;
            };
            if !val_symbols.contains(&root.id) {
                continue;
            }
            let Some(file) = lowered
                .iter()
                .find(|f| f.source_path == pass.anchor.source_path)
            else {
                continue;
            };
            let (line, col) = rlc::line_col(&file.source, pass.anchor.offset);
            reported += 1;
            eprintln!(
                "rlc: {}:{}:{}: cannot pass val binding `{}` to mutable parameter {} of \
                 `{}` (the parameter is not declared with `val`, so the function may mutate \
                 through it)",
                shown(&file.source_path),
                line,
                col,
                pass.name,
                pass.param,
                pass.callee,
            );
        }

        Ok(Report {
            reported,
            blocked: false,
        })
    }
}

/// Re-checks on every change, against the compiler started for the first
/// pass. The project is opened once and updated after that, which is what
/// makes the wait a re-check rather than a cold start.
fn watch_loop(pass: &Pass<'_>, root: &Path, out_dir: Option<&Path>) -> ExitCode {
    let mut stamps: std::collections::HashMap<PathBuf, std::time::SystemTime> =
        std::collections::HashMap::new();
    let mut first = true;
    loop {
        let files = match project_sources(root, out_dir, RL_EXTENSIONS) {
            Ok(files) => files,
            // A file can disappear mid-edit; keep watching rather than
            // tearing the session down.
            Err(_) => {
                std::thread::sleep(crate::WATCH_INTERVAL);
                continue;
            }
        };
        let current: std::collections::HashMap<PathBuf, std::time::SystemTime> = files
            .iter()
            .map(|file| {
                let stamp = std::fs::metadata(file)
                    .and_then(|meta| meta.modified())
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                (file.clone(), stamp)
            })
            .collect();

        if first || current != stamps {
            let started = std::time::Instant::now();
            match pass.once(&files) {
                Ok(report) => eprintln!(
                    "rlc: {} file(s), {} reported in {} ms — watching",
                    files.len(),
                    report.reported,
                    started.elapsed().as_millis()
                ),
                Err(e) => eprintln!("rlc: {e}"),
            }
        }
        if first {
            eprintln!("rlc: watching {} file(s) — Ctrl-C to stop", files.len());
            first = false;
        }
        stamps = current;
        std::thread::sleep(crate::WATCH_INTERVAL);
    }
}

/// Writes the emitted declarations under `out_dir`, mirroring their layout
/// under the project root — never beside the sources.
///
/// A declaration emitted for a lowered module becomes an **editor sidecar**:
/// `src/token.rl.d.ts` plus a `.d.ts.map` whose `sources` is the `.rl` file,
/// so "go to definition" lands in what the user wrote rather than in a
/// declaration. The body is the compiler's; only the map is rlc's, and it is
/// built by the same [`rlc::build_sidecar`] the `--sidecar` mode uses.
fn write_declarations(
    declarations: &[super::backend::Declaration],
    lowered: &[project::Lowered],
    requested: &std::collections::HashSet<PathBuf>,
    inputs: &[String],
    out_dir: Option<&Path>,
    root: &Path,
) -> std::io::Result<()> {
    // The standard library's own declarations, so a consumer running plain
    // tsc can point `@rl/std` at them (`paths`). It is a module of the
    // project like any other, so the compiler emitted it too — it just has
    // no `.rl` source to sit beside, and lands under the sidecar directory
    // as `rl.d.ts`.
    let std_declaration = root.join(project::STD_MODULE).with_extension("d.ts");
    for declaration in declarations {
        if declaration.path == std_declaration {
            let dir = out_dir.unwrap_or(root);
            std::fs::create_dir_all(dir)?;
            std::fs::write(dir.join("rl.d.ts"), &declaration.text)?;
            continue;
        }
        // Only what was asked for is written; the rest of the project is in
        // the graph so that it resolves, not so that it is emitted.
        let Some(file) = lowered
            .iter()
            .find(|f| project::declaration_path_of(f) == declaration.path)
            .filter(|f| requested.contains(&f.source_path))
        else {
            continue;
        };

        // The same placement `--types` and `--sidecar` use: beside the
        // source, or mirroring the input layout under `-o`.
        let name = format!(
            "{}.d.ts",
            file.source_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        );
        let target = match out_dir {
            Some(dir) => dir
                .join(crate::input_relative(&file.source_path, inputs))
                .with_file_name(name),
            None => file.source_path.with_file_name(name),
        };
        let dir = target.parent().unwrap_or(Path::new(".")).to_path_buf();
        std::fs::create_dir_all(&dir)?;

        let sidecar = rlc::build_sidecar(
            &file.source,
            &declaration.text,
            &crate::relative_path(&dir, &file.source_path),
        );
        std::fs::write(&target, &sidecar.declarations)?;
        std::fs::write(target.with_extension("ts.map"), &sidecar.map)?;
    }
    Ok(())
}

/// A path as a diagnostic should name it: relative to the directory the
/// command was run in, when it is under it.
///
/// The compiler resolves modules by absolute path, so that is what comes
/// back — but `rlc: /tmp/build-42/src/a.rl:3:1: ...` is not what the other
/// modes print, and not what an editor's problem matcher expects.
fn shown(path: &Path) -> String {
    let relative = std::env::current_dir()
        .ok()
        .and_then(|cwd| path.strip_prefix(cwd).ok().map(Path::to_path_buf));
    relative
        .unwrap_or_else(|| path.to_path_buf())
        .display()
        .to_string()
}

/// A covered literal as it reads in a message.
fn display_literal(literal: &rlc::Literal) -> String {
    match literal {
        rlc::Literal::String(s) => format!("{s:?}"),
        rlc::Literal::Number(n) => n.to_string(),
        rlc::Literal::BigInt(d) => format!("{d}n"),
        rlc::Literal::Boolean(b) => b.to_string(),
    }
}

/// Every file of the project with one of `extensions`, as absolute paths.
/// `node_modules`, dot directories and the output tree are skipped — nothing
/// there is a source.
fn project_sources(
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

/// The `.rl` files of the inputs, as absolute paths — the compiler resolves
/// modules by absolute path, and so must the modules rlc adds.
fn collect(inputs: &[String]) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for input in inputs {
        crate::collect_sources(Path::new(input), false, &mut files)?;
    }
    files
        .into_iter()
        .filter(|f| f.extension().is_some_and(|e| e == "rl"))
        .map(|f| f.canonicalize())
        .collect()
}

/// The nearest `tsconfig.json` at or above the inputs' common directory.
fn find_tsconfig(files: &[PathBuf]) -> Option<PathBuf> {
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
