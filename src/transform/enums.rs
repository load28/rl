//! rl `enum` parsing and emission.
//!
//! An `enum` declaration is treated as an rl enum only when at least one case
//! carries a payload `(...)` or the declaration has generics; every other
//! `enum` (including `const enum` / `declare enum`, filtered before reaching
//! this module) is plain TypeScript and passes through untouched.

use super::{Ctx, at, is_reserved};
use crate::error::RlError;
use crate::scanner::*;
use crate::verify;

pub(super) struct Field {
    name: String,
    optional: bool,
    ty: String,
    ty_off: usize,
}

pub(super) struct EnumCase {
    tag: String,
    tag_off: usize,
    /// None = unit case (no parens); Some(vec) = case with a field list.
    fields: Option<Vec<Field>>,
}

pub(super) fn parse_enum(
    ctx: &Ctx,
    j: usize,
    end: usize,
    exported: bool,
) -> Result<Option<(usize, String)>, RlError> {
    let src = ctx.bytes;
    let mut i = skip_ws_comments(src, j, end);
    if i >= end || !is_ident_start(src[i]) {
        return Ok(None);
    }
    let k = ident_end(src, i, end);
    let name = &ctx.src[i..k];
    if is_reserved(name) {
        return Ok(None);
    }

    i = skip_ws_comments(src, k, end);
    let mut generics = "";
    if at(src, i, end) == Some(b'<') {
        let close = match find_matching(src, i, end) {
            Some(c) => c,
            None => return Ok(None),
        };
        generics = &ctx.src[i..close + 1];
        i = skip_ws_comments(src, close + 1, end);
    }
    if at(src, i, end) != Some(b'{') {
        return Ok(None);
    }
    let close_brace = match find_matching(src, i, end) {
        Some(c) => c,
        None => return Ok(None),
    };

    let cases = match parse_enum_cases(ctx, i + 1, close_brace)? {
        Some(cases) if !cases.is_empty() => cases,
        _ => return Ok(None),
    };

    // A declaration with no payload case and no generics is a plain
    // TypeScript enum — pass it through untouched. (TS enum members can
    // never look like `Tag(...)`, and TS enums can never have generics, so
    // this rule never captures valid TypeScript.)
    let is_rl_enum = !generics.is_empty() || cases.iter().any(|c| c.fields.is_some());
    if !is_rl_enum {
        return Ok(None);
    }

    let mut seen: Vec<&str> = Vec::new();
    for case in &cases {
        if seen.contains(&case.tag.as_str()) {
            return Err(RlError::at(
                case.tag_off,
                format!("enum {}: duplicate case \"{}\"", name, case.tag),
            ));
        }
        seen.push(&case.tag);
    }

    if ctx.verify {
        for case in &cases {
            if let Some(fields) = &case.fields {
                for field in fields {
                    if let Err(msg) = verify::check_type_fragment(&field.ty) {
                        return Err(RlError::at(
                            field.ty_off,
                            format!(
                                "enum {}: invalid type for field `{}`: {}",
                                name, field.name, msg
                            ),
                        ));
                    }
                }
            }
        }
    }

    ctx.enums.borrow_mut().insert(
        name.to_string(),
        cases.iter().map(|c| c.tag.clone()).collect(),
    );

    Ok(Some((
        close_brace + 1,
        emit_enum(name, generics, &cases, exported),
    )))
}

fn parse_enum_cases(ctx: &Ctx, start: usize, end: usize) -> Result<Option<Vec<EnumCase>>, RlError> {
    let src = ctx.bytes;
    let mut cases = Vec::new();
    let mut i = start;
    loop {
        i = skip_ws_comments(src, i, end);
        if i >= end {
            break;
        }
        if !is_ident_start(src[i]) {
            return Ok(None);
        }
        let tag_off = i;
        let j = ident_end(src, i, end);
        let tag = &ctx.src[i..j];
        if is_reserved(tag) {
            return Ok(None);
        }
        i = skip_ws_comments(src, j, end);

        let mut fields = None;
        if at(src, i, end) == Some(b'(') {
            let close = match find_matching(src, i, end) {
                Some(c) => c,
                None => return Ok(None),
            };
            fields = match parse_fields(ctx, i + 1, close)? {
                Some(f) => Some(f),
                None => return Ok(None),
            };
            i = close + 1;
        }
        cases.push(EnumCase {
            tag: tag.to_string(),
            tag_off,
            fields,
        });

        i = skip_ws_comments(src, i, end);
        if i >= end {
            break;
        }
        if src[i] == b',' {
            i += 1;
            continue;
        }
        return Ok(None);
    }
    Ok(Some(cases))
}

