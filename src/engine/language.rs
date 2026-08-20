//! The engine's language-service surface — editor semantics in rl's terms.
//!
//! Every question here arrives as a position **in an `.rl` source** and is
//! answered in the same coordinates: hover a value inside a `match` arm and
//! the range that comes back is in the file the user is looking at, never
//! in the TypeScript it lowered to. The whole journey — projection, the
//! byte-exact mappings, serving lowered modules to the TypeScript language
//! service, the completion probe for a construct the user has not finished
//! typing — happens inside the engine. A consumer (the VSCode adapter, or
//! anything else) converts these results to its protocol and nothing more.
//!
//! The TypeScript reach is [`crate::typescript::service`] (`tsgo --lsp`),
//! chosen feature by feature because the API server does not carry the
//! language-service surface yet — an implementation detail this module
//! hides completely (see `docs/design/lsp-architecture.md`).
//!
//! Projections here use [`crate::emit_mapped`], not the typed pipeline's
//! projection: a buffer mid-edit must still project (emit-map is
//! infallible), because the moment completion matters most is the moment
//! the buffer does not compile.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::project::Project;
use super::projection::module_path_of;
use crate::typescript::mapper;
use crate::typescript::service::{Service, file_uri, service_binary, uri_path};
use crate::{EmitMapping, MappedEmit};

/// A position in a document: zero-based line, UTF-16 code units — the LSP
/// convention, so an adapter converts nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    /// Zero-based line.
    pub line: u32,
    /// Zero-based character offset on the line, in UTF-16 code units.
    pub character: u32,
}

/// A range in a document, `[start, end)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    /// Where the range starts.
    pub start: Position,
    /// Where it ends (exclusive).
    pub end: Position,
}

/// A place in a file the user can open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    /// The file — an `.rl` source, or a hand-written TypeScript file.
    pub path: PathBuf,
    /// The range within it, in that file's own text.
    pub range: Range,
}

/// One reference to a symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    /// Where.
    pub location: Location,
    /// Whether this is the declaration. (The service does not mark it; the
    /// first result stands in, as it always has — presentation only.)
    pub is_definition: bool,
}

/// What hover shows: a code-ish signature, optional prose, and the span the
/// answer covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoverInfo {
    /// The signature, as the checker displays it.
    pub signature: String,
    /// Documentation prose, empty when there is none.
    pub documentation: String,
    /// The span the hover applies to, in the `.rl` source.
    pub range: Range,
}

/// One completion entry, in the raw terms the adapter ranks and renders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionItem {
    /// The name offered.
    pub label: String,
    /// The service's element kind, normalized to the same strings the
    /// editor has always mapped ("function", "method", "property", ...).
    pub kind: String,
    /// The service's own sort text (the adapter adds its layer prefix).
    pub sort_text: String,
}

/// A completion answer.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CompletionAnswer {
    /// The entries.
    pub items: Vec<CompletionItem>,
    /// Whether the service answered as a *member* completion — what tells a
    /// real member list from the global scope offered while recovering from
    /// unfinished rl syntax.
    pub member: bool,
    /// Set when the answer came from a completion probe; pass it back to
    /// [`Project::completion_resolve`] so the entry is resolved against the
    /// same probed text it was listed from.
    pub probe: Option<u64>,
}

/// The signature and documentation behind one completion entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionDetail {
    /// The declaration as the checker displays it.
    pub signature: String,
    /// The entry's JSDoc, empty when it has none.
    pub documentation: String,
}

/// One edit of a rename, in the target file's own coordinates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameEdit {
    /// Where the edit applies.
    pub location: Location,
    /// What the service wants written there, with [`RENAME_PLACEHOLDER`]
    /// standing in for the new name — a destructuring shorthand (what an rl
    /// pattern binding compiles to) expands to `field: <placeholder>`, and
    /// dropping that expansion would silently rebind a different field.
    /// `None` means the bare new name.
    pub new_text: Option<String>,
}

/// The name a rename asks the service for, so every edit's text can be read
/// as "the new name, in whatever shape this location needs it".
pub const RENAME_PLACEHOLDER: &str = "rlRenamePlaceholder";

/// One overload in a signature-help answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    /// The rendered signature.
    pub label: String,
    /// Its documentation, empty when there is none.
    pub documentation: String,
    /// The parameters, as `[start, end)` spans into `label`.
    pub parameters: Vec<SignatureParameter>,
}

/// One parameter of a [`Signature`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureParameter {
    /// `[start, end)` of the parameter inside the signature label.
    pub label: (u32, u32),
    /// The parameter's documentation, empty when there is none.
    pub documentation: String,
}

/// Signature help at a call site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureHelp {
    /// The overloads.
    pub signatures: Vec<Signature>,
    /// Which overload the cursor is in.
    pub active_signature: u32,
    /// Which parameter the cursor is at.
    pub active_parameter: u32,
}

/// One TypeScript diagnostic, mapped onto the `.rl` source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceDiagnostic {
    /// Where, in the `.rl` source.
    pub range: Range,
    /// The message, verbatim.
    pub message: String,
    /// TypeScript's error number, 0 when it had none.
    pub code: u32,
    /// True for a warning; everything else reported here is an error.
    pub warning: bool,
}

/// The live language-service half of a [`Project`]: the running `tsgo --lsp`
/// conversation and everything served into it.
#[derive(Debug)]
pub(crate) struct ServiceSession {
    client: Service,
    /// The text last served for each `.rl` file — the emitted TypeScript,
    /// or a probe standing in for it.
    served: HashMap<PathBuf, String>,
    /// Service projections by source path, reused while the text matches.
    docs: HashMap<PathBuf, Arc<ServiceDoc>>,
    /// The raw items of the last completion answer, so one can be resolved
    /// later: the server resolves the item it produced, not a name. Keyed
    /// by (file, asked offset, label).
    last_completion: HashMap<(PathBuf, usize, String), serde_json::Value>,
    /// The probe the last completion list was answered from, kept so
    /// resolving one of its items can install it again.
    last_probe: Option<ProbeDoc>,
    probe_count: u64,
}

/// One file's language-service projection: the source as it stands (open
/// buffer or disk), the TypeScript it emits, and the byte mappings between
/// them. Unlike the typed pipeline's projection this never fails — a buffer
/// mid-edit still projects, by the emit-map contract.
#[derive(Debug)]
pub(crate) struct ServiceDoc {
    source: String,
    code: String,
    mappings: Vec<EmitMapping>,
    /// The glue each construct wrote — what a diagnostic landing outside
    /// every mapping is *about* (`crate::EmitAnchor`).
    anchors: Vec<crate::EmitAnchor>,
}

