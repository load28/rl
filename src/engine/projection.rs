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

use super::snapshot::BlockedFile;
#[cfg(test)]
use crate::CompileError;
use crate::typescript::backend::{LiteralQuery, Module, Query, SymbolQuery, TagQuery};
use crate::typescript::mapper;
use crate::{LiteralMatch, MappedEmit, Options, TagMatch, ValProbes};

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
    /// The file's own rl-level diagnostics, found while projecting — the
    /// recoverable ones (a duplicate arm, an unknown case) that do not stop
    /// the file from lowering. The typed pass reports them **alongside**
    /// its own answers, so one rl error no longer hides a file's type
    /// errors and exhaustiveness holes (TASK-117 symptom 3).
    pub(crate) rl_diagnostics: Vec<crate::Diagnostic>,
    /// Source ranges replaced by parser-owned error placeholders in the
    /// typed projection. Diagnostics originating inside these ranges are
    /// recovery effects; diagnostics elsewhere remain reportable.
    pub(crate) recovered: Vec<(usize, usize)>,
    /// The file's enum declaration symbols, parsed once per content
    /// version — what an importer's extern collection reads, so a file
    /// that did not change is never re-parsed for its exports
    /// (`docs/design/compiler-core.md` §11).
    enum_symbols: std::sync::OnceLock<Vec<crate::EnumSymbol>>,
    /// The file's relative `.rl` imports, parsed once per content version —
    /// the dependency edges the semantic cache keys off.
    imports: std::sync::OnceLock<Vec<crate::RlImport>>,
}

impl ProjectedDocument {
    /// The file's enum declaration symbols (exported or not), computed on
    /// first use and pinned to this projection's content version.
    pub(crate) fn enum_symbols(&self) -> &[crate::EnumSymbol] {
        self.enum_symbols
            .get_or_init(|| crate::enum_symbols(&self.source))
    }

    /// The file's relative `.rl` imports, computed on first use.
    pub(crate) fn rl_imports(&self) -> &[crate::RlImport] {
        self.imports.get_or_init(|| crate::rl_imports(&self.source))
    }
}

impl ProjectedDocument {
    /// Projects one file: lowers it to ordinary TypeScript and derives every
    /// probe the typed pass will ask about. Recoverable rl-level errors ride
    /// along in [`ProjectedDocument::rl_diagnostics`]; only an error that
    /// leaves the file impossible to lower
    /// ([`crate::DiagnosticCode::blocks_projection`]) fails the projection.
    #[cfg(test)]
    pub(crate) fn project(
        source_path: &Path,
        source: String,
    ) -> Result<ProjectedDocument, CompileError> {
        Self::project_for_snapshot(source_path, source).map_err(|blocked| {
            blocked
                .diagnostics
                .first()
                .expect("a blocked projection has a diagnostic")
                .to_compile_error(
                    &blocked.source,
                    Some(blocked.source_path.to_string_lossy().as_ref()),
                )
        })
    }

