//! rlc — compile .rl sources into a complete TypeScript tree.
//!
//!   rlc -o build src/              builds src/ under build/: .rl files are
//!                                  compiled, hand-written .ts files pass
//!                                  through (with .rl import specifiers
//!                                  rewritten), and the standard library is
//!                                  materialized when something imports it
//!   rlc --types src/               writes editor/typecheck sidecars for the
//!                                  same tree (compiles to .rl-build/, runs
//!                                  tsc --emitDeclarationOnly, emits
//!                                  .rl-types/<name>.rl.d.ts + .map)
//!   rlc --check src/               compiles without writing anything
//!   rlc -p file.rl                 prints one compiled module to stdout

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::thread;
use std::time::{Duration, SystemTime};

use rlc::{EnumSymbol, ExternEnum, ImportRewrite, Options, RlImportNames, compile};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn usage() {
    println!(
        "rlc v{VERSION} — rl to TypeScript compiler

Usage: rlc [options] <file | dir> ...

Builds a complete TypeScript tree: .rl files are compiled, hand-written
.ts files pass through byte-for-byte (with relative .rl import specifiers
rewritten), and the standard library is materialized when an input
imports \"@rl/std\". Types come from the same sources via --types.

Options:
  -o, --out-dir <dir>   write outputs under <dir> (mirrors input paths)
  -w, --watch           keep running; recompile inputs (and their importers)
                        as they change
  --check               compile only; write nothing (syntax check)
  --types               write editor/typecheck sidecars instead of building:
                        compiles the tree to .rl-build/, runs
                        tsc --emitDeclarationOnly over it, and emits
                        <name>.rl.d.ts + .map under -o (default .rl-types)
  --node <path>         node binary --types should run its host with
                        (default: node on PATH; TypeScript comes from the
                        project's node_modules)
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

/// Collects enum declarations from the file's direct relative `.rl`
/// imports, so matches over imported enums get exhaustiveness-checked
/// (module graph phase 2). One hop, import declarations only — re-exports
/// bring nothing into scope. A specifier that cannot be read is skipped
/// silently: module resolution is tsc's domain (`TS2307`), and an unknown
/// enum simply stays unchecked, exactly as before.
fn collect_extern_enums(file: &Path, source: &str) -> Vec<ExternEnum> {
    let dir = file.parent().unwrap_or(Path::new("."));
    let mut externs: Vec<ExternEnum> = Vec::new();
    for import in rlc::rl_imports(source) {
        if matches!(import.names, RlImportNames::None) {
            continue;
        }
        let Ok(imported_src) = fs::read_to_string(dir.join(&import.specifier)) else {
            continue;
        };
        let decls = rlc::exported_enums(&imported_src);
        let from = Some(import.specifier.clone());
        match &import.names {
            RlImportNames::Namespace(ns) => {
                externs.extend(decls.into_iter().map(|d| ExternEnum {
                    name: format!("{ns}.{}", d.name),
                    from: from.clone(),
                    ..d
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

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();

    let mut inputs: Vec<String> = Vec::new();
    let mut out_dir: Option<PathBuf> = None;
    let mut emit_std = false;
    let mut print = false;
    let mut watch = false;
    let mut check = false;
    let mut types = false;
    let mut banner = true;
    let mut verify = true;
    let mut symbols = false;
    let mut emit_map = false;
    let mut sidecar_dir: Option<PathBuf> = None;
    let mut node: Option<PathBuf> = None;
    let mut rewrite_imports = ImportRewrite::default();

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
            "--types" => types = true,
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

    if types && (print || check || symbols || emit_map || sidecar_dir.is_some()) {
        eprintln!(
            "rlc: --types does not combine with -p, --check, --symbols, --emit-map, or --sidecar"
        );
        return ExitCode::FAILURE;
    }

    // Tooling modes stay .rl-only; the compile modes carry hand-written
    // TypeScript along so the output tree is complete.
    let include_ts = !symbols && !emit_map && sidecar_dir.is_none();

    if types {
        let opts = TypesOptions {
            out_dir: out_dir.unwrap_or_else(|| PathBuf::from(TYPES_DIR)),
            node,
            verify,
        };
        return if watch {
            types_watch(&inputs, &opts)
        } else {
            match types_once(&inputs, &opts) {
                Ok(false) => ExitCode::SUCCESS,
                Ok(true) | Err(_) => ExitCode::FAILURE,
            }
        };
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
}

/// Where the standard library goes when an input imports `@rl/std`: the
/// output root (`-o`), or the common ancestor of the outputs when compiling
/// in place. `None` when nothing imports it.
fn std_placement(jobs: &[Job], out_dir: Option<&Path>) -> Option<PathBuf> {
    let needed = jobs
        .iter()
        .any(|job| fs::read_to_string(&job.file).is_ok_and(|src| rlc::imports_std(&src)));
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

/// Compiles every job. Returns true if any of them failed.
fn compile_jobs(jobs: &[Job], opts: &BuildOptions) -> bool {
    let mut failed = false;

    // The standard library is written out for the project, not per file:
    // one module the outputs point at.
    let std_file = std_placement(jobs, opts.out_dir.as_deref());
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

    for job in jobs {
        let filename = job.file.display().to_string();
        // A pass-through `.ts` compiled in place would land on top of its
        // own source (with specifiers rewritten) — refuse rather than
        // destroy hand-written code.
        if !opts.print && !opts.check && same_file(&job.file, &job.out_path) {
            eprintln!("rlc: {filename}: output would overwrite the input — pass -o <dir>");
            failed = true;
            continue;
        }
        let source = match fs::read_to_string(&job.file) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("rlc: {filename}: {e}");
                failed = true;
                continue;
            }
        };
        let extern_enums = collect_extern_enums(&job.file, &source);
        let std_import = std_file
            .as_ref()
            .and_then(|file| std_specifier(job, file, opts.rewrite_imports));
        let options = Options {
            filename: Some(&filename),
            verify: opts.verify,
            rewrite_imports: opts.rewrite_imports,
            extern_enums: &extern_enums,
            std_import: std_import.as_deref(),
        };
        let mut code = match compile(&source, &options) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("rlc: {e}");
                failed = true;
                continue;
            }
        };
        if opts.banner {
            let base = job.file.file_name().unwrap().to_string_lossy();
            code = format!("// @generated from {base} by rlc — do not edit directly.\n{code}");
        }
        if opts.print {
            print!("{code}");
        } else if !opts.check {
            if let Some(parent) = job.out_path.parent()
                && let Err(e) = fs::create_dir_all(parent)
            {
                eprintln!("rlc: {}: {e}", parent.display());
                failed = true;
                continue;
            }
            if let Err(e) = fs::write(&job.out_path, &code) {
                eprintln!("rlc: {}: {e}", job.out_path.display());
                failed = true;
                continue;
            }
            eprintln!("rlc: {} → {}", job.file.display(), job.out_path.display());
        }
    }
    failed
}

/// How often `--watch` re-reads the inputs' timestamps.
const WATCH_INTERVAL: Duration = Duration::from_millis(300);

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

/// The in-memory host script, embedded so there is nothing to install and
/// no version to drift: it is written to a temp file per run and executed
/// with `node`.
const TYPES_HOST: &str = include_str!("types_host.mjs");

/// Virtual module name the standard library takes when declarations are
/// emitted — the same shape the bundler plugin uses.
const STD_VIRTUAL: &str = "__rl_std__.ts";

/// The compiler options declaration emit runs with. Passed as data, not as
/// a synthesized `tsconfig.json`: `.rl` specifiers and `@rl/std` are
/// resolved by the host itself, so no `paths`, no shim files, and no
/// `allowArbitraryExtensions` are involved.
const TYPES_COMPILER_OPTIONS: &str = r#"{
  "target": "es2022",
  "module": "preserve",
  "moduleResolution": "bundler",
  "allowImportingTsExtensions": true,
  "declaration": true,
  "emitDeclarationOnly": true,
  "skipLibCheck": true,
  "strict": true
}"#;

/// Everything the `--types` pipeline needs beyond the file list.
struct TypesOptions {
    /// Where the sidecars are written (mirrors the input layout).
    out_dir: PathBuf,
    /// Explicit `node` binary (`--node`); otherwise `node` on PATH.
    node: Option<PathBuf>,
    verify: bool,
}

/// One module handed to the host: the text a `.rl` file compiles to, keyed
/// by the path it would occupy on disk.
struct VirtualModule {
    /// Path relative to the project root (`src/engine.ts`).
    path: String,
    text: String,
    /// The `.rl` file it came from — `None` for the standard library.
    source: Option<PathBuf>,
}

/// `--types`: one pass of the unified type pipeline.
///
/// Compiles every `.rl` in memory, has the embedded host emit declarations
/// over those modules plus the project's own `.ts` files (read from disk,
/// never copied), and writes an editor sidecar per `.rl`. Nothing but the
/// sidecars touches the file system. `Ok(failed)` reports a round that ran;
/// `Err` is fatal (bad inputs, no node, no TypeScript) and ends a watch
/// session too.
fn types_once(inputs: &[String], opts: &TypesOptions) -> Result<bool, ExitCode> {
    // `out_dir: None` puts each output next to its source, which is exactly
    // the virtual path a compiled module should occupy.
    let jobs = build_jobs(inputs, None, true)?;
    if jobs.is_empty() {
        eprintln!("rlc: no sources found");
        return Err(ExitCode::FAILURE);
    }

    let mut failed = false;
    let mut modules: Vec<VirtualModule> = Vec::new();
    let mut rl_map: Vec<(String, String)> = Vec::new();
    let mut sources: Vec<String> = Vec::new();
    let mut needs_std = false;

    for job in &jobs {
        if job.file.extension().is_none_or(|e| e != "rl") {
            // Hand-written TypeScript: the host reads it in place. It still
            // joins the program by path so its own type errors surface.
            sources.push(slash_path(&job.file));
            continue;
        }
        // A virtual module must not shadow a real file of the same name.
        if job.out_path.exists() {
            eprintln!(
                "rlc: {} would shadow {} — rename one of them",
                job.file.display(),
                job.out_path.display()
            );
            return Err(ExitCode::FAILURE);
        }
        let filename = job.file.display().to_string();
        let source = match fs::read_to_string(&job.file) {
            Ok(text) => text,
            Err(e) => {
                eprintln!("rlc: {filename}: {e}");
                failed = true;
                continue;
            }
        };
        needs_std |= rlc::imports_std(&source);
        let extern_enums = collect_extern_enums(&job.file, &source);
        let options = Options {
            filename: Some(&filename),
            verify: opts.verify,
            // Declaration emit preserves specifiers, and the sidecars must
            // carry the ones the consumer resolves — the originals.
            rewrite_imports: ImportRewrite::Off,
            extern_enums: &extern_enums,
            std_import: None,
        };
        match compile(&source, &options) {
            Ok(text) => {
                let path = slash_path(&job.out_path);
                rl_map.push((slash_path(&job.file), path.clone()));
                modules.push(VirtualModule {
                    path,
                    text,
                    source: Some(job.file.clone()),
                });
            }
            Err(e) => {
                eprintln!("rlc: {e}");
                failed = true;
            }
        }
    }

    if modules.is_empty() {
        return Ok(failed);
    }

    let std_module = needs_std.then(|| VirtualModule {
        path: STD_VIRTUAL.to_string(),
        text: rlc::STD_SOURCE.to_string(),
        source: None,
    });

    let emitted = run_types_host(
        opts.node.as_deref(),
        &modules,
        std_module.as_ref(),
        &sources,
        &rl_map,
    )?;
    for diagnostic in &emitted.diagnostics {
        failed = true;
        eprintln!("rlc: {diagnostic}");
    }

    for module in modules.iter().chain(std_module.as_ref()) {
        let Some(declarations) = emitted.declarations.get(&module.path) else {
            eprintln!("rlc: no declarations emitted for {}", module.path);
            failed = true;
            continue;
        };
        match &module.source {
            Some(file) => {
                if write_sidecar(file, declarations, &opts.out_dir, inputs) {
                    failed = true;
                }
            }
            None => {
                // The standard library's declarations ride along so
                // `@rl/std` can be mapped in the consumer's `paths`.
                let to = opts.out_dir.join("rl.d.ts");
                let written =
                    fs::create_dir_all(&opts.out_dir).and_then(|()| fs::write(&to, declarations));
                match written {
                    Ok(()) => eprintln!("rlc: std types → {}", to.display()),
                    Err(e) => {
                        eprintln!("rlc: {}: {e}", to.display());
                        failed = true;
                    }
                }
            }
        }
    }

    Ok(failed)
}

/// Writes one `.rl` file's sidecar (`<name>.rl.d.ts` + `.map`) under
/// `out_dir`, mirroring the input layout. Returns true on failure.
fn write_sidecar(file: &Path, declarations: &str, out_dir: &Path, inputs: &[String]) -> bool {
    let source = match fs::read_to_string(file) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("rlc: {}: {e}", file.display());
            return true;
        }
    };
    let file_name = file.file_name().unwrap_or_default().to_string_lossy();
    let relative = input_relative(file, inputs);
    let sidecar_path = out_dir
        .join(relative)
        .with_file_name(format!("{file_name}.d.ts"));
    let map_path = sidecar_path.with_extension("ts.map");
    let dir = sidecar_path
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf();
    if let Err(e) = fs::create_dir_all(&dir) {
        eprintln!("rlc: {}: {e}", dir.display());
        return true;
    }
    let sidecar = rlc::build_sidecar(&source, declarations, &relative_path(&dir, file));
    match fs::write(&sidecar_path, &sidecar.declarations)
        .and_then(|()| fs::write(&map_path, &sidecar.map))
    {
        Ok(()) => {
            eprintln!("rlc: {} → {}", file.display(), sidecar_path.display());
            false
        }
        Err(e) => {
            eprintln!("rlc: {}: {e}", sidecar_path.display());
            true
        }
    }
}

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

/// What the host sent back.
struct EmittedTypes {
    /// Virtual module path → declaration text.
    declarations: HashMap<String, String>,
    /// Already formatted `file:line:col: message` diagnostics.
    diagnostics: Vec<String>,
}

/// Runs the embedded host with `node`, handing it the compiled modules on
/// stdin and reading declarations back from stdout.
fn run_types_host(
    node: Option<&Path>,
    modules: &[VirtualModule],
    std_module: Option<&VirtualModule>,
    sources: &[String],
    rl_map: &[(String, String)],
) -> Result<EmittedTypes, ExitCode> {
    let dir = std::env::temp_dir().join(format!("rlc-types-{}", std::process::id()));
    let script = dir.join("types_host.mjs");
    if let Err(e) = fs::create_dir_all(&dir).and_then(|()| fs::write(&script, TYPES_HOST)) {
        eprintln!("rlc: {}: {e}", script.display());
        return Err(ExitCode::FAILURE);
    }

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let job = types_job(&cwd, modules, std_module, sources, rl_map);
    let binary = node.map_or_else(|| PathBuf::from("node"), Path::to_path_buf);

    let mut child = match Command::new(&binary)
        .arg(&script)
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

    // Write the job from another thread: a large job would otherwise fill
    // the pipe while this side is still waiting to read.
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
        if output.status.code() == Some(2) {
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

/// Serializes the host's job. Hand-written `.ts` files are deliberately
/// absent — the host reads them from disk.
fn types_job(
    cwd: &Path,
    modules: &[VirtualModule],
    std_module: Option<&VirtualModule>,
    sources: &[String],
    rl_map: &[(String, String)],
) -> String {
    let list = |items: &[&VirtualModule]| {
        items
            .iter()
            .map(|m| {
                format!(
                    "{{\"path\":{},\"text\":{}}}",
                    json_str(&m.path),
                    json_str(&m.text)
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    };
    let refs: Vec<&VirtualModule> = modules.iter().collect();
    let std_json = match std_module {
        Some(module) => list(&[module]),
        None => "null".to_string(),
    };
    let map = rl_map
        .iter()
        .map(|(source, virtual_path)| format!("{}:{}", json_str(source), json_str(virtual_path)))
        .collect::<Vec<_>>()
        .join(",");

    let source_list = sources
        .iter()
        .map(|path| json_str(path))
        .collect::<Vec<_>>()
        .join(",");

    format!(
        "{{\"cwd\":{},\"compilerOptions\":{},\"modules\":[{}],\"std\":{},\"sources\":[{}],\"rlModules\":{{{}}}}}",
        json_str(&cwd.display().to_string()),
        TYPES_COMPILER_OPTIONS,
        list(&refs),
        std_json,
        source_list,
        map
    )
}

/// Reads the host's reply. The shapes are fixed and produced by our own
/// script, so a minimal scan beats pulling in a JSON parser.
fn parse_types_result(stdout: &str) -> EmittedTypes {
    let mut declarations = HashMap::new();
    let mut diagnostics = Vec::new();

    for entry in json_objects(stdout, "\"declarations\":[") {
        if let (Some(path), Some(text)) = (json_field(&entry, "path"), json_field(&entry, "text")) {
            declarations.insert(path, text);
        }
    }
    for entry in json_objects(stdout, "\"diagnostics\":[") {
        let message = json_field(&entry, "message").unwrap_or_default();
        match json_field(&entry, "file") {
            Some(file) => {
                let line = json_number(&entry, "line");
                let col = json_number(&entry, "col");
                diagnostics.push(format!("{file}:{line}:{col}: {message}"));
            }
            None => diagnostics.push(message),
        }
    }
    EmittedTypes {
        declarations,
        diagnostics,
    }
}

/// The `{...}` objects of the array that follows `key`, as raw slices.
fn json_objects(text: &str, key: &str) -> Vec<String> {
    let Some(start) = text.find(key) else {
        return Vec::new();
    };
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = start + key.len();
    let mut depth = 0usize;
    let mut begin = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    while i < bytes.len() {
        let byte = bytes[i];
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
        } else {
            match byte {
                b'"' => in_string = true,
                b'{' => {
                    if depth == 0 {
                        begin = i;
                    }
                    depth += 1;
                }
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        out.push(text[begin..=i].to_string());
                    }
                }
                b']' if depth == 0 => break,
                _ => {}
            }
        }
        i += 1;
    }
    out
}

/// The string value of `field` in a flat JSON object, unescaped.
fn json_field(object: &str, field: &str) -> Option<String> {
    let key = format!("\"{field}\":\"");
    let start = object.find(&key)? + key.len();
    let bytes = object.as_bytes();
    let mut out = String::new();
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => return Some(out),
            b'\\' => {
                i += 1;
                match bytes.get(i) {
                    Some(b'n') => out.push('\n'),
                    Some(b'r') => out.push('\r'),
                    Some(b't') => out.push('\t'),
                    Some(b'u') => {
                        let hex = object.get(i + 1..i + 5)?;
                        let code = u32::from_str_radix(hex, 16).ok()?;
                        out.push(char::from_u32(code)?);
                        i += 4;
                    }
                    Some(other) => out.push(*other as char),
                    None => return None,
                }
            }
            _ => {
                // Copy whole UTF-8 sequences, not bytes.
                let rest = &object[i..];
                let ch = rest.chars().next()?;
                out.push(ch);
                i += ch.len_utf8() - 1;
            }
        }
        i += 1;
    }
    None
}

/// The numeric value of `field`, or 0.
fn json_number(object: &str, field: &str) -> u32 {
    let key = format!("\"{field}\":");
    object
        .find(&key)
        .map(|at| {
            object[at + key.len()..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
        })
        .and_then(|digits| digits.parse().ok())
        .unwrap_or(0)
}

/// A path in `/`-separated form, which is what the host expects.
fn slash_path(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

/// `--types -w`: rerun the whole pipeline whenever a source changes.
/// Declaration emit is a whole-program pass, so there is no per-file
/// increment to exploit yet — a full rerun keeps the sidecars coherent.
fn types_watch(inputs: &[String], opts: &TypesOptions) -> ExitCode {
    let mut stamps: HashMap<PathBuf, SystemTime> = HashMap::new();
    let mut first = true;

    loop {
        let jobs = match build_jobs(inputs, None, true) {
            Ok(jobs) => jobs,
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

        if first || current != stamps {
            match types_once(inputs, opts) {
                Ok(failed) => eprintln!(
                    "rlc: types {} — watching",
                    if failed { "failed" } else { "ok" }
                ),
                Err(code) => return code,
            }
            if first {
                eprintln!("rlc: watching {} file(s) — Ctrl-C to stop", jobs.len());
                first = false;
            }
        }
        stamps = current;
        thread::sleep(WATCH_INTERVAL);
    }
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
