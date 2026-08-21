//! Hints — what rl has to say about a file that is not an error.
//!
//! rl's diagnostics are errors, and only errors: the CLI either compiles a
//! file or reports a position and stops. That is a deliberate part of the
//! error-layer contract (`CLAUDE.md`), and it leaves no room for the one
//! thing the pattern analysis knows but must not reject — an **unreachable
//! arm**. Rust reports it as a lint; making it an error here would reject
//! programs that compile today, and rl has no warning level to put it at.
//!
//! An editor does, though. A hint is not part of the compile answer: it is
//! attached to a range, it never fails a build, and the CLI never prints
//! one. So the arms the usefulness algorithm already computes as dead
//! ([`crate::Coverage::unreachable`], TASK-103) surface here and nowhere
//! else.
//!
//! Like [`super::names`] and [`super::completions`] this is **parse-only**:
//! source in, hints out, no toolchain and no project — so an editor shows
//! them mid-edit and in a workspace with no TypeScript installed.

use std::path::Path;

use super::language::{Range, span_range};

/// What a hint is about. One kind today; the enum is the seam that keeps a
/// consumer from switching on message text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RlHintKind {
    /// An arm that matches nothing an earlier arm has not already matched.
    UnreachableArm,
}

/// One thing rl has to say about a range that is not an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RlHint {
    /// What kind of hint it is.
    pub kind: RlHintKind,
    /// The range it is about — the whole arm, so an editor can dim it.
    pub range: Range,
    /// The message, as it is shown.
    pub message: String,
}

/// Every hint rl has about `source`.
///
/// `path` resolves relative `.rl` imports (from disk, so an unsaved
/// imported file is seen as last saved), exactly as the other parse-only
/// surfaces resolve them.
pub fn rl_hints(path: &Path, source: &str) -> Vec<RlHint> {
    let analyses = super::language::analyses_for(path, source);
    let mut out = Vec::new();
    for analysis in &analyses.matches {
        let Some(coverage) = &analysis.coverage else {
            continue;
        };
        for &index in &coverage.unreachable {
            let Some(arm) = analysis.arms.get(index) else {
                continue;
            };
            // The body span runs to the arm's delimiter, so it can carry
            // trailing whitespace — a dimmed range should stop at the code.
            let end = source[..arm.body_end].trim_end().len();
            out.push(RlHint {
                kind: RlHintKind::UnreachableArm,
                range: span_range(source, arm.pattern_start, end),
                message: "unreachable arm: an earlier arm already matches every value \
                          this one would"
                    .to_string(),
            });
        }
    }
    out.sort_by_key(|hint| (hint.range.start.line, hint.range.start.character));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hints(source: &str) -> Vec<RlHint> {
        rl_hints(Path::new("/p/a.rl"), source)
    }

    #[test]
    fn an_arm_an_earlier_one_already_covers_is_hinted() {
        let src = "enum E { A(x: string), B(y: number) }\n\
                   const v = match (e) { A(x) => x, B(y) => y, A(x: z) => z };\n";
        let found = hints(src);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].kind, RlHintKind::UnreachableArm);
        // The whole third arm, from its pattern to the end of its body.
        let line = src.lines().nth(1).expect("second line");
        let start = line.find("A(x: z)").expect("the dead arm") as u32;
        assert_eq!(found[0].range.start.line, 1);
        assert_eq!(found[0].range.start.character, start);
        assert_eq!(
            found[0].range.end.character,
            start + "A(x: z) => z".len() as u32
        );
    }

    #[test]
    fn a_live_match_has_nothing_to_say() {
        let src = "enum E { A(x: string), B(y: number) }\n\
                   const v = match (e) { A(x) => x, B(y) => y };\n";
        assert!(hints(src).is_empty());
    }

    #[test]
    fn a_guarded_arm_is_never_dead() {
        // The guard may be false, so a later arm for the same case still
        // matches values the guarded one does not.
        let src = "enum E { A(x: string), B(y: number) }\n\
                   const v = match (e) { A(x) if ok => x, A(x: z) => z, B(y) => y };\n";
        assert!(hints(src).is_empty());
    }

    #[test]
    fn a_tuple_combination_an_earlier_arm_covers_is_hinted() {
        let src = "enum D { N(a: number), S(b: number) }\n\
                   enum P { F(c: number), G(d: number) }\n\
                   const v = match (x, y) { (N, _) => 1, (S, F) => 2, (S, G) => 3, (N, F) => 4 };\n";
        let found = hints(src);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].range.start.line, 2);
    }
}
