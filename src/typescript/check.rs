//! `rlc --native-check` — the whole pipeline over one project graph.
//!
//! Lowers every `.rl` input to ordinary TypeScript, hands the lowered modules
//! to the real TypeScript compiler as part of the user's own project, and
//! reports what comes back **at positions in the `.rl` source**.
//!
//! The two diagnostic layers stay distinguishable. An rl-level error (a
//! duplicate case, a misplaced wildcard, a mutation through a `val` binding)
//! is rlc's and is reported as `rl:`; a type error is TypeScript's and is
//! reported as `ts(CODE):`. Both name a place in the file the user wrote.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use super::backend::{Resolution, TypeScriptBackend};
use super::native::NativeBackend;
use super::project;

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
    let files = match project_sources(&root, out_dir) {
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

    let pass = Pass {
        backend: &backend,
        requested: &requested,
        inputs,
        tsconfig: tsconfig.as_deref(),
        root: &root,
        out_dir,
        emit,
    };

    if watch {
        return watch_loop(&pass, &root, out_dir);
    }

    match pass.once(&files) {
        // Writing is what a sidecar run is for: the declarations are emitted
        // even when the code has type errors (they are reported, and a stale
        // sidecar would be worse than one built from erroring code), so the
        // exit code reflects whether the files were written. A run that only
        // checks fails on anything it reported.
        Ok(reported) if emit || reported == 0 => ExitCode::SUCCESS,
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
}

impl Pass<'_> {
    /// Lowers `files`, asks the compiler about them and reports what comes
    /// back. Returns how much was reported; an rl-level error that leaves
    /// nothing to lower counts as one.
    fn once(&self, files: &[PathBuf]) -> Result<usize, String> {
        let lowered = match project::lower(files) {
            Ok(lowered) => lowered,
            Err((file, error)) => {
                println!(
                    "{}:{}:{}: rl: {}",
                    file.display(),
                    error.line,
                    error.col,
                    error.message
                );
                return Ok(1);
            }
        };

        let (mut query, probes) = project::query(&lowered, self.root);
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
                println!(
                    "{}: ts({}): {}",
                    diagnostic.file.display(),
                    diagnostic.code,
                    diagnostic.message,
                );
                continue;
            };
            match project::diagnostic_source_offset(file, diagnostic.start) {
                Some((offset, exact)) => {
                    let (line, col) = rlc::line_col(&file.source, offset);
                    println!(
                        "{}:{}:{}: ts({}): {}{}",
                        file.source_path.display(),
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
                None => println!(
                    "{}: ts({}): {}",
                    file.source_path.display(),
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
            println!(
                "{}:{}:{}: rl: match is not exhaustive: missing {} \
                 (add the missing arms or a final `_` arm)",
                file.source_path.display(),
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
            println!(
                "{}:{}:{}: rl: match is not exhaustive: missing {} \
                 (add the missing arms or a final `_` arm)",
                file.source_path.display(),
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
                Some(method) => println!(
                    "{}:{}:{}: rl: `{}` is a val binding: `{}` mutates it",
                    file.source_path.display(),
                    line,
                    col,
                    mutation.name,
                    method,
                ),
                None => println!(
                    "{}:{}:{}: rl: cannot mutate through val binding `{}` \
                     (the binding is declared with `val`, so every access path \
                     from it is read-only)",
                    file.source_path.display(),
                    line,
                    col,
                    mutation.name,
                ),
            }
        }

        Ok(reported)
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
        let files = match project_sources(root, out_dir) {
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
                Ok(reported) => eprintln!(
                    "rlc: {} file(s), {} reported in {} ms — watching",
                    files.len(),
                    reported,
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
) -> std::io::Result<()> {
    for declaration in declarations {
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

/// A covered literal as it reads in a message.
fn display_literal(literal: &rlc::Literal) -> String {
    match literal {
        rlc::Literal::String(s) => format!("{s:?}"),
        rlc::Literal::Number(n) => n.to_string(),
        rlc::Literal::BigInt(d) => format!("{d}n"),
        rlc::Literal::Boolean(b) => b.to_string(),
    }
}

/// Every `.rl` file of the project, as absolute paths. `node_modules`, dot
/// directories and the output tree are skipped — nothing there is a source.
fn project_sources(root: &Path, out_dir: Option<&Path>) -> std::io::Result<Vec<PathBuf>> {
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
            } else if path.extension().is_some_and(|e| e == "rl") {
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