    pub(crate) fn project_for_snapshot(
        source_path: &Path,
        source: String,
    ) -> Result<ProjectedDocument, BlockedFile> {
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
        let report = crate::compile_projection_report(&source, &options);
        let Some(emit) = report.emit else {
            return Err(BlockedFile::new(
                source_path.to_path_buf(),
                source,
                report.diagnostics,
            ));
        };
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
            rl_diagnostics: report.diagnostics,
            recovered: report.recovered,
            enum_symbols: std::sync::OnceLock::new(),
            imports: std::sync::OnceLock::new(),
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
            if file
                .recovered
                .iter()
                .any(|&(start, end)| start <= probe.offset && probe.offset < end)
            {
                continue;
            }
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
                end: scrutinee_end(&file.source, probe.scrutinee_end),
            });
        }

        for probe in &file.tag_probes {
            if file
                .recovered
                .iter()
                .any(|&(start, end)| start <= probe.offset && probe.offset < end)
            {
                continue;
            }
            // One question per scrutinee position, in the order the
            // temporaries were emitted — which is the order of the
            // positions themselves.
            let temps: Vec<usize> = file
                .emit
                .scrutinee_temps
                .iter()
                .filter(|t| t.src == probe.offset)
                .map(|t| mapper::to_utf16(&file.emit.code, t.out))
                .collect();
            if temps.len() != probe.arity {
                continue;
            }
            for position in temps {
                query.tags.push(TagQuery {
                    module: file.module_path.clone(),
                    position,
                    covered: probe.covered.clone(),
                });
                probes.tags.push(SourceAnchor {
                    source_path: file.source_path.clone(),
                    offset: probe.offset,
                    end: scrutinee_end(&file.source, probe.scrutinee_end),
                });
            }
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
                    end: mutation.root + mutation.name.len(),
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
                    end: pass.offset + pass.name.len(),
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

/// The end of a `match`'s underlined range: the scrutinee's last byte,
/// extended over the whitespace and closing paren that follow it so the
/// span reads as the construct the user wrote — `match (shape)`.
fn scrutinee_end(source: &str, after_scrutinee: usize) -> usize {
    let rest = source.get(after_scrutinee..).unwrap_or_default();
    let closing = rest.trim_start();
    if closing.starts_with(')') {
        after_scrutinee + (rest.len() - closing.len()) + 1
    } else {
        after_scrutinee
    }
}

/// Where a question's answer is reported: a byte range in the `.rl`
/// source. `offset` is the position the CLI prints; `end` closes the range
/// an editor underlines (`offset` again when the anchor is a single point).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceAnchor {
    pub source_path: PathBuf,
    pub offset: usize,
    pub end: usize,
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
/// the source span to report it over — `None` when the diagnostic is not
/// on glue, or when nothing in the whitelist covers it.
///
/// A diagnostic whose span *is* mapped is the user's own code and is never
/// translated: their type error is TypeScript's to phrase.
///
/// `declarations` is the file's declaration table — what lets the wording
/// name the enum case a structural type in the message lowers from. It is
/// the caller's because the table costs a parse of the file (and of what it
/// imports), which is worth doing once per file rather than once per
/// diagnostic.
#[cfg(test)]
fn translate_on_glue(
    file: &ProjectedDocument,
    diagnostic: &crate::typescript::backend::Diagnostic,
    declarations: &[crate::analysis::DeclaredEnum],
) -> Option<(crate::EmitAnchor, String)> {
    let anchor = glue_anchor(file, diagnostic.start)?;
    let said = super::semantics::translate(
        anchor.kind,
        diagnostic.code,
        &diagnostic.message,
        declarations,
    )?;
    Some((anchor, said))
}

/// The construct whose glue a diagnostic's start lands in — `None` when the
/// position is the user's own text (mapped) or belongs to no construct.
#[cfg(test)]
fn glue_anchor(file: &ProjectedDocument, utf16_start: usize) -> Option<crate::EmitAnchor> {
    let out = mapper::from_utf16(&file.emit.code, utf16_start);
    if mapper::to_source_inclusive(&file.emit.mappings, out).is_some() {
        return None;
    }
    file.emit.anchor_at(out).copied()
}

pub(crate) fn diagnostic_origin(
    file: &ProjectedDocument,
    utf16_start: usize,
    utf16_end: usize,
) -> Option<mapper::DiagnosticOrigin> {
    mapper::diagnostic_origin(
        &file.emit.mappings,
        &file.emit.anchors,
        mapper::from_utf16(&file.emit.code, utf16_start),
        mapper::from_utf16(&file.emit.code, utf16_end),
    )
}

fn source_extent(origin: mapper::DiagnosticOrigin) -> (usize, usize) {
    match origin {
        mapper::DiagnosticOrigin::Exact { start, end } => (start, end.max(start.saturating_add(1))),
        mapper::DiagnosticOrigin::Anchor(anchor) => (anchor.src, anchor.src_end),
        mapper::DiagnosticOrigin::Nearest { start } => (start, start.saturating_add(1)),
    }
}

/// Whether a checker diagnostic origin is already explained by a direct RL
/// cause. Display spans may be narrow; syntax-owner identity is what links
/// a cause to consequences emitted elsewhere in the same lowering.
pub(crate) fn origin_intersects_rl_error(
    origin: mapper::DiagnosticOrigin,
    rl_diagnostics: &[crate::Diagnostic],
) -> bool {
    let (start, end) = source_extent(origin);
    rl_diagnostics.iter().any(|rl| {
        if let (mapper::DiagnosticOrigin::Anchor(anchor), Some(owner)) = (origin, rl.owner)
            && owner.start == anchor.src
            && owner.end == anchor.owner_end
        {
            return true;
        }
        let Some(rl_start) = rl.start else {
            return false;
        };
        let rl_end = rl.end.unwrap_or_else(|| rl_start.saturating_add(1));
        start < rl_end && rl_start < end
    })
}

pub(crate) fn diagnostic_intersects_recovery(
    file: &ProjectedDocument,
    diagnostic: &crate::typescript::backend::Diagnostic,
) -> bool {
    let Some(origin) = diagnostic_origin(file, diagnostic.start, diagnostic.end) else {
        return false;
    };
    let (start, end) = source_extent(origin);
    file.recovered
        .iter()
        .any(|&(recovery_start, recovery_end)| start < recovery_end && recovery_start < end)
}

/// Whether a checker diagnostic covers source already owned by a direct RL
/// cause. The mismatch span, when available, is the checker's more precise
/// statement of where the consequence originated.
pub(crate) fn diagnostic_intersects_rl_error(
    file: &ProjectedDocument,
    diagnostic: &crate::typescript::backend::Diagnostic,
) -> bool {
    let (diagnostic_start, diagnostic_end) = diagnostic
        .mismatch
        .as_ref()
        .map_or((diagnostic.start, diagnostic.end), |mismatch| {
            (mismatch.start, mismatch.end)
        });
    let Some(origin) = diagnostic_origin(file, diagnostic_start, diagnostic_end) else {
        return false;
    };
    origin_intersects_rl_error(origin, &file.rl_diagnostics)
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
            mismatch: None,
        }
    }

    /// The declaration table the report path hands the translation.
    fn declarations(file: &ProjectedDocument) -> Vec<crate::analysis::DeclaredEnum> {
        crate::pattern_analyses(&file.source, &[]).declarations
    }

    fn project(source: &str) -> ProjectedDocument {
        ProjectedDocument::project(Path::new("/p/src/a.rl"), source.to_string()).expect("projects")
    }

    #[test]
    fn parser_error_nodes_recover_only_their_own_source_ranges() {
        let source = "const broken = 1 |> ;\n\
            const independent: string = 1;\n\
            const malformed = match value { Missing => 0 };\n";
        let file = project(source);
        assert_eq!(file.recovered.len(), 2, "{:#?}", file.recovered);
        assert!(file.emit.code.contains("const independent: string = 1;"));
        let independent = source.find("independent").unwrap();
        assert!(
            file.recovered
                .iter()
                .all(|&(start, end)| independent < start || independent >= end)
        );
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
        let (anchor, said) =
            translate_on_glue(&file, &diagnostic, &declarations(&file)).expect("translated");
        // The propagation itself is the span — not the whole declaration,
        // and not one character of it.
        assert_eq!(&file.source[anchor.src..anchor.src_end], "try plain()");
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
        assert!(translate_on_glue(&file, &diagnostic, &declarations(&file)).is_none());
    }

    #[test]
    fn an_unrecognized_code_on_glue_is_not_guessed_at() {
        let file = project("function f() {\n  const a = try plain();\n  return a;\n}\n");
        let diagnostic = ts_at(&file, "$rl_t0.kind", 2739, "Type is missing properties.");
        assert!(translate_on_glue(&file, &diagnostic, &declarations(&file)).is_none());
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
        let (anchor, said) =
            translate_on_glue(&file, &diagnostic, &declarations(&file)).expect("translated");
        assert_eq!(&file.source[anchor.src..anchor.src_end], "match (e)");
        assert!(said.starts_with("match on a tag pattern"), "{said}");
    }

    #[test]
    fn a_recoverable_rl_error_does_not_block_the_projection() {
        // TASK-117 symptom 3: a duplicate arm used to fail the projection
        // (`Blocked`) and silence the file's typed diagnostics wholesale.
        // Now the file lowers, and the error rides along for the report.
        let file = project(
            "enum E { A(x: number), B }\n\
             const v = match (E.A(1)) { A(x) => x, A(x) => 0, B => 1 };\n",
        );
        assert!(file.emit.code.contains("switch ($rl_m.kind)"));
        assert_eq!(file.rl_diagnostics.len(), 1);
        assert_eq!(
            file.rl_diagnostics[0].code,
            crate::DiagnosticCode::MatchDuplicateArm
        );
    }

    #[test]
    fn resolution_diagnostics_ride_along_on_the_typed_path_too() {
        let file = project(
            "enum Shape { Circle(r: number), Empty }\n\
             const v = match (s) { Circel(r) => r, Empty => 0 };\n",
        );
        assert_eq!(file.rl_diagnostics.len(), 1);
        assert_eq!(
            file.rl_diagnostics[0].code,
            crate::DiagnosticCode::UnknownCase
        );
    }

    #[test]
    fn text_that_cannot_lower_gets_an_engine_only_error_node() {
        let file = project("const x = 1 |> ;\n");
        assert_eq!(file.recovered, [(10, 14)]);
        assert!(file.emit.code.contains("const x = 0"), "{}", file.emit.code);
        assert_eq!(file.rl_diagnostics.len(), 1);
        assert_eq!(
            file.rl_diagnostics[0].code,
            crate::DiagnosticCode::StrayPipe
        );
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
