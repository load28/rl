//! The one project graph.
//!
//! rlc lowers every `.rl` file to ordinary TypeScript and hands those modules
//! to the compiler *as part of the user's own project* — the same
//! `tsconfig.json`, the same `lib`, the same module resolution, the same
//! `node_modules`. Hand-written `.ts` files are not handed over at all: they
//! are already on disk, where the compiler reads them. That is what makes a
//! `.ts` file and an `.rl` file see each other.

use std::collections::HashMap;
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
    /// The module path an `.rl` file takes in the project graph: its own
    /// path with `.ts` appended, so `src/token.rl` becomes
    /// `src/token.rl.ts`.
    ///
    /// This is what makes the whole arrangement need no configuration. A
    /// specifier written `"./token.rl"` — which is what a hand-written `.ts`
    /// and an `.rl` alike write — resolves to `token.rl.ts` by ordinary
    /// TypeScript resolution, with no `allowImportingTsExtensions`, no
    /// `paths`, and no rewriting. And the declaration the compiler emits for
    /// it lands on `token.rl.d.ts`, which is exactly the editor sidecar the
    /// same specifier resolves to when no compiler is running.
    fn module_path_of(source_path: &Path) -> PathBuf {
        let mut name = source_path.as_os_str().to_os_string();
        name.push(".ts");
        PathBuf::from(name)
    }
}

/// Lowers every `.rl` file for the project graph. A file that fails an
/// rl-level check is returned as an error with its own position — rl
/// diagnostics come first and are never delegated.
/// `overlay` substitutes text for a file's contents on disk, keyed by the
/// canonical path. It is how an editor has the buffer it is showing checked
/// as part of the project it belongs to: the module keeps its real path — so
/// its imports, and the imports that name it, resolve exactly as they do on
/// disk — and only its text is the unsaved one.
pub(crate) fn lower(
    files: &[PathBuf],
    overlay: &HashMap<PathBuf, String>,
) -> Result<Vec<Lowered>, (PathBuf, rlc::CompileError)> {
    let mut out = Vec::with_capacity(files.len());
    for file in files {
        let source = match overlay.get(file) {
            Some(text) => text.clone(),
            None => std::fs::read_to_string(file).map_err(|e| {
                (
                    file.clone(),
                    rlc::CompileError {
                        message: format!("cannot read: {e}"),
                        filename: Some(file.display().to_string()),
                        line: 0,
                        col: 0,
                    },
                )
            })?,
        };
        let options = Options {
            filename: Some(file.to_str().unwrap_or("<input>")),
            // Exhaustiveness and `val`'s pairing are the checker's answers
            // here — see `Options::defer_to_checker`.
            defer_to_checker: true,
            // Specifiers stay exactly as written. `"./token.rl"` already
            // names the lowered module ([`Lowered::module_path_of`]), and
            // `"@rl/std"` already names the standard library ([`STD_MODULE`])
            // — so the declarations the compiler emits are usable as they
            // are, by a consumer that never sees this compile.
            rewrite_imports: rlc::ImportRewrite::Off,
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

/// Where the standard library sits in the project graph, relative to the
/// project root: the package `"@rl/std"` names.
///
/// It is a module of the project like any other — served from the same
/// layered file system, resolved by ordinary node resolution — so the
/// specifier stays bare in the source and in every declaration emitted from
/// it. Nothing is written to the user's `node_modules`.
pub(crate) const STD_MODULE: &str = "node_modules/@rl/std/index.ts";

/// The path the compiler emits a lowered module's declarations to:
/// `src/token.rl.ts` → `src/token.rl.d.ts`, which is the sidecar name a
/// specifier written `"./token.rl"` resolves to.
pub(crate) fn declaration_path_of(file: &Lowered) -> PathBuf {
    file.module_path.with_extension("d.ts")
}

/// Builds the batch of questions the whole project asks in one round trip.
///
/// Every question is anchored at a byte the compiler can see: a probe whose
/// anchor did not survive lowering as verbatim text (a nested rl construct)
/// is dropped rather than asked about at an approximate position.
pub(crate) fn query(lowered: &[Lowered], root: &Path, sources: &[PathBuf]) -> (Query, Probes) {
    let mut query = Query {
        sources: sources.to_vec(),
        ..Query::default()
    };
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
            let Some(position) = scrutinee_position(&file.emit, probe.offset) else {
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
            let Some(position) = scrutinee_position(&file.emit, probe.offset) else {
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
        for pass in val.passes {
            let Some(position) = anchor(&file.emit, pass.offset) else {
                continue;
            };
            query.symbols.push(SymbolQuery {
                module: file.module_path.clone(),
                position,
            });
            probes.passes.push(PassAnchor {
                anchor: SourceAnchor {
                    source_path: file.source_path.clone(),
                    offset: pass.offset,
                },
                name: pass.name,
                param: pass.param,
                callee: pass.callee,
                root: query.symbols.len() - 1,
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

/// One argument handed to a parameter its callee did not declare `val`,
/// with the symbol question that decides whether that is a violation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PassAnchor {
    /// Where the diagnostic is reported: the argument.
    pub anchor: SourceAnchor,
    /// The argument's root identifier, for the message.
    pub name: String,
    /// The parameter as the message names it.
    pub param: String,
    /// The called function's name.
    pub callee: String,
    /// Index into [`Query::symbols`] for the root identifier.
    pub root: usize,
}

/// The `.rl`-side halves of a [`Query`], parallel to its own vectors.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Probes {
    pub literals: Vec<SourceAnchor>,
    pub tags: Vec<SourceAnchor>,
    /// Indices into [`Query::symbols`] for every `val` binding's identifier.
    pub val_bindings: Vec<usize>,
    pub mutations: Vec<MutationAnchor>,
    pub passes: Vec<PassAnchor>,
}

/// The UTF-16 offset in the emitted module a source byte landed at, or
/// `None` when it was not copied verbatim.
fn anchor(emit: &MappedEmit, source_byte: usize) -> Option<usize> {
    let out = mapper::to_output(&emit.mappings, source_byte)?;
    Some(mapper::to_utf16(&emit.code, out))
}

/// Where to ask about the type a `match` is over: the temporary the emitted
/// code binds the scrutinee to, found by the `match` keyword's own offset.
///
/// Not the scrutinee's text. `getTypeAtPosition` answers about the node at a
/// position, and for `match (getShape())` the node at the scrutinee's first
/// byte is `getShape` — a function, whose type has no `kind` property and no
/// literal constituents, so every exhaustiveness question came back silent.
/// The temporary is the scrutinee's *value*, and the type the checker gives
/// it is the narrowed one at the match. See [`rlc::ScrutineeTemp`].
fn scrutinee_position(emit: &MappedEmit, keyword_offset: usize) -> Option<usize> {
    let temp = emit
        .scrutinee_temps
        .iter()
        .find(|temp| temp.src == keyword_offset)?;
    Some(mapper::to_utf16(&emit.code, temp.out))
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
    fn a_lowered_module_is_named_so_that_an_rl_specifier_resolves_to_it() {
        // `import "./state.rl"` resolves to `state.rl.ts` — and the
        // declaration emitted for it lands on `state.rl.d.ts`, the sidecar
        // the same specifier resolves to without a compiler.
        assert_eq!(
            Lowered::module_path_of(Path::new("/p/src/state.rl")),
            PathBuf::from("/p/src/state.rl.ts"),
        );
    }
}
