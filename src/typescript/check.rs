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
use super::native::{NativeBackend, is_builtin};
use super::project;

/// Runs the check. Returns failure when anything was reported, exactly like
/// `--check`.
pub(crate) fn run(
    inputs: &[String],
    project_arg: Option<&Path>,
    node: Option<&Path>,
    out_dir: Option<&Path>,
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

    let tsconfig = match project_arg
        .map(PathBuf::from)
        .or_else(|| find_tsconfig(&files))
    {
        Some(path) => path,
        None => {
            eprintln!(
                "rlc: no tsconfig.json found above the inputs — the project's own \
                 configuration is what the checker runs with; name it with --project"
            );
            return ExitCode::FAILURE;
        }
    };
    let tsconfig = tsconfig.canonicalize().unwrap_or(tsconfig);

    let root = tsconfig.parent().unwrap_or(Path::new(".")).to_path_buf();
    let lowered = match project::lower(&files) {
        Ok(lowered) => lowered,
        Err((file, error)) => {
            eprintln!(
                "{}:{}:{}: rl: {}",
                file.display(),
                error.line,
                error.col,
                error.message
            );
            return ExitCode::FAILURE;
        }
    };

    let backend = match NativeBackend::new(node.map(PathBuf::from)) {
        Ok(backend) => backend,
        Err(e) => {
            eprintln!("rlc: {e}");
            return ExitCode::FAILURE;
        }
    };

    let (mut query, probes) = project::query(&lowered, &root);
    query.emit_declarations = out_dir.is_some();
    let answers = match backend.ask(&tsconfig, &query) {
        Ok(answers) => answers,
        Err(e) => {
            eprintln!("rlc: {e}");
            return ExitCode::FAILURE;
        }
    };

    // The declarations the compiler emitted for the lowered modules, laid
    // out under `-o` the way the sources are laid out under the project.
    if let Some(dir) = out_dir
        && let Err(e) = write_declarations(&answers.declarations, &lowered, &root, dir)
    {
        eprintln!("rlc: {e}");
        return ExitCode::FAILURE;
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
                Some(resolution) if is_builtin(resolution) => {}
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

    if reported > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
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
    root: &Path,
    out_dir: &Path,
) -> std::io::Result<()> {
    for declaration in declarations {
        let relative = declaration.path.strip_prefix(root).unwrap_or(
            declaration
                .path
                .file_name()
                .map(Path::new)
                .unwrap_or(&declaration.path),
        );
        let target = out_dir.join(relative);
        let dir = target.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(dir)?;

        match lowered
            .iter()
            .find(|f| project::declaration_path_of(f) == declaration.path)
        {
            Some(file) => {
                let sidecar = rlc::build_sidecar(
                    &file.source,
                    &declaration.text,
                    &crate::relative_path(dir, &file.source_path),
                );
                std::fs::write(&target, &sidecar.declarations)?;
                std::fs::write(target.with_extension("ts.map"), &sidecar.map)?;
            }
            // The standard library has no `.rl` source to map back to.
            None => std::fs::write(&target, &declaration.text)?,
        }
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
