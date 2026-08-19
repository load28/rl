//! rlc — compile .rl sources into a complete TypeScript tree.
//!
//!   rlc -o build src/              builds src/ under build/: .rl files are
//!                                  compiled, hand-written .ts files pass
//!                                  through (with .rl import specifiers
//!                                  rewritten), and the standard library is
//!                                  materialized when something imports it
//!   rlc --check-types src/         type-checks the tree with the real
//!                                  TypeScript compiler, reporting at
//!                                  positions in the .rl sources
//!   rlc --types src/               the same check, and writes the editor/
//!                                  typecheck sidecars it emits
//!                                  (.rl-types/<name>.rl.d.ts + .map)
//!   rlc --check src/               compiles without writing anything
//!   rlc -p file.rl                 prints one compiled module to stdout

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};

use rlc::{
    EnumSymbol, ExternEnum, ImportRewrite, ModuleScan, Options, RlImport, RlImportNames, compile,
};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn usage() {
    println!(
        "rlc v{VERSION} — rl to TypeScript compiler

Usage: rlc [options] <file | dir> ...
       rlc help [topic]      language & workflow reference (topics: rlc help)

Builds a complete TypeScript tree: .rl files are compiled, hand-written
.ts files pass through byte-for-byte (with relative .rl import specifiers
rewritten), and the standard library is materialized when an input
imports \"@rl/std\". Types come from the same sources via --types.

Options:
  -o, --out-dir <dir>   write outputs under <dir> (mirrors input paths)
  -j, --jobs <n>        compile n files at a time (default: one per core;
                        1 compiles sequentially). Output is identical
                        either way.
  -w, --watch           keep running; recompile inputs (and their importers)
                        as they change
  --check               compile only; write nothing (rl-level checks; needs
                        no TypeScript)
  --check-types         also type-check: the tree is lowered into the real
                        TypeScript project and the compiler answers, with
                        every diagnostic at a position in the .rl source
  --types               --check-types, and write the editor/typecheck
                        sidecars the compiler emits: <name>.rl.d.ts + .map
                        under -o (default .rl-types)
  --project <path>      tsconfig.json the two modes above check against
                        (default: the nearest one at or above the inputs)
  --node <path>         node binary the TypeScript compiler's client runs
                        with (default: node on PATH)
  -h, --help            show this help
  -v, --version         show version

Tooling options (bundler plugins, editors):
  -p, --print           print compiled output to stdout instead of writing
  --emit-std            print the standard library module to stdout
  --no-banner           omit the \"generated\" banner comment
  --no-verify           skip swc validation of types and generated output
  --rewrite-imports <js|ts|off>
                        how relative .rl import specifiers are emitted:
                        js = ./x.js (default), ts = ./x.ts, off = untouched
  --sidecar <dir>       write <name>.rl.d.ts and .map next to each input from
                        <dir>/<name>.d.ts (tsc --emitDeclarationOnly output);
                        compiles nothing (--types runs this step for you)
  --symbols             print rl enum declarations (with positions) and the
                        direct .rl imports of each input as JSON; compiles
                        nothing (for language tooling)
  --emit-map            print each input's emitted TypeScript plus source<->
                        output byte mappings as JSON; parse + emit only (no
                        rl-level checks, .rl specifiers untouched) — the
                        editor's virtual-document feed (for language tooling)"
    );
}

/// The language & workflow guide (docs/ai/rl.md), embedded so `rlc help`
/// serves documentation offline. The file is the source of truth; `##`
/// headings are the topic boundaries.
const GUIDE: &str = include_str!("../docs/ai/rl.md");

/// Help topics: (name, aliases, `##` heading prefix in GUIDE). An empty
/// prefix selects the preamble (everything before the first `## `).
const HELP_TOPICS: &[(&str, &[&str], &str)] = &[
    ("overview", &["contracts", "intro"], ""),
    ("enum", &["enums"], "## enum"),
    ("match", &["tuple", "patterns"], "## match"),
    ("try", &[], "## try"),
    ("let-else", &["letelse"], "## let-else"),
    ("if-let", &["iflet"], "## if let"),
    ("pipe", &["pipeline", "|>", "flow"], "## |>"),
    ("result", &["do", "result-block"], "## result block"),
    ("val", &["mutation", "readonly"], "## val"),
    ("std", &["option"], "## @rl/std"),
    ("modules", &["imports"], "## Modules"),
    ("install", &["update"], "## Install"),
    ("setup", &["init"], "## Setup"),
    ("workflow", &["dev", "build"], "## Workflow"),
    ("errors", &[], "## Errors"),
    ("checklist", &[], "## Checklist"),
];

/// The slice of GUIDE for one topic: from its heading line up to the next
/// `## ` heading. An empty `heading` returns the preamble.
fn guide_section(heading: &str) -> &'static str {
    let start = if heading.is_empty() {
        0
    } else {
        match GUIDE
            .lines()
            .scan(0usize, |off, line| {
                let at = *off;
                *off += line.len() + 1;
                Some((at, line))
            })
            .find(|(_, line)| line.starts_with(heading))
        {
            Some((at, _)) => at,
            None => return "",
        }
    };
    let body = &GUIDE[start..];
    // A section's own heading sits at offset 0 (no leading newline), so
    // searching for "\n## " only ever finds the NEXT heading.
    match body.find("\n## ") {
        Some(end) => &body[..end + 1],
        None => body,
    }
}

