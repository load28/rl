//! The rl → TypeScript transform.
//!
//! Design goal: every valid TypeScript file is a valid .rl file and compiles
//! to itself, byte for byte. Only the two constructs rl adds — Rust-style
//! `enum` declarations and `match` expressions — are rewritten; anything that
//! does not fully parse as one of them is passed through untouched.
//!
//! Plain TypeScript enums keep working: an `enum` declaration is treated as
//! an rl enum only when at least one case carries a payload `(...)` or the
//! declaration has generics — neither is valid TypeScript enum syntax, so no
//! valid TS enum is ever rewritten.
//!
//! Error layering: rl-level errors (duplicate cases, non-exhaustive matches,
//! bad field types) are rlc compile errors with exact positions. The emitted
//! output is plain TypeScript with no type-level tricks; exhaustiveness is
//! checked by rlc itself against the enums declared in the file, not
//! delegated to tsc.
//!
//! The transform always recurses with absolute `(start, end)` ranges over
//! the full source, so every error can be reported with an exact position.

use std::cell::RefCell;
use std::collections::BTreeMap;

use crate::error::RlError;
use crate::scanner::*;
use crate::verify;

/// A deferred exhaustiveness check for one wildcard-free `match`, resolved
/// once the whole file has been scanned (so declaration order doesn't matter).
pub(crate) struct MatchCheck {
    /// Offset of the `match` keyword, for error reporting.
    pub offset: usize,
    /// Non-wildcard arm tags.
    pub tags: Vec<String>,
}

pub(crate) struct Ctx<'a> {
    pub src: &'a str,
    pub bytes: &'a [u8],
    pub verify: bool,
    /// rl enums declared in this file: name → case tags.
    pub enums: RefCell<BTreeMap<String, Vec<String>>>,
    /// Wildcard-free matches to exhaustiveness-check after the pass.
    pub match_checks: RefCell<Vec<MatchCheck>>,
}

impl<'a> Ctx<'a> {
    pub fn new(src: &'a str, verify: bool) -> Self {
        Ctx {
            src,
            bytes: src.as_bytes(),
            verify,
            enums: RefCell::new(BTreeMap::new()),
            match_checks: RefCell::new(Vec::new()),
        }
    }
}

/// Resolves the deferred exhaustiveness checks: a wildcard-free `match` whose
/// arm tags all belong to an rl enum declared in this file must cover every
/// case of that enum. Matches whose tags belong to no known enum (imported
/// enums, hand-written unions) are not checked — rlc has no type information
/// for them.
pub(crate) fn check_exhaustiveness(ctx: &Ctx) -> Result<(), RlError> {
    let enums = ctx.enums.borrow();
    for check in ctx.match_checks.borrow().iter() {
        let mut best: Option<(&str, Vec<&str>)> = None; // candidate with fewest missing cases
        let mut satisfied = false;
        for (name, cases) in enums.iter() {
            if !check.tags.iter().all(|t| cases.contains(t)) {
                continue; // not a candidate: some arm tag is not a case of this enum
            }
            let missing: Vec<&str> = cases
                .iter()
                .filter(|c| !check.tags.contains(c))
                .map(String::as_str)
                .collect();
            if missing.is_empty() {
                satisfied = true;
                break;
            }
            if best.as_ref().is_none_or(|(_, m)| missing.len() < m.len()) {
                best = Some((name, missing));
            }
        }
        if let (false, Some((name, missing))) = (satisfied, best) {
            let list = missing
                .iter()
                .map(|m| format!("\"{m}\""))
                .collect::<Vec<_>>()
                .join(", ");
            return Err(RlError::at(
                check.offset,
                format!(
                    "match on enum {name} is not exhaustive: missing {list} (add the missing arms or a final `_` arm)"
                ),
            ));
        }
    }
    Ok(())
}

