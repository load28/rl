//! The three coordinate spaces, and the conversions between them.
//!
//! A position travels: `.rl` source byte → emitted TypeScript byte
//! ([`crate::EmitMapping`]) → UTF-16 code unit, which is what TypeScript
//! itself counts in. Questions travel that way; diagnostics travel back.
//!
//! Only bytes copied **verbatim** from the source have a source position at
//! all. Compiler-written glue — the `switch` IIFE, a destructuring, an enum
//! emission — belongs to no `.rl` byte, and a diagnostic landing there is
//! reported without a mapped position rather than at a made-up one.

use crate::EmitMapping;

/// Offset of `byte` in `text`, counted in UTF-16 code units — TypeScript's
/// own coordinate space. An offset past the end clamps to the end.
pub(crate) fn to_utf16(text: &str, byte: usize) -> usize {
    match text.get(..byte) {
        Some(prefix) => prefix.encode_utf16().count(),
        None => text.encode_utf16().count(),
    }
}

/// The inverse of [`to_utf16`]: the byte offset a UTF-16 offset names. An
/// offset past the end clamps to the length; one landing inside a surrogate
/// pair clamps to the start of that character.
pub(crate) fn from_utf16(text: &str, utf16: usize) -> usize {
    let mut units = 0;
    for (byte, ch) in text.char_indices() {
        if units >= utf16 {
            return byte;
        }
        units += ch.len_utf16();
    }
    text.len()
}

/// Where a source byte landed in the emitted output, or `None` when it was
/// not copied verbatim.
pub(crate) fn to_output(mappings: &[EmitMapping], src: usize) -> Option<usize> {
    mappings
        .iter()
        .find(|m| src >= m.src && src < m.src + m.len)
        .map(|m| m.out + (src - m.src))
}

/// Where an emitted byte came from in the source, or `None` when it is
/// compiler-written glue.
pub(crate) fn to_source(mappings: &[EmitMapping], out: usize) -> Option<usize> {
    mappings
        .iter()
        .find(|m| out >= m.out && out < m.out + m.len)
        .map(|m| m.src + (out - m.out))
}

/// Where an emitted byte came from, or — when it is compiler-written glue —
/// where the nearest preceding verbatim byte came from.
///
/// A diagnostic on glue still belongs somewhere the user can look: the
/// construct it was generated for starts at the last source byte copied
/// before it. The caller says which of the two happened, so the message can
/// too.
pub(crate) fn to_source_or_nearest(mappings: &[EmitMapping], out: usize) -> Option<(usize, bool)> {
    if let Some(exact) = to_source(mappings, out) {
        return Some((exact, true));
    }
    mappings
        .iter()
        .filter(|m| m.out < out)
        .max_by_key(|m| m.out)
        .map(|m| (m.src + m.len.saturating_sub(1), false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_round_trips_through_multibyte_text() {
        let text = "const 한글 = \"안녕😀\";\nconst bad = 1;";
        let byte = text.find("bad").unwrap();
        let units = to_utf16(text, byte);
        assert_ne!(units, byte, "the prefix has multi-byte characters");
        assert_eq!(from_utf16(text, units), byte);
    }

    #[test]
    fn utf16_offsets_count_a_surrogate_pair_as_two() {
        let text = "😀x";
        assert_eq!(to_utf16(text, text.find('x').unwrap()), 2);
        assert_eq!(from_utf16(text, 2), "😀".len());
    }

    #[test]
    fn offsets_past_the_end_clamp() {
        let text = "abc";
        assert_eq!(to_utf16(text, 99), 3);
        assert_eq!(from_utf16(text, 99), 3);
    }

    #[test]
    fn glue_falls_back_to_the_nearest_preceding_source_byte() {
        let mappings = [
            EmitMapping {
                src: 0,
                out: 0,
                len: 4,
            },
            EmitMapping {
                src: 10,
                out: 20,
                len: 6,
            },
        ];
        assert_eq!(to_source_or_nearest(&mappings, 22), Some((12, true)));
        // Between the chunks: the last byte of the chunk before it.
        assert_eq!(to_source_or_nearest(&mappings, 10), Some((3, false)));
        // Before any chunk: nothing to point at.
        assert_eq!(to_source_or_nearest(&mappings, 0), Some((0, true)));
    }

    #[test]
    fn mappings_round_trip_and_reject_glue() {
        let mappings = [
            EmitMapping {
                src: 0,
                out: 0,
                len: 4,
            },
            EmitMapping {
                src: 10,
                out: 20,
                len: 6,
            },
        ];
        assert_eq!(to_output(&mappings, 2), Some(2));
        assert_eq!(to_output(&mappings, 12), Some(22));
        assert_eq!(to_source(&mappings, 22), Some(12));
        // Between the chunks is compiler-written glue.
        assert_eq!(to_output(&mappings, 8), None);
        assert_eq!(to_source(&mappings, 10), None);
    }
}