/// A compiled completion probe: the buffer with `$rl_probe` spliced in at
/// the cursor, emitted, and the placeholder's position in that output.
#[derive(Debug, Clone)]
struct ProbeDoc {
    path: PathBuf,
    code: String,
    /// UTF-16 offset of the placeholder in `code` — where the service is
    /// asked.
    offset: usize,
    version: u64,
}

/// Inserted at the cursor to complete the construct being typed. `$`-led so
/// it cannot collide with the name the user is in the middle of typing.
const PROBE_NAME: &str = "$rl_probe";

impl Project {
    /// Hover at a position: the checker's answer first, rl's own match
    /// analysis second.
    ///
    /// The service answers everything it can see — which is everything the
    /// emit-map ties to the source. What it cannot see is a pattern binding
    /// inside an or-pattern (`A(x) | B(x)`): the emitted destructuring
    /// speaks for every alternative at once, so those spans map to nothing
    /// (mapping them to one alternative would let a rename rewrite that one
    /// alone). For those, [`crate::pattern_analyses`] knows the span and the
    /// alternative it belongs to, and the answer is still the checker's
    /// wherever possible: the alternative is *isolated* — the same
    /// serve-a-stand-in move as the completion probe — so the service sees
    /// a single-alternative pattern narrowed to that constructor, payload
    /// types instantiated and all. Only when the checker cannot be asked at
    /// all does the analysis' declared type answer (`Ok(None)` when it too
    /// knows nothing).
    pub fn hover(&mut self, path: &Path, position: Position) -> Result<Option<HoverInfo>, String> {
        let (doc, path) = match self.serve(path) {
            Ok(served) => served,
            // No toolchain to ask: rl's own declaration table still answers
            // pattern bindings, instead of failing hover outright.
            Err(error) => {
                return match self.declared_hover_unserved(path, position) {
                    Some(info) => Ok(Some(info)),
                    None => Err(error),
                };
            }
        };
        if let Some(info) = self.service_hover(&doc, &path, position)? {
            return Ok(Some(info));
        }
        self.match_binding_hover(&doc, &path, position)
    }

    /// The plain service hover: the signature TypeScript shows, mapped onto
    /// the `.rl` source. `Ok(None)` when there is nothing to show.
    fn service_hover(
        &mut self,
        doc: &Arc<ServiceDoc>,
        path: &Path,
        position: Position,
    ) -> Result<Option<HoverInfo>, String> {
        let doc = doc.clone();
        let path = path.to_path_buf();
        let session = self.session();
        let Some(at) = to_service(&doc, position) else {
            return Ok(None);
        };
        let uri = served_uri(&path);
        let hover = session.client.request(
            "textDocument/hover",
            serde_json::json!({
                "textDocument": { "uri": uri },
                "position": lsp_position(u16_position(&doc.code, at)),
            }),
        )?;
        let contents = match &hover["contents"] {
            serde_json::Value::String(s) => s.clone(),
            value => value["value"].as_str().unwrap_or_default().to_string(),
        };
        if contents.is_empty() {
            return Ok(None);
        }
        let (signature, documentation) = split_hover(&contents);
        if signature.is_empty() {
            return Ok(None);
        }
        let (start, end) = match hover.get("range").filter(|r| !r.is_null()) {
            Some(range) => (
                u16_offset(&doc.code, position_of(&range["start"])),
                u16_offset(&doc.code, position_of(&range["end"])),
            ),
            None => (at, at),
        };
        let Some((s, e)) = from_service_span(&doc, start, end) else {
            return Ok(None);
        };
        Ok(Some(HoverInfo {
            signature,
            documentation,
            range: source_range(&doc.source, s, e),
        }))
    }

    /// Hover answered from the match analysis, for positions the service
    /// could not see: a pattern binding span (isolate the alternative and
    /// ask the checker; fall back to the declared type), or a body
    /// reference of one (the merged declared type).
    fn match_binding_hover(
        &mut self,
        doc: &Arc<ServiceDoc>,
        path: &Path,
        position: Position,
    ) -> Result<Option<HoverInfo>, String> {
        let byte = source_byte(&doc.source, position);
        let analyses = analyses_of(&self.overlays, path, &doc.source);
        if let Some(binding) = analyses.binding_at(byte) {
            let range = source_range(
                &doc.source,
                mapper::to_utf16(&doc.source, binding.start),
                mapper::to_utf16(&doc.source, binding.end),
            );
            if binding.alternatives > 1
                && let Some(info) = self.isolated_alternative_hover(doc, path, binding, byte, range)
            {
                return Ok(Some(info));
            }
            return Ok(declared_binding_hover(binding, range));
        }
        // A body reference the service had no answer for (the or-pattern
        // destructuring is glue): the merged declared type.
        if let Some((binding, (start, end))) = analyses.body_binding_at(&doc.source, byte)
            && let Some(ty) = &binding.ty
        {
            return Ok(Some(HoverInfo {
                signature: format!("const {}: {}", binding.name, ty),
                documentation: String::new(),
                range: source_range(
                    &doc.source,
                    mapper::to_utf16(&doc.source, start),
                    mapper::to_utf16(&doc.source, end),
                ),
            }));
        }
        Ok(None)
    }

    /// Asks the service about one or-pattern alternative in isolation: the
    /// buffer with the alternative list replaced by this alternative alone
    /// is emitted and served in the document's stead — the completion
    /// probe's move — so the emitted destructuring is single-alternative,
    /// mapped, and narrowed to this constructor. The checker's answer is
    /// then the alternative's own payload type. `None` (never an error —
    /// the declared type still stands behind it) when the probe cannot be
    /// built or the service has nothing to say.
    fn isolated_alternative_hover(
        &mut self,
        doc: &Arc<ServiceDoc>,
        path: &Path,
        binding: &crate::PatternBinding,
        byte: usize,
        range: Range,
    ) -> Option<HoverInfo> {
        let (code, offset) = isolate_alternative(&doc.source, binding, byte)?;

        let uri = served_uri(path);
        let session = self.session();
        session.client.open(&uri, &code);
        let answer = session.client.request(
            "textDocument/hover",
            serde_json::json!({
                "textDocument": { "uri": uri },
                "position": lsp_position(u16_position(&code, offset)),
            }),
        );
        // The stand-in answered one question; the real projection is served
        // back before the answer is even read.
        session.client.open(&uri, &doc.code);
        session.served.insert(path.to_path_buf(), doc.code.clone());

        let hover = answer.ok()?;
        let contents = match &hover["contents"] {
            serde_json::Value::String(s) => s.clone(),
            value => value["value"].as_str().unwrap_or_default().to_string(),
        };
        let (signature, documentation) = split_hover(&contents);
        if signature.is_empty() {
            return None;
        }
        Some(HoverInfo {
            signature,
            documentation,
            // The span is the binding the user is looking at, not the
            // probe's — the probe was never their text.
            range,
        })
    }

