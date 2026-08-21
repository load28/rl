//! `rlc --server` — the engine behind a pipe, for tools that ask often.
//!
//! An editor asks the compiler the same three questions on every keystroke:
//! "does this buffer pass `--check`?", "what does it emit?" and "what does
//! the typed layer say?". Answering each by spawning a process is fine for
//! the first two and ruinous for the third — a typed check opens a project
//! and starts a TypeScript compiler. This mode keeps one `rlc` process
//! alive and, behind it, one [`rlc::engine::Project`] per project identity,
//! so a typed check after the first reuses the running compiler and every
//! unchanged file's projection.
//!
//! The protocol is one JSON object per line, on stdin and stdout:
//!
//! ```text
//! → { "id": 1, "method": "check", "params": { "text", "filename"?, "verify"? } }
//! ← { "id": 1, "result": { "diagnostics":
//!        [{ "line", "col", "endLine", "endCol", "message" }] } }
//!
//! → { "id": 2, "method": "emitMap", "params": { "text" } }
//! ← { "id": 2, "result": { "code", "mappings": [{ "src", "out", "len" }] } }
//!
//! → { "id": 3, "method": "typedCheck", "params": { "path", "text" } }
//! ← { "id": 3, "result": { "blocked", "diagnostics":
//!        [{ "path", "line", "col", "endLine", "endCol", "message" }] } }
//!
//! → { "id": 4, "method": "semanticTokens", "params": { "text" } }
//! ← { "id": 4, "result": { "tokens": [{ "range", "kind" }] } }
//!
//! → { "id": 5, "method": "rlSymbol", "params": { "path", "text", "position" } }
//! ← { "id": 5, "result": { "kind", "range", "name", "enumName",
//!                          "signature", "detail", "definition" } | null }
//!
//! → { "id": 6, "method": "rlCompletions", "params": { "path", "text", "position" } }
//! ← { "id": 6, "result": { "items": [{ "label", "kind", "detail", "covered" }] } }
//!
//! → { "id": 7, "method": "rlHints", "params": { "path", "text" } }
//! ← { "id": 7, "result": { "hints": [{ "kind", "range", "message" }] } }
//!
//! → { "id": 8, "method": "declarations", "params": { "path", "text" } }
//! ← { "id": 8, "result": { "enums": [{ "name", "generics", "origin",
//!        "specifier", "nameSpan", "span", "cases" }],
//!        "matches": [{ "keyword", "bodyClose" }] } }
//!
//! ← { "id": N, "error": "sentence" }   // the request failed; the session lives
//! ```
//!
//! Every answer is computed by the same code the one-shot modes run —
//! `check` is [`rlc::compile`] with the caller's text standing alone (its
//! relative imports unresolvable, exactly like the one-shot's temp file),
//! `emitMap` is [`rlc::emit_mapped`], and `typedCheck` is the engine's
//! rl-only pass with the buffer as an overlay — so a consumer that falls
//! back from the server to the one-shot commands sees the same diagnostics
//! either way. A `typedCheck` overlay lasts one request: the answer is
//! stateless, the reuse (projection cache, running compiler) is not.
//!
//! Exit: end of stdin, code 0. A failed request never ends the session.

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use rlc::engine::{
    CheckRequest, CompletionAnswer, Engine, Location, Position, Project, ProjectOptions, Range,
};

/// A project's identity: the `(tsconfig, root)` pair it was opened as.
type Identity = (Option<PathBuf>, PathBuf);

/// Everything the server keeps between requests.
struct Sessions {
    engine: Engine,
    /// One live project per identity — the map a server exists to keep.
    projects: HashMap<Identity, Project>,
    /// The documents a consumer holds open, and which project each landed
    /// in — so `closeDocument` releases the right overlay, and a
    /// `typedCheck` for an open document leaves its overlay in place.
    docs: HashMap<PathBuf, Identity>,
}

/// Runs the server until stdin closes.
pub(crate) fn run(node: Option<PathBuf>) -> ExitCode {
    let mut sessions = Sessions {
        engine: Engine::new(node),
        projects: HashMap::new(),
        docs: HashMap::new(),
    };

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }
        let response = respond(&mut sessions, &line);
        let mut out = stdout.lock();
        if writeln!(out, "{response}")
            .and_then(|_| out.flush())
            .is_err()
        {
            break; // the consumer is gone
        }
    }
    ExitCode::SUCCESS
}

