//! Semantic results in rl's own vocabulary.
//!
//! The checker's answers come back as [`crate::typescript::backend::Answers`]
//! — coordinates in emitted modules, symbol ids, raw diagnostics. This module
//! turns them into what a consumer of the engine actually wants: diagnostics
//! at positions in the `.rl` source, with the exact wording the CLI has
//! always printed, and declarations matched back to the files they were
//! emitted for. Nothing TypeScript-shaped leaves the engine.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use super::projection::{self, Probes, ProjectedDocument};
use super::snapshot::Snapshot;
use crate::typescript::backend::{Answers, Diagnostic as TsDiagnostic, Resolution};

/// One reported problem, at a position in a file the user can open.
///
/// The message carries its full wording — including the `ts(CODE):` prefix
/// of a type diagnostic — so a consumer prints or displays it verbatim and
/// two consumers can never drift apart on phrasing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// The file the problem is in — an `.rl` source, or a hand-written
    /// TypeScript file when the checker reported one of those.
    pub path: PathBuf,
    /// 1-based line and column (columns count UTF-8 code points), or `None`
    /// when the diagnostic has no position in a file the user wrote.
    pub position: Option<(usize, usize)>,
    /// The full message, as it is shown.
    pub message: String,
}

/// What one checked snapshot came back with.
#[derive(Debug, Default)]
pub struct Checked {
    /// Every diagnostic of the pass, in report order: the type layer first
    /// (unless the request was rl-only), then literal exhaustiveness, tag
    /// exhaustiveness, `val` mutations, and `val` passes.
    pub diagnostics: Vec<Diagnostic>,
    /// The declarations the compiler emitted, when they were requested.
    pub declarations: Declarations,
}

/// The declarations of one emitting pass, matched back to their sources.
#[derive(Debug, Default)]
pub struct Declarations {
    /// The standard library's own declarations, when `@rl/std` is in the
    /// graph — the `rl.d.ts` a consumer's `paths` points at.
    pub std: Option<String>,
    /// One entry per requested `.rl` file the compiler emitted for.
    pub modules: Vec<ModuleDeclaration>,
}

/// One lowered module's declarations, paired with the file they belong to.
#[derive(Debug)]
pub struct ModuleDeclaration {
    /// The projected file the declarations were emitted for.
    pub file: Arc<ProjectedDocument>,
    /// The declaration text — the compiler's own; only the sidecar map is
    /// rlc's to build.
    pub text: String,
}