// Words that can never be a variant tag, match pattern tag, or binding name.
// Meeting one of these while trying to parse an rl construct aborts the
// attempt, so ordinary TypeScript (e.g. a class method named `match`) is
// left untouched.
const RESERVED: &[&str] = &[
    "async", "await", "break", "case", "catch", "class", "const", "continue",
    "debugger", "default", "delete", "do", "else", "enum", "export",
    "extends", "false", "finally", "for", "function", "if", "import", "in",
    "instanceof", "let", "new", "null", "of", "return", "static", "super",
    "switch", "this", "throw", "true", "try", "typeof", "var", "void",
    "while", "with", "yield",
];

// After one of these words, a `/` starts a regex literal, not division.
const REGEX_PRECEDING_WORDS: &[&str] = &[
    "return", "typeof", "instanceof", "in", "of", "new", "delete", "void",
    "throw", "case", "do", "else", "yield", "await",
];

fn is_reserved(word: &str) -> bool {
    RESERVED.contains(&word)
}

fn regex_allowed(prev_sig: u8, prev_word: &str) -> bool {
    if !prev_word.is_empty() {
        return REGEX_PRECEDING_WORDS.contains(&prev_word);
    }
    if prev_sig == 0 {
        return true;
    }
    b"(,=:[!&|?{};~+-*%^<>".contains(&prev_sig)
}

fn at(src: &[u8], i: usize, end: usize) -> Option<u8> {
    if i < end { Some(src[i]) } else { None }
}

pub(crate) fn transform(ctx: &Ctx, start: usize, end: usize) -> Result<String, RlError> {
    let src = ctx.bytes;
    let mut out: Vec<u8> = Vec::with_capacity(end - start);
    let mut i = start;
    let mut prev_sig: u8 = 0; // last significant byte emitted
    let mut prev_word: &str = ""; // last identifier/keyword emitted

    while i < end {
        let c = src[i];

        // comments — copied verbatim
        if c == b'/' && at(src, i + 1, end) == Some(b'/') {
            let e = line_end(src, i, end);
            out.extend_from_slice(&src[i..e]);
            i = e;
            continue;
        }
        if c == b'/' && at(src, i + 1, end) == Some(b'*') {
            let e = match find_subslice(src, b"*/", i + 2, end) {
                Some(e) => e + 2,
                None => end,
            };
            out.extend_from_slice(&src[i..e]);
            i = e;
            continue;
        }

        // string literals — copied verbatim
        if c == b'"' || c == b'\'' {
            let e = scan_string(src, i, end);
            out.extend_from_slice(&src[i..e.min(end)]);
            i = e;
            prev_sig = c;
            prev_word = "";
            continue;
        }

        // template literals — interpolations are transformed recursively
        if c == b'`' {
            let (e, text) = transform_template(ctx, i, end)?;
            out.extend_from_slice(text.as_bytes());
            i = e;
            prev_sig = b'`';
            prev_word = "";
            continue;
        }

        // regex literals — copied verbatim (heuristic: by preceding token)
        if c == b'/' && regex_allowed(prev_sig, prev_word)
            && let Some(e) = scan_regex(src, i, end) {
                out.extend_from_slice(&src[i..e]);
                i = e;
                prev_sig = b'/';
                prev_word = "";
                continue;
            }

        if is_ident_start(c) {
            let j = ident_end(src, i, end);
            let word = &ctx.src[i..j];
            let dotted = prev_sig == b'.'; // property access like `str.match(...)`

            // `const enum` / `declare enum` are TypeScript-only forms — never rl.
            let ts_enum_prefix = prev_word == "const" || prev_word == "declare";
            if !dotted && !ts_enum_prefix && (word == "enum" || word == "export") {
                let mut exported = false;
                let mut kw_end = j;
                if word == "export" {
                    let k = skip_ws_comments(src, j, end);
                    if k < end && is_ident_start(src[k]) {
                        let m = ident_end(src, k, end);
                        if &ctx.src[k..m] == "enum" {
                            exported = true;
                            kw_end = m;
                        }
                    }
                }
                if (word == "enum" || exported)
                    && let Some((parsed_end, text)) = parse_enum(ctx, kw_end, end, exported)? {
                        out.extend_from_slice(text.as_bytes());
                        i = parsed_end;
                        prev_sig = b';';
                        prev_word = "";
                        continue;
                    }
            }

            if !dotted && word == "match"
                && let Some((parsed_end, text)) = parse_match(ctx, j, end)? {
                    out.extend_from_slice(text.as_bytes());
                    i = parsed_end;
                    prev_sig = b')';
                    prev_word = "";
                    continue;
                }

            out.extend_from_slice(word.as_bytes());
            i = j;
            prev_word = word;
            prev_sig = *word.as_bytes().last().unwrap();
            continue;
        }

        out.push(c);
        if !is_ws(c) {
            prev_sig = c;
            prev_word = "";
        }
        i += 1;
    }

    // Safe: the output is a recombination of valid UTF-8 slices of the input
    // plus ASCII text emitted by the compiler.
    Ok(String::from_utf8(out).expect("transform output is valid UTF-8"))
}