/// `rlc help [topic]` — print the embedded guide (whole, one section, or
/// the topic list).
fn run_help(args: &[String]) -> ExitCode {
    let topic = match args {
        [] => {
            println!(
                "rlc help <topic> — rl language & workflow reference\n\n\
                 Topics:\n  {}\n\n\
                 `rlc help all` prints the whole guide; `rlc -h` shows CLI options.",
                HELP_TOPICS
                    .iter()
                    .map(|(name, aliases, _)| if aliases.is_empty() {
                        (*name).to_string()
                    } else {
                        format!("{name} ({})", aliases.join(", "))
                    })
                    .collect::<Vec<_>>()
                    .join("\n  ")
            );
            return ExitCode::SUCCESS;
        }
        [topic] => topic.to_lowercase(),
        _ => {
            eprintln!("rlc: help takes at most one topic (run `rlc help` for the list)");
            return ExitCode::FAILURE;
        }
    };
    if topic == "all" || topic == "guide" {
        print!("{GUIDE}");
        return ExitCode::SUCCESS;
    }
    let found = HELP_TOPICS
        .iter()
        .find(|(name, aliases, _)| *name == topic || aliases.contains(&topic.as_str()));
    match found {
        Some((_, _, heading)) => {
            print!("{}", guide_section(heading));
            ExitCode::SUCCESS
        }
        None => {
            eprintln!("rlc: unknown help topic \"{topic}\" (run `rlc help` for the list)");
            ExitCode::FAILURE
        }
    }
}

/// Extensions a directory walk picks up when hand-written TypeScript rides
/// along (build/--check/--types). `.rl` is always collected.
const TS_EXTENSIONS: &[&str] = &["ts", "mts", "cts"];

fn collect_sources(entry: &Path, include_ts: bool, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    let meta = fs::metadata(entry)?;
    if meta.is_file() {
        out.push(entry.to_path_buf());
        return Ok(());
    }
    if meta.is_dir() {
        let mut children: Vec<PathBuf> = fs::read_dir(entry)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .collect();
        children.sort();
        for child in children {
            let meta = fs::metadata(&child)?;
            if meta.is_dir() {
                // Dot-directories (.git, .rl-build, .rl-types, ...) and
                // node_modules are never sources; descending into them
                // would pull generated or vendored TypeScript into the
                // build — or the cache tree into itself.
                let skip = child.file_name().is_some_and(|name| {
                    let name = name.to_string_lossy();
                    name.starts_with('.') || name == "node_modules"
                });
                if !skip {
                    collect_sources(&child, include_ts, out)?;
                }
            } else if meta.is_file()
                && child.extension().is_some_and(|e| {
                    e == "rl" || (include_ts && TS_EXTENSIONS.iter().any(|ts| *ts == e))
                })
            {
                out.push(child);
            }
        }
    }
    Ok(())
}

#[derive(Clone)]
struct Job {
    file: PathBuf,
    out_path: PathBuf,
}