/// Builds the pass's diagnostics from the checker's answers, in the exact
/// order and wording the report has always had.
pub(crate) fn report(
    snapshot: &Snapshot,
    answers: &Answers,
    probes: &Probes,
    rl_only: bool,
) -> Vec<Diagnostic> {
    let files = snapshot.files();
    let mut out = Vec::new();

    // TypeScript's own diagnostics, at the position in the `.rl` file the
    // offending code was written at.
    let type_diagnostics: &[TsDiagnostic] = if rl_only { &[] } else { &answers.diagnostics };
    for diagnostic in type_diagnostics {
        let Some(file) = files.iter().find(|f| f.module_path == diagnostic.file) else {
            // A hand-written file: TypeScript's own coordinates already name
            // a file the user can open, so they are used as they are.
            out.push(Diagnostic {
                path: diagnostic.file.clone(),
                position: None,
                message: format!("ts({}): {}", diagnostic.code, diagnostic.message),
            });
            continue;
        };
        match projection::diagnostic_source_offset(file, diagnostic.start) {
            Some((offset, exact)) => {
                let (line, col) = crate::line_col(&file.source, offset);
                out.push(Diagnostic {
                    path: file.source_path.clone(),
                    position: Some((line, col)),
                    message: format!(
                        "ts({}): {}{}",
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
                    ),
                });
            }
            None => out.push(Diagnostic {
                path: file.source_path.clone(),
                position: None,
                message: format!("ts({}): {}", diagnostic.code, diagnostic.message),
            }),
        }
    }

    // Literal-match exhaustiveness, decided by the type TypeScript computes
    // at the scrutinee — narrowing included.
    for missing in &answers.literal_missing {
        let Some(anchor) = probes.literals.get(missing.index) else {
            continue;
        };
        let Some(file) = files.iter().find(|f| f.source_path == anchor.source_path) else {
            continue;
        };
        let (line, col) = crate::line_col(&file.source, anchor.offset);
        out.push(Diagnostic {
            path: file.source_path.clone(),
            position: Some((line, col)),
            message: format!(
                "match on literal union is not exhaustive: missing {} \
                 (add the missing arms or a final `_` arm)",
                missing
                    .missing
                    .iter()
                    .map(display_literal)
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
        });
    }

    // Tag exhaustiveness, from the same narrowed type.
    for missing in &answers.tag_missing {
        let Some(anchor) = probes.tags.get(missing.index) else {
            continue;
        };
        let Some(file) = files.iter().find(|f| f.source_path == anchor.source_path) else {
            continue;
        };
        let (line, col) = crate::line_col(&file.source, anchor.offset);
        out.push(Diagnostic {
            path: file.source_path.clone(),
            position: Some((line, col)),
            message: format!(
                "match is not exhaustive: missing {} \
                 (add the missing arms or a final `_` arm)",
                missing
                    .missing
                    .iter()
                    .map(|t| format!("{t:?}"))
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
        });
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
    let symbols: HashMap<usize, &Resolution> =
        answers.resolutions.iter().map(|r| (r.index, r)).collect();
    let val_symbols: HashSet<i64> = probes
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
                // Two halves make the verdict: the checker's — the
                // method is one of TypeScript's own — and rl's policy —
                // that method is one of the mutating ones. A built-in
                // `get` fails the second; a user-defined `set`, or a
                // method the checker could not resolve, fails the first.
                Some(resolution)
                    if resolution.builtin && crate::is_builtin_mutator_name(&resolution.name) => {}
                _ => continue,
            }
        }
        let Some(file) = files
            .iter()
            .find(|f| f.source_path == mutation.anchor.source_path)
        else {
            continue;
        };
        let (line, col) = crate::line_col(&file.source, mutation.anchor.offset);
        let message = match &mutation.method_name {
            // The built-in itself is not named: the compiler answered
            // "this method is one of TypeScript's own", which is the
            // verdict — not which interface declares it.
            Some(method) => format!(
                "cannot call mutating method `{}` through val binding `{}` \
                 (the binding is declared with `val`, so every access path from it is \
                 read-only)",
                method, mutation.name,
            ),
            None => format!(
                "cannot mutate through val binding `{}` \
                 (the binding is declared with `val`, so every access path \
                 from it is read-only)",
                mutation.name,
            ),
        };
        out.push(Diagnostic {
            path: file.source_path.clone(),
            position: Some((line, col)),
            message,
        });
    }

    // The callee table: a declaration's symbol names its parameter
    // list. One symbol carrying declarations with *different* lists
    // (TypeScript overloads, `var` merging) makes that callee
    // ambiguous, and an ambiguous callee is not judged — the same
    // caution the name-keyed table of the untyped path takes, here at
    // symbol granularity, so two functions merely sharing a name stay
    // two callees.
    let mut callees: HashMap<i64, Option<&[crate::ValParam]>> = HashMap::new();
    for function in &probes.functions {
        let Some(resolution) = symbols.get(&function.root) else {
            continue;
        };
        match callees.get(&resolution.id) {
            Some(Some(prev)) if *prev == function.params.as_slice() => {}
            Some(_) => {
                callees.insert(resolution.id, None);
            }
            None => {
                callees.insert(resolution.id, Some(&function.params));
            }
        }
    }

    // The function boundary: a `val` binding may only be handed to a
    // parameter that is itself `val`. Which binding the argument names,
    // and which declaration the call names, are the same symbol
    // question the mutations above ask — an unresolved callee, or one
    // no collected declaration matches (an import, a method), is never
    // a verdict.
    for pass in &probes.passes {
        let Some(root) = symbols.get(&pass.root) else {
            continue;
        };
        if !val_symbols.contains(&root.id) {
            continue;
        }
        let Some(callee) = symbols.get(&pass.callee_symbol) else {
            continue;
        };
        let Some(Some(params)) = callees.get(&callee.id) else {
            continue;
        };
        let Some(param) = params.get(pass.arg_index) else {
            continue;
        };
        if param.is_val {
            continue;
        }
        let described = match &param.name {
            Some(name) => format!("`{name}`"),
            None => format!("#{}", pass.arg_index + 1),
        };
        let Some(file) = files
            .iter()
            .find(|f| f.source_path == pass.anchor.source_path)
        else {
            continue;
        };
        let (line, col) = crate::line_col(&file.source, pass.anchor.offset);
        out.push(Diagnostic {
            path: file.source_path.clone(),
            position: Some((line, col)),
            message: format!(
                "cannot pass val binding `{}` to mutable parameter {} of \
                 `{}` (the parameter is not declared with `val`, so the function may mutate \
                 through it)",
                pass.name, described, pass.callee,
            ),
        });
    }

    out
}

/// Matches the compiler's emitted declarations back to the snapshot's files.
/// Only `requested` files are kept — the rest of the project is in the graph
/// so that it resolves, not so that it is emitted.
pub(crate) fn match_declarations(
    snapshot: &Snapshot,
    answers: &Answers,
    root: &std::path::Path,
    requested: &HashSet<PathBuf>,
) -> Declarations {
    let mut out = Declarations::default();
    // The standard library's own declarations, so a consumer running plain
    // tsc can point `@rl/std` at them (`paths`). It is a module of the
    // project like any other, so the compiler emitted it too — it just has
    // no `.rl` source to sit beside.
    let std_declaration = root.join(projection::STD_MODULE).with_extension("d.ts");
    for declaration in &answers.declarations {
        if declaration.path == std_declaration {
            out.std = Some(declaration.text.clone());
            continue;
        }
        let Some(file) = snapshot
            .files()
            .iter()
            .find(|f| projection::declaration_path_of(f) == declaration.path)
            .filter(|f| requested.contains(&f.source_path))
        else {
            continue;
        };
        out.modules.push(ModuleDeclaration {
            file: file.clone(),
            text: declaration.text.clone(),
        });
    }
    out
}

/// A covered literal as it reads in a message.
fn display_literal(literal: &crate::Literal) -> String {
    match literal {
        crate::Literal::String(s) => format!("{s:?}"),
        crate::Literal::Number(n) => n.to_string(),
        crate::Literal::BigInt(d) => format!("{d}n"),
        crate::Literal::Boolean(b) => b.to_string(),
    }
}