/// Parses `name: Type, name?: Type, ...`. Returns None on failure.
fn parse_fields(ctx: &Ctx, start: usize, end: usize) -> Result<Option<Vec<Field>>, RlError> {
    let src = ctx.bytes;
    let mut fields = Vec::new();
    let mut i = start;
    loop {
        i = skip_ws_comments(src, i, end);
        if i >= end {
            break;
        }
        if !is_ident_start(src[i]) {
            return Ok(None);
        }
        let j = ident_end(src, i, end);
        let name = &ctx.src[i..j];
        if is_reserved(name) {
            return Ok(None);
        }
        i = skip_ws_comments(src, j, end);

        let mut optional = false;
        if at(src, i, end) == Some(b'?') {
            optional = true;
            i = skip_ws_comments(src, i + 1, end);
        }
        if at(src, i, end) != Some(b':') {
            return Ok(None);
        }
        i += 1;
        let ty_start = i;
        i = scan_type_end(src, i, end);
        let ty = ctx.src[ty_start..i].trim();
        if ty.is_empty() {
            return Ok(None);
        }
        let ty_off =
            ty_start + (ctx.src[ty_start..i].len() - ctx.src[ty_start..i].trim_start().len());
        fields.push(Field {
            name: name.to_string(),
            optional,
            ty: ty.to_string(),
            ty_off,
        });

        if i >= end {
            break;
        }
        if src[i] == b',' {
            i += 1;
            continue;
        }
        return Ok(None);
    }
    Ok(Some(fields))
}

/// Scans a type annotation until a top-level `,` or closing bracket.
fn scan_type_end(src: &[u8], mut i: usize, end: usize) -> usize {
    let mut depth = 0usize;
    while i < end {
        let c = src[i];
        if c == b'/' && at(src, i + 1, end) == Some(b'/') {
            i = line_end(src, i, end);
            continue;
        }
        if c == b'/' && at(src, i + 1, end) == Some(b'*') {
            i = match find_subslice(src, b"*/", i + 2, end) {
                Some(e) => e + 2,
                None => end,
            };
            continue;
        }
        if c == b'"' || c == b'\'' {
            i = scan_string(src, i, end);
            continue;
        }
        if c == b'`' {
            i = skip_template(src, i, end);
            continue;
        }
        if c == b'=' && at(src, i + 1, end) == Some(b'>') {
            i += 2;
            continue;
        }
        match c {
            b'(' | b'[' | b'{' | b'<' => depth += 1,
            b')' | b']' | b'}' => {
                if depth == 0 {
                    return i;
                }
                depth -= 1;
            }
            b'>' => {
                depth = depth.saturating_sub(1);
            }
            b',' if depth == 0 => return i,
            _ => {}
        }
        i += 1;
    }
    i
}

fn emit_enum(name: &str, generics: &str, cases: &[EnumCase], exported: bool) -> String {
    let exp = if exported { "export " } else { "" };

    let type_arms: Vec<String> = cases
        .iter()
        .map(|c| match &c.fields {
            Some(fields) if !fields.is_empty() => {
                let list = fields
                    .iter()
                    .map(|f| format!("{}{}: {}", f.name, if f.optional { "?" } else { "" }, f.ty))
                    .collect::<Vec<_>>()
                    .join("; ");
                format!("{{ kind: \"{}\"; {} }}", c.tag, list)
            }
            _ => format!("{{ kind: \"{}\" }}", c.tag),
        })
        .collect();
    let type_decl = format!(
        "{}type {}{} =\n  | {};",
        exp,
        name,
        generics,
        type_arms.join("\n  | ")
    );

    let type_args = if generics.is_empty() {
        String::new()
    } else {
        format!("<{}>", generic_param_names(generics).join(", "))
    };
    let ctors: Vec<String> = cases
        .iter()
        .map(|c| match &c.fields {
            None => {
                // unit case: a singleton value, kept narrow via `as const`
                format!("  {}: {{ kind: \"{}\" }} as const,", c.tag, c.tag)
            }
            Some(fields) => {
                let params = fields
                    .iter()
                    .map(|f| format!("{}{}: {}", f.name, if f.optional { "?" } else { "" }, f.ty))
                    .collect::<Vec<_>>()
                    .join(", ");
                let mut obj = vec![format!("kind: \"{}\"", c.tag)];
                obj.extend(fields.iter().map(|f| f.name.clone()));
                format!(
                    "  {}: {}({}): {}{} => ({{ {} }}),",
                    c.tag,
                    generics,
                    params,
                    name,
                    type_args,
                    obj.join(", ")
                )
            }
        })
        .collect();
    let const_decl = format!("{}const {} = {{\n{}\n}};", exp, name, ctors.join("\n"));

    format!("{}\n{}", type_decl, const_decl)
}

/// `<T extends string, U = number>` → ["T", "U"]
fn generic_param_names(generics: &str) -> Vec<String> {
    let inner = &generics[1..generics.len() - 1];
    let src = inner.as_bytes();
    let end = src.len();
    let mut names = Vec::new();
    let mut i = 0usize;
    loop {
        i = skip_ws_comments(src, i, end);
        if i >= end || !is_ident_start(src[i]) {
            break;
        }
        let j = ident_end(src, i, end);
        let word = &inner[i..j];
        if word == "const" || word == "in" || word == "out" {
            // modifier — the actual name follows
            i = j;
            continue;
        }
        names.push(word.to_string());
        i = scan_type_end(src, j, end); // skip constraint/default up to the next comma
        if at(src, i, end) == Some(b',') {
            i += 1;
        }
    }
    names
}