/// `--symbols`: prints, as a JSON array on stdout, each input file's rl
/// enum declarations (positions included) and its direct relative `.rl`
/// imports with the referenced files' exported declarations — the symbol
/// interface language tooling consumes (module graph phase 3). Compiles
/// nothing; unreadable *imported* files yield `"resolved": null` while
/// unreadable *input* files fail the run.
fn symbols_mode(jobs: &[Job]) -> ExitCode {
    let mut entries: Vec<String> = Vec::new();
    let mut failed = false;
    for job in jobs {
        let filename = job.file.display().to_string();
        let source = match fs::read_to_string(&job.file) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("rlc: {filename}: {e}");
                failed = true;
                continue;
            }
        };
        let mut entry = format!("{{\"file\":{}", json_str(&filename));
        entry.push_str(",\"enums\":");
        entry.push_str(&enums_json(&source, &rlc::enum_symbols(&source)));
        entry.push_str(",\"imports\":[");
        let dir = job.file.parent().unwrap_or(Path::new("."));
        let imports = rlc::rl_imports(&source)
            .iter()
            .map(|import| {
                let mut o = format!("{{\"specifier\":{}", json_str(&import.specifier));
                o.push_str(",\"names\":");
                o.push_str(&names_json(&import.names));
                let target = dir.join(&import.specifier);
                match fs::read_to_string(&target) {
                    Ok(imported_src) => {
                        o.push_str(&format!(
                            ",\"resolved\":{}",
                            json_str(&target.display().to_string())
                        ));
                        let exported: Vec<EnumSymbol> = rlc::enum_symbols(&imported_src)
                            .into_iter()
                            .filter(|e| e.exported)
                            .collect();
                        o.push_str(",\"enums\":");
                        o.push_str(&enums_json(&imported_src, &exported));
                    }
                    Err(_) => o.push_str(",\"resolved\":null,\"enums\":[]"),
                }
                o.push('}');
                o
            })
            .collect::<Vec<_>>();
        entry.push_str(&imports.join(","));
        entry.push_str("]}");
        entries.push(entry);
    }
    println!("[{}]", entries.join(","));
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// `--emit-map`: prints, as a JSON array on stdout, each input file's
/// emitted TypeScript and the source<->output byte mappings of every chunk
/// copied verbatim from the source (`rlc::emit_mapped`). Parse + emit only —
/// no rl-level checks, no verification, `.rl`/`@rl/std` specifiers left
/// untouched — so a buffer mid-edit still emits. This is the feed for the
/// language server's virtual TypeScript documents (TASK-050).
fn emit_map_mode(jobs: &[Job]) -> ExitCode {
    let mut entries: Vec<String> = Vec::new();
    let mut failed = false;
    for job in jobs {
        let filename = job.file.display().to_string();
        let source = match fs::read_to_string(&job.file) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("rlc: {filename}: {e}");
                failed = true;
                continue;
            }
        };
        let mapped = rlc::emit_mapped(&source);
        let mappings = mapped
            .mappings
            .iter()
            .map(|m| format!("{{\"src\":{},\"out\":{},\"len\":{}}}", m.src, m.out, m.len))
            .collect::<Vec<_>>()
            .join(",");
        entries.push(format!(
            "{{\"file\":{},\"code\":{},\"mappings\":[{}]}}",
            json_str(&filename),
            json_str(&mapped.code),
            mappings
        ));
    }
    println!("[{}]", entries.join(","));
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// `--sidecar <dir>`: writes `<name>.rl.d.ts` and `<name>.rl.d.ts.map` next
/// to each input `.rl`, from the declarations tsc emitted for that module
/// (`<dir>/<name>.d.ts`, produced with `--emitDeclarationOnly` over rlc's
/// output). The map's `sources` is the `.rl` file, so an editor's "go to
/// definition" from a `.ts` importer lands in the original — not in the
/// generated declarations. Compiles nothing.
fn sidecar_mode(jobs: &[Job], decl_dir: &Path) -> ExitCode {
    let mut failed = false;
    for job in jobs {
        let Some(stem) = job
            .file
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
        else {
            continue;
        };
        let Some(file_name) = job
            .file
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
        else {
            continue;
        };

        let source = match fs::read_to_string(&job.file) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("rlc: {}: {e}", job.file.display());
                failed = true;
                continue;
            }
        };
        let decl_path = decl_dir.join(format!("{stem}.d.ts"));
        let declarations = match fs::read_to_string(&decl_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("rlc: {}: {e}", decl_path.display());
                failed = true;
                continue;
            }
        };

        // `-o` puts the declarations in their own tree (mirroring the input
        // layout); without it they sit next to the source.
        let dts_path = job.out_path.with_file_name(format!("{file_name}.d.ts"));
        let map_path = job.out_path.with_file_name(format!("{file_name}.d.ts.map"));
        let dir = dts_path.parent().unwrap_or(Path::new(".")).to_path_buf();
        if let Err(e) = fs::create_dir_all(&dir) {
            eprintln!("rlc: {}: {e}", dir.display());
            failed = true;
            continue;
        }

        // The map's `sources` is read relative to the map itself, so it has
        // to point back across whatever distance `-o` introduced.
        let sidecar = rlc::build_sidecar(&source, &declarations, &relative_path(&dir, &job.file));
        if let Err(e) = fs::write(&dts_path, &sidecar.declarations) {
            eprintln!("rlc: {}: {e}", dts_path.display());
            failed = true;
            continue;
        }
        if let Err(e) = fs::write(&map_path, &sidecar.map) {
            eprintln!("rlc: {}: {e}", map_path.display());
            failed = true;
            continue;
        }
        eprintln!("rlc: {} → {}", job.file.display(), dts_path.display());
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Path from `from_dir` to `to_file`, `/`-separated — the form a source map
/// needs for its `sources`.
fn relative_path(from_dir: &Path, to_file: &Path) -> String {
    // Canonicalize both or neither: an output directory may not exist yet,
    // and mixing an absolute path with a relative one yields nonsense.
    let (from, to) = match (from_dir.canonicalize(), to_file.canonicalize()) {
        (Ok(from), Ok(to)) => (from, to),
        _ => (from_dir.to_path_buf(), to_file.to_path_buf()),
    };

    let from_parts: Vec<_> = from.components().collect();
    let to_parts: Vec<_> = to.components().collect();
    let shared = from_parts
        .iter()
        .zip(&to_parts)
        .take_while(|(a, b)| a == b)
        .count();

    let mut parts: Vec<String> = vec!["..".to_string(); from_parts.len() - shared];
    parts.extend(
        to_parts[shared..]
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().to_string()),
    );
    if parts.is_empty() {
        return ".".to_string();
    }
    parts.join("/")
}

