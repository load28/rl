//! The one project graph.
//!
//! rlc lowers every `.rl` file to ordinary TypeScript and hands those modules
//! to the compiler *as part of the user's own project* — the same
//! `tsconfig.json`, the same `lib`, the same module resolution, the same
//! `node_modules`. Hand-written `.ts` files are not handed over at all: they
//! are already on disk, where the compiler reads them. That is what makes a
//! `.ts` file and an `.rl` file see each other.

use std::path::{Path, PathBuf};

use rlc::{EmitMapping, MappedEmit, Options};

use super::backend::*;
use super::mapper;

/// One `.rl` file as both halves of the pipeline see it: the source the user
/// wrote, and the TypeScript the compiler is given.
#[derive(Debug, Clone)]
pub(crate) struct Lowered {
    /// The `.rl` file.
    pub source_path: PathBuf,
    /// The source text — the coordinate space diagnostics are reported in.
    pub source: String,
    /// The path the lowered module occupies in the project: the same place,
    /// with a `.ts` extension.
    pub module_path: PathBuf,
    /// The emitted TypeScript and its verbatim-chunk mappings.
    pub emit: MappedEmit,
}

impl Lowered {
    /// The module path an `.rl` file takes in the project graph.
    fn module_path_of(source_path: &Path) -> PathBuf {
        source_path.with_extension("ts")
    }
}

/// Lowers every `.rl` file for the project graph. A file that fails an
/// rl-level check is returned as an error with its own position — rl
/// diagnostics come first and are never delegated.
pub(crate) fn lower(
    files: &[PathBuf],
    root: &Path,
) -> Result<Vec<Lowered>, (PathBuf, rlc::CompileError)> {
    let mut out = Vec::with_capacity(files.len());
    for file in files {
        let source = std::fs::read_to_string(file).map_err(|e| {
            (
                file.clone(),
                rlc::CompileError {
                    message: format!("cannot read: {e}"),
                    filename: Some(file.display().to_string()),
                    line: 0,
                    col: 0,
                },
            )
        })?;
        // `@rl/std` is not a package: the standard library enters the graph
        // as one more module of the project, and the importing file points at
        // it by relative path. No resolver hook, no `paths` entry.
        let std_import = rlc::imports_std(&source)
            .then(|| relative_specifier(file.parent().unwrap_or(root), &root.join(STD_MODULE)));
        let options = Options {
            filename: Some(file.to_str().unwrap_or("<input>")),
            std_import: std_import.as_deref(),
            // Exhaustiveness and `val`'s pairing are the checker's answers
            // here — see `Options::defer_to_checker`.
            defer_to_checker: true,
            // The lowered module sits where the source did, so a relative
            // `.rl` specifier names the module beside it: `./x.rl` → `./x.ts`.
            rewrite_imports: rlc::ImportRewrite::Ts,
            ..Options::default()
        };
        let emit = rlc::compile_mapped(&source, &options).map_err(|e| (file.clone(), e))?;
        out.push(Lowered {
            module_path: Lowered::module_path_of(file),
            source_path: file.clone(),
            source,
            emit,
        });
    }
    Ok(out)
}

/// The standard library's file name in the project graph. It is emitted
/// only when something imports it, and it sits at the project root so every
/// module can name it relatively.
pub(crate) const STD_MODULE: &str = "__rl_std__.ts";

/// A specifier for `target` as written in a file living in `from`: always
/// relative, always with the `.ts` extension the lowered modules use.
fn relative_specifier(from: &Path, target: &Path) -> String {
    let mut from_parts: Vec<_> = from.components().collect();
    let mut target_parts: Vec<_> = target.components().collect();
    let shared = from_parts
        .iter()
        .zip(target_parts.iter())
        .take_while(|(a, b)| a == b)
        .count();
    from_parts.drain(..shared);
    target_parts.drain(..shared);
    let mut specifier = String::new();
    for _ in 0..from_parts.len() {
        specifier.push_str("../");
    }
    if specifier.is_empty() {
        specifier.push_str("./");
    }
    let tail: Vec<_> = target_parts
        .iter()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect();
    specifier.push_str(&tail.join("/"));
    specifier
}

