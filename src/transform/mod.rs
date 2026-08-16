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
//!
//! Module layout: this file owns the main scan/transform loop and the
//! deferred exhaustiveness check; [`enums`] owns rl `enum` parsing and
//! emission; [`matches`] owns `match` parsing and emission.

mod enums;
mod matches;

use std::cell::RefCell;
use std::collections::BTreeMap;

use crate::error::RlError;
use crate::scanner::*;

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
                    && let Some((parsed_end, text)) = enums::parse_enum(ctx, kw_end, end, exported)? {
                        out.extend_from_slice(text.as_bytes());
                        i = parsed_end;
                        prev_sig = b';';
                        prev_word = "";
                        continue;
                    }
            }

            if !dotted && word == "match"
                && let Some((parsed_end, text)) = matches::parse_match(ctx, j, end)? {
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
