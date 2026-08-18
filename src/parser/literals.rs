//! Literal match patterns — `"north"`, `200`, `-1`, `true`.
//!
//! Numbers are not a token kind: the lexer only fuses identifiers, strings,
//! templates, regexes and a handful of operators, so `1_000` arrives as a
//! `Punct('1')` followed by an `Ident("_000")`. Rather than reshape the
//! lexer for one construct (and risk the regex heuristic and every existing
//! scan), a numeric literal is scanned straight from the source bytes here
//! and the cursor is advanced past the tokens it covers — the scan must end
//! exactly on a token boundary, otherwise the candidate is not a literal
//! pattern and the whole match passes through.
//!
//! The pattern keeps the literal's *span*, not a re-rendered value: codegen
//! copies those bytes into the `case` label, so `0xff` stays `0xff`. The
//! decoded [`LiteralValue`] rides along only for duplicate detection and the
//! typed exhaustiveness probe.

use super::cursor::Cursor;
use crate::ast::{LiteralPattern, LiteralValue, Span};
use crate::lexer::TokenKind;
use crate::scanner::{at, is_ident_char};

/// True when the token under the cursor can start a literal pattern.
pub(super) fn at_literal(cur: &Cursor) -> bool {
    match cur.peek() {
        Some(t) => match t.kind {
            TokenKind::Str => true,
            TokenKind::Ident => matches!(cur.text(t), "true" | "false"),
            TokenKind::Punct(b) => {
                b.is_ascii_digit()
                    || (b == b'.' && next_byte_is_digit(cur, 1))
                    || (b == b'-' && starts_number(cur, 1))
            }
            _ => false,
        },
        None => false,
    }
}

/// Whether the token `ahead` positions past the cursor starts a number.
fn starts_number(cur: &Cursor, ahead: usize) -> bool {
    match cur.tokens.get(cur.idx + ahead).map(|t| &t.kind) {
        Some(TokenKind::Punct(b)) => {
            b.is_ascii_digit() || (*b == b'.' && next_byte_is_digit(cur, ahead + 1))
        }
        _ => false,
    }
}

fn next_byte_is_digit(cur: &Cursor, ahead: usize) -> bool {
    cur.tokens
        .get(cur.idx + ahead)
        .is_some_and(|t| matches!(t.kind, TokenKind::Punct(b) if b.is_ascii_digit()))
}

/// Parses `literal (| literal)*` alternatives starting at the token under
/// the cursor. `||` lexes as a single OrOr token, so it can never be an
/// alternative separator — the candidate then fails the parse.
pub(super) fn parse_literal_alternatives(cur: &mut Cursor) -> Option<Vec<LiteralPattern>> {
    let mut alts = vec![parse_literal(cur)?];
    while cur.at_punct(b'|') {
        cur.bump();
        alts.push(parse_literal(cur)?);
    }
    Some(alts)
}

/// Parses one literal alternative.
fn parse_literal(cur: &mut Cursor) -> Option<LiteralPattern> {
    let first = cur.peek()?;
    match first.kind {
        TokenKind::Str => {
            let span = first.span;
            cur.bump();
            let text = &cur.parser.src[span.start..span.end];
            Some(LiteralPattern {
                span,
                value: LiteralValue::Str(decode_string(text)?),
            })
        }
        TokenKind::Ident => {
            let value = match cur.text(first) {
                "true" => true,
                "false" => false,
                _ => return None,
            };
            let span = first.span;
            cur.bump();
            Some(LiteralPattern {
                span,
                value: LiteralValue::Bool(value),
            })
        }
        TokenKind::Punct(b'-') => {
            let start = first.span.start;
            cur.bump();
            let (span, value) = parse_number(cur)?;
            Some(LiteralPattern {
                span: Span {
                    start,
                    end: span.end,
                },
                value: negate(value)?,
            })
        }
        TokenKind::Punct(_) => {
            let (span, value) = parse_number(cur)?;
            Some(LiteralPattern { span, value })
        }
        _ => None,
    }
}