/// `bytes[i]` is a backtick — copies the template, transforming code inside `${ }`.
fn transform_template(ctx: &Ctx, mut i: usize, end: usize) -> Result<(usize, String), RlError> {
    let src = ctx.bytes;
    let mut text: Vec<u8> = vec![b'`'];
    i += 1;
    while i < end {
        let c = src[i];
        if c == b'\\' {
            text.extend_from_slice(&src[i..(i + 2).min(end)]);
            i += 2;
            continue;
        }
        if c == b'`' {
            text.push(b'`');
            i += 1;
            return Ok((i, String::from_utf8(text).expect("valid UTF-8")));
        }
        if c == b'$' && at(src, i + 1, end) == Some(b'{') {
            let close = find_matching(src, i + 1, end).unwrap_or(end);
            text.extend_from_slice(b"${");
            text.extend_from_slice(transform(ctx, i + 2, close)?.as_bytes());
            text.push(b'}');
            i = (close + 1).min(end);
            continue;
        }
        text.push(c);
        i += 1;
    }
    Ok((i, String::from_utf8(text).expect("valid UTF-8")))
}

/* ------------------------------------------------------------------ */
/* enum                                                                */
/* ------------------------------------------------------------------ */

struct Field {
    name: String,
    optional: bool,
    ty: String,
    ty_off: usize,
}

struct EnumCase {
    tag: String,
    tag_off: usize,
    /// None = unit case (no parens); Some(vec) = case with a field list.
    fields: Option<Vec<Field>>,
}

fn parse_enum(
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

    Ok(Some((close_brace + 1, emit_enum(name, generics, &cases, exported))))
}