/// One request, one answer — errors included, so the session survives them.
fn respond(sessions: &mut Sessions, line: &str) -> serde_json::Value {
    use serde_json::json;
    let request: serde_json::Value = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(e) => return json!({ "id": null, "error": format!("malformed request: {e}") }),
    };
    let id = request["id"].clone();
    let params = &request["params"];
    let result = match request["method"].as_str().unwrap_or_default() {
        "check" => check(params),
        "emitMap" => emit_map(params),
        "typedCheck" => typed_check(sessions, params),
        "openDocument" | "updateDocument" => open_document(sessions, params),
        "closeDocument" => close_document(sessions, params),
        "hover" => semantic(sessions, params, |project, path, position| {
            Ok(match project.hover(path, position)? {
                None => serde_json::Value::Null,
                Some(info) => json!({
                    "signature": info.signature,
                    "documentation": info.documentation,
                    "range": range_json(info.range),
                }),
            })
        }),
        "definition" => semantic(sessions, params, |project, path, position| {
            let locations: Vec<_> = project
                .definition(path, position)?
                .into_iter()
                .map(location_json)
                .collect();
            Ok(json!({ "locations": locations }))
        }),
        "references" => semantic(sessions, params, |project, path, position| {
            let locations: Vec<_> = project
                .references(path, position)?
                .into_iter()
                .map(|reference| {
                    let mut value = location_json(reference.location);
                    value["isDefinition"] = json!(reference.is_definition);
                    value
                })
                .collect();
            Ok(json!({ "locations": locations }))
        }),
        "completion" => semantic(sessions, params, |project, path, position| {
            let member = params["member"].as_bool().unwrap_or(false);
            let CompletionAnswer {
                items,
                member,
                probe,
            } = project.completion(path, position, member)?;
            Ok(json!({
                "items": items.iter().map(|item| json!({
                    "label": item.label,
                    "kind": item.kind,
                    "sortText": item.sort_text,
                })).collect::<Vec<_>>(),
                "member": member,
                "probe": probe,
            }))
        }),
        "completionResolve" => semantic(sessions, params, |project, path, position| {
            let label = params["label"].as_str().unwrap_or_default();
            let probe = params["probe"].as_u64();
            Ok(
                match project.completion_resolve(path, position, label, probe)? {
                    None => serde_json::Value::Null,
                    Some(detail) => json!({
                        "signature": detail.signature,
                        "documentation": detail.documentation,
                    }),
                },
            )
        }),
        "rename" => semantic(sessions, params, |project, path, position| {
            Ok(match project.rename(path, position)? {
                None => json!({ "edits": serde_json::Value::Null }),
                Some(edits) => json!({
                    "edits": edits.into_iter().map(|edit| {
                        let mut value = location_json(edit.location);
                        value["newText"] = match edit.new_text {
                            Some(text) => json!(text),
                            None => serde_json::Value::Null,
                        };
                        value
                    }).collect::<Vec<_>>(),
                }),
            })
        }),
        "signatureHelp" => semantic(sessions, params, |project, path, position| {
            Ok(match project.signature_help(path, position)? {
                None => serde_json::Value::Null,
                Some(help) => json!({
                    "signatures": help.signatures.iter().map(|signature| json!({
                        "label": signature.label,
                        "documentation": signature.documentation,
                        "parameters": signature.parameters.iter().map(|parameter| json!({
                            "label": [parameter.label.0, parameter.label.1],
                            "documentation": parameter.documentation,
                        })).collect::<Vec<_>>(),
                    })).collect::<Vec<_>>(),
                    "activeSignature": help.active_signature,
                    "activeParameter": help.active_parameter,
                }),
            })
        }),
        "semanticTokens" => semantic_tokens(params),
        "declarations" => declarations(params),
        "rlSymbol" => rl_symbol(params),
        "rlCompletions" => rl_completions(params),
        "rlHints" => rl_hints(params),
        "tsDiagnostics" => semantic(sessions, params, |project, path, _position| {
            let diagnostics: Vec<_> = project
                .service_diagnostics(path)?
                .into_iter()
                .map(|d| {
                    json!({
                        "range": range_json(d.range),
                        "message": d.message,
                        "code": d.code,
                        "warning": d.warning,
                    })
                })
                .collect();
            Ok(json!({ "diagnostics": diagnostics }))
        }),
        method => Err(format!("unknown method \"{method}\"")),
    };
    match result {
        Ok(result) => json!({ "id": id, "result": result }),
        Err(error) => json!({ "id": id, "error": error }),
    }
}