fn negate(value: LiteralValue) -> Option<LiteralValue> {
    match value {
        LiteralValue::Num(n) => Some(LiteralValue::Num(normalize_zero(-n))),
        LiteralValue::BigInt(digits) => {
            Some(LiteralValue::BigInt(match digits.strip_prefix('-') {
                Some(rest) => rest.to_string(),
                None if digits == "0" => digits,
                None => format!("-{digits}"),
            }))
        }
        _ => None,
    }
}

/// `-0 === 0` in JavaScript, so the two spellings are one `switch` case.
fn normalize_zero(n: f64) -> f64 {
    if n == 0.0 { 0.0 } else { n }
}

/// Scans a numeric literal from the bytes at the cursor's token, advancing
/// the cursor past every token it covers. None unless the scan ends exactly
/// on a token boundary (so `1e5x` is not a literal pattern).
fn parse_number(cur: &mut Cursor) -> Option<(Span, LiteralValue)> {
    let start = cur.peek()?.span.start;
    let src = cur.parser.src.as_bytes();
    let limit = cur.parser.src.len();
    let end = scan_number(src, start, limit)?;

    // Advance past every token inside the scanned range; the last of them
    // must end exactly where the number does.
    let mut last_end = start;
    while let Some(t) = cur.peek() {
        if t.span.start >= end {
            break;
        }
        last_end = t.span.end;
        cur.bump();
    }
    if last_end != end {
        return None;
    }

    let text = &cur.parser.src[start..end];
    let value = numeric_value(text)?;
    Some((Span { start, end }, value))
}

/// The end of the JavaScript numeric literal starting at `i`, or None if
/// there is none. Covers the decimal, hex, octal, binary and exponent
/// forms, `_` separators, and the `n` BigInt suffix.
fn scan_number(src: &[u8], i: usize, end: usize) -> Option<usize> {
    let mut j = i;
    let radix_digits = |b: u8, radix: u32| (b as char).is_digit(radix) || b == b'_';

    if at(src, j, end) == Some(b'0')
        && let Some(marker) = at(src, j + 1, end)
        && let Some(radix) = match marker {
            b'x' | b'X' => Some(16),
            b'o' | b'O' => Some(8),
            b'b' | b'B' => Some(2),
            _ => None,
        }
    {
        j += 2;
        let digits_from = j;
        while j < end && radix_digits(src[j], radix) {
            j += 1;
        }
        if j == digits_from {
            return None;
        }
        if at(src, j, end) == Some(b'n') {
            j += 1;
        }
        return boundary(src, j, end);
    }

    let int_from = j;
    while j < end && radix_digits(src[j], 10) {
        j += 1;
    }
    let had_int = j > int_from;
    if had_int && at(src, j, end) == Some(b'n') {
        j += 1;
        return boundary(src, j, end);
    }
    let mut had_frac = false;
    if at(src, j, end) == Some(b'.') {
        let frac_from = j + 1;
        j += 1;
        while j < end && radix_digits(src[j], 10) {
            j += 1;
        }
        had_frac = j > frac_from;
    }
    if !had_int && !had_frac {
        return None;
    }
    if matches!(at(src, j, end), Some(b'e' | b'E')) {
        let mut k = j + 1;
        if matches!(at(src, k, end), Some(b'+' | b'-')) {
            k += 1;
        }
        let exp_from = k;
        while k < end && src[k].is_ascii_digit() {
            k += 1;
        }
        if k > exp_from {
            j = k;
        }
    }
    boundary(src, j, end)
}

/// A numeric literal must not run into an identifier character — `1e5x`
/// and `0xffz` are not numbers, and treating them as one would leave the
/// trailing text unparsed.
fn boundary(src: &[u8], j: usize, end: usize) -> Option<usize> {
    match at(src, j, end) {
        Some(b) if is_ident_char(b) => None,
        _ => Some(j),
    }
}