fn parse_enum_cases(
    ctx: &Ctx,
    start: usize,
    end: usize,
) -> Result<Option<Vec<EnumCase>>, RlError> {
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
        cases.push(EnumCase { tag: tag.to_string(), tag_off, fields });

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
        let ty_off = ty_start + (ctx.src[ty_start..i].len() - ctx.src[ty_start..i].trim_start().len());
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

/* ------------------------------------------------------------------ */
/* match                                                               */
/* ------------------------------------------------------------------ */

struct Binding {
    name: String,
    alias: Option<String>,
}

struct Arm {
    wildcard: bool,
    tag: String,
    pattern_off: usize,
    bindings: Option<Vec<Binding>>,
    /// Absolute (start, end) range of the raw body text.
    body: (usize, usize),
    block: bool,
}

fn parse_match(ctx: &Ctx, j: usize, end: usize) -> Result<Option<(usize, String)>, RlError> {
    let src = ctx.bytes;
    let k = skip_ws_comments(src, j, end);
    if at(src, k, end) != Some(b'(') {
        return Ok(None);
    }
    let close_paren = match find_matching(src, k, end) {
        Some(c) => c,
        None => return Ok(None),
    };
    let scrut = (k + 1, close_paren);
    if ctx.src[scrut.0..scrut.1].trim().is_empty() {
        return Ok(None);
    }

    let b = skip_ws_comments(src, close_paren + 1, end);
    if at(src, b, end) != Some(b'{') {
        return Ok(None);
    }
    let close_brace = match find_matching(src, b, end) {
        Some(c) => c,
        None => return Ok(None),
    };

    let arms = match parse_arms(ctx, b + 1, close_brace)? {
        Some(arms) if !arms.is_empty() => arms,
        _ => return Ok(None),
    };

    // sanity checks — by now this is clearly meant to be an rl match
    let mut seen: Vec<&str> = Vec::new();
    for (idx, arm) in arms.iter().enumerate() {
        if arm.wildcard {
            if idx != arms.len() - 1 {
                return Err(RlError::at(
                    arm.pattern_off,
                    "match: the wildcard arm `_` must be the last arm".to_string(),
                ));
            }
        } else {
            if seen.contains(&arm.tag.as_str()) {
                return Err(RlError::at(
                    arm.pattern_off,
                    format!("match: duplicate arm \"{}\"", arm.tag),
                ));
            }
            seen.push(&arm.tag);
        }
    }

    // Wildcard-free matches are exhaustiveness-checked by rlc once the whole
    // file has been scanned (deferred so declaration order doesn't matter).
    if !arms.iter().any(|a| a.wildcard) {
        ctx.match_checks.borrow_mut().push(MatchCheck {
            offset: j.saturating_sub("match".len()),
            tags: arms.iter().map(|a| a.tag.clone()).collect(),
        });
    }

    Ok(Some((close_brace + 1, emit_match(ctx, scrut, &arms)?)))
}

fn parse_arms(ctx: &Ctx, start: usize, end: usize) -> Result<Option<Vec<Arm>>, RlError> {
    let src = ctx.bytes;
    let mut arms = Vec::new();
    let mut i = start;
    loop {
        i = skip_ws_comments(src, i, end);
        if i >= end {
            break;
        }

        // pattern
        let pattern_off = i;
        let mut wildcard = false;
        let mut tag = "";
        let mut bindings = None;
        if src[i] == b'_' && !matches!(at(src, i + 1, end), Some(b) if is_ident_char(b)) {
            wildcard = true;
            i += 1;
        } else if is_ident_start(src[i]) {
            let j = ident_end(src, i, end);
            tag = &ctx.src[i..j];
            if is_reserved(tag) {
                return Ok(None);
            }
            i = skip_ws_comments(src, j, end);
            if at(src, i, end) == Some(b'(') {
                let close = match find_matching(src, i, end) {
                    Some(c) => c,
                    None => return Ok(None),
                };
                bindings = match parse_bindings(ctx, i + 1, close) {
                    Some(b) => Some(b),
                    None => return Ok(None),
                };
                i = close + 1;
            }
        } else {
            return Ok(None);
        }

        i = skip_ws_comments(src, i, end);
        if !(at(src, i, end) == Some(b'=') && at(src, i + 1, end) == Some(b'>')) {
            return Ok(None);
        }
        i = skip_ws_comments(src, i + 2, end);

        // body: `{ ... }` block or a single expression
        let body;
        let mut block = false;
        if at(src, i, end) == Some(b'{') {
            let close = match find_matching(src, i, end) {
                Some(c) => c,
                None => return Ok(None),
            };
            body = (i + 1, close);
            block = true;
            i = close + 1;
        } else {
            let body_start = i;
            i = scan_expr_end(src, i, end);
            body = (body_start, i);
            if ctx.src[body.0..body.1].trim().is_empty() {
                return Ok(None);
            }
        }

        arms.push(Arm {
            wildcard,
            tag: tag.to_string(),
            pattern_off,
            bindings,
            body,
            block,
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
    Ok(Some(arms))
}

/// Parses `a, b: alias, ...` between the parens of a pattern. None on failure.
fn parse_bindings(ctx: &Ctx, start: usize, end: usize) -> Option<Vec<Binding>> {
    let src = ctx.bytes;
    let mut bindings = Vec::new();
    let mut i = start;
    loop {
        i = skip_ws_comments(src, i, end);
        if i >= end {
            break;
        }
        if !is_ident_start(src[i]) {
            return None;
        }
        let j = ident_end(src, i, end);
        let name = &ctx.src[i..j];
        if is_reserved(name) {
            return None;
        }
        i = skip_ws_comments(src, j, end);

        let mut alias = None;
        if at(src, i, end) == Some(b':') {
            i = skip_ws_comments(src, i + 1, end);
            if i >= end || !is_ident_start(src[i]) {
                return None;
            }
            let m = ident_end(src, i, end);
            let alias_name = &ctx.src[i..m];
            if is_reserved(alias_name) {
                return None;
            }
            alias = Some(alias_name.to_string());
            i = skip_ws_comments(src, m, end);
        }
        bindings.push(Binding { name: name.to_string(), alias });

        if i >= end {
            break;
        }
        if src[i] == b',' {
            i += 1;
            continue;
        }
        return None;
    }
    Some(bindings)
}

/// Scans an arm's expression body until a top-level `,` or closing bracket.
fn scan_expr_end(src: &[u8], mut i: usize, end: usize) -> usize {
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
        match c {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                if depth == 0 {
                    return i;
                }
                depth -= 1;
            }
            b',' if depth == 0 => return i,
            _ => {}
        }
        i += 1;
    }
    i
}

fn emit_match(ctx: &Ctx, scrut: (usize, usize), arms: &[Arm]) -> Result<String, RlError> {
    let scrutinee = transform(ctx, scrut.0, scrut.1)?;
    let is_async = contains_await(ctx.bytes, scrut.0, scrut.1)
        || arms.iter().any(|a| contains_await(ctx.bytes, a.body.0, a.body.1));

    let mut cases = String::new();
    let mut has_wildcard = false;
    for arm in arms {
        let label = if arm.wildcard {
            has_wildcard = true;
            "default".to_string()
        } else {
            format!("case \"{}\"", arm.tag)
        };

        let mut bind = String::new();
        if let Some(bindings) = &arm.bindings
            && !bindings.is_empty() {
                let parts = bindings
                    .iter()
                    .map(|b| match &b.alias {
                        Some(alias) => format!("{}: {}", b.name, alias),
                        None => b.name.clone(),
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                bind = format!("const {{ {} }} = $rl_m; ", parts);
            }

        if arm.block {
            let body = transform(ctx, arm.body.0, arm.body.1)?;
            let body = body.trim();
            // `break` (not `return`) so an arm whose block always returns doesn't
            // widen the match's type with `undefined`; if the block doesn't return,
            // the arm evaluates to undefined, which the inferred type then reflects.
            cases.push_str(&format!("    {}: {{ {}{}\n      break; }}\n", label, bind, body));
        } else {
            let expr = transform(ctx, arm.body.0, arm.body.1)?;
            let expr = expr.trim();
            // a trailing line comment would swallow the closing paren
            let nl = if expr.rsplit('\n').next().unwrap_or("").contains("//") {
                "\n    "
            } else {
                ""
            };
            cases.push_str(&format!("    {}: {{ {}return ({}{}); }}\n", label, bind, expr, nl));
        }
    }

    if !has_wildcard {
        cases.push_str(
            "    default: { throw new Error(\"rl match: unexpected case \" + JSON.stringify($rl_m)); }\n",
        );
    }

    let f = if is_async { "async () => {" } else { "() => {" };
    let body = format!(
        "({}\n  const $rl_m = ({});\n  switch ($rl_m.kind) {{\n{}  }}\n}})()",
        f, scrutinee, cases
    );

    Ok(if is_async {
        format!("(await {})", body)
    } else {
        format!("({})", body)
    })
}