fn enums_json(source: &str, symbols: &[EnumSymbol]) -> String {
    let objects = symbols
        .iter()
        .map(|e| {
            let (line, col) = rlc::line_col(source, e.offset);
            let cases = e
                .cases
                .iter()
                .map(|c| {
                    let (line, col) = rlc::line_col(source, c.offset);
                    let fields = match &c.fields {
                        None => "null".to_string(),
                        Some(fields) => format!(
                            "[{}]",
                            fields
                                .iter()
                                .map(|f| format!(
                                    "{{\"name\":{},\"optional\":{},\"type\":{}}}",
                                    json_str(&f.name),
                                    f.optional,
                                    json_str(&f.ty)
                                ))
                                .collect::<Vec<_>>()
                                .join(",")
                        ),
                    };
                    format!(
                        "{{\"tag\":{},\"line\":{line},\"col\":{col},\"fields\":{fields}}}",
                        json_str(&c.tag)
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "{{\"name\":{},\"exported\":{},\"generics\":{},\"line\":{line},\"col\":{col},\"cases\":[{cases}]}}",
                json_str(&e.name),
                e.exported,
                json_str(&e.generics)
            )
        })
        .collect::<Vec<_>>();
    format!("[{}]", objects.join(","))
}

fn names_json(names: &RlImportNames) -> String {
    match names {
        RlImportNames::Namespace(ns) => {
            format!("{{\"kind\":\"namespace\",\"name\":{}}}", json_str(ns))
        }
        RlImportNames::Named(entries) => format!(
            "{{\"kind\":\"named\",\"entries\":[{}]}}",
            entries
                .iter()
                .map(|(name, alias)| format!(
                    "{{\"name\":{},\"alias\":{}}}",
                    json_str(name),
                    alias.as_deref().map_or("null".to_string(), json_str)
                ))
                .collect::<Vec<_>>()
                .join(",")
        ),
        RlImportNames::None => "{\"kind\":\"none\"}".to_string(),
    }
}

/// Minimal JSON string encoding (quotes, backslashes, control characters).
fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Declaration tables of the `.rl` modules a run imports, shared by every
/// job.
///
/// The same module is typically imported by many files, and each import
/// used to mean another disk read and another full parse of that module.
/// Here every module is read and parsed at most once per run, and modules
/// that are themselves inputs are served from the sources the run already
/// holds — no second read at all.
struct ExternCache<'a> {
    /// The run's own input sources, keyed by path.
    inputs: HashMap<&'a Path, &'a str>,
    /// Exported declarations per imported path, filled on first use. An
    /// unreadable module caches as an empty table: module resolution is
    /// tsc's domain (`TS2307`), so its enums simply stay unknown.
    decls: Mutex<HashMap<PathBuf, Arc<Vec<ExternEnum>>>>,
}

impl<'a> ExternCache<'a> {
    fn new(inputs: HashMap<&'a Path, &'a str>) -> Self {
        ExternCache {
            inputs,
            decls: Mutex::new(HashMap::new()),
        }
    }

    fn exported_enums(&self, path: &Path) -> Arc<Vec<ExternEnum>> {
        if let Some(hit) = self.decls.lock().expect("extern cache").get(path) {
            return Arc::clone(hit);
        }
        // Parsed outside the lock: a slow miss must not stall other jobs.
        // Two jobs racing on the same module both parse it once; the first
        // insertion wins and both see the same table.
        let decls = Arc::new(match self.inputs.get(path) {
            Some(source) => rlc::exported_enums(source),
            None => match fs::read_to_string(path) {
                Ok(source) => rlc::exported_enums(&source),
                Err(_) => Vec::new(),
            },
        });
        Arc::clone(
            self.decls
                .lock()
                .expect("extern cache")
                .entry(path.to_path_buf())
                .or_insert(decls),
        )
    }
}

/// Collects enum declarations from the file's direct relative `.rl`
/// imports, so matches over imported enums get exhaustiveness-checked
/// (module graph phase 2). One hop, import declarations only — re-exports
/// bring nothing into scope. A specifier that cannot be read is skipped
/// silently: module resolution is tsc's domain (`TS2307`), and an unknown
/// enum simply stays unchecked, exactly as before.
fn collect_extern_enums(file: &Path, imports: &[RlImport], cache: &ExternCache) -> Vec<ExternEnum> {
    let dir = file.parent().unwrap_or(Path::new("."));
    let mut externs: Vec<ExternEnum> = Vec::new();
    for import in imports {
        if matches!(import.names, RlImportNames::None) {
            continue;
        }
        let decls = cache.exported_enums(&dir.join(&import.specifier));
        let from = Some(import.specifier.clone());
        match &import.names {
            RlImportNames::Namespace(ns) => {
                externs.extend(decls.iter().map(|d| ExternEnum {
                    name: format!("{ns}.{}", d.name),
                    tags: d.tags.clone(),
                    from: from.clone(),
                }));
            }
            RlImportNames::Named(entries) => {
                for (name, alias) in entries {
                    if let Some(d) = decls.iter().find(|d| &d.name == name) {
                        externs.push(ExternEnum {
                            name: alias.clone().unwrap_or_else(|| name.clone()),
                            tags: d.tags.clone(),
                            from: from.clone(),
                        });
                    }
                }
            }
            RlImportNames::None => unreachable!(),
        }
    }
    externs
}

/// One input, read and scanned once for the whole run — or the diagnostic
/// its read failed with.
struct Loaded {
    source: String,
    scan: ModuleScan,
}

/// Reads and scans every job's source, in parallel.
fn load_jobs(jobs: &[Job], jobs_limit: Option<usize>) -> Vec<Result<Loaded, String>> {
    par_map(jobs, jobs_limit, |job| {
        let source =
            fs::read_to_string(&job.file).map_err(|e| format!("{}: {e}", job.file.display()))?;
        let scan = rlc::scan_module(&source);
        Ok(Loaded { source, scan })
    })
}

/// How many worker threads a parallel phase should use: the `--jobs` value
/// when given, otherwise one per available core. Never more than there is
/// work for.
fn worker_count(items: usize, jobs_limit: Option<usize>) -> usize {
    let want = jobs_limit.unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
    });
    want.clamp(1, items.max(1))
}

