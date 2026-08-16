//! `match` expression parsing and emission.
//!
//! A `match` compiles to a `switch` IIFE discriminating on the `kind` field.
//! Anything that does not fully parse as an rl match (a method named `match`,
//! `String.prototype.match` calls, ...) is passed through untouched by
//! returning `Ok(None)`.

use super::{Ctx, MatchCheck, at, is_reserved, transform};
use crate::error::RlError;
use crate::scanner::*;

pub(super) struct Binding {
    name: String,
    alias: Option<String>,
}

pub(super) struct Arm {
    wildcard: bool,
    tag: String,
    pattern_off: usize,
    bindings: Option<Vec<Binding>>,
    /// Absolute (start, end) range of the raw body text.
    body: (usize, usize),
    block: bool,
}

pub(super) fn parse_match(
    ctx: &Ctx,
    j: usize,
    end: usize,
) -> Result<Option<(usize, String)>, RlError> {
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
        bindings.push(Binding {
            name: name.to_string(),
            alias,
        });

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
        || arms
            .iter()
            .any(|a| contains_await(ctx.bytes, a.body.0, a.body.1));

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
            && !bindings.is_empty()
        {
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
            cases.push_str(&format!(
                "    {}: {{ {}{}\n      break; }}\n",
                label, bind, body
            ));
        } else {
            let expr = transform(ctx, arm.body.0, arm.body.1)?;
            let expr = expr.trim();
            // a trailing line comment would swallow the closing paren
            let nl = if expr.rsplit('\n').next().unwrap_or("").contains("//") {
                "\n    "
            } else {
                ""
            };
            cases.push_str(&format!(
                "    {}: {{ {}return ({}{}); }}\n",
                label, bind, expr, nl
            ));
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
