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

use super::backend::TypeScriptBackend;
use super::native::{NativeBackend, is_builtin};
use super::project;

/// Runs the check. Returns failure when anything was reported, exactly like
/// `--check`.
pub(crate) fn run(inputs: &[String], project_arg: Option<&Path>, node: Option<&Path>) -> ExitCode {
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

    let lowered = match project::lower(&files) {
        Ok(lowered) => lowered,
        Err((file, error)) => {
            eprintln!("{}: {}", file.display(), error.message);
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

    let (query, probes) = project::query(&lowered);
    let answers = match backend.ask(&tsconfig, &query) {
        Ok(answers) => answers,
        Err(e) => {
            eprintln!("rlc: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut reported = 0usize;

    // TypeScript's own diagnostics, at the position in the `.rl` file the
    // offending code was written at.
    for diagnostic in &answers.diagnostics {
        let Some(file) = lowered.iter().find(|f| f.module_path == diagnostic.file) else {
            continue;
        };
        reported += 1;
        match project::diagnostic_source_offset(file, diagnostic.start) {
            Some(offset) => {
                let (line, col) = rlc::line_col(&file.source, offset);
                println!(
                    "{}:{}:{}: ts({}): {}",
                    file.source_path.display(),
                    line,
                    col,
                    diagnostic.code,
                    diagnostic.message,
                );
            }
            // Compiler-written glue. By the error-layer contract rlc's own
            // output must not draw type errors, so this is an rlc bug and is
            // named as one rather than pinned on the user's line.
            None => println!(
                "{}: ts({}): {} (in generated code — this is an rlc bug)",
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
            "{}:{}:{}: rl: match is not exhaustive: missing {}",
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

    // `val`: a call mutates only when the method it resolves to is declared
    // in TypeScript's own lib files. A user-defined method that shares a
    // name — and anything the checker could not resolve — is not a mutation.
    for resolution in &answers.val_resolutions {
        if !is_builtin(resolution) {
            continue;
        }
        let Some(anchor) = probes.vals.get(resolution.index) else {
            continue;
        };
        let Some(file) = lowered
            .iter()
            .find(|f| f.source_path == anchor.anchor.source_path)
        else {
            continue;
        };
        let (line, col) = rlc::line_col(&file.source, anchor.anchor.offset);
        reported += 1;
        println!(
            "{}:{}:{}: rl: `{}` is a val binding: `{}` mutates it",
            file.source_path.display(),
            line,
            col,
            anchor.binding,
            anchor.method,
        );
    }

    if reported > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
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
