//! The projection — rl's own layer between a source and the type checker.
//!
//! An `.rl` file enters the TypeScript project as ordinary TypeScript. The
//! [`ProjectedDocument`] is that fact made first-class: one file's source
//! text, the module it becomes, the byte-exact mappings between the two, and
//! every question the file wants asked of the checker (match probes, `val`
//! probes) — computed **once per content version** and reused across
//! snapshots. This is where the engine's incrementality lives: a file whose
//! text did not change between two snapshots costs nothing to re-project.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::typescript::backend::{LiteralQuery, Module, Query, SymbolQuery, TagQuery};
use crate::typescript::mapper;
use crate::{CompileError, LiteralMatch, MappedEmit, Options, TagMatch, ValProbes};

/// One `.rl` file as every consumer of the engine sees it: the source the
/// user wrote, and the TypeScript the compiler is given — plus everything
/// the engine derives from the text, cached with it.
#[derive(Debug)]
pub struct ProjectedDocument {
    /// The `.rl` file.
    pub source_path: PathBuf,
    /// The source text — the coordinate space diagnostics are reported in.
    pub source: String,
    /// The path the lowered module occupies in the project: the same place,
    /// with a `.ts` extension (`src/token.rl` → `src/token.rl.ts`).
    pub(crate) module_path: PathBuf,
    /// The emitted TypeScript and its verbatim-chunk mappings.
    pub(crate) emit: MappedEmit,
    /// Whether the file imports `@rl/std` — decides whether the standard
    /// library joins the project graph.
    pub(crate) imports_std: bool,
    /// The literal-match exhaustiveness probes of this file.
    pub(crate) literal_probes: Vec<LiteralMatch>,
    /// The tag-match exhaustiveness probes of this file.
    pub(crate) tag_probes: Vec<TagMatch>,
    /// The nested patterns of this file — the payload positions the
    /// checker is asked to name the alphabet of.
    pub(crate) payload_probes: Vec<crate::PayloadProbe>,
    /// The `val` bindings, mutations, declarations and passes of this file,
    /// unpaired — pairing is symbol identity, the checker's answer.
    pub(crate) val: ValProbes,
}

impl ProjectedDocument {
    /// Projects one file: lowers it to ordinary TypeScript and derives every
    /// probe the typed pass will ask about. An rl-level error is the file's
    /// own, with its position.
    pub(crate) fn project(
        source_path: &Path,
        source: String,
    ) -> Result<ProjectedDocument, CompileError> {
        let options = Options {
            filename: Some(source_path.to_str().unwrap_or("<input>")),
            // Exhaustiveness and `val`'s pairing are the checker's answers
            // here — see `Options::defer_to_checker`.
            defer_to_checker: true,
            // Specifiers stay exactly as written. `"./token.rl"` already
            // names the lowered module ([`module_path_of`]), and `"@rl/std"`
            // already names the standard library ([`STD_MODULE`]) — so the
            // declarations the compiler emits are usable as they are, by a
            // consumer that never sees this compile.
            rewrite_imports: crate::ImportRewrite::Off,
            ..Options::default()
        };
        let emit = crate::compile_mapped(&source, &options)?;
        Ok(ProjectedDocument {
            module_path: module_path_of(source_path),
            imports_std: crate::imports_std(&source),
            literal_probes: crate::literal_matches(&source),
            tag_probes: crate::tag_matches(&source),
            payload_probes: crate::payload_probes(&source),
            val: crate::val_probes(&source),
            source_path: source_path.to_path_buf(),
            source,
            emit,
        })
    }
}

