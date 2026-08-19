//! The native TypeScript backend — the only module that knows how the
//! compiler is reached.
//!
//! The compiler is `tsgo`, the native TypeScript 7 compiler, driven through
//! its API server. rlc talks to that server the way the TypeScript team's own
//! client does: a small host process ([`HOST`]) running under `node`, which
//! imports the JS client and speaks the server's MessagePack protocol.
//!
//! Client and server are **one unit**: the protocol carries no version
//! negotiation and the client is generated from the server's Go source, so
//! both are resolved from the same typescript-go tree (see [`Toolchain`]).
//! Everything unstable about TypeScript 7 lives behind this module and
//! [`super::backend::TypeScriptBackend`].

use std::cell::RefCell;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use super::backend::*;

/// The host script, embedded so a released `rlc` needs no files beside it.
const HOST: &str = include_str!("host.mjs");

/// Where the TypeScript compiler comes from.
///
/// Two shapes work, and both keep the client and the server from one build —
/// the protocol between them carries no version negotiation:
///
/// - an **installed package** (`node_modules/typescript`, or
///   `@typescript/native-preview`), which ships the API client and the
///   native executable together. The client finds its own executable, so
///   rlc names only the client.
/// - a **built typescript-go tree**, for working against the compiler's own
///   `main`. Both halves are named explicitly.
///
/// Resolution order, first hit wins:
///
/// 1. `RLC_TSGO_API` (+ optional `RLC_TSGO_BIN`) — named directly.
/// 2. `RLC_TSGO_ROOT` — a typescript-go checkout, built in place.
/// 3. `../typescript-go`, likewise built.
/// 4. an installed package, searched for in `node_modules` from the project
///    upwards — the workspace's own TypeScript, which is the one its code is
///    written against.
///
/// No path is compiled in, and a tree that is not built yet is reported as
/// such rather than guessed around.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Toolchain {
    /// The `tsgo` executable to run as the API server, or `None` to let the
    /// API client run the one shipped beside it.
    pub bin: Option<PathBuf>,
    /// The JS client module the host imports (`.../api/sync/api.js`).
    pub api: PathBuf,
}

/// `tsgo`'s path inside a built typescript-go tree.
const BIN_IN_TREE: &str = "built/local/tsgo";
/// The JS API client's path inside a built typescript-go tree.
const API_IN_TREE: &str = "_packages/native-preview/dist/api/sync/api.js";

impl Toolchain {
    /// Resolves the toolchain from the environment. The error names what is
    /// missing and how to produce it.
    pub(crate) fn resolve(from: &Path) -> Result<Toolchain, String> {
        if let Some(api) = env_path("RLC_TSGO_API") {
            return Toolchain {
                bin: env_path("RLC_TSGO_BIN"),
                api,
            }
            .check();
        }
        if let Some(root) = env_path("RLC_TSGO_ROOT") {
            return Toolchain::in_tree(&root).check();
        }
        let tree = Toolchain::in_tree(Path::new("../typescript-go"));
        if tree.api.exists() {
            return tree.check();
        }
        match installed(from) {
            Some(toolchain) => toolchain.check(),
            None => Err(format!(
                "no TypeScript compiler found — install one \
                 (`npm i -D typescript@7`), or build a typescript-go checkout \
                 (`go build -o {BIN_IN_TREE} ./cmd/tsgo` plus `npm ci && npx \
                 tsc -b _packages/native-preview`) and point rlc at it with \
                 RLC_TSGO_ROOT"
            )),
        }
    }

    /// The two halves of a built typescript-go checkout.
    fn in_tree(root: &Path) -> Toolchain {
        Toolchain {
            bin: Some(root.join(BIN_IN_TREE)),
            api: root.join(API_IN_TREE),
        }
    }

    /// Rejects a toolchain whose halves are not both present, naming the
    /// step that produces the missing one.
    fn check(self) -> Result<Toolchain, String> {
        if let Some(bin) = &self.bin
            && !bin.exists()
        {
            return Err(format!(
                "no tsgo executable at {} — build one with `go build -o {} \
                 ./cmd/tsgo` in a typescript-go checkout",
                bin.display(),
                BIN_IN_TREE,
            ));
        }
        if !self.api.exists() {
            return Err(format!(
                "no TypeScript API client at {} — in a typescript-go checkout \
                 build it with `npm ci && npx tsc -b _packages/native-preview` \
                 (the client and the executable must come from one build)",
                self.api.display(),
            ));
        }
        // The host imports the client by path and runs in the project's
        // directory, so a relative path here would resolve against neither.
        Ok(Toolchain {
            bin: self.bin.map(absolute),
            api: absolute(self.api),
        })
    }
}