/// Maps `f` over `items` across worker threads, returning the results in
/// input order — so diagnostics and outputs stay byte-identical to a
/// sequential run whatever order the work actually finished in.
///
/// Compilation is per-file and shares nothing mutable, which is what makes
/// this the compiler's main lever on large trees; the ordered result is
/// what keeps the CLI deterministic.
fn par_map<T, R>(items: &[T], jobs_limit: Option<usize>, f: impl Fn(&T) -> R + Sync) -> Vec<R>
where
    T: Sync,
    R: Send,
{
    let workers = worker_count(items.len(), jobs_limit);
    if workers <= 1 || items.len() <= 1 {
        return items.iter().map(f).collect();
    }
    let next = AtomicUsize::new(0);
    let f = &f;
    let batches: Vec<Vec<(usize, R)>> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                let next = &next;
                scope.spawn(move || {
                    let mut done: Vec<(usize, R)> = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        match items.get(i) {
                            Some(item) => done.push((i, f(item))),
                            None => return done,
                        }
                    }
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e)))
            .collect()
    });
    let mut slots: Vec<Option<R>> = (0..items.len()).map(|_| None).collect();
    for (i, r) in batches.into_iter().flatten() {
        slots[i] = Some(r);
    }
    slots
        .into_iter()
        .map(|r| r.expect("every index is produced exactly once"))
        .collect()
}

mod typescript;

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();

    // `rlc help [topic]` — only as the first argument, so a file that
    // happens to be named "help" can still be passed as `./help`.
    if argv.first().is_some_and(|a| a == "help") {
        return run_help(&argv[1..]);
    }

    let mut inputs: Vec<String> = Vec::new();
    let mut out_dir: Option<PathBuf> = None;
    let mut emit_std = false;
    let mut print = false;
    let mut watch = false;
    let mut check = false;
    let mut check_types = false;
    let mut types = false;
    let mut project: Option<PathBuf> = None;
    let mut banner = true;
    let mut verify = true;
    let mut symbols = false;
    let mut emit_map = false;
    let mut sidecar_dir: Option<PathBuf> = None;
    let mut node: Option<PathBuf> = None;
    let mut rewrite_imports = ImportRewrite::default();
    let mut jobs_limit: Option<usize> = None;

    let mut it = argv.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-h" | "--help" => {
                usage();
                return ExitCode::SUCCESS;
            }
            "-v" | "--version" => {
                println!("{VERSION}");
                return ExitCode::SUCCESS;
            }
            "-p" | "--print" => print = true,
            "-w" | "--watch" => watch = true,
            "--check" => check = true,
            "--check-types" => check_types = true,
            "--types" => {
                check_types = true;
                types = true;
            }
            "--project" => match it.next() {
                Some(path) => project = Some(PathBuf::from(path)),
                None => {
                    eprintln!("rlc: --project requires a path to a tsconfig.json");
                    return ExitCode::FAILURE;
                }
            },
            "--symbols" => symbols = true,
            "--emit-map" => emit_map = true,
            "--no-banner" => banner = false,
            "--no-verify" => verify = false,
            "--emit-std" => emit_std = true,
            "--sidecar" => match it.next() {
                Some(dir) => sidecar_dir = Some(PathBuf::from(dir)),
                None => {
                    eprintln!("rlc: --sidecar requires a directory of tsc-emitted .d.ts files");
                    return ExitCode::FAILURE;
                }
            },
            "-j" | "--jobs" => match it.next().map(|n| n.parse::<usize>()) {
                Some(Ok(n)) if n >= 1 => jobs_limit = Some(n),
                Some(_) => {
                    eprintln!("rlc: --jobs expects a positive number of parallel compiles");
                    return ExitCode::FAILURE;
                }
                None => {
                    eprintln!("rlc: --jobs requires a value");
                    return ExitCode::FAILURE;
                }
            },
            "-o" | "--out-dir" => match it.next() {
                Some(dir) => out_dir = Some(PathBuf::from(dir)),
                None => {
                    eprintln!("rlc: --out-dir requires a value");
                    return ExitCode::FAILURE;
                }
            },
            "--node" => match it.next() {
                Some(path) => node = Some(PathBuf::from(path)),
                None => {
                    eprintln!("rlc: --node requires a path to the node binary");
                    return ExitCode::FAILURE;
                }
            },
            "--rewrite-imports" => match it.next().map(String::as_str) {
                Some("js") => rewrite_imports = ImportRewrite::Js,
                Some("ts") => rewrite_imports = ImportRewrite::Ts,
                Some("off") => rewrite_imports = ImportRewrite::Off,
                Some(other) => {
                    eprintln!("rlc: --rewrite-imports expects js, ts, or off (got {other})");
                    return ExitCode::FAILURE;
                }
                None => {
                    eprintln!("rlc: --rewrite-imports requires a value (js, ts, or off)");
                    return ExitCode::FAILURE;
                }
            },
            other if other.starts_with('-') => {
                eprintln!("rlc: unknown option {other}");
                return ExitCode::FAILURE;
            }
            other => inputs.push(other.to_string()),
        }
    }

    // The standard library on stdout — how a bundler plugin serves the
    // module from memory. Since the build materializes it on its own
    // (`@rl/std` auto-emission), this combines with nothing else.
    if emit_std {
        if !inputs.is_empty() {
            eprintln!("rlc: --emit-std takes no inputs (the build materializes @rl/std itself)");
            return ExitCode::FAILURE;
        }
        let mut code = rlc::STD_SOURCE.to_string();
        if banner {
            code = format!("// @generated by rlc --emit-std — do not edit directly.\n{code}");
        }
        print!("{code}");
        return ExitCode::SUCCESS;
    }

    if inputs.is_empty() {
        usage();
        return ExitCode::FAILURE;
    }

    if check_types && (print || check || symbols || emit_map || sidecar_dir.is_some()) {
        eprintln!(
            "rlc: --types/--check-types does not combine with -p, --check, --symbols, \
             --emit-map, or --sidecar"
        );
        return ExitCode::FAILURE;
    }

    // Tooling modes stay .rl-only; the compile modes carry hand-written
    // TypeScript along so the output tree is complete.
    let include_ts = !symbols && !emit_map && sidecar_dir.is_none();

    if check_types {
        // Sidecars keep the directory `--types` has always written them to,
        // so a project's tsconfig `paths` and `.gitignore` keep pointing at
        // the same place. A check that writes nothing needs no directory.
        let sidecar_out = types.then(|| out_dir.unwrap_or_else(|| PathBuf::from(TYPES_DIR)));
        return typescript::check::run(
            &inputs,
            project.as_deref(),
            node.as_deref(),
            sidecar_out.as_deref(),
            types,
            watch,
        );
    }

    let jobs = match build_jobs(&inputs, out_dir.as_deref(), include_ts) {
        Ok(jobs) => jobs,
        Err(code) => return code,
    };

    if jobs.is_empty() {
        eprintln!("rlc: no sources found");
        return ExitCode::FAILURE;
    }

    if symbols {
        return symbols_mode(&jobs);
    }

    if emit_map {
        return emit_map_mode(&jobs);
    }

    if let Some(dir) = &sidecar_dir {
        return sidecar_mode(&jobs, dir);
    }

    let build = BuildOptions {
        banner,
        print,
        check,
        verify,
        rewrite_imports,
        out_dir: out_dir.clone(),
        jobs: jobs_limit,
    };

    if watch {
        return watch_mode(&inputs, out_dir.as_deref(), &build);
    }

    if compile_jobs(&jobs, &build) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Everything the compile step needs beyond the file list.