    /// The declaration-table hover for a session whose toolchain could not
    /// be reached: the file is read as the editor sees it (overlay first)
    /// and only rl's own analysis answers.
    fn declared_hover_unserved(&mut self, path: &Path, position: Position) -> Option<HoverInfo> {
        let canonical = path.canonicalize().ok()?;
        let source = match self.overlays.get(&canonical) {
            Some(text) => text.clone(),
            None => std::fs::read_to_string(&canonical).ok()?,
        };
        let byte = source_byte(&source, position);
        let analyses = analyses_of(&self.overlays, &canonical, &source);
        let binding = analyses.binding_at(byte)?;
        let range = source_range(
            &source,
            mapper::to_utf16(&source, binding.start),
            mapper::to_utf16(&source, binding.end),
        );
        declared_binding_hover(binding, range)
    }

    /// Go to definition, every target already in its own file's coordinates.
    pub fn definition(&mut self, path: &Path, position: Position) -> Result<Vec<Location>, String> {
        let found = self.locations(
            path,
            position,
            "textDocument/definition",
            serde_json::json!({}),
        )?;
        if !found.is_empty() {
            return Ok(found);
        }
        // The service found nothing — for a name an or-pattern binds, the
        // target it resolved to is compiler glue, which navigation drops.
        // The match analysis knows the spans the user actually wrote: a
        // body reference goes to every alternative's binding; a binding is
        // its own declaration.
        self.match_binding_definitions(path, position)
    }

    /// Definition targets from the match analysis — the fallback for names
    /// the emitted glue owns. Empty when the position is not on one.
    fn match_binding_definitions(
        &mut self,
        path: &Path,
        position: Position,
    ) -> Result<Vec<Location>, String> {
        let (doc, path) = self.serve(path)?;
        let byte = source_byte(&doc.source, position);
        let analyses = analyses_of(&self.overlays, &path, &doc.source);
        let spans = match analyses.binding_at(byte) {
            Some(binding) => vec![(binding.start, binding.end)],
            None => analyses.body_definitions(&doc.source, byte),
        };
        Ok(spans
            .into_iter()
            .map(|(start, end)| Location {
                path: path.clone(),
                range: source_range(
                    &doc.source,
                    mapper::to_utf16(&doc.source, start),
                    mapper::to_utf16(&doc.source, end),
                ),
            })
            .collect())
    }

    /// Find references. `is_definition` marks the first result, as the
    /// editor has always presented it.
    pub fn references(
        &mut self,
        path: &Path,
        position: Position,
    ) -> Result<Vec<Reference>, String> {
        let locations = self.locations(
            path,
            position,
            "textDocument/references",
            serde_json::json!({ "context": { "includeDeclaration": true } }),
        )?;
        Ok(locations
            .into_iter()
            .enumerate()
            .map(|(index, location)| Reference {
                location,
                is_definition: index == 0,
            })
            .collect())
    }

    fn locations(
        &mut self,
        path: &Path,
        position: Position,
        method: &str,
        extra: serde_json::Value,
    ) -> Result<Vec<Location>, String> {
        let (doc, path) = self.serve(path)?;
        let Project {
            service, overlays, ..
        } = self;
        let session = service.as_mut().expect("serve started it");
        let Some(at) = to_service(&doc, position) else {
            return Ok(Vec::new());
        };
        let mut params = serde_json::json!({
            "textDocument": { "uri": served_uri(&path) },
            "position": lsp_position(u16_position(&doc.code, at)),
        });
        if let (Some(into), Some(from)) = (params.as_object_mut(), extra.as_object()) {
            for (key, value) in from {
                into.insert(key.clone(), value.clone());
            }
        }
        let answer = session.client.request(method, params)?;
        let raw: Vec<serde_json::Value> = match answer {
            serde_json::Value::Array(items) => items,
            serde_json::Value::Null => Vec::new(),
            one => vec![one],
        };
        let mut out = Vec::new();
        for location in raw {
            let Some(uri) = location["uri"].as_str() else {
                continue;
            };
            // Anything unmappable is dropped — a reference into glue is not
            // a place the user can go.
            if let Some(mapped) = map_target(session, overlays, uri, &location["range"]) {
                out.push(mapped);
            }
        }
        Ok(out)
    }

    /// Completions at a position. `member` says whether the *source* cursor
    /// sits at a member access (the adapter knows, from the rl syntax layer)
    /// — at a member access only a member answer means anything, and when
    /// the plain answer is not one, a probe mends the unfinished construct
    /// and asks again.
    pub fn completion(
        &mut self,
        path: &Path,
        position: Position,
        member: bool,
    ) -> Result<CompletionAnswer, String> {
        let (doc, path) = self.serve(path)?;
        let session = self.session();
        let plain = match to_service(&doc, position) {
            Some(at) => ts_completions(session, &path, at, &doc.code)?,
            None => CompletionAnswer::default(),
        };
        if !member {
            return Ok(plain);
        }
        if plain.member && !plain.items.is_empty() {
            return Ok(plain);
        }

        // The construct is unfinished (`x |> .`): splice the placeholder in,
        // emit, and ask at its mapped position. The probe stands in for the
        // buffer only for this question — the next serve restores the real
        // text — and no diagnostic is ever computed from it.
        let source_at = {
            let u16 = u16_offset(&doc.source, position);
            mapper::from_utf16(&doc.source, u16)
        };
        let Some(probe) = build_probe(&path, &doc.source, source_at, session.probe_count + 1)
        else {
            return Ok(if plain.member {
                plain
            } else {
                CompletionAnswer::default()
            });
        };
        session.probe_count += 1;
        session.client.open(&served_uri(&path), &probe.code);
        session.served.insert(path.clone(), probe.code.clone());
        let mut probed = ts_completions(session, &path, probe.offset, &probe.code)?;
        probed.probe = Some(probe.version);
        session.last_probe = Some(probe);
        Ok(if probed.member {
            probed
        } else {
            CompletionAnswer::default()
        })
    }

