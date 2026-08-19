//! The TypeScript language service — reached through `tsgo --lsp`.
//!
//! The API server ([`super::native`]) carries the checker's primitives —
//! types, symbols, diagnostics, emit — but not the *language-service*
//! surface an editor needs: quick info, go-to-definition, rename with
//! prepare, signature help, completion with per-item resolve. As of
//! typescript-go `c6b013f5` those exist only behind the compiler's own LSP
//! (`internal/lsp`), so that is what this module drives: one `tsgo --lsp`
//! child per project, spoken to over Content-Length framing.
//!
//! This is an implementation detail of the TypeScript adapter, decided
//! feature by feature (see `docs/design/lsp-architecture.md`): nothing
//! above [`crate::engine`]'s semantic API knows an LSP is involved, and if
//! a future API-server release grows these entrypoints this module can be
//! retired without moving the seam.
//!
//! Two behaviors are load-bearing, learned from the editor client this
//! replaces:
//!
//! - **Server-initiated requests must be answered.** tsgo sends
//!   `client/registerCapability` during startup and waits for the reply; a
//!   client that ignores it hangs on every later request. The reader thread
//!   answers them the moment they arrive.
//! - **Documents are the conversation.** A file that exists only as text in
//!   this process — the lowered TypeScript of an `.rl` buffer — is served
//!   with `didOpen`/`didChange` and answered exactly like one on disk.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long any one request may take before the client gives up on it. A
/// hung request must not hang the editor; the feature simply has no answer.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);

/// A running `tsgo --lsp`, and the conversation with it.
pub(crate) struct Service {
    child: Child,
    /// Shared with the reader thread, which answers server-initiated
    /// requests itself so the server never blocks on one.
    stdin: Arc<Mutex<ChildStdin>>,
    /// Responses to *our* requests, forwarded by the reader thread.
    responses: Receiver<Response>,
    next_id: i64,
    /// Versions of the documents we serve, by URI.
    opened: HashMap<String, i64>,
    alive: bool,
}

/// One answer from the server: the result, or the error it gave instead.
struct Response {
    id: i64,
    result: serde_json::Value,
    error: Option<String>,
}

impl std::fmt::Debug for Service {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Service")
            .field("alive", &self.alive)
            .finish()
    }
}

impl Service {
    /// Starts the server and completes the LSP handshake. `root` is the
    /// workspace the server opens.
    pub(crate) fn start(binary: &Path, root: &Path) -> Result<Service, String> {
        let mut child = Command::new(binary)
            .args(["--lsp", "-stdio"])
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("cannot run {}: {e}", binary.display()))?;
        let stdin = Arc::new(Mutex::new(child.stdin.take().expect("stdin piped")));
        let stdout = child.stdout.take().expect("stdout piped");
        let stderr = child.stderr.take().expect("stderr piped");

        // The server's own log output must not fill its pipe and stall it.
        std::thread::spawn(move || {
            let mut sink = BufReader::new(stderr);
            let mut scratch = [0u8; 4096];
            while matches!(sink.read(&mut scratch), Ok(n) if n > 0) {}
        });

        let (tx, rx) = channel();
        let reader_stdin = Arc::clone(&stdin);
        std::thread::spawn(move || read_loop(stdout, reader_stdin, tx));

        let mut service = Service {
            child,
            stdin,
            responses: rx,
            next_id: 1,
            opened: HashMap::new(),
            alive: true,
        };