/// Routes a semantic request to the live project the file belongs to. The
/// position defaults to 0:0 for the requests that do not carry one.
fn semantic(
    sessions: &mut Sessions,
    params: &serde_json::Value,
    handle: impl FnOnce(&mut Project, &Path, Position) -> Result<serde_json::Value, String>,
) -> Result<serde_json::Value, String> {
    let path = params["path"]
        .as_str()
        .ok_or_else(|| "the request needs a \"path\"".to_string())?
        .to_string();
    let position = Position {
        line: params["position"]["line"].as_u64().unwrap_or(0) as u32,
        character: params["position"]["character"].as_u64().unwrap_or(0) as u32,
    };
    let project = project_for(sessions, &path)?;
    handle(project, Path::new(&path), position)
}

/// The live project `path` belongs to, opened on first use.
fn project_for<'a>(sessions: &'a mut Sessions, path: &str) -> Result<&'a mut Project, String> {
    let inputs = vec![path.to_string()];
    let options = ProjectOptions::default();
    let identity = Engine::project_identity(&inputs, &options)?;
    match sessions.projects.entry(identity) {
        std::collections::hash_map::Entry::Occupied(entry) => Ok(entry.into_mut()),
        std::collections::hash_map::Entry::Vacant(entry) => {
            Ok(entry.insert(sessions.engine.open_project(&inputs, &options)?))
        }
    }
}

/// `openDocument` / `updateDocument`: the consumer's buffer stands in for
/// the file, in whichever project it belongs to, until `closeDocument`.
fn open_document(
    sessions: &mut Sessions,
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let path = params["path"]
        .as_str()
        .ok_or_else(|| "the request needs a \"path\"".to_string())?
        .to_string();
    let text = text_param(params)?.to_string();
    let canonical = PathBuf::from(&path)
        .canonicalize()
        .map_err(|e| format!("{path}: {e}"))?;
    let inputs = vec![path.to_string()];
    let options = ProjectOptions::default();
    let identity = Engine::project_identity(&inputs, &options)?;
    let project = match sessions.projects.entry(identity.clone()) {
        std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
        std::collections::hash_map::Entry::Vacant(entry) => {
            entry.insert(sessions.engine.open_project(&inputs, &options)?)
        }
    };
    project.open_document(canonical.clone(), text);
    sessions.docs.insert(canonical, identity);
    Ok(serde_json::json!({}))
}

/// `closeDocument`: the file's text is the disk's again.
fn close_document(
    sessions: &mut Sessions,
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let path = params["path"]
        .as_str()
        .ok_or_else(|| "the request needs a \"path\"".to_string())?;
    let canonical = PathBuf::from(path)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(path));
    if let Some(identity) = sessions.docs.remove(&canonical)
        && let Some(project) = sessions.projects.get_mut(&identity)
    {
        project.close_document(&canonical);
    }
    Ok(serde_json::json!({}))
}

/// A [`Range`] as the JSON the protocol speaks.
fn range_json(range: Range) -> serde_json::Value {
    serde_json::json!({
        "start": { "line": range.start.line, "character": range.start.character },
        "end": { "line": range.end.line, "character": range.end.character },
    })
}

/// A [`Location`] as the JSON the protocol speaks.
fn location_json(location: Location) -> serde_json::Value {
    serde_json::json!({
        "path": location.path,
        "range": range_json(location.range),
    })
}

/// `--check` for a buffer: rl-level diagnostics from the text alone.
fn check(params: &serde_json::Value) -> Result<serde_json::Value, String> {
    use serde_json::json;
    let text = text_param(params)?;
    let filename = params["filename"].as_str();
    let options = rlc::Options {
        filename,
        verify: params["verify"].as_bool().unwrap_or(true),
        ..rlc::Options::default()
    };
    // Every rl-level diagnostic of the buffer, in source order (TASK-120).
    // `endLine`/`endCol` close the range the diagnostic covers — the
    // construct as written. Zero means "position only": the consumer
    // decides the width. `code` is the rule's stable identity.
    let diagnostics: Vec<_> = rlc::compile_report(text, &options)
        .diagnostics
        .iter()
        .map(|d| {
            let e = d.to_compile_error(text, filename);
            json!({
                "line": e.line,
                "col": e.col,
                "endLine": e.end_line,
                "endCol": e.end_col,
                "message": e.message,
                "code": d.code.as_str(),
            })
        })
        .collect();
    Ok(json!({ "diagnostics": diagnostics }))
}