struct BuildOptions {
    banner: bool,
    print: bool,
    check: bool,
    verify: bool,
    rewrite_imports: ImportRewrite,
    /// Output root, when `-o` was given — also where the standard library
    /// module is written if an input imports it.
    out_dir: Option<PathBuf>,
    /// Worker threads for the parallel phases (`--jobs`); `None` means one
    /// per available core.
    jobs: Option<usize>,
}

/// Where the standard library goes when an input imports `@rl/std`: the
/// output root (`-o`), or the common ancestor of the outputs when compiling
/// in place. `None` when nothing imports it.
fn std_placement(
    jobs: &[Job],
    loaded: &[Result<Loaded, String>],
    out_dir: Option<&Path>,
) -> Option<PathBuf> {
    let needed = loaded
        .iter()
        .any(|l| l.as_ref().is_ok_and(|l| l.scan.imports_std));
    if !needed {
        return None;
    }
    let dir = match out_dir {
        Some(dir) => dir.to_path_buf(),
        None => common_ancestor(jobs)?,
    };
    Some(dir.join("rl.ts"))
}

/// The deepest directory every output shares.
fn common_ancestor(jobs: &[Job]) -> Option<PathBuf> {
    let mut dirs = jobs.iter().map(|job| {
        job.out_path
            .parent()
            .unwrap_or(Path::new("."))
            .to_path_buf()
    });
    let first = dirs.next()?;
    Some(dirs.fold(first, |acc, dir| {
        let shared: PathBuf = acc
            .components()
            .zip(dir.components())
            .take_while(|(a, b)| a == b)
            .map(|(a, _)| a.as_os_str())
            .collect();
        if shared.as_os_str().is_empty() {
            PathBuf::from(".")
        } else {
            shared
        }
    }))
}

/// How one output refers to the standard library module. `None` leaves the
/// bare `@rl/std` in place (`--rewrite-imports off`).
fn std_specifier(job: &Job, std_file: &Path, rewrite: ImportRewrite) -> Option<String> {
    let name = match rewrite {
        ImportRewrite::Js => "rl.js",
        ImportRewrite::Ts => "rl.ts",
        ImportRewrite::Off => return None,
    };
    let job_dir = job.out_path.parent().unwrap_or(Path::new("."));
    let std_dir = std_file.parent().unwrap_or(Path::new("."));
    let rel = relative_path(job_dir, std_dir);
    Some(if rel == "." {
        format!("./{name}")
    } else {
        format!("{rel}/{name}")
    })
}

/// Expands the command line's inputs into one job per source file. `.rl`
/// files compile to a `.ts` of the same stem; hand-written TypeScript
/// (collected when `include_ts` is set) keeps its file name and passes
/// through with its `.rl` import specifiers rewritten.
fn build_jobs(
    inputs: &[String],
    out_dir: Option<&Path>,
    include_ts: bool,
) -> Result<Vec<Job>, ExitCode> {
    let mut jobs: Vec<Job> = Vec::new();
    for input in inputs {
        let input_path = Path::new(input);
        if !input_path.exists() {
            eprintln!("rlc: no such file or directory: {input}");
            return Err(ExitCode::FAILURE);
        }
        let is_dir = input_path.is_dir();
        let mut files = Vec::new();
        if let Err(e) = collect_sources(input_path, include_ts, &mut files) {
            eprintln!("rlc: {input}: {e}");
            return Err(ExitCode::FAILURE);
        }
        for file in files {
            let out_name = if file.extension().is_some_and(|e| e == "rl") {
                file.with_extension("ts")
            } else {
                file.clone()
            };
            let out_path = match out_dir {
                Some(dir) => {
                    let rel = if is_dir {
                        out_name
                            .strip_prefix(input_path)
                            .unwrap_or(&out_name)
                            .to_path_buf()
                    } else {
                        PathBuf::from(out_name.file_name().unwrap())
                    };
                    dir.join(rel)
                }
                None => out_name,
            };
            jobs.push(Job { file, out_path });
        }
    }
    Ok(jobs)
}