/// The API client of an installed package, searched for in `node_modules`
/// from `from` upwards. The client resolves the executable shipped beside
/// it, so only the client is named.
fn installed(from: &Path) -> Option<Toolchain> {
    const PACKAGES: [&str; 2] = ["typescript", "@typescript/native-preview"];
    let mut dir = from.canonicalize().unwrap_or_else(|_| from.to_path_buf());
    loop {
        for package in PACKAGES {
            let api = dir
                .join("node_modules")
                .join(package)
                .join("dist/api/sync/api.js");
            if api.exists() {
                return Some(Toolchain { bin: None, api });
            }
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// The path as the host will see it, from wherever it is run.
fn absolute(path: PathBuf) -> PathBuf {
    path.canonicalize().unwrap_or(path)
}

fn env_path(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// A [`TypeScriptBackend`] over a running compiler.
///
/// The first question starts the host and opens the project; every question
/// after it reuses both. That is the difference between a watch that
/// re-checks in milliseconds and one that pays for a whole project open per
/// keystroke.
#[derive(Debug)]
pub(crate) struct NativeBackend {
    toolchain: Toolchain,
    /// The `node` binary that runs the host (`--node`, else `node` on PATH).
    node: PathBuf,
    session: RefCell<Option<Session>>,
}

/// The running host: a process, and the two pipes a request travels over.
#[derive(Debug)]
struct Session {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    /// What the project was opened as. A question about a different project
    /// needs a different session.
    opened: (Option<PathBuf>, PathBuf),
    /// The host script, kept until the session ends.
    script: PathBuf,
}

impl NativeBackend {
    /// Resolves the toolchain and prepares a backend over it. Nothing is
    /// started until the first question.
    pub(crate) fn new(node: Option<PathBuf>, from: &Path) -> Result<NativeBackend, String> {
        Ok(NativeBackend {
            toolchain: Toolchain::resolve(from)?,
            node: node.unwrap_or_else(|| PathBuf::from("node")),
            session: RefCell::new(None),
        })
    }

    /// Starts the host and opens the project.
    fn start(&self, tsconfig: Option<&Path>, root: &Path) -> Result<Session, String> {
        // The host is written beside the run rather than piped in: node reads
        // a module from a path, and the path is what import specifiers in the
        // job resolve against.
        let dir = std::env::temp_dir().join(format!("rlc-host-{}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(|e| format!("cannot prepare the host: {e}"))?;
        let script = dir.join("host.mjs");
        std::fs::write(&script, HOST).map_err(|e| format!("cannot write the host: {e}"))?;

        let mut child = Command::new(&self.node)
            .arg(&script)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("cannot run {}: {e}", self.node.display()))?;
        let mut stdin = child.stdin.take().expect("stdin piped");
        let mut stdout = BufReader::new(child.stdout.take().expect("stdout piped"));

        let open = serde_json::json!({
            "tsgoBin": self.toolchain.bin,  // null: the client runs the one beside it
            "apiModule": self.toolchain.api,
            "cwd": root,
            "tsconfig": tsconfig,
        });
        writeln!(stdin, "{open}").map_err(|e| format!("cannot start the host: {e}"))?;

        let mut ack = String::new();
        if stdout.read_line(&mut ack).map_err(|e| e.to_string())? == 0 {
            return Err(host_died(&mut child));
        }
        Ok(Session {
            child,
            stdin,
            stdout,
            opened: (tsconfig.map(Path::to_path_buf), root.to_path_buf()),
            script,
        })
    }
}

/// What the host said on its way out. A crash before the first answer is
/// usually a missing API or an unreadable client, and its message is on
/// stderr.
fn host_died(child: &mut Child) -> String {
    let status = child.wait().ok();
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        use std::io::Read;
        let _ = pipe.read_to_string(&mut stderr);
    }
    if status.and_then(|s| s.code()) == Some(5) {
        return "the resolved TypeScript can check but cannot emit declarations \
                — that API is newer than the released package. Drop -o, or point \
                rlc at a built typescript-go checkout with RLC_TSGO_ROOT"
            .to_string();
    }
    let stderr = stderr.trim();
    format!(
        "the TypeScript backend failed:\n{}",
        if stderr.is_empty() {
            "(no output)"
        } else {
            stderr
        }
    )
}

impl Drop for Session {
    fn drop(&mut self) {
        // Closing stdin ends the host's loop; the wait keeps it from
        // outliving the run.
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.script);
    }
}

impl TypeScriptBackend for NativeBackend {
    fn ask(&self, tsconfig: Option<&Path>, root: &Path, query: &Query) -> Result<Answers, String> {
        let wanted = (tsconfig.map(Path::to_path_buf), root.to_path_buf());
        let mut slot = self.session.borrow_mut();
        // A question about a different project needs its own session: the
        // project is opened once and never reopened.
        if slot.as_ref().is_some_and(|s| s.opened != wanted) {
            *slot = None;
        }
        if slot.is_none() {
            *slot = Some(self.start(tsconfig, root)?);
        }
        let session = slot.as_mut().expect("started");

        let job = job_json(query);
        let answer = exchange(session, &job.to_string());
        match answer {
            Ok(line) => parse_answers(&line),
            Err(_) => {
                // The host is gone; take its last words, and let the next
                // question start a fresh one.
                let mut session = slot.take().expect("started");
                Err(host_died(&mut session.child))
            }
        }
    }
}

/// One request, one answer. An I/O error means the host is no longer there.
fn exchange(session: &mut Session, request: &str) -> std::io::Result<String> {
    writeln!(session.stdin, "{request}")?;
    session.stdin.flush()?;
    let mut line = String::new();
    if session.stdout.read_line(&mut line)? == 0 {
        return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof));
    }
    Ok(line)
}

/// One `ask` — see `host.mjs` for the protocol.
fn job_json(query: &Query) -> serde_json::Value {
    use serde_json::json;
    json!({
        "modules": query.modules.iter()
            .map(|m| json!({ "path": m.path, "text": m.text }))
            .collect::<Vec<_>>(),
        "sources": query.sources,
        "literalChecks": query.literals.iter()
            .map(|l| json!({
                "module": l.module,
                "start": l.position,
                "covered": l.covered.iter().map(literal_json).collect::<Vec<_>>(),
            }))
            .collect::<Vec<_>>(),
        "tagChecks": query.tags.iter()
            .map(|t| json!({
                "module": t.module,
                "start": t.position,
                "covered": t.covered,
            }))
            .collect::<Vec<_>>(),
        "symbolChecks": query.symbols.iter()
            .map(|v| json!({ "module": v.module, "start": v.position }))
            .collect::<Vec<_>>(),
        "emitDeclarations": query.emit_declarations,
    })
}

/// A covered literal as the value JavaScript compares with `===`.
fn literal_json(literal: &rlc::Literal) -> serde_json::Value {
    use serde_json::json;
    match literal {
        rlc::Literal::String(s) => json!(s),
        rlc::Literal::Number(n) => json!(n),
        rlc::Literal::Boolean(b) => json!(b),
        // No finite literal union TypeScript reports holds a BigInt, so a
        // match covering one is never asked about; carried as text for
        // completeness.
        rlc::Literal::BigInt(d) => json!(d),
    }
}

/// Reads the host's answer. A shape that does not match is a bug in the pair
/// of this file and `host.mjs`, and is reported as one.
fn parse_answers(stdout: &str) -> Result<Answers, String> {
    let value: serde_json::Value = serde_json::from_str(stdout.trim())
        .map_err(|e| format!("the TypeScript backend answered with malformed JSON: {e}"))?;
    if let Some(error) = value["error"].as_str() {
        return Err(format!("the TypeScript backend failed:\n{error}"));
    }

    let mut answers = Answers::default();
    for d in array(&value, "diagnostics") {
        answers.diagnostics.push(Diagnostic {
            file: PathBuf::from(d["file"].as_str().unwrap_or_default()),
            start: d["start"].as_u64().unwrap_or_default() as usize,
            end: d["end"].as_u64().unwrap_or_default() as usize,
            code: d["code"].as_u64().unwrap_or_default() as u32,
            message: d["message"].as_str().unwrap_or_default().to_string(),
        });
    }
    for m in array(&value, "literalMissing") {
        answers.literal_missing.push(LiteralMissing {
            index: m["index"].as_u64().unwrap_or_default() as usize,
            missing: m["missing"]
                .as_array()
                .map(|a| a.iter().filter_map(json_literal).collect())
                .unwrap_or_default(),
        });
    }
    for m in array(&value, "tagMissing") {
        answers.tag_missing.push(TagMissing {
            index: m["index"].as_u64().unwrap_or_default() as usize,
            missing: m["missing"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|s| s.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
        });
    }
    for v in array(&value, "symbols") {
        answers.resolutions.push(Resolution {
            index: v["index"].as_u64().unwrap_or_default() as usize,
            id: v["id"].as_i64().unwrap_or_default(),
            name: v["name"].as_str().unwrap_or_default().to_string(),
            builtin: v["builtin"].as_bool().unwrap_or(false),
        });
    }
    for d in array(&value, "declarations") {
        answers.declarations.push(Declaration {
            path: PathBuf::from(d["path"].as_str().unwrap_or_default()),
            text: d["text"].as_str().unwrap_or_default().to_string(),
        });
    }
    Ok(answers)
}

fn array<'a>(value: &'a serde_json::Value, key: &str) -> &'a [serde_json::Value] {
    value[key].as_array().map_or(&[], |a| a.as_slice())
}

/// A literal the checker reported, in rl's own vocabulary.
fn json_literal(value: &serde_json::Value) -> Option<rlc::Literal> {
    match value {
        serde_json::Value::String(s) => Some(rlc::Literal::String(s.clone())),
        serde_json::Value::Number(n) => n.as_f64().map(rlc::Literal::Number),
        serde_json::Value::Bool(b) => Some(rlc::Literal::Boolean(*b)),
        _ => None,
    }
}
