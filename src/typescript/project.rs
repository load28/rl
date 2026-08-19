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
pub(crate) fn lower(files: &[PathBuf]) -> Result<Vec<Lowered>, (PathBuf, rlc::CompileError)> {
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
        let options = Options {
            filename: Some(file.to_str().unwrap_or("<input>")),
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

/// Builds the batch of questions the whole project asks in one round trip.
///
/// Every question is anchored at a byte the compiler can see: a probe whose
/// anchor did not survive lowering as verbatim text (a nested rl construct)
/// is dropped rather than asked about at an approximate position.
pub(crate) fn query(lowered: &[Lowered]) -> (Query, Probes) {
    let mut query = Query::default();
    let mut probes = Probes::default();

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

        for call in rlc::val_method_calls(&file.source) {
            let Some(position) = anchor(&file.emit, call.name) else {
                continue;
            };
            query.vals.push(ValQuery {
                module: file.module_path.clone(),
                position,
            });
            probes.vals.push(ValAnchor {
                anchor: SourceAnchor {
                    source_path: file.source_path.clone(),
                    offset: call.offset,
                },
                binding: call.binding,
                method: call.method,
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

/// The `.rl`-side half of a `val` question, kept for the message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValAnchor {
    pub anchor: SourceAnchor,
    pub binding: String,
    pub method: String,
}

/// The `.rl`-side halves of a [`Query`], parallel to its own vectors.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Probes {
    pub literals: Vec<SourceAnchor>,
    pub vals: Vec<ValAnchor>,
}

/// The UTF-16 offset in the emitted module a source byte landed at, or
/// `None` when it was not copied verbatim.
fn anchor(emit: &MappedEmit, source_byte: usize) -> Option<usize> {
    let out = mapper::to_output(&emit.mappings, source_byte)?;
    Some(mapper::to_utf16(&emit.code, out))
}

/// Where a TypeScript diagnostic belongs in the `.rl` source, or `None` when
/// it landed on compiler-written glue — which, by the error-layer contract,
/// is an rlc bug rather than something to report at a made-up position.
pub(crate) fn diagnostic_source_offset(file: &Lowered, utf16_start: usize) -> Option<usize> {
    let out = mapper::from_utf16(&file.emit.code, utf16_start);
    mapper::to_source(mappings(&file.emit), out)
}

fn mappings(emit: &MappedEmit) -> &[EmitMapping] {
    &emit.mappings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lowered_module_takes_its_source_path_with_a_ts_extension() {
        assert_eq!(
            Lowered::module_path_of(Path::new("/p/src/state.rl")),
            PathBuf::from("/p/src/state.ts"),
        );
    }
}