        let root_uri = file_uri(root);
        service.request(
            "initialize",
            serde_json::json!({
                "processId": std::process::id(),
                "rootUri": root_uri,
                "workspaceFolders": [{ "uri": root_uri, "name": "rl" }],
                "capabilities": {
                    "textDocument": {
                        "synchronization": { "dynamicRegistration": true },
                        "hover": { "contentFormat": ["plaintext", "markdown"] },
                        "definition": {},
                        "references": {},
                        "completion": { "completionItem": { "labelDetailsSupport": true } },
                        "signatureHelp": {},
                        "rename": { "prepareSupport": true },
                        "diagnostic": {},
                    },
                    "workspace": { "configuration": true, "workspaceFolders": true },
                },
            }),
        )?;
        service.notify("initialized", serde_json::json!({}));
        Ok(service)
    }

    /// Whether the server is still there to answer.
    pub(crate) fn alive(&self) -> bool {
        self.alive
    }

    /// Serves `text` as `uri`, whether or not anything is there on disk.
    /// Whole-document sync: the lowered text is regenerated as a whole, so
    /// there is no incremental edit to describe.
    pub(crate) fn open(&mut self, uri: &str, text: &str) {
        let version = self.opened.get(uri).copied().unwrap_or(0) + 1;
        self.opened.insert(uri.to_string(), version);
        if version == 1 {
            self.notify(
                "textDocument/didOpen",
                serde_json::json!({
                    "textDocument": {
                        "uri": uri, "languageId": "typescript",
                        "version": version, "text": text,
                    },
                }),
            );
        } else {
            self.notify(
                "textDocument/didChange",
                serde_json::json!({
                    "textDocument": { "uri": uri, "version": version },
                    "contentChanges": [{ "text": text }],
                }),
            );
        }
    }

    /// Asks the server something.
    ///
    /// `Ok(Null)` covers every "no answer" that leaves the conversation
    /// intact — the server answered with an error, or the request timed out
    /// — so a feature simply has no result, exactly as the editor's old
    /// client behaved. `Err` means the conversation itself broke (the
    /// process died): the caller treats the service as gone, and the next
    /// question starts a fresh one.
    pub(crate) fn request(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        if !self.alive {
            return Err("the TypeScript server is not running".to_string());
        }
        let id = self.next_id;
        self.next_id += 1;
        self.send(serde_json::json!({
            "jsonrpc": "2.0", "id": id, "method": method, "params": params,
        }))?;

        let deadline = Instant::now() + REQUEST_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self.responses.recv_timeout(remaining) {
                Ok(response) if response.id == id => {
                    return match response.error {
                        Some(_) => Ok(serde_json::Value::Null),
                        None => Ok(response.result),
                    };
                }
                Ok(_) => continue, // a stale answer to a request we gave up on
                Err(RecvTimeoutError::Timeout) => return Ok(serde_json::Value::Null),
                Err(RecvTimeoutError::Disconnected) => {
                    self.alive = false;
                    return Err("the TypeScript server exited".to_string());
                }
            }
        }
    }

    fn notify(&mut self, method: &str, params: serde_json::Value) {
        let _ = self.send(serde_json::json!({
            "jsonrpc": "2.0", "method": method, "params": params,
        }));
    }

    fn send(&mut self, message: serde_json::Value) -> Result<(), String> {
        let text = message.to_string();
        let mut stdin = self.stdin.lock().expect("stdin lock");
        let framed = format!("Content-Length: {}\r\n\r\n{text}", text.len());
        stdin
            .write_all(framed.as_bytes())
            .and_then(|_| stdin.flush())
            .map_err(|e| {
                self.alive = false;
                format!("the TypeScript server is gone: {e}")
            })
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Parses frames off the server's stdout: answers server-initiated requests
/// in place, forwards responses to [`Service::request`], drops notifications.
fn read_loop(
    stdout: std::process::ChildStdout,
    stdin: Arc<Mutex<ChildStdin>>,
    responses: Sender<Response>,
) {
    let mut reader = BufReader::new(stdout);
    loop {
        // Headers, ending at an empty line; only Content-Length matters.
        let mut length: Option<usize> = None;
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some(value) = line.strip_prefix("Content-Length:") {
                length = value.trim().parse().ok();
            }
        }
        let Some(length) = length else { return };
        let mut body = vec![0u8; length];
        if reader.read_exact(&mut body).is_err() {
            return;
        }
        let Ok(message) = serde_json::from_slice::<serde_json::Value>(&body) else {
            continue;
        };

        let id = message.get("id");
        let method = message.get("method").and_then(|m| m.as_str());
        match (id, method) {
            // A response to one of our requests.
            (Some(id), None) => {
                let response = Response {
                    id: id.as_i64().unwrap_or(-1),
                    result: message
                        .get("result")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null),
                    error: message
                        .get("error")
                        .and_then(|e| e.get("message"))
                        .and_then(|m| m.as_str())
                        .map(String::from),
                };
                if responses.send(response).is_err() {
                    return;
                }
            }
            // A request from the server. Answering is not optional: tsgo
            // registers capabilities during startup and waits for the reply
            // before serving anything else.
            (Some(id), Some(method)) => {
                let result = if method == "workspace/configuration" {
                    serde_json::json!([{}])
                } else {
                    serde_json::Value::Null
                };
                let reply =
                    serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string();
                let mut stdin = stdin.lock().expect("stdin lock");
                let framed = format!("Content-Length: {}\r\n\r\n{reply}", reply.len());
                if stdin
                    .write_all(framed.as_bytes())
                    .and_then(|_| stdin.flush())
                    .is_err()
                {
                    return;
                }
            }
            // A notification (published diagnostics, logs) — not consumed:
            // diagnostics are pulled, and logs are the server's own.
            _ => {}
        }
    }
}