    /// The signature and documentation behind one completion entry, fetched
    /// when the consumer asks about the one entry the user is looking at.
    /// `probe` re-installs the probed text the entry was listed from;
    /// `Ok(None)` when that probe is gone (the buffer has moved on) or the
    /// entry cannot be resolved.
    pub fn completion_resolve(
        &mut self,
        path: &Path,
        position: Position,
        label: &str,
        probe: Option<u64>,
    ) -> Result<Option<CompletionDetail>, String> {
        let (doc, path) = self.serve(path)?;
        let session = self.session();
        let at = match probe {
            Some(version) => {
                let Some(installed) = session
                    .last_probe
                    .clone()
                    .filter(|p| p.version == version && p.path == path)
                else {
                    return Ok(None);
                };
                session.client.open(&served_uri(&path), &installed.code);
                session.served.insert(path.clone(), installed.code.clone());
                installed.offset
            }
            None => match to_service(&doc, position) {
                Some(at) => at,
                None => return Ok(None),
            },
        };
        let code = session
            .served
            .get(&path)
            .cloned()
            .unwrap_or_else(|| doc.code.clone());

        let key = (path.clone(), at, label.to_string());
        if !session.last_completion.contains_key(&key) {
            // The server resolves the item *it* produced, not a name, so the
            // list has to have been asked for first.
            let _ = ts_completions(session, &path, at, &code)?;
        }
        let Some(item) = session.last_completion.get(&key).cloned() else {
            return Ok(None);
        };
        let resolved = session.client.request("completionItem/resolve", item)?;
        if resolved.is_null() {
            return Ok(None);
        }
        Ok(Some(CompletionDetail {
            signature: resolved["detail"].as_str().unwrap_or_default().to_string(),
            documentation: docs_text(&resolved["documentation"]),
        }))
    }

    /// Rename: every edit, each mapped to the file the user can open — or
    /// `Ok(None)` when the rename cannot be done *whole*. An edit that lands
    /// in compiler-written glue, a target that is not a file, or an edit
    /// shape that cannot be accounted for refuses the entire rename: a
    /// program renamed by halves is corrupted, not renamed.
    pub fn rename(
        &mut self,
        path: &Path,
        position: Position,
    ) -> Result<Option<Vec<RenameEdit>>, String> {
        let (doc, path) = self.serve(path)?;
        let Project {
            service, overlays, ..
        } = self;
        let session = service.as_mut().expect("serve started it");
        let Some(at) = to_service(&doc, position) else {
            return Ok(None);
        };
        let uri = served_uri(&path);
        let lsp_at = lsp_position(u16_position(&doc.code, at));

        // The server's own "can this be renamed?" — a keyword or a literal
        // answers null, and forcing it would rename nothing while looking
        // like it worked.
        let prepared = session.client.request(
            "textDocument/prepareRename",
            serde_json::json!({ "textDocument": { "uri": uri }, "position": lsp_at }),
        )?;
        if prepared.is_null() {
            return Ok(None);
        }

        let edit = session.client.request(
            "textDocument/rename",
            serde_json::json!({
                "textDocument": { "uri": uri },
                "position": lsp_at,
                "newName": RENAME_PLACEHOLDER,
            }),
        )?;
        let Some(changes) = edit["changes"].as_object() else {
            return Ok(None);
        };

        let changes = changes.clone();
        let mut out = Vec::new();
        for (edited_uri, edits) in &changes {
            let Some(edits) = edits.as_array() else {
                return Ok(None);
            };
            for one in edits {
                let Some(location) = map_target(session, overlays, edited_uri, &one["range"])
                else {
                    return Ok(None);
                };
                let new_text = one["newText"].as_str().map(String::from);
                if let Some(text) = &new_text
                    && text != RENAME_PLACEHOLDER
                    && !text.contains(RENAME_PLACEHOLDER)
                {
                    // A shape we cannot account for — refusing beats
                    // silently rebinding a different field.
                    return Ok(None);
                }
                out.push(RenameEdit { location, new_text });
            }
        }
        Ok(if out.is_empty() { None } else { Some(out) })
    }

    /// Signature help at a call site.
    pub fn signature_help(
        &mut self,
        path: &Path,
        position: Position,
    ) -> Result<Option<SignatureHelp>, String> {
        let (doc, path) = self.serve(path)?;
        let session = self.session();
        let Some(at) = to_service(&doc, position) else {
            return Ok(None);
        };
        let help = session.client.request(
            "textDocument/signatureHelp",
            serde_json::json!({
                "textDocument": { "uri": served_uri(&path) },
                "position": lsp_position(u16_position(&doc.code, at)),
            }),
        )?;
        let Some(signatures) = help["signatures"].as_array().filter(|s| !s.is_empty()) else {
            return Ok(None);
        };
        Ok(Some(SignatureHelp {
            signatures: signatures
                .iter()
                .map(|signature| {
                    let label = signature["label"].as_str().unwrap_or_default().to_string();
                    Signature {
                        parameters: signature["parameters"]
                            .as_array()
                            .map(|parameters| {
                                parameters
                                    .iter()
                                    .map(|parameter| SignatureParameter {
                                        label: parameter_span(&label, &parameter["label"]),
                                        documentation: docs_text(&parameter["documentation"]),
                                    })
                                    .collect()
                            })
                            .unwrap_or_default(),
                        documentation: docs_text(&signature["documentation"]),
                        label,
                    }
                })
                .collect(),
            active_signature: help["activeSignature"].as_u64().unwrap_or(0) as u32,
            active_parameter: help["activeParameter"].as_u64().unwrap_or(0) as u32,
        }))
    }