/// Whether two paths name the same file. The output side may not exist
/// yet, so the parents are compared canonically and the file names
/// literally.
fn same_file(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    if a.file_name() != b.file_name() {
        return false;
    }
    let canon = |p: &Path| p.parent().unwrap_or(Path::new(".")).canonicalize();
    matches!((canon(a), canon(b)), (Ok(x), Ok(y)) if x == y)
}

/// What compiling one job produced: the diagnostics it wants printed (in
/// job order), whether it failed, and any text the parent still has to hand
/// out — stdout under `-p`, or an output file whose path more than one job
/// claims, which only the parent can serialize deterministically.
#[derive(Default)]
struct Outcome {
    messages: Vec<String>,
    failed: bool,
    pending: Option<String>,
}

/// Compiles every job. Returns true if any of them failed.
///
/// The run is staged so each input is touched once: read and scanned in
/// parallel, then compiled in parallel against a shared table of imported
/// declarations. Diagnostics are collected per job and printed in job
/// order, so the output of a parallel run is identical to a sequential one.
fn compile_jobs(jobs: &[Job], opts: &BuildOptions) -> bool {
    let mut failed = false;
    let loaded = load_jobs(jobs, opts.jobs);

    // The standard library is written out for the project, not per file:
    // one module the outputs point at.
    let std_file = std_placement(jobs, &loaded, opts.out_dir.as_deref());
    if let Some(file) = &std_file
        && !opts.check
        && !opts.print
        && jobs.iter().any(|job| same_file(&job.file, file))
    {
        eprintln!(
            "rlc: {}: the standard library would overwrite an input — pass -o <dir>",
            file.display()
        );
        failed = true;
    } else if let Some(file) = &std_file
        && !opts.check
        && !opts.print
    {
        let mut code = rlc::STD_SOURCE.to_string();
        if opts.banner {
            code = format!("// @generated by rlc — do not edit directly.\n{code}");
        }
        let wrote = file
            .parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|()| fs::write(file, &code));
        match wrote {
            Ok(()) => eprintln!("rlc: std → {}", file.display()),
            Err(e) => {
                eprintln!("rlc: {}: {e}", file.display());
                failed = true;
            }
        }
    }

    // Two inputs can claim one output path (`x.rl` and a hand-written
    // `x.ts` both emit `x.ts`), and the later job wins. Those writes go
    // back to the parent so the winner stays the same as in a sequential
    // run; every other job writes itself, straight from its worker.
    let mut claims: HashMap<&Path, usize> = HashMap::with_capacity(jobs.len());
    for job in jobs {
        *claims.entry(job.out_path.as_path()).or_default() += 1;
    }
    let contested = |job: &Job| claims[job.out_path.as_path()] > 1;

    let cache = ExternCache::new(
        jobs.iter()
            .zip(&loaded)
            .filter_map(|(job, l)| Some((job.file.as_path(), l.as_ref().ok()?.source.as_str())))
            .collect(),
    );

    let outcomes = par_map(
        &jobs.iter().zip(&loaded).collect::<Vec<_>>(),
        opts.jobs,
        |(job, loaded)| {
            let mut out = Outcome::default();
            let filename = job.file.display().to_string();
            // A pass-through `.ts` compiled in place would land on top of
            // its own source (with specifiers rewritten) — refuse rather
            // than destroy hand-written code.
            if !opts.print && !opts.check && same_file(&job.file, &job.out_path) {
                out.messages.push(format!(
                    "rlc: {filename}: output would overwrite the input — pass -o <dir>"
                ));
                out.failed = true;
                return out;
            }
            let loaded = match loaded {
                Ok(loaded) => loaded,
                Err(e) => {
                    out.messages.push(format!("rlc: {e}"));
                    out.failed = true;
                    return out;
                }
            };
            let extern_enums = collect_extern_enums(&job.file, &loaded.scan.imports, &cache);
            let std_import = std_file
                .as_ref()
                .and_then(|file| std_specifier(job, file, opts.rewrite_imports));
            let options = Options {
                filename: Some(&filename),
                verify: opts.verify,
                rewrite_imports: opts.rewrite_imports,
                extern_enums: &extern_enums,
                defer_to_checker: false,
                std_import: std_import.as_deref(),
            };
            let mut code = match compile(&loaded.source, &options) {
                Ok(c) => c,
                Err(e) => {
                    out.messages.push(format!("rlc: {e}"));
                    out.failed = true;
                    return out;
                }
            };
            if opts.banner {
                let base = job.file.file_name().unwrap().to_string_lossy();
                code = format!("// @generated from {base} by rlc — do not edit directly.\n{code}");
            }
            if opts.print || (!opts.check && contested(job)) {
                out.pending = Some(code);
                return out;
            }
            if !opts.check {
                if let Err(e) = write_output(&job.out_path, &code) {
                    out.messages.push(e);
                    out.failed = true;
                    return out;
                }
                out.messages.push(format!(
                    "rlc: {} → {}",
                    job.file.display(),
                    job.out_path.display()
                ));
            }
            out
        },
    );

    for (job, outcome) in jobs.iter().zip(outcomes) {
        for message in &outcome.messages {
            eprintln!("{message}");
        }
        failed |= outcome.failed;
        let Some(code) = outcome.pending else {
            continue;
        };
        if opts.print {
            print!("{code}");
            continue;
        }
        match write_output(&job.out_path, &code) {
            Ok(()) => eprintln!("rlc: {} → {}", job.file.display(), job.out_path.display()),
            Err(e) => {
                eprintln!("{e}");
                failed = true;
            }
        }
    }
    failed
}