/// The `file://` URI of an absolute path, with the few characters that
/// would break a URI escaped.
pub(crate) fn file_uri(path: &Path) -> String {
    let mut out = String::from("file://");
    for byte in path.to_string_lossy().bytes() {
        match byte {
            b' ' => out.push_str("%20"),
            b'%' => out.push_str("%25"),
            b'#' => out.push_str("%23"),
            b'?' => out.push_str("%3F"),
            _ => out.push(byte as char),
        }
    }
    out
}

/// The inverse of [`file_uri`]: the path a `file://` URI names, or `None`
/// for any other scheme — the compiler carries its standard library inside
/// the executable as `bundled:///...`, and there is no document behind such
/// a URI for an editor to open.
pub(crate) fn uri_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let mut out = Vec::with_capacity(rest.len());
    let bytes = rest.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(byte) = u8::from_str_radix(&rest[i + 1..i + 3], 16)
        {
            out.push(byte);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    Some(PathBuf::from(String::from_utf8_lossy(&out).into_owned()))
}

/// Where the `tsgo` executable that serves the LSP comes from — the same
/// places the compiler backend looks ([`super::native::Toolchain`]), plus
/// the platform package an installed TypeScript ships its binary in.
pub(crate) fn service_binary(root: &Path) -> Result<PathBuf, String> {
    if let Some(bin) = env_path("RLC_TSGO_BIN")
        && bin.exists()
    {
        return Ok(bin);
    }
    if let Some(tree) = env_path("RLC_TSGO_ROOT") {
        let bin = tree.join("built/local/tsgo");
        if bin.exists() {
            return Ok(bin);
        }
    }
    let sibling = Path::new("../typescript-go/built/local/tsgo");
    if sibling.exists() {
        return Ok(sibling.to_path_buf());
    }
    let platform = format!("{}-{}", os_name(), arch_name());
    let mut dir = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    loop {
        for package in [
            format!("@typescript/typescript-{platform}"),
            format!("@typescript/native-preview-{platform}"),
        ] {
            let exe = dir.join("node_modules").join(&package).join("lib/tsc");
            if exe.exists() {
                return Ok(exe);
            }
        }
        if !dir.pop() {
            return Err("no tsgo language server found — install TypeScript 7 \
                 (`npm i -D typescript@7`) or point RLC_TSGO_ROOT at a built \
                 typescript-go checkout"
                .to_string());
        }
    }
}

fn os_name() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    }
}

fn arch_name() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    }
}

fn env_path(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}