/// The value of a scanned numeric literal.
fn numeric_value(text: &str) -> Option<LiteralValue> {
    let digits: String = text.chars().filter(|c| *c != '_').collect();
    if let Some(rest) = digits.strip_suffix('n') {
        let value = if let Some(hex) = strip_radix(rest, 'x') {
            u128::from_str_radix(hex, 16).ok()?.to_string()
        } else if let Some(oct) = strip_radix(rest, 'o') {
            u128::from_str_radix(oct, 8).ok()?.to_string()
        } else if let Some(bin) = strip_radix(rest, 'b') {
            u128::from_str_radix(bin, 2).ok()?.to_string()
        } else {
            rest.trim_start_matches('0').to_string()
        };
        let value = if value.is_empty() {
            "0".to_string()
        } else {
            value
        };
        return Some(LiteralValue::BigInt(value));
    }
    let value = if let Some(hex) = strip_radix(&digits, 'x') {
        u128::from_str_radix(hex, 16).ok()? as f64
    } else if let Some(oct) = strip_radix(&digits, 'o') {
        u128::from_str_radix(oct, 8).ok()? as f64
    } else if let Some(bin) = strip_radix(&digits, 'b') {
        u128::from_str_radix(bin, 2).ok()? as f64
    } else {
        digits.parse::<f64>().ok()?
    };
    if !value.is_finite() {
        return None;
    }
    Some(LiteralValue::Num(normalize_zero(value)))
}

/// `0x1f` → `Some("1f")` for marker `x` (case-insensitive).
fn strip_radix(text: &str, marker: char) -> Option<&str> {
    let rest = text.strip_prefix('0')?;
    let mut chars = rest.chars();
    let first = chars.next()?.to_ascii_lowercase();
    (first == marker).then_some(&rest[1..])
}

/// Decodes a string literal's escapes to the value JavaScript would see.
/// None for an unterminated literal (the byte scanner tolerates those, but
/// a pattern is not the place for one).
fn decode_string(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let quote = *bytes.first()?;
    if bytes.len() < 2 || *bytes.last()? != quote {
        return None;
    }
    let mut out = String::with_capacity(text.len());
    let inner = &text[1..text.len() - 1];
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next()? {
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            'b' => out.push('\u{8}'),
            'f' => out.push('\u{c}'),
            'v' => out.push('\u{b}'),
            '0' => out.push('\0'),
            '\n' => {} // line continuation
            'x' => out.push(char::from_u32(hex_escape(&mut chars, 2)?)?),
            'u' => out.push(unicode_char(&mut chars)?),
            other => out.push(other),
        }
    }
    Some(out)
}

fn hex_escape(chars: &mut std::str::Chars, width: usize) -> Option<u32> {
    let mut value = 0u32;
    for _ in 0..width {
        value = value * 16 + chars.next()?.to_digit(16)?;
    }
    Some(value)
}

/// A `\u` escape's character. `\u{...}` reads the braced form; the
/// four-digit form pairs a high surrogate with a following `\uXXXX` low
/// surrogate, exactly as JavaScript joins them into one code point.
fn unicode_char(chars: &mut std::str::Chars) -> Option<char> {
    let mut peek = chars.clone();
    if peek.next()? == '{' {
        let mut value = 0u32;
        let mut digits = 0;
        loop {
            let c = peek.next()?;
            if c == '}' {
                break;
            }
            value = value.checked_mul(16)?.checked_add(c.to_digit(16)?)?;
            digits += 1;
        }
        if digits == 0 {
            return None;
        }
        *chars = peek;
        return char::from_u32(value);
    }

    let high = hex_escape(chars, 4)?;
    if (0xd800..0xdc00).contains(&high) {
        let mut peek = chars.clone();
        if peek.next() == Some('\\')
            && peek.next() == Some('u')
            && let Some(low) = hex_escape(&mut peek, 4)
            && (0xdc00..0xe000).contains(&low)
        {
            *chars = peek;
            return char::from_u32(0x10000 + ((high - 0xd800) << 10) + (low - 0xdc00));
        }
    }
    char::from_u32(high)
}