/// Builds the batch of questions the whole project asks in one round trip.
///
/// Every question is anchored at a byte the compiler can see: a probe whose
/// anchor did not survive lowering as verbatim text (a nested rl construct)
/// is dropped rather than asked about at an approximate position.
pub(crate) fn query(lowered: &[Lowered], root: &Path) -> (Query, Probes) {
    let mut query = Query::default();
    let mut probes = Probes::default();

    if lowered.iter().any(|f| rlc::imports_std(&f.source)) {
        query.modules.push(Module {
            path: root.join(STD_MODULE),
            text: rlc::STD_SOURCE.to_string(),
        });
    }

    for file in lowered {
        query.modules.push(Module {
            path: file.module_path.clone(),
            text: file.emit.code.clone(),
        });

        for probe in rlc::literal_matches(&file.source) {
            // A BigInt is never a member of a finite literal union
            // TypeScript reports, so such a match is left unchecked.
            if probe
                .covered
                .iter()
                .any(|l| matches!(l, rlc::Literal::BigInt(_)))
            {
                continue;
            }
            let Some(position) = anchor(&file.emit, probe.scrutinee) else {
                continue;
            };
            query.literals.push(LiteralQuery {
                module: file.module_path.clone(),
                position,
                covered: probe.covered,
            });
            probes.literals.push(SourceAnchor {
                source_path: file.source_path.clone(),
                offset: probe.offset,
            });
        }

        for probe in rlc::tag_matches(&file.source) {
            let Some(position) = anchor(&file.emit, probe.scrutinee) else {
                continue;
            };
            query.tags.push(TagQuery {
                module: file.module_path.clone(),
                position,
                covered: probe.covered,
            });
            probes.tags.push(SourceAnchor {
                source_path: file.source_path.clone(),
                offset: probe.offset,
            });
        }

        // `val`: rlc finds the bindings and the mutations; which mutation
        // belongs to which binding is symbol identity, which is the
        // checker's to answer.
        let val = rlc::val_probes(&file.source);
        for binding in val.bindings {
            let Some(position) = anchor(&file.emit, binding.ident) else {
                continue;
            };
            query.symbols.push(SymbolQuery {
                module: file.module_path.clone(),
                position,
            });
            probes.val_bindings.push(query.symbols.len() - 1);
        }
        for mutation in val.mutations {
            let Some(root) = anchor(&file.emit, mutation.root) else {
                continue;
            };
            // A method call needs a second question: is the method one of
            // TypeScript's own?
            let method = match &mutation.method {
                Some((_, at)) => match anchor(&file.emit, *at) {
                    Some(position) => {
                        query.symbols.push(SymbolQuery {
                            module: file.module_path.clone(),
                            position,
                        });
                        Some(query.symbols.len() - 1)
                    }
                    None => continue,
                },
                None => None,
            };
            query.symbols.push(SymbolQuery {
                module: file.module_path.clone(),
                position: root,
            });
            probes.mutations.push(MutationAnchor {
                anchor: SourceAnchor {
                    source_path: file.source_path.clone(),
                    offset: mutation.root,
                },
                name: mutation.name,
                root: query.symbols.len() - 1,
                method,
                method_name: mutation.method.map(|(name, _)| name),
            });
        }
    }
    (query, probes)
}

/// Where a question's answer is reported: a byte in the `.rl` source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceAnchor {
    pub source_path: PathBuf,
    pub offset: usize,
}

/// One mutation, with the symbol questions that decide whether it is one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MutationAnchor {
    /// Where the diagnostic is reported.
    pub anchor: SourceAnchor,
    /// The root identifier's text, for the message.
    pub name: String,
    /// Index into [`Query::symbols`] for the root identifier: which binding
    /// this path is rooted at.
    pub root: usize,
    /// Index into [`Query::symbols`] for the method name, when the mutation
    /// is a method call. `None` for an assignment, an increment or a
    /// `delete`, which mutate on syntax alone.
    pub method: Option<usize>,
    /// The method's name, for the message.
    pub method_name: Option<String>,
}

/// The `.rl`-side halves of a [`Query`], parallel to its own vectors.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Probes {
    pub literals: Vec<SourceAnchor>,
    pub tags: Vec<SourceAnchor>,
    /// Indices into [`Query::symbols`] for every `val` binding's identifier.
    pub val_bindings: Vec<usize>,
    pub mutations: Vec<MutationAnchor>,
}

/// The UTF-16 offset in the emitted module a source byte landed at, or
/// `None` when it was not copied verbatim.
fn anchor(emit: &MappedEmit, source_byte: usize) -> Option<usize> {
    let out = mapper::to_output(&emit.mappings, source_byte)?;
    Some(mapper::to_utf16(&emit.code, out))
}

/// Where a TypeScript diagnostic belongs in the `.rl` source, and whether
/// that position is exact.
///
/// A diagnostic on compiler-written glue is not the user's code, so its
/// position is approximate — the construct it was generated for — and the
/// message says so. By the error-layer contract it should not happen at
/// all: rlc's own output must not draw type errors.
pub(crate) fn diagnostic_source_offset(
    file: &Lowered,
    utf16_start: usize,
) -> Option<(usize, bool)> {
    let out = mapper::from_utf16(&file.emit.code, utf16_start);
    mapper::to_source_or_nearest(mappings(&file.emit), out)
}

fn mappings(emit: &MappedEmit) -> &[EmitMapping] {
    &emit.mappings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_specifier_is_relative_to_the_importing_file() {
        assert_eq!(
            relative_specifier(Path::new("/p/src"), Path::new("/p/__rl_std__.ts")),
            "../__rl_std__.ts",
        );
        assert_eq!(
            relative_specifier(Path::new("/p"), Path::new("/p/__rl_std__.ts")),
            "./__rl_std__.ts",
        );
        assert_eq!(
            relative_specifier(Path::new("/p/a/b"), Path::new("/p/__rl_std__.ts")),
            "../../__rl_std__.ts",
        );
    }

    #[test]
    fn a_lowered_module_takes_its_source_path_with_a_ts_extension() {
        assert_eq!(
            Lowered::module_path_of(Path::new("/p/src/state.rl")),
            PathBuf::from("/p/src/state.ts"),
        );
    }
}
