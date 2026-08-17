//! rlc — compile .rl files to .ts.
//!
//!   rlc file.rl [more.rl ...]      writes file.ts next to each input
//!   rlc src/                       compiles every .rl under src/ recursively
//!   rlc -p file.rl                 prints the output to stdout
//!   rlc -o out/ src/               mirrors the input tree under out/
//!   rlc --check src/               compiles without writing anything
//!   rlc --emit-std src/rl.ts       writes the standard library module

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use rlc::{ExternEnum, ImportRewrite, Options, RlImportNames, compile};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn usage() {
    println!(
        "rlc v{VERSION} — rl to TypeScript compiler

Usage: rlc [options] <file.rl | dir> ...

Options:
  -o, --out-dir <dir>   write outputs under <dir> (mirrors input paths)
  -p, --print           print compiled output to stdout instead of writing
  --check               compile only; write nothing (syntax check)
  --emit-std <file>     write the standard library module (Option/Result) to <file>
  --no-banner           omit the \"generated\" banner comment
  --no-verify           skip swc validation of types and generated output
  --rewrite-imports <js|bare|off>
                        how relative .rl import specifiers are emitted:
                        js = ./x.js (default), bare = ./x, off = untouched
  -h, --help            show this help
  -v, --version         show version"
    );
}

fn collect_rl_files(entry: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
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
                collect_rl_files(&child, out)?;
            } else if meta.is_file() && child.extension().is_some_and(|e| e == "rl") {
                out.push(child);
            }
        }
    }
    Ok(())
}

struct Job {
    file: PathBuf,
    out_path: PathBuf,
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
    let mut emit_std: Option<PathBuf> = None;
    let mut print = false;
    let mut check = false;
    let mut banner = true;
    let mut verify = true;
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
            "--check" => check = true,
            "--no-banner" => banner = false,
            "--no-verify" => verify = false,
            "-o" | "--out-dir" => match it.next() {
                Some(dir) => out_dir = Some(PathBuf::from(dir)),
                None => {
                    eprintln!("rlc: --out-dir requires a value");
                    return ExitCode::FAILURE;
                }
            },
            "--rewrite-imports" => match it.next().map(String::as_str) {
                Some("js") => rewrite_imports = ImportRewrite::Js,
                Some("bare") => rewrite_imports = ImportRewrite::Bare,
                Some("off") => rewrite_imports = ImportRewrite::Off,
                Some(other) => {
                    eprintln!("rlc: --rewrite-imports expects js, bare, or off (got {other})");
                    return ExitCode::FAILURE;
                }
                None => {
                    eprintln!("rlc: --rewrite-imports requires a value (js, bare, or off)");
                    return ExitCode::FAILURE;
                }
            },
            "--emit-std" => match it.next() {
                Some(path) => emit_std = Some(PathBuf::from(path)),
                None => {
                    eprintln!("rlc: --emit-std requires a value");
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

    if inputs.is_empty() && emit_std.is_none() {
        usage();
        return ExitCode::FAILURE;
    }

    if let Some(path) = &emit_std {
        let mut code = rlc::STD_SOURCE.to_string();
        if banner {
            code = format!("// @generated by rlc --emit-std — do not edit directly.\n{code}");
        }
        if let Some(parent) = path.parent()
            && let Err(e) = fs::create_dir_all(parent)
        {
            eprintln!("rlc: {}: {e}", parent.display());
            return ExitCode::FAILURE;
        }
        if let Err(e) = fs::write(path, &code) {
            eprintln!("rlc: {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
        eprintln!("rlc: std → {}", path.display());
        if inputs.is_empty() {
            return ExitCode::SUCCESS;
        }
    }

    let mut jobs: Vec<Job> = Vec::new();
    for input in &inputs {
        let input_path = Path::new(input);
        if !input_path.exists() {
            eprintln!("rlc: no such file or directory: {input}");
            return ExitCode::FAILURE;
        }
        let is_dir = input_path.is_dir();
        let mut files = Vec::new();
        if let Err(e) = collect_rl_files(input_path, &mut files) {
            eprintln!("rlc: {input}: {e}");
            return ExitCode::FAILURE;
        }
        for file in files {
            let out_path = match &out_dir {
                Some(dir) => {
                    let rel = if is_dir {
                        file.strip_prefix(input_path).unwrap_or(&file).to_path_buf()
                    } else {
                        PathBuf::from(file.file_name().unwrap())
                    };
                    dir.join(rel).with_extension("ts")
                }
                None => file.with_extension("ts"),
            };
            jobs.push(Job { file, out_path });
        }
    }

    if jobs.is_empty() {
        eprintln!("rlc: no .rl files found");
        return ExitCode::FAILURE;
    }

    let mut failed = false;
    for job in &jobs {
        let filename = job.file.display().to_string();
        let source = match fs::read_to_string(&job.file) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("rlc: {filename}: {e}");
                failed = true;
                continue;
            }
        };
        let extern_enums = collect_extern_enums(&job.file, &source);
        let options = Options {
            filename: Some(&filename),
            verify,
            rewrite_imports,
            extern_enums: &extern_enums,
        };
        let mut code = match compile(&source, &options) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("rlc: {e}");
                failed = true;
                continue;
            }
        };
        if banner {
            let base = job.file.file_name().unwrap().to_string_lossy();
            code = format!("// @generated from {base} by rlc — do not edit directly.\n{code}");
        }
        if print {
            print!("{code}");
        } else if !check {
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
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