/// The module path an `.rl` file takes in the project graph: its own path
/// with `.ts` appended, so `src/token.rl` becomes `src/token.rl.ts`.
///
/// This is what makes the whole arrangement need no configuration. A
/// specifier written `"./token.rl"` — which is what a hand-written `.ts`
/// and an `.rl` alike write — resolves to `token.rl.ts` by ordinary
/// TypeScript resolution, with no `allowImportingTsExtensions`, no
/// `paths`, and no rewriting. And the declaration the compiler emits for
/// it lands on `token.rl.d.ts`, which is exactly the editor sidecar the
/// same specifier resolves to when no compiler is running.
pub(crate) fn module_path_of(source_path: &Path) -> PathBuf {
    let mut name = source_path.as_os_str().to_os_string();
    name.push(".ts");
    PathBuf::from(name)
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
pub(crate) fn declaration_path_of(file: &ProjectedDocument) -> PathBuf {
    file.module_path.with_extension("d.ts")
}

/// Builds the batch of questions the whole snapshot asks in one round trip.
///
/// Every question is anchored at a byte the compiler can see: a probe whose
/// anchor did not survive lowering as verbatim text (a nested rl construct)
/// is dropped rather than asked about at an approximate position.
pub(crate) fn assemble(
    files: &[Arc<ProjectedDocument>],
    root: &Path,
    sources: &[PathBuf],
) -> (Query, Probes) {
    let mut query = Query {
        sources: sources.to_vec(),
        ..Query::default()
    };
    let mut probes = Probes::default();

    if files.iter().any(|f| f.imports_std) {
        query.modules.push(Module {
            path: root.join(STD_MODULE),
            text: crate::STD_SOURCE.to_string(),
        });
    }

    for file in files {
        query.modules.push(Module {
            path: file.module_path.clone(),
            text: file.emit.code.clone(),
        });

        for probe in &file.literal_probes {
            // A BigInt is never a member of a finite literal union
            // TypeScript reports, so such a match is left unchecked.
            if probe
                .covered
                .iter()
                .any(|l| matches!(l, crate::Literal::BigInt(_)))
            {
                continue;
            }
            let Some(position) = scrutinee_position(&file.emit, probe.offset) else {
                continue;
            };
            query.literals.push(LiteralQuery {
                module: file.module_path.clone(),
                position,
                covered: probe.covered.clone(),
            });
            probes.literals.push(SourceAnchor {
                source_path: file.source_path.clone(),
                offset: probe.offset,
            });
        }

        for probe in &file.tag_probes {
            let Some(position) = scrutinee_position(&file.emit, probe.offset) else {
                continue;
            };
            query.tags.push(TagQuery {
                module: file.module_path.clone(),
                position,
                covered: probe.covered.clone(),
            });
            probes.tags.push(SourceAnchor {
                source_path: file.source_path.clone(),
                offset: probe.offset,
            });
        }

        // `val`: rlc finds the bindings and the mutations; which mutation
        // belongs to which binding is symbol identity, which is the
        // checker's to answer.
        let val = &file.val;
        for binding in &val.bindings {
            let Some(position) = anchor(&file.emit, binding.ident) else {
                continue;
            };
            query.symbols.push(SymbolQuery {
                module: file.module_path.clone(),
                position,
            });
            probes.val_bindings.push(query.symbols.len() - 1);
        }
        for mutation in &val.mutations {
            // A method call outside rl's mutator policy can never be
            // reported — the verdict needs the checker's `builtin` *and*
            // the policy name — so nothing is asked about it. The policy
            // itself lives at the verdict ([`crate::is_builtin_mutator_name`],
            // applied by the engine's report); skipping here is only the
            // observation that a question whose answer is settled is not
            // worth asking.
            if let Some((name, _)) = &mutation.method
                && !crate::is_builtin_mutator_name(name)
            {
                continue;
            }
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
                name: mutation.name.clone(),
                root: query.symbols.len() - 1,
                method,
                method_name: mutation.method.as_ref().map(|(name, _)| name.clone()),
            });
        }
        // The callee half: every declaration a call might name, as a node.
        // Which call names which declaration is symbol identity, so the
        // declaration identifiers are asked about alongside the calls.
        for function in &val.functions {
            let Some(position) = anchor(&file.emit, function.ident) else {
                continue;
            };
            query.symbols.push(SymbolQuery {
                module: file.module_path.clone(),
                position,
            });
            probes.functions.push(FnAnchor {
                root: query.symbols.len() - 1,
                params: function.params.clone(),
            });
        }
        for pass in &val.passes {
            let Some(position) = anchor(&file.emit, pass.offset) else {
                continue;
            };
            let Some(callee_position) = anchor(&file.emit, pass.callee_at) else {
                continue;
            };
            query.symbols.push(SymbolQuery {
                module: file.module_path.clone(),
                position,
            });
            let root = query.symbols.len() - 1;
            query.symbols.push(SymbolQuery {
                module: file.module_path.clone(),
                position: callee_position,
            });
            probes.passes.push(PassAnchor {
                anchor: SourceAnchor {
                    source_path: file.source_path.clone(),
                    offset: pass.offset,
                },
                name: pass.name.clone(),
                callee: pass.callee.clone(),
                root,
                callee_symbol: query.symbols.len() - 1,
                arg_index: pass.arg_index,
            });
        }
    }
    // A nested pattern narrows over the *payload*, whose type rlc may not
    // know — a type parameter, a hand-written union. The emitted condition
    // tests a receiver expression at exactly that type, and the emitter
    // recorded where; asking there names that column's alphabet for the
    // exhaustiveness algorithm.
    //
    // These ride in the same `tags` list (the question is the same: "which
    // `kind` values does this type allow?") with nothing covered, so the
    // answer is the whole alphabet. They are asked in a pass of their own,
    // **after** every file's match questions, so an answer's index splits
    // cleanly: below `probes.tags.len()` it is a match, at or above it a
    // payload. Interleaving them per file would misattribute every answer
    // from the second file on.
    for file in files {
        for probe in &file.payload_probes {
            let Some(temp) = file
                .emit
                .payload_temps
                .iter()
                .find(|t| t.src == probe.offset)
            else {
                continue;
            };
            query.tags.push(TagQuery {
                module: file.module_path.clone(),
                position: mapper::to_utf16(&file.emit.code, temp.out),
                covered: Vec::new(),
            });
            probes.payloads.push(PayloadAnchor {
                source_path: file.source_path.clone(),
                tag: probe.tag.clone(),
                field: probe.field.clone(),
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

/// One function declaration's symbol question, with the parameter list rl
/// read off it — the callee table's raw material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FnAnchor {
    /// Index into [`Query::symbols`] for the declared name's identifier.
    pub root: usize,
    pub params: Vec<crate::ValParam>,
}

/// One plain-path argument of a call to a name its file declares, with the
/// two symbol questions that decide whether it is a violation: the
/// argument's root (a `val` binding?) and the callee (which declaration?).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PassAnchor {
    /// Where the diagnostic is reported: the argument.
    pub anchor: SourceAnchor,
    /// The argument's root identifier, for the message.
    pub name: String,
    /// The called function's name.
    pub callee: String,
    /// Index into [`Query::symbols`] for the root identifier.
    pub root: usize,
    /// Index into [`Query::symbols`] for the callee identifier.
    pub callee_symbol: usize,
    /// Which argument this is, zero-based.
    pub arg_index: usize,
}

/// The `.rl`-side halves of a [`Query`], parallel to its own vectors.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Probes {
    pub literals: Vec<SourceAnchor>,
    pub tags: Vec<SourceAnchor>,
    /// One per nested pattern asked about, in the order the query lists
    /// them after [`Probes::tags`]: which file, and which
    /// `(constructor, field)` column the answer names the alphabet of.
    pub payloads: Vec<PayloadAnchor>,
    /// Indices into [`Query::symbols`] for every `val` binding's identifier.
    pub val_bindings: Vec<usize>,
    pub mutations: Vec<MutationAnchor>,
    /// The declarations a pass's callee may resolve to, project-wide.
    pub functions: Vec<FnAnchor>,
    pub passes: Vec<PassAnchor>,
}

/// A payload column asked about: where it was written, and which
/// `(constructor, field)` it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PayloadAnchor {
    pub source_path: PathBuf,
    pub tag: String,
    pub field: String,
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
/// it is the narrowed one at the match. See [`crate::ScrutineeTemp`].
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
/// The rl wording for a TypeScript diagnostic that landed on glue, with
/// the source offset to report it at — `None` when the diagnostic is not
/// on glue, or when nothing in the whitelist covers it.
///
/// A diagnostic whose span *is* mapped is the user's own code and is never
/// translated: their type error is TypeScript's to phrase.
pub(crate) fn translate_on_glue(
    file: &ProjectedDocument,
    diagnostic: &crate::typescript::backend::Diagnostic,
) -> Option<(usize, String)> {
    let out = mapper::from_utf16(&file.emit.code, diagnostic.start);
    if mapper::to_source_inclusive(&file.emit.mappings, out).is_some() {
        return None;
    }
    let anchor = file.emit.anchor_at(out)?;
    let said = super::semantics::translate(anchor.kind, diagnostic.code, &diagnostic.message)?;
    Some((anchor.src, said))
}

pub(crate) fn diagnostic_source_offset(
    file: &ProjectedDocument,
    utf16_start: usize,
) -> Option<(usize, bool)> {
    let out = mapper::from_utf16(&file.emit.code, utf16_start);
    mapper::to_source_or_nearest(&file.emit.mappings, out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typescript::backend::Diagnostic as TsDiagnostic;

    /// A diagnostic as the backend hands it over, at a byte of the emitted
    /// module found by searching for the glue in question.
    fn ts_at(file: &ProjectedDocument, needle: &str, code: u32, message: &str) -> TsDiagnostic {
        let at = file
            .emit
            .code
            .find(needle)
            .unwrap_or_else(|| panic!("no {needle:?} in emitted code"));
        TsDiagnostic {
            file: file.module_path.clone(),
            start: at,
            end: at + needle.len(),
            code,
            message: message.to_string(),
        }
    }

    fn project(source: &str) -> ProjectedDocument {
        ProjectedDocument::project(Path::new("/p/src/a.rl"), source.to_string()).expect("projects")
    }

    #[test]
    fn a_type_error_on_a_constructs_glue_is_reported_in_rls_words() {
        // `try` on a non-Result: TypeScript reaches for `.kind` on a number
        // and says so about code the user never wrote.
        let file = project("function f() {\n  const a = try plain();\n  return a;\n}\n");
        let diagnostic = ts_at(
            &file,
            "$rl_t0.kind",
            2339,
            "Property 'kind' does not exist on type 'number'.",
        );
        let (offset, said) = translate_on_glue(&file, &diagnostic).expect("translated");
        assert!(
            file.source[offset..].starts_with("const a = try"),
            "reported at the construct, not the glue"
        );
        assert!(said.starts_with("`try` needs a `Result`"), "{said}");
        // The original rides along — a translation the user can check.
        assert!(said.contains("ts2339: Property 'kind'"), "{said}");
    }

    #[test]
    fn a_type_error_on_the_users_own_code_is_left_to_typescript() {
        let file = project("function f() {\n  const a = try plain();\n  return a;\n}\n");
        // `plain()` is copied from the source, so it is mapped — the user's
        // own text, and their type error to read as TypeScript phrased it.
        let diagnostic = ts_at(&file, "plain()", 2554, "Expected 1 arguments, but got 0.");
        assert!(translate_on_glue(&file, &diagnostic).is_none());
    }

    #[test]
    fn an_unrecognized_code_on_glue_is_not_guessed_at() {
        let file = project("function f() {\n  const a = try plain();\n  return a;\n}\n");
        let diagnostic = ts_at(&file, "$rl_t0.kind", 2739, "Type is missing properties.");
        assert!(translate_on_glue(&file, &diagnostic).is_none());
    }

    #[test]
    fn the_innermost_construct_owns_its_glue() {
        let file = project(
            "enum E { A(x: number), B }\nfunction f() {\n  const a = try wrap(match (e) { A(x) => x, B => 0 });\n}\n",
        );
        let diagnostic = ts_at(
            &file,
            "$rl_m.kind",
            2339,
            "Property 'kind' does not exist on type 'Plain'.",
        );
        let (offset, said) = translate_on_glue(&file, &diagnostic).expect("translated");
        assert!(file.source[offset..].starts_with("match"), "at the match");
        assert!(said.starts_with("match on a tag pattern"), "{said}");
    }

    #[test]
    fn a_lowered_module_is_named_so_that_an_rl_specifier_resolves_to_it() {
        // `import "./state.rl"` resolves to `state.rl.ts` — and the
        // declaration emitted for it lands on `state.rl.d.ts`, the sidecar
        // the same specifier resolves to without a compiler.
        assert_eq!(
            module_path_of(Path::new("/p/src/state.rl")),
            PathBuf::from("/p/src/state.rl.ts"),
        );
    }
}