/// Writes one compiled output, creating its directory. The error is already
/// formatted as a diagnostic line.
fn write_output(out_path: &Path, code: &str) -> Result<(), String> {
    if let Some(parent) = out_path.parent()
        && let Err(e) = fs::create_dir_all(parent)
    {
        return Err(format!("rlc: {}: {e}", parent.display()));
    }
    fs::write(out_path, code).map_err(|e| format!("rlc: {}: {e}", out_path.display()))
}

/// How often `--watch` re-reads the inputs' timestamps.
pub(crate) const WATCH_INTERVAL: Duration = Duration::from_millis(300);

/// `--watch`: compile once, then keep compiling what changes.
///
/// Inputs are re-expanded every round, so files added to a watched directory
/// are picked up. A changed file drags its **dependents** along: a `.rl` that
/// imports it is checked against the new declarations, which is what makes
/// project-wide exhaustiveness errors appear on the importing side.
///
/// Runs until interrupted; the exit code is only reached on a fatal input
/// error.
fn watch_mode(inputs: &[String], out_dir: Option<&Path>, opts: &BuildOptions) -> ExitCode {
    let mut stamps: HashMap<PathBuf, SystemTime> = HashMap::new();
    let mut first = true;

    loop {
        let jobs = match build_jobs(inputs, out_dir, true) {
            Ok(jobs) => jobs,
            // An input can disappear mid-edit; keep watching rather than
            // tearing the session down.
            Err(_) => {
                thread::sleep(WATCH_INTERVAL);
                continue;
            }
        };

        let current: HashMap<PathBuf, SystemTime> = jobs
            .iter()
            .map(|job| {
                let stamp = fs::metadata(&job.file)
                    .and_then(|meta| meta.modified())
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                (job.file.clone(), stamp)
            })
            .collect();

        let changed: Vec<PathBuf> = if first {
            jobs.iter().map(|job| job.file.clone()).collect()
        } else {
            current
                .iter()
                .filter(|(file, stamp)| stamps.get(*file) != Some(stamp))
                .map(|(file, _)| file.clone())
                .collect()
        };

        if !changed.is_empty() {
            let targets = with_dependents(&jobs, &changed);
            let selected: Vec<Job> = jobs
                .iter()
                .filter(|job| targets.contains(&job.file))
                .cloned()
                .collect();
            let failed = compile_jobs(&selected, opts);
            eprintln!(
                "rlc: {} file(s) {} — watching",
                selected.len(),
                if failed { "failed" } else { "ok" }
            );
        }

        if first {
            eprintln!("rlc: watching {} file(s) — Ctrl-C to stop", jobs.len());
            first = false;
        }
        stamps = current;
        thread::sleep(WATCH_INTERVAL);
    }
}

/// Default `-o` of `--types` — where the sidecars land.
const TYPES_DIR: &str = ".rl-types";

/// The file's path relative to whichever input directory contains it, so
/// the sidecar tree mirrors the source tree rather than the whole cwd.
fn input_relative(file: &Path, inputs: &[String]) -> PathBuf {
    for input in inputs {
        let root = Path::new(input);
        if root.is_dir()
            && let Ok(relative) = file.strip_prefix(root)
        {
            return relative.to_path_buf();
        }
    }
    PathBuf::from(file.file_name().unwrap_or_default())
}

/// The changed files plus every job that imports one of them.
fn with_dependents(jobs: &[Job], changed: &[PathBuf]) -> HashSet<PathBuf> {
    let mut targets: HashSet<PathBuf> = changed.iter().cloned().collect();
    let changed_real: HashSet<PathBuf> = changed
        .iter()
        .filter_map(|file| file.canonicalize().ok())
        .collect();

    for job in jobs {
        if targets.contains(&job.file) {
            continue;
        }
        let Ok(source) = fs::read_to_string(&job.file) else {
            continue;
        };
        let dir = job.file.parent().unwrap_or(Path::new("."));
        let imports_changed = rlc::rl_imports(&source).iter().any(|import| {
            dir.join(&import.specifier)
                .canonicalize()
                .is_ok_and(|target| changed_real.contains(&target))
        });
        if imports_changed {
            targets.insert(job.file.clone());
        }
    }
    targets
}

#[cfg(test)]
mod help_tests {
    use super::*;

    #[test]
    fn every_topic_resolves_to_a_nonempty_section() {
        for (name, _, heading) in HELP_TOPICS {
            let section = guide_section(heading);
            assert!(
                !section.trim().is_empty(),
                "topic {name}: heading {heading:?} not found in docs/ai/rl.md"
            );
            if !heading.is_empty() {
                assert!(section.starts_with(heading), "topic {name}: wrong slice");
            }
        }
    }

    #[test]
    fn sections_stop_at_the_next_heading() {
        let section = guide_section("## match");
        assert!(section.contains("or-pattern"));
        assert!(!section.contains("\n## try"), "section leaked past its end");
        let preamble = guide_section("");
        assert!(preamble.contains("CONTRACTS"));
        assert!(!preamble.contains("\n## "));
    }

    #[test]
    fn topic_names_and_aliases_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for (name, aliases, _) in HELP_TOPICS {
            assert!(seen.insert(*name), "duplicate topic {name}");
            for alias in *aliases {
                assert!(seen.insert(*alias), "duplicate alias {alias}");
            }
        }
        assert!(!seen.contains("all") && !seen.contains("guide"));
    }
}
