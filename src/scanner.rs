//! Low-level byte scanning over rl/TypeScript source.
//!
//! All scanning is byte-based: every character the scanner makes decisions on
//! is ASCII, and UTF-8 continuation bytes (0x80+) never compare equal to any
//! ASCII byte, so multi-byte characters pass through opaquely.

pub(crate) fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b == b'$'
}

pub(crate) fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}

pub(crate) fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

/// The byte at `i`, or None at or past `end`.
pub(crate) fn at(src: &[u8], i: usize, end: usize) -> Option<u8> {
    if i < end { Some(src[i]) } else { None }
}

/// Reads the identifier starting at `i`; returns the end index (exclusive).
pub(crate) fn ident_end(src: &[u8], i: usize, end: usize) -> usize {
    let mut j = i + 1;
    while j < end && is_ident_char(src[j]) {
        j += 1;
    }
    j
}

/// Skips whitespace and comments; returns the index of the next significant byte.
pub(crate) fn skip_ws_comments(src: &[u8], mut i: usize, end: usize) -> usize {
    loop {
        while i < end && is_ws(src[i]) {
            i += 1;
        }
        if at(src, i, end) == Some(b'/') && at(src, i + 1, end) == Some(b'/') {
            while i < end && src[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if at(src, i, end) == Some(b'/') && at(src, i + 1, end) == Some(b'*') {
            i = match find_subslice(src, b"*/", i + 2, end) {
                Some(e) => e + 2,
                None => end,
            };
            continue;
        }
        return i;
    }
}

pub(crate) fn find_subslice(src: &[u8], needle: &[u8], from: usize, end: usize) -> Option<usize> {
    if from >= end {
        return None;
    }
    src[from..end]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| from + p)
}

pub(crate) fn line_end(src: &[u8], from: usize, end: usize) -> usize {
    match src[from..end].iter().position(|&b| b == b'\n') {
        Some(p) => from + p,
        None => end,
    }
}

/// `src[i]` is `'` or `"` — returns the index just past the closing quote.
pub(crate) fn scan_string(src: &[u8], mut i: usize, end: usize) -> usize {
    let quote = src[i];
    i += 1;
    while i < end {
        match src[i] {
            b'\\' => i += 2,
            b'\n' => return i, // unterminated string: stop at the newline
            b if b == quote => return i + 1,
            _ => i += 1,
        }
    }
    i.min(end)
}

/// `src[i]` is a backtick — returns the index just past the closing backtick.
pub(crate) fn skip_template(src: &[u8], mut i: usize, end: usize) -> usize {
    i += 1;
    while i < end {
        match src[i] {
            b'\\' => i += 2,
            b'`' => return i + 1,
            b'$' if at(src, i + 1, end) == Some(b'{') => {
                i = match find_matching(src, i + 1, end) {
                    Some(close) => close + 1,
                    None => end,
                };
            }
            _ => i += 1,
        }
    }
    i.min(end)
}

/// `src[i]` is one of `( { [ <` — returns the index of the matching closer,
/// or None if unbalanced. Skips strings, templates and comments. When matching
/// `< >`, `=>` is skipped so arrow/function types don't miscount.
pub(crate) fn find_matching(src: &[u8], mut i: usize, end: usize) -> Option<usize> {
    let open = src[i];
    let close = match open {
        b'{' => b'}',
        b'(' => b')',
        b'[' => b']',
        b'<' => b'>',
        _ => return None,
    };
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
        if open == b'<' && c == b'=' && at(src, i + 1, end) == Some(b'>') {
            i += 2;
            continue;
        }
        if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// `src[i]` is `/` where a regex literal is allowed — returns the index just
/// past the literal (including flags), or None if it doesn't scan as a regex.
pub(crate) fn scan_regex(src: &[u8], mut i: usize, end: usize) -> Option<usize> {
    i += 1;
    let mut in_class = false;
    while i < end {
        match src[i] {
            b'\\' => i += 2,
            b'\n' => return None,
            b'[' => {
                in_class = true;
                i += 1;
            }
            b']' => {
                in_class = false;
                i += 1;
            }
            b'/' if !in_class => {
                i += 1;
                while i < end && is_ident_char(src[i]) {
                    i += 1;
                }
                return Some(i.min(end));
            }
            _ => i += 1,
        }
    }
    None
}

/// Scans a type annotation until a top-level `,` or closing bracket.
pub(crate) fn scan_type_end(src: &[u8], mut i: usize, end: usize) -> usize {
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

/// True if the range contains an `await` token in code position (including
/// inside template interpolations, excluding strings and comments).
pub(crate) fn contains_await(src: &[u8], mut i: usize, end: usize) -> bool {
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
            i += 1;
            while i < end {
                match src[i] {
                    b'\\' => i += 2,
                    b'`' => {
                        i += 1;
                        break;
                    }
                    b'$' if at(src, i + 1, end) == Some(b'{') => {
                        let close = find_matching(src, i + 1, end).unwrap_or(end);
                        if contains_await(src, i + 2, close) {
                            return true;
                        }
                        i = (close + 1).min(end);
                    }
                    _ => i += 1,
                }
            }
            continue;
        }
        if is_ident_start(c) {
            let j = ident_end(src, i, end);
            if &src[i..j] == b"await" {
                return true;
            }
            i = j;
            continue;
        }
        i += 1;
    }
    false
}
