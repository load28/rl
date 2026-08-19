use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::thread;

use super::protocol::{parse_types_result, types_job};
use super::{EmittedTypes, LiteralCheck, ValCheck, VirtualModule};

/// Legacy TypeScript JS Compiler API host.
///
/// The script is embedded so `rlc --types` does not need an installed helper
/// package; the host itself resolves TypeScript from the project and runs
/// with `node`.
const TYPES_HOST: &str = include_str!("../types_host.mjs");

/// Experimental TypeScript 7 native host. Selected only with
/// `RLC_TS_BACKEND=tsgo`; the legacy JS host stays the default until the
/// native path reaches feature parity.
const TSGO_HOST: &str = include_str!("../tsgo_host.mjs");

#[derive(Clone, Copy, PartialEq, Eq)]
enum TypesBackend {
    LegacyJs,
    Tsgo,
}

impl TypesBackend {
    fn from_env() -> Self {
        match std::env::var("RLC_TS_BACKEND") {
            Ok(backend) if backend.eq_ignore_ascii_case("tsgo") => Self::Tsgo,
            _ => Self::LegacyJs,
        }
    }

    fn script_name(self) -> &'static str {
        match self {
            Self::LegacyJs => "types_host.mjs",
            Self::Tsgo => "tsgo_host.mjs",
        }
    }

    fn script_body(self) -> &'static str {
        match self {
            Self::LegacyJs => TYPES_HOST,
            Self::Tsgo => TSGO_HOST,
        }
    }

    fn node_args(self) -> &'static [&'static str] {
        match self {
            Self::LegacyJs => &[],
            Self::Tsgo => &[
                "--experimental-strip-types",
                "--no-warnings",
                "--conditions",
                "@typescript/source",
            ],
        }
    }
}

/// Runs the embedded host with `node`, handing it the compiled modules on
/// stdin and reading declarations back from stdout.
pub(crate) fn run_types_host(
    node: Option<&Path>,
    modules: &[VirtualModule],
    std_module: Option<&VirtualModule>,
    sources: &[String],
    rl_map: &[(String, String)],
    checks: &[LiteralCheck],
    val_probes: &[ValCheck],
) -> Result<EmittedTypes, ExitCode> {
    let dir = std::env::temp_dir().join(format!("rlc-types-{}", std::process::id()));
    let backend = TypesBackend::from_env();
    let script = dir.join(backend.script_name());
    if let Err(e) =
        fs::create_dir_all(&dir).and_then(|()| fs::write(&script, backend.script_body()))
    {
        eprintln!("rlc: {}: {e}", script.display());
        return Err(ExitCode::FAILURE);
    }

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let job = types_job(
        &cwd, modules, std_module, sources, rl_map, checks, val_probes,
    );
    let binary = node.map_or_else(|| PathBuf::from("node"), Path::to_path_buf);

    let mut command = Command::new(&binary);
    command.args(backend.node_args());
    command.arg(&script);

    let mut child = match command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "rlc: node not found — install Node.js or pass --node <path> (--types needs it)"
            );
            return Err(ExitCode::FAILURE);
        }
        Err(e) => {
            eprintln!("rlc: {}: {e}", binary.display());
            return Err(ExitCode::FAILURE);
        }
    };

    // Write the job from another thread: a large job would otherwise fill the
    // pipe while this side is still waiting to read.
    let mut stdin = child.stdin.take().expect("piped stdin");
    let writer = thread::spawn(move || {
        use std::io::Write;
        let _ = stdin.write_all(job.as_bytes());
    });
    let output = child.wait_with_output();
    let _ = writer.join();
    let _ = fs::remove_dir_all(&dir);

    let output = match output {
        Ok(output) => output,
        Err(e) => {
            eprintln!("rlc: {}: {e}", binary.display());
            return Err(ExitCode::FAILURE);
        }
    };
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        let detail = detail.trim();
        if backend == TypesBackend::Tsgo && output.status.code() == Some(2) {
            eprintln!(
                "rlc: typescript-go not found — set RLC_TSGO_ROOT to a built \
                 microsoft/typescript-go checkout"
            );
        } else if output.status.code() == Some(2) {
            eprintln!("rlc: typescript not found — install it (npm i -D typescript)");
        } else if output.status.code() == Some(4) {
            // TypeScript 7 is the native compiler: `require("typescript")`
            // exposes no JS compiler API, which --types drives directly.
            eprintln!(
                "rlc: the resolved typescript has no JS compiler API \
                 (TypeScript 7's native compiler) — --types needs \
                 typescript 5 or 6 (npm i -D typescript@6)"
            );
        } else {
            eprintln!("rlc: declaration emit failed: {detail}");
        }
        return Err(ExitCode::FAILURE);
    }

    Ok(parse_types_result(&String::from_utf8_lossy(&output.stdout)))
}