/// Semantic tokens for a buffer: the parser's own classification of the
/// ambiguous surface, in the buffer's coordinates. Like `check`, this is
/// stateless and parse-only — it needs no project and no TypeScript
/// toolchain, so the editor's colors stay exact in every environment.
/// The declarations visible in a buffer — the compiler's own enum table
/// (local, imported, built-in, under the compiler's shadowing) plus the
/// buffer's `match` sites. This is the surface that replaces the editor's
/// regex re-implementation of rl semantics (`engine::rl_declarations`).
/// Text-only; `path` resolves the buffer's relative `.rl` imports.
fn declarations(params: &serde_json::Value) -> Result<serde_json::Value, String> {
    use serde_json::json;
    let path = params["path"]
        .as_str()
        .ok_or_else(|| "the request needs a \"path\"".to_string())?;
    let decls = rlc::engine::rl_declarations(Path::new(path), text_param(params)?);
    let enums: Vec<_> = decls
        .enums
        .iter()
        .map(|e| {
            let (origin, specifier, name_span, span) = match &e.origin {
                rlc::engine::RlEnumOrigin::Local { name_span, span } => (
                    "local",
                    serde_json::Value::Null,
                    Some(*name_span),
                    Some(*span),
                ),
                rlc::engine::RlEnumOrigin::Imported { specifier } => (
                    "imported",
                    specifier
                        .clone()
                        .map(serde_json::Value::String)
                        .unwrap_or(serde_json::Value::Null),
                    None,
                    None,
                ),
                rlc::engine::RlEnumOrigin::Builtin => {
                    ("builtin", serde_json::Value::Null, None, None)
                }
            };
            json!({
                "name": e.name,
                "generics": e.generics,
                "origin": origin,
                "specifier": specifier,
                "nameSpan": name_span.map(|(start, end)| json!({ "start": start, "end": end })),
                "span": span.map(|(start, end)| json!({ "start": start, "end": end })),
                "cases": e.cases.iter().map(|c| json!({
                    "tag": c.tag,
                    "span": c.span.map(|(start, end)| json!({ "start": start, "end": end })),
                    "unit": c.unit,
                    "fields": c.fields.iter().map(|f| json!({
                        "name": f.name,
                        "optional": f.optional,
                        "ty": f.ty,
                    })).collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    let matches: Vec<_> = decls
        .matches
        .iter()
        .map(|m| json!({ "keyword": m.keyword, "bodyClose": m.body_close }))
        .collect();
    Ok(json!({ "enums": enums, "matches": matches }))
}

/// The rl name at a position — an enum, a case tag, a payload field.
///
/// Text-only like `semanticTokens`: the answer needs no project and no
/// toolchain, because these names exist nowhere in the emitted TypeScript
/// and are rl's to answer (`engine::names`). `path` is still required, to
/// resolve the file's relative `.rl` imports.
fn rl_symbol(params: &serde_json::Value) -> Result<serde_json::Value, String> {
    use serde_json::json;
    let path = params["path"]
        .as_str()
        .ok_or_else(|| "the request needs a \"path\"".to_string())?;
    let position = Position {
        line: params["position"]["line"].as_u64().unwrap_or(0) as u32,
        character: params["position"]["character"].as_u64().unwrap_or(0) as u32,
    };
    let Some(symbol) = rlc::engine::rl_symbol_at(Path::new(path), text_param(params)?, position)
    else {
        return Ok(serde_json::Value::Null);
    };
    Ok(json!({
        "kind": match symbol.kind {
            rlc::engine::RlSymbolKind::Enum => "enum",
            rlc::engine::RlSymbolKind::Case => "case",
            rlc::engine::RlSymbolKind::Field => "field",
        },
        "range": range_json(symbol.range),
        "name": symbol.name,
        "enumName": symbol.enum_name,
        "signature": symbol.signature,
        "detail": symbol.detail,
        "definition": symbol.definition.map(|location| json!({
            "path": location.path.to_string_lossy(),
            "range": range_json(location.range),
        })),
    }))
}

/// What can be written at a pattern position — case tags, payload field
/// names. Text-only, for the same reason [`rl_symbol`] is.
fn rl_completions(params: &serde_json::Value) -> Result<serde_json::Value, String> {
    use serde_json::json;
    let path = params["path"]
        .as_str()
        .ok_or_else(|| "the request needs a \"path\"".to_string())?;
    let position = Position {
        line: params["position"]["line"].as_u64().unwrap_or(0) as u32,
        character: params["position"]["character"].as_u64().unwrap_or(0) as u32,
    };
    let items: Vec<_> =
        rlc::engine::rl_completions_at(Path::new(path), text_param(params)?, position)
            .into_iter()
            .map(|item| {
                json!({
                    "label": item.label,
                    "kind": match item.kind {
                        rlc::engine::RlCompletionKind::Case => "case",
                        rlc::engine::RlCompletionKind::Field => "field",
                        rlc::engine::RlCompletionKind::Wildcard => "wildcard",
                    },
                    "detail": item.detail,
                    "covered": item.covered,
                })
            })
            .collect();
    Ok(json!({ "items": items }))
}

/// What rl has to say about a buffer that is not an error — today, the
/// arms an earlier arm already covers. Text-only like [`rl_symbol`], and
/// separate from `check` on purpose: a hint never fails a build, so it
/// never travels in the diagnostics of a compile answer.
fn rl_hints(params: &serde_json::Value) -> Result<serde_json::Value, String> {
    use serde_json::json;
    let path = params["path"]
        .as_str()
        .ok_or_else(|| "the request needs a \"path\"".to_string())?;
    let hints: Vec<_> = rlc::engine::rl_hints(Path::new(path), text_param(params)?)
        .into_iter()
        .map(|hint| {
            json!({
                "kind": match hint.kind {
                    rlc::engine::RlHintKind::UnreachableArm => "unreachableArm",
                },
                "range": range_json(hint.range),
                "message": hint.message,
            })
        })
        .collect();
    Ok(json!({ "hints": hints }))
}

fn semantic_tokens(params: &serde_json::Value) -> Result<serde_json::Value, String> {
    use serde_json::json;
    let tokens: Vec<_> = rlc::engine::semantic_tokens(text_param(params)?)
        .into_iter()
        .map(|token| {
            json!({
                "range": range_json(token.range),
                "kind": token.kind.as_str(),
            })
        })
        .collect();
    Ok(json!({ "tokens": tokens }))
}

/// `--emit-map` for a buffer: the emitted TypeScript and its byte mappings.
fn emit_map(params: &serde_json::Value) -> Result<serde_json::Value, String> {
    use serde_json::json;
    let emit = rlc::emit_mapped(text_param(params)?);
    let mappings: Vec<_> = emit
        .mappings
        .iter()
        .map(|m| json!({ "src": m.src, "out": m.out, "len": m.len }))
        .collect();
    Ok(json!({ "code": emit.code, "mappings": mappings }))
}

/// `--check-types --rl-only --overlay <path>` for a buffer, against the live
/// project it belongs to.
fn typed_check(
    sessions: &mut Sessions,
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    use serde_json::json;
    let path = params["path"]
        .as_str()
        .ok_or_else(|| "typedCheck needs a \"path\"".to_string())?
        .to_string();
    let text = text_param(params)?.to_string();
    let canonical = PathBuf::from(&path)
        .canonicalize()
        .map_err(|e| format!("--overlay {path}: {e}"))?;
    // A document the consumer holds open keeps its overlay after the check;
    // a one-off buffer's overlay is scoped to this request, so the answer
    // stays stateless while the projection cache keeps the incremental win.
    let registered = sessions.docs.contains_key(&canonical);
    let project = project_for(sessions, &path)?;

    project.open_document(canonical.clone(), text);
    let files = {
        let scanned = project.scan().map_err(|e| e.to_string())?;
        if scanned.is_empty() {
            vec![canonical.clone()]
        } else {
            scanned
        }
    };
    let outcome = project.update(&files);
    let response = match outcome {
        Err(blocked) => json!({
            "blocked": true,
            "diagnostics": [{
                "path": blocked.path,
                "line": blocked.error.line,
                "col": blocked.error.col,
                "endLine": blocked.error.end_line,
                "endCol": blocked.error.end_col,
                "message": blocked.error.message,
            }],
        }),
        Ok(snapshot) => {
            let checked = project.check(
                &snapshot,
                &CheckRequest {
                    emit_declarations: false,
                    rl_only: true,
                },
            );
            match checked {
                Err(e) => {
                    if !registered {
                        project.close_document(&canonical);
                    }
                    return Err(e);
                }
                Ok(checked) => {
                    let diagnostics: Vec<_> = checked
                        .diagnostics
                        .iter()
                        .map(|d| {
                            let (line, col) = d.position.unwrap_or((0, 0));
                            let (end_line, end_col) = d.end.unwrap_or((0, 0));
                            json!({
                                "path": d.path,
                                "line": line,
                                "col": col,
                                "endLine": end_line,
                                "endCol": end_col,
                                "message": d.message,
                                "code": d.code,
                            })
                        })
                        .collect();
                    // `backendError`: the TypeScript layer could not run —
                    // the rl diagnostics above are still complete.
                    json!({
                        "blocked": false,
                        "diagnostics": diagnostics,
                        "backendError": checked.backend_error,
                    })
                }
            }
        }
    };
    if !registered {
        project.close_document(&canonical);
    }
    Ok(response)
}

fn text_param(params: &serde_json::Value) -> Result<&str, String> {
    params["text"]
        .as_str()
        .ok_or_else(|| "the request needs a \"text\"".to_string())
}
