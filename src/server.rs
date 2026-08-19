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
//! ← { "id": 1, "result": { "diagnostics": [{ "line", "col", "message" }] } }
//!
//! → { "id": 2, "method": "emitMap", "params": { "text" } }
//! ← { "id": 2, "result": { "code", "mappings": [{ "src", "out", "len" }] } }
//!
//! → { "id": 3, "method": "typedCheck", "params": { "path", "text" } }
//! ← { "id": 3, "result": { "blocked", "diagnostics":
//!        [{ "path", "line", "col", "message" }] } }
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
use std::path::PathBuf;
use std::process::ExitCode;

use rlc::engine::{CheckRequest, Engine, Project, ProjectOptions};

/// Runs the server until stdin closes.
pub(crate) fn run(node: Option<PathBuf>) -> ExitCode {
    let engine = Engine::new(node);
    // One live project per identity — the map a server exists to keep.
    let mut projects: HashMap<(Option<PathBuf>, PathBuf), Project> = HashMap::new();

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
        let response = respond(&engine, &mut projects, &line);
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
fn respond(
    engine: &Engine,
    projects: &mut HashMap<(Option<PathBuf>, PathBuf), Project>,
    line: &str,
) -> serde_json::Value {
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
        "typedCheck" => typed_check(engine, projects, params),
        method => Err(format!("unknown method \"{method}\"")),
    };
    match result {
        Ok(result) => json!({ "id": id, "result": result }),
        Err(error) => json!({ "id": id, "error": error }),
    }
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
    let diagnostics = match rlc::compile(text, &options) {
        Ok(_) => Vec::new(),
        Err(e) => vec![json!({ "line": e.line, "col": e.col, "message": e.message })],
    };
    Ok(json!({ "diagnostics": diagnostics }))
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
    engine: &Engine,
    projects: &mut HashMap<(Option<PathBuf>, PathBuf), Project>,
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    use serde_json::json;
    let path = params["path"]
        .as_str()
        .ok_or_else(|| "typedCheck needs a \"path\"".to_string())?;
    let text = text_param(params)?;
    let canonical = PathBuf::from(path)
        .canonicalize()
        .map_err(|e| format!("--overlay {path}: {e}"))?;

    let inputs = vec![path.to_string()];
    let options = ProjectOptions::default();
    let identity = Engine::project_identity(&inputs, &options)?;
    let project = match projects.entry(identity) {
        std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
        std::collections::hash_map::Entry::Vacant(entry) => {
            entry.insert(engine.open_project(&inputs, &options)?)
        }
    };

    // The overlay is scoped to this request: the *answer* is stateless (a
    // fallen-back one-shot sees the same project), while the projection
    // cache keeps the incremental win — the same buffer next time is a
    // text-equality hit, not a recompile.
    project.open_document(canonical.clone(), text.to_string());
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
                    project.close_document(&canonical);
                    return Err(e);
                }
                Ok(checked) => {
                    let diagnostics: Vec<_> = checked
                        .diagnostics
                        .iter()
                        .map(|d| {
                            let (line, col) = d.position.unwrap_or((0, 0));
                            json!({
                                "path": d.path,
                                "line": line,
                                "col": col,
                                "message": d.message,
                            })
                        })
                        .collect();
                    json!({ "blocked": false, "diagnostics": diagnostics })
                }
            }
        }
    };
    project.close_document(&canonical);
    Ok(response)
}

fn text_param(params: &serde_json::Value) -> Result<&str, String> {
    params["text"]
        .as_str()
        .ok_or_else(|| "the request needs a \"text\"".to_string())
}