    /// TypeScript's type errors for one file, mapped onto its `.rl` source.
    /// Exact source spans are reported as-is. Diagnostics that land in
    /// compiler-written glue are still surfaced at the nearest source
    /// construct, matching the batch typed-check path.
    pub fn service_diagnostics(&mut self, path: &Path) -> Result<Vec<ServiceDiagnostic>, String> {
        let (doc, path) = self.serve(path)?;
        let session = self.session();
        let answer = session.client.request(
            "textDocument/diagnostic",
            serde_json::json!({ "textDocument": { "uri": served_uri(&path) } }),
        )?;
        let items = answer["items"].as_array().cloned().unwrap_or_default();
        // TypeScript numbers its parse errors 1000–1999 and its checker
        // errors from 2000 up. A text with a parse error is not TypeScript
        // at all, and the checker's recovery invents errors all through it,
        // so none of them is reported.
        if items
            .iter()
            .any(|item| item["code"].as_u64().is_some_and(|code| code < 2000))
        {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for item in items {
            let severity = item["severity"].as_u64().unwrap_or(1);
            if severity > 2 {
                continue;
            }
            let start = u16_offset(&doc.code, position_of(&item["range"]["start"]));
            let end = u16_offset(&doc.code, position_of(&item["range"]["end"]));
            let Some((s, e, exact)) = diagnostic_source_span(&doc, start, end) else {
                continue;
            };
            // An empty span (an error at a position, not over one) would
            // render as an invisible squiggle; give it the character it
            // points at.
            let e = if e > s { e } else { s + 1 };
            let raw = item["message"].as_str().unwrap_or_default().to_string();
            let code = item["code"].as_u64().unwrap_or(0) as u32;
            // On glue, rlc says what the construct meant, at the
            // construct's own keyword — the same table the CLI reports
            // through, so the two surfaces cannot drift.
            if !exact
                && let Some(anchor) = glue_anchor(&doc, start)
                && let Some(said) = crate::engine::semantics::translate(anchor.kind, code, &raw)
            {
                let at = mapper::to_utf16(&doc.source, anchor.src);
                let range = source_range(&doc.source, at, at + 1);
                let entry = ServiceDiagnostic {
                    range,
                    message: said,
                    code,
                    warning: severity == 2,
                };
                // One construct's glue can draw several TypeScript errors
                // that all mean the same rl thing.
                if !out.contains(&entry) {
                    out.push(entry);
                }
                continue;
            }
            let mut message = raw;
            if !exact {
                message.push_str(" (in code rlc generated for this construct)");
            }
            out.push(ServiceDiagnostic {
                range: source_range(&doc.source, s, e),
                message,
                code,
                warning: severity == 2,
            });
        }
        Ok(out)
    }

    /// Starts (or reuses) the service session and serves `path` and its
    /// transitive `.rl` imports as the TypeScript they lower to. Returns the
    /// file's projection and its canonical path; the session is then live.
    fn serve(&mut self, path: &Path) -> Result<(Arc<ServiceDoc>, PathBuf), String> {
        let canonical = path
            .canonicalize()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if !self.service.as_ref().is_some_and(|s| s.client.alive()) {
            // (Re)start: the previous conversation, if any, is gone — served
            // state with it. The next questions rebuild both.
            ensure_std_module(&self.root);
            let binary = service_binary(&self.root)?;
            let client = Service::start(&binary, &self.root)?;
            self.service = Some(ServiceSession {
                client,
                served: HashMap::new(),
                docs: HashMap::new(),
                last_completion: HashMap::new(),
                last_probe: None,
                probe_count: 0,
            });
        }
        let Project {
            service, overlays, ..
        } = self;
        let session = service.as_mut().expect("just ensured");

        let doc = serve_one(session, overlays, &canonical)
            .ok_or_else(|| format!("cannot read {}", canonical.display()))?;

        // The `.rl` modules it imports are served too, transitively. That is
        // not an optimization: the server resolves `"./x.rl"` to `x.rl.ts`,
        // and that name only exists as a document *this session serves*.
        let mut seen: HashSet<PathBuf> = HashSet::from([canonical.clone()]);
        let mut stack = vec![(canonical.clone(), doc.clone())];
        while let Some((file, doc)) = stack.pop() {
            for import in crate::rl_imports(&doc.source) {
                let target = match file
                    .parent()
                    .unwrap_or(Path::new("."))
                    .join(&import.specifier)
                    .canonicalize()
                {
                    Ok(target) => target,
                    Err(_) => continue, // unresolvable — tsc's TS2307, not ours
                };
                if !seen.insert(target.clone()) {
                    continue;
                }
                if let Some(imported) = serve_one(session, overlays, &target) {
                    stack.push((target, imported));
                }
            }
        }
        Ok((doc, canonical))
    }

    /// The live session, after [`Project::serve`] has run.
    fn session(&mut self) -> &mut ServiceSession {
        self.service.as_mut().expect("serve started it")
    }
}

/// Serves one `.rl` file's projection, creating or refreshing it from the
/// overlay or the disk. `None` when the file cannot be read.
fn serve_one(
    session: &mut ServiceSession,
    overlays: &HashMap<PathBuf, String>,
    path: &Path,
) -> Option<Arc<ServiceDoc>> {
    let text = match overlays.get(path) {
        Some(text) => text.clone(),
        None => std::fs::read_to_string(path).ok()?,
    };
    let doc = match session.docs.get(path) {
        Some(doc) if doc.source == text => doc.clone(),
        _ => {
            let MappedEmit {
                code,
                mappings,
                anchors,
                ..
            } = crate::emit_mapped(&text);
            let doc = Arc::new(ServiceDoc {
                source: text,
                code,
                mappings,
                anchors,
            });
            session.docs.insert(path.to_path_buf(), doc.clone());
            doc
        }
    };
    if session.served.get(path) != Some(&doc.code) {
        session.client.open(&served_uri(path), &doc.code);
        session.served.insert(path.to_path_buf(), doc.code.clone());
    }
    Some(doc)
}

/// The URI an `.rl` file is served under: the lowered module's name, which
/// is what an `import "./x.rl"` resolves to.
fn served_uri(path: &Path) -> String {
    file_uri(&module_path_of(path))
}

/// Completions at a service offset, with the raw items cached for resolve.
fn ts_completions(
    session: &mut ServiceSession,
    path: &Path,
    at: usize,
    code: &str,
) -> Result<CompletionAnswer, String> {
    let answer = session.client.request(
        "textDocument/completion",
        serde_json::json!({
            "textDocument": { "uri": served_uri(path) },
            "position": lsp_position(u16_position(code, at)),
        }),
    )?;
    let items: Vec<serde_json::Value> = match answer {
        serde_json::Value::Array(items) => items,
        value => value["items"].as_array().cloned().unwrap_or_default(),
    };
    session.last_completion.clear();
    let mut entries = Vec::with_capacity(items.len());
    for item in items {
        let label = item["label"].as_str().unwrap_or_default().to_string();
        session
            .last_completion
            .insert((path.to_path_buf(), at, label.clone()), item.clone());
        entries.push(CompletionItem {
            kind: completion_kind(item["kind"].as_u64()),
            sort_text: item["sortText"].as_str().unwrap_or(&label).to_string(),
            label,
        });
    }
    Ok(CompletionAnswer {
        items: entries,
        // A member completion is one the server answered for a `.` — what
        // tells a real member list from the global scope.
        member: is_member_context(code, at),
        probe: None,
    })
}

/// The source byte a position names — the analysis speaks bytes, the
/// protocol UTF-16.
fn source_byte(source: &str, position: Position) -> usize {
    mapper::from_utf16(source, u16_offset(source, position))
}

/// The match analysis of one document, its imported declarations collected
/// the way the CLI collects them for sema: the file's direct relative `.rl`
/// imports, one hop, under their in-scope names — except the sources come
/// from the overlays first, as everywhere in the engine. Carried as
/// [`crate::EnumSymbol`]s because the analysis wants field types, not just
/// tags.
fn analyses_of(
    overlays: &HashMap<PathBuf, String>,
    path: &Path,
    source: &str,
) -> crate::PatternAnalyses {
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut externs: Vec<crate::EnumSymbol> = Vec::new();
    for import in crate::rl_imports(source) {
        if matches!(import.names, crate::RlImportNames::None) {
            continue; // a re-export brings nothing into scope
        }
        let target = match dir.join(&import.specifier).canonicalize() {
            Ok(target) => target,
            Err(_) => continue, // unresolvable — tsc's TS2307, not ours
        };
        let text = match overlays.get(&target) {
            Some(text) => text.clone(),
            None => match std::fs::read_to_string(&target) {
                Ok(text) => text,
                Err(_) => continue,
            },
        };
        let decls: Vec<crate::EnumSymbol> = crate::enum_symbols(&text)
            .into_iter()
            .filter(|d| d.exported)
            .collect();
        match &import.names {
            crate::RlImportNames::Namespace(ns) => {
                externs.extend(decls.into_iter().map(|mut d| {
                    d.name = format!("{ns}.{}", d.name);
                    d
                }));
            }
            crate::RlImportNames::Named(entries) => {
                for (name, alias) in entries {
                    if let Some(d) = decls.iter().find(|d| &d.name == name) {
                        let mut d = d.clone();
                        d.name = alias.clone().unwrap_or_else(|| name.clone());
                        externs.push(d);
                    }
                }
            }
            crate::RlImportNames::None => unreachable!("skipped above"),
        }
    }
    crate::pattern_analyses(source, &externs)
}

/// The isolated-alternative stand-in: the source with `binding`'s whole
/// alternative list replaced by this occurrence's own alternative, emitted
/// — and the hovered byte's UTF-16 offset in that output. Isolation makes
/// codegen take its single-alternative path, whose destructuring is mapped
/// and narrowed to the one constructor, so the checker's answer at the
/// offset is that alternative's own payload type. `None` when the spans do
/// not line up (a stale analysis) or the byte lands in glue anyway.
fn isolate_alternative(
    source: &str,
    binding: &crate::PatternBinding,
    byte: usize,
) -> Option<(String, usize)> {
    let ordered = binding.group_start <= binding.alt_start
        && binding.alt_start <= binding.start
        && binding.start <= binding.end
        && binding.end <= binding.alt_end
        && binding.alt_end <= binding.group_end
        && binding.group_end <= source.len();
    if !ordered {
        return None;
    }
    let mut synthetic = String::with_capacity(source.len());
    synthetic.push_str(&source[..binding.group_start]);
    synthetic.push_str(&source[binding.alt_start..binding.alt_end]);
    synthetic.push_str(&source[binding.group_end..]);
    let emit = crate::emit_mapped(&synthetic);
    // The hovered byte, relocated into the isolated pattern.
    let at = binding.group_start + (byte.clamp(binding.start, binding.end) - binding.alt_start);
    let out = mapper::to_output_inclusive(&emit.mappings, at)?;
    let offset = mapper::to_utf16(&emit.code, out);
    Some((emit.code, offset))
}

/// The declared-type hover of one pattern binding — the analysis' own
/// answer, shown when the checker cannot be asked. `None` when the subject
/// is unknown (an unknown type would be a claim, not an answer).
fn declared_binding_hover(binding: &crate::PatternBinding, range: Range) -> Option<HoverInfo> {
    let ty = binding.ty.as_deref()?;
    let case = match &binding.enum_name {
        Some(enum_name) => format!("{enum_name}.{}", binding.tag),
        None => binding.tag.clone(),
    };
    Some(HoverInfo {
        signature: format!("const {}: {}", binding.name, ty),
        documentation: format!("Pattern binding of `{case}` (declared type)."),
        range,
    })
}

/// Builds a completion probe: the source with the placeholder spliced in at
/// `at` (a byte offset), emitted, and the placeholder's mapped position.
/// `None` when the buffer is broken somewhere a placeholder does not reach.
fn build_probe(path: &Path, source: &str, at: usize, version: u64) -> Option<ProbeDoc> {
    if !source.is_char_boundary(at) {
        return None;
    }
    let spliced = format!("{}{}{}", &source[..at], PROBE_NAME, &source[at..]);
    let emit = crate::emit_mapped(&spliced);
    let out = mapper::to_output_inclusive(&emit.mappings, at)?;
    Some(ProbeDoc {
        path: path.to_path_buf(),
        offset: mapper::to_utf16(&emit.code, out),
        code: emit.code,
        version,
    })
}

/// Maps one service answer target back to a user-visible file. `None` when
/// the target is not a file, cannot be read, or the span has no source
/// counterpart — the caller decides whether that skips one result
/// (navigation) or refuses the whole operation (rename).
fn map_target(
    session: &mut ServiceSession,
    overlays: &HashMap<PathBuf, String>,
    uri: &str,
    range: &serde_json::Value,
) -> Option<Location> {
    let path = uri_path(uri)?;
    let lsp_range = Range {
        start: position_of(&range["start"]),
        end: position_of(&range["end"]),
    };
    let name = path.to_string_lossy();
    if let Some(rl) = name.strip_suffix(".ts").filter(|n| n.ends_with(".rl")) {
        let rl_path = PathBuf::from(rl);
        let doc = serve_doc_only(session, overlays, &rl_path)?;
        let start = u16_offset(&doc.code, lsp_range.start);
        let end = u16_offset(&doc.code, lsp_range.end);
        let (s, e) = from_service_span(&doc, start, end)?;
        return Some(Location {
            path: rl_path,
            range: source_range(&doc.source, s, e),
        });
    }
    // A hand-written TypeScript file: the answer's coordinates are already
    // the file's own.
    Some(Location {
        path,
        range: lsp_range,
    })
}

/// A projection for mapping an answer's coordinates — built (and cached)
/// without serving, for targets the question never travelled through.
fn serve_doc_only(
    session: &mut ServiceSession,
    overlays: &HashMap<PathBuf, String>,
    path: &Path,
) -> Option<Arc<ServiceDoc>> {
    let text = match overlays.get(path) {
        Some(text) => text.clone(),
        None => std::fs::read_to_string(path).ok()?,
    };
    match session.docs.get(path) {
        Some(doc) if doc.source == text => Some(doc.clone()),
        _ => {
            let MappedEmit {
                code,
                mappings,
                anchors,
                ..
            } = crate::emit_mapped(&text);
            let doc = Arc::new(ServiceDoc {
                source: text,
                code,
                mappings,
                anchors,
            });
            session.docs.insert(path.to_path_buf(), doc.clone());
            Some(doc)
        }
    }
}

/// An rl position translated into the served text, or `None` when it sits
/// in compiler-written glue.
fn to_service(doc: &ServiceDoc, position: Position) -> Option<usize> {
    let u16 = u16_offset(&doc.source, position);
    let byte = mapper::from_utf16(&doc.source, u16);
    let out = mapper::to_output_inclusive(&doc.mappings, byte)?;
    Some(mapper::to_utf16(&doc.code, out))
}

/// A service span translated back to source UTF-16 offsets, or `None` when
/// either end has no source counterpart.
fn from_service_span(doc: &ServiceDoc, start: usize, end: usize) -> Option<(usize, usize)> {
    let sb = mapper::from_utf16(&doc.code, start);
    let eb = mapper::from_utf16(&doc.code, end);
    let ss = mapper::to_source_inclusive(&doc.mappings, sb)?;
    let se = mapper::to_source_inclusive(&doc.mappings, eb)?;
    if se < ss {
        return None;
    }
    Some((
        mapper::to_utf16(&doc.source, ss),
        mapper::to_utf16(&doc.source, se),
    ))
}

/// A TypeScript diagnostic span translated back to source UTF-16 offsets.
///
/// Unlike navigation and rename, diagnostics on generated glue are still
/// useful: the CLI reports them at the construct that produced the glue, so
/// the language service follows the same policy.
/// The construct whose glue a served-text UTF-16 offset falls in.
fn glue_anchor(doc: &ServiceDoc, utf16_start: usize) -> Option<crate::EmitAnchor> {
    let out = mapper::from_utf16(&doc.code, utf16_start);
    doc.anchors
        .iter()
        .find(|a| a.out <= out && out < a.end)
        .copied()
}

fn diagnostic_source_span(
    doc: &ServiceDoc,
    start: usize,
    end: usize,
) -> Option<(usize, usize, bool)> {
    let sb = mapper::from_utf16(&doc.code, start);
    let eb = mapper::from_utf16(&doc.code, end);
    if let (Some(ss), Some(se)) = (
        mapper::to_source_inclusive(&doc.mappings, sb),
        mapper::to_source_inclusive(&doc.mappings, eb),
    ) && se >= ss
    {
        return Some((
            mapper::to_utf16(&doc.source, ss),
            mapper::to_utf16(&doc.source, se),
            true,
        ));
    }

    let (ss, exact) = mapper::to_source_or_nearest(&doc.mappings, sb)?;
    let s = mapper::to_utf16(&doc.source, ss);
    Some((s, s + 1, exact))
}

/// A `[start, end)` pair of UTF-16 offsets as a [`Range`] over `text`.
fn source_range(text: &str, start: usize, end: usize) -> Range {
    Range {
        start: u16_position(text, start),
        end: u16_position(text, end),
    }
}

/// The UTF-16 offset a zero-based line/character names in `text` — the LSP
/// convention: a character past the line's end spills forward, and both
/// clamp to the text's end.
pub(crate) fn u16_offset(text: &str, position: Position) -> usize {
    let mut line = 0u32;
    let mut u16 = 0usize;
    let mut line_start = 0usize;
    if position.line > 0 {
        for ch in text.chars() {
            u16 += ch.len_utf16();
            if ch == '\n' {
                line += 1;
                line_start = u16;
                if line == position.line {
                    break;
                }
            }
        }
        if line < position.line {
            return text.encode_utf16().count();
        }
    }
    let total = text.encode_utf16().count();
    (line_start + position.character as usize).min(total)
}

/// The zero-based line/character a UTF-16 offset names in `text`.
pub(crate) fn u16_position(text: &str, offset: usize) -> Position {
    let mut u16 = 0usize;
    let mut line = 0u32;
    let mut line_start = 0usize;
    for ch in text.chars() {
        if u16 >= offset {
            break;
        }
        u16 += ch.len_utf16();
        if ch == '\n' {
            line += 1;
            line_start = u16;
        }
    }
    Position {
        line,
        character: (u16.min(offset).max(line_start) - line_start) as u32,
    }
}

/// Whether the offset follows a `.` (walking back over identifier
/// characters), which is what makes an answer a member list.
fn is_member_context(text: &str, offset: usize) -> bool {
    let mut i = mapper::from_utf16(text, offset);
    let bytes = text.as_bytes();
    while i > 0 {
        let b = bytes[i - 1];
        if b.is_ascii_alphanumeric() || b == b'_' || b == b'$' {
            i -= 1;
        } else {
            break;
        }
    }
    i > 0 && bytes[i - 1] == b'.'
}

/// Splits hover contents into the signature and the prose under it: the
/// first fenced block is the signature, else the first paragraph.
fn split_hover(contents: &str) -> (String, String) {
    let trimmed = contents.trim();
    if let Some(rest) = trimmed.strip_prefix("```") {
        // ```lang\n ... \n``` and whatever prose follows.
        if let Some(newline) = rest.find('\n') {
            let body = &rest[newline + 1..];
            if let Some(close) = body.find("\n```") {
                let signature = body[..close].trim().to_string();
                let documentation = body[close + 4..].trim().to_string();
                return (signature, documentation);
            }
        }
    }
    match trimmed.split_once("\n\n") {
        Some((first, rest)) => (first.trim().to_string(), rest.trim().to_string()),
        None => (trimmed.to_string(), String::new()),
    }
}

/// Documentation as plain text, whichever shape the server used.
fn docs_text(documentation: &serde_json::Value) -> String {
    match documentation {
        serde_json::Value::String(s) => s.trim().to_string(),
        value => value["value"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_string(),
    }
}

/// Where a parameter's label sits inside its signature — the span form the
/// presentation needs, computed from the substring form when the server
/// used that.
fn parameter_span(signature: &str, label: &serde_json::Value) -> (u32, u32) {
    if let Some(span) = label.as_array()
        && span.len() == 2
    {
        return (
            span[0].as_u64().unwrap_or(0) as u32,
            span[1].as_u64().unwrap_or(0) as u32,
        );
    }
    let Some(text) = label.as_str() else {
        return (0, 0);
    };
    // The span is in UTF-16 units of the label string.
    match signature.find(text) {
        Some(byte) => {
            let start = signature[..byte].encode_utf16().count();
            (start as u32, (start + text.encode_utf16().count()) as u32)
        }
        None => (0, 0),
    }
}

/// The LSP completion kinds the server answers with, as the element-kind
/// strings the editor has always mapped. Anything else is a plain property.
fn completion_kind(kind: Option<u64>) -> String {
    match kind {
        Some(3) => "function",
        Some(2) | Some(4) => "method",
        Some(5) => "property",
        Some(6) => "var",
        Some(7) | Some(22) => "class",
        Some(8) => "interface",
        Some(9) => "module",
        Some(13) => "enum",
        Some(14) => "keyword",
        Some(21) => "const",
        Some(25) => "type",
        _ => "property",
    }
    .to_string()
}

/// A [`Position`] as the JSON the protocol speaks.
fn lsp_position(position: Position) -> serde_json::Value {
    serde_json::json!({ "line": position.line, "character": position.character })
}

/// A JSON position as a [`Position`].
fn position_of(value: &serde_json::Value) -> Position {
    Position {
        line: value["line"].as_u64().unwrap_or(0) as u32,
        character: value["character"].as_u64().unwrap_or(0) as u32,
    }
}

/// Makes `"@rl/std"` resolvable in `root`, by putting the standard library
/// where the TypeScript server looks for a package of that name.
///
/// The compiler backend serves the library from memory; a language server
/// cannot — module resolution reads the file system. So for the service the
/// library has to *be* there. Only rl's own scoped package is ever written,
/// and never over one that already exists: a project that installs
/// `@rl/std` itself keeps its own copy.
fn ensure_std_module(root: &Path) {
    let pkg = root.join("node_modules/@rl/std");
    let entry = pkg.join("index.ts");
    if entry.exists() || pkg.exists() {
        return;
    }
    if std::fs::create_dir_all(&pkg).is_err() {
        return;
    }
    let source = format!(
        "// @generated by rlc --emit-std — do not edit directly.\n{}",
        crate::STD_SOURCE
    );
    let _ = std::fs::write(&entry, source);
    let _ = std::fs::write(
        pkg.join("package.json"),
        "{\n  \"name\": \"@rl/std\",\n  \"version\": \"0.0.0\",\n  \"types\": \"index.ts\"\n}\n",
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u16_positions_round_trip_over_multibyte_text() {
        let text = "한글\nab한c\n";
        // Offsets count UTF-16 units: each Hangul syllable is one unit.
        assert_eq!(
            u16_offset(
                text,
                Position {
                    line: 1,
                    character: 3
                }
            ),
            6
        );
        assert_eq!(
            u16_position(text, 6),
            Position {
                line: 1,
                character: 3
            }
        );
        // Past-the-line characters spill forward, clamped to the end.
        assert_eq!(
            u16_offset(
                text,
                Position {
                    line: 9,
                    character: 0
                }
            ),
            8
        );
    }

    #[test]
    fn a_chunk_end_offset_belongs_to_the_chunk() {
        // The service lookup is inclusive of a chunk's end — completion and
        // hover sit at the end of what was just typed.
        let mappings = [crate::EmitMapping {
            src: 0,
            out: 10,
            len: 5,
        }];
        assert_eq!(mapper::to_output_inclusive(&mappings, 5), Some(15));
        assert_eq!(mapper::to_source_inclusive(&mappings, 15), Some(5));
        // ... and one past it is glue.
        assert_eq!(mapper::to_output_inclusive(&mappings, 6), None);
    }

    #[test]
    fn isolating_an_alternative_maps_its_binding_into_narrowed_output() {
        let src =
            "enum E { A(x: string), B(x: number) }\nconst v = match (e) { A(x) | B(x) => x };\n";
        let analyses = crate::pattern_analyses(src, &[]);
        let b_x = src.find("B(x)").unwrap() + 2;
        let binding = analyses.binding_at(b_x).unwrap().clone();
        let (code, offset) = isolate_alternative(src, &binding, b_x).unwrap();
        // The or-arm became a single `B(x)` arm — the emitted switch
        // narrows to `B` alone...
        assert!(code.contains("case \"B\""), "{code}");
        assert!(!code.contains("case \"A\""), "{code}");
        // ...and the question lands on the (now mapped) destructured `x`.
        let byte = mapper::from_utf16(&code, offset);
        assert_eq!(&code[byte..byte + 1], "x");
        assert!(code[..byte].ends_with("const { "), "{code}");

        // The A occurrence isolates to the A arm the same way.
        let a_x = src.find("A(x)").unwrap() + 2;
        let binding = analyses.binding_at(a_x).unwrap().clone();
        let (code, _) = isolate_alternative(src, &binding, a_x).unwrap();
        assert!(code.contains("case \"A\""), "{code}");
        assert!(!code.contains("case \"B\""), "{code}");
    }

    #[test]
    fn declared_hover_names_the_constructor_and_its_type() {
        let src =
            "enum E { A(x: string), B(x: number) }\nconst v = match (e) { A(x) | B(x) => x };\n";
        let analyses = crate::pattern_analyses(src, &[]);
        let binding = analyses.binding_at(src.find("B(x)").unwrap() + 2).unwrap();
        let range = source_range(src, 0, 1);
        let info = declared_binding_hover(binding, range).unwrap();
        assert_eq!(info.signature, "const x: number");
        assert!(
            info.documentation.contains("`E.B`"),
            "{}",
            info.documentation
        );

        // An unresolved subject answers nothing rather than guessing.
        let unknown = "const v = match (e) { What(x) | Ever(x) => x };\n";
        let analyses = crate::pattern_analyses(unknown, &[]);
        let binding = analyses
            .binding_at(unknown.find("What(x)").unwrap() + 5)
            .unwrap();
        assert!(declared_binding_hover(binding, range).is_none());
    }

    #[test]
    fn analyses_collect_imported_declarations_like_the_cli() {
        let dir = std::env::temp_dir().join(format!("rl-analyses-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("token.rl"),
            "export enum Token { Num(value: number), Eof }\n",
        )
        .unwrap();
        let source = "import { Token as T } from \"./token.rl\";\nconst v = match (t) { Num(value) | Eof => 0 };\n";
        let main = dir.join("main.rl");
        std::fs::write(&main, source).unwrap();

        // The disk copy answers...
        let analyses = analyses_of(&HashMap::new(), &main, source);
        let binding = analyses
            .binding_at(source.find("Num(value)").unwrap() + 4)
            .unwrap();
        assert_eq!(binding.ty.as_deref(), Some("number"));
        assert_eq!(binding.enum_name.as_deref(), Some("T"));

        // ...and an overlay of the imported file wins over its disk copy.
        let mut overlays = HashMap::new();
        overlays.insert(
            dir.join("token.rl").canonicalize().unwrap(),
            "export enum Token { Num(value: string), Eof }\n".to_string(),
        );
        let analyses = analyses_of(&overlays, &main, source);
        let binding = analyses
            .binding_at(source.find("Num(value)").unwrap() + 4)
            .unwrap();
        assert_eq!(binding.ty.as_deref(), Some("string"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn member_context_walks_back_over_the_identifier() {
        assert!(is_member_context("value.le", 8));
        assert!(is_member_context("value.", 6));
        assert!(!is_member_context("value", 5));
        assert!(!is_member_context("a . b", 1));
    }
}
