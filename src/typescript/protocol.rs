use std::collections::HashMap;
use std::path::Path;

use rlc::Literal;

use super::mapper::TypeDiagnostic;
use super::{LiteralCheck, ValCheck, VirtualModule};

/// The compiler options declaration emit runs with. Passed as data, not as a
/// synthesized `tsconfig.json`: `.rl` specifiers and `@rl/std` are resolved by
/// the host itself.
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

/// What the host sent back.
pub(crate) struct EmittedTypes {
    /// Virtual module path -> declaration text.
    pub(crate) declarations: HashMap<String, String>,
    /// Type errors the host reported, in its own coordinates.
    pub(crate) diagnostics: Vec<TypeDiagnostic>,
    /// Literal matches the checker found non-exhaustive.
    pub(crate) literal_missing: Vec<LiteralMissing>,
    /// `val` path method calls the checker resolved to a built-in mutator.
    pub(crate) val_mutations: Vec<ValMutation>,
}

/// One non-exhaustive literal match, as the host reports it: the index of the
/// probe it answers and the missing literals, already rendered as a
/// comma-separated list.
pub(crate) struct LiteralMissing {
    pub(crate) index: usize,
    pub(crate) missing: String,
}

/// One proven built-in mutation, as the host reports it: the index of the
/// probe it answers and the built-in that declares the method.
pub(crate) struct ValMutation {
    pub(crate) index: usize,
    pub(crate) receiver: String,
}

/// Serializes the host's job. Hand-written `.ts` files are deliberately
/// absent; the host reads them from disk.
pub(crate) fn types_job(
    cwd: &Path,
    modules: &[VirtualModule],
    std_module: Option<&VirtualModule>,
    sources: &[String],
    rl_map: &[(String, String)],
    checks: &[LiteralCheck],
    val_probes: &[ValCheck],
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

    let literal_checks = checks
        .iter()
        .map(|c| {
            format!(
                "{{\"module\":{},\"start\":{},\"end\":{},\"covered\":[{}]}}",
                json_str(&c.module),
                c.start,
                c.end,
                c.covered
                    .iter()
                    .map(json_literal)
                    .collect::<Vec<_>>()
                    .join(",")
            )
        })
        .collect::<Vec<_>>()
        .join(",");

    let val_checks = val_probes
        .iter()
        .map(|c| {
            format!(
                "{{\"module\":{},\"start\":{},\"end\":{},\"method\":{}}}",
                json_str(&c.module),
                c.start,
                c.end,
                json_str(&c.method)
            )
        })
        .collect::<Vec<_>>()
        .join(",");

    format!(
        "{{\"cwd\":{},\"compilerOptions\":{},\"modules\":[{}],\"std\":{},\"sources\":[{}],\"rlModules\":{{{}}},\"literalChecks\":[{}],\"valChecks\":[{}]}}",
        json_str(&cwd.display().to_string()),
        TYPES_COMPILER_OPTIONS,
        list(&refs),
        std_json,
        source_list,
        map,
        literal_checks,
        val_checks
    )
}

/// Reads the host's reply. The shapes are fixed and produced by our own
/// script, so a minimal scan beats pulling in a JSON parser.
pub(crate) fn parse_types_result(stdout: &str) -> EmittedTypes {
    let mut declarations = HashMap::new();
    let mut diagnostics = Vec::new();

    for entry in json_objects(stdout, "\"declarations\":[") {
        if let (Some(path), Some(text)) = (json_field(&entry, "path"), json_field(&entry, "text")) {
            declarations.insert(path, text);
        }
    }
    for entry in json_objects(stdout, "\"diagnostics\":[") {
        diagnostics.push(TypeDiagnostic {
            file: json_field(&entry, "file"),
            line: json_number(&entry, "line") as usize,
            col: json_number(&entry, "col") as usize,
            message: json_field(&entry, "message").unwrap_or_default(),
        });
    }
    let mut literal_missing = Vec::new();
    for entry in json_objects(stdout, "\"literalMissing\":[") {
        if let Some(missing) = json_field(&entry, "missing") {
            literal_missing.push(LiteralMissing {
                index: json_number(&entry, "index") as usize,
                missing,
            });
        }
    }

    let mut val_mutations = Vec::new();
    for entry in json_objects(stdout, "\"valMutations\":[") {
        if let Some(receiver) = json_field(&entry, "receiver") {
            val_mutations.push(ValMutation {
                index: json_number(&entry, "index") as usize,
                receiver,
            });
        }
    }

    EmittedTypes {
        declarations,
        diagnostics,
        literal_missing,
        val_mutations,
    }
}

/// One covered literal as a JSON value the host compares against the checker's
/// literal types.
fn json_literal(literal: &Literal) -> String {
    match literal {
        Literal::String(s) => json_str(s),
        Literal::Number(n) => format!("{n}"),
        Literal::Boolean(b) => b.to_string(),
        // Filtered out before a check is built; never reaches the host.
        Literal::BigInt(d) => json_str(d),
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
