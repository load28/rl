//! Structural parsing of rl pipeline expressions (`head |> step |> ...`).
//!
//! Contract safety: `|>` (a `|` immediately followed by `>`) cannot occur
//! anywhere in valid TypeScript — after a `|` (bitwise OR / union type) an
//! expression or a type must follow, and `>` starts neither — so the lexer
//! fuses it into a single [`TokenKind::PipeOp`] token and claiming it never
//! affects the passthrough contract. Inside strings, comments, regexes, and
//! template text the byte pair is trivia/literal content and never lexes as
//! a token.
//!
//! The *head* is found by the main token loop, which tracks the start of
//! the expression currently being scanned (see `expr start tracking` in
//! [`super`]); this module owns the forward scan over the steps. A head
//! that is exactly the bare identifier `flow` is the composition keyword
//! (`flow |> f |> g` builds a function instead of flowing a value) — a
//! contextual keyword, so a variable named `flow` still pipes when
//! parenthesized (`(flow) |> f`). A step
//! runs to the next top-level `|>` or to a terminator the pipeline cannot
//! contain at its top level (`;`, `,`, an unmatched closer, or the region
//! end). A top-level `?`, `:`, `=` (assignment), `=>`, or statement-only
//! keyword aborts the claim — ternaries and arrow functions must be
//! parenthesized (a normative rule, like the match scrutinee parens) — and
//! the unclaimed `|>` is recorded for the semantic phase to report.

use super::cursor::dotted_at;
use crate::ast::{PipeExpr, PipeStep, Span};
use crate::lexer::{Token, TokenKind};

/// True for a `=` Punct that is (the start of) an assignment operator —
/// not part of `==`/`===`/`!=`/`<=`/`>=`. Compound assignments (`+=`,
/// `&&=`, ...) classify as assignment via their trailing `=`.
pub(super) fn is_assignment_eq(bytes: &[u8], span: Span) -> bool {
    let prev = span.start.checked_sub(1).map(|p| bytes[p]);
    let next = bytes.get(span.end).copied();
    next != Some(b'=') && !matches!(prev, Some(b'=' | b'!' | b'<' | b'>'))
}

/// True when the head is exactly the bare `flow` keyword — one identifier
/// token spelled `flow`. A dotted or called head (`a.flow`, `flow()`) has
/// more tokens and stays an ordinary value head.
fn is_flow_head(
    parser: &super::Parser,
    tokens: &[Token],
    head_idx: usize,
    pipe_idx: usize,
) -> bool {
    pipe_idx - head_idx == 1
        && matches!(tokens[head_idx].kind, TokenKind::Ident)
        && &parser.src[tokens[head_idx].span.start..tokens[head_idx].span.end] == "flow"
}

/// `tokens[pipe_idx]` is a `|>` token and `tokens[head_idx..pipe_idx]` is
/// the head expression (non-empty, guaranteed by the caller). Parses the
/// step chain; on success returns the token index just past the last step
/// and the parsed pipeline. `None` leaves everything unclaimed (the caller
/// records the stray `|>` for sema).
pub(super) fn parse_pipeline(
    parser: &super::Parser,
    tokens: &[Token],
    head_idx: usize,
    pipe_idx: usize,
) -> Option<(usize, PipeExpr)> {
    let head_span = Span {
        start: tokens[head_idx].span.start,
        end: tokens[pipe_idx - 1].span.end,
    };

    let mut steps: Vec<PipeStep> = Vec::new();
    let mut k = pipe_idx;
    while matches!(tokens.get(k).map(|t| &t.kind), Some(TokenKind::PipeOp)) {
        k += 1;
        let step_from = k;
        let mut depth = 0usize;
        while let Some(t) = tokens.get(k) {
            match &t.kind {
                TokenKind::PipeOp if depth == 0 => break,
                TokenKind::Punct(b';' | b',') if depth == 0 => break,
                TokenKind::Punct(b')' | b']' | b'}') if depth == 0 => break,
                TokenKind::Punct(b'?' | b':') if depth == 0 => return None,
                TokenKind::Arrow if depth == 0 => return None,
                TokenKind::Punct(b'=') if depth == 0 && is_assignment_eq(parser.bytes, t.span) => {
                    return None;
                }
                TokenKind::Ident
                    if depth == 0
                        && !dotted_at(tokens, step_from, k)
                        && super::tries::STMT_ONLY_WORDS
                            .contains(&&parser.src[t.span.start..t.span.end]) =>
                {
                    return None;
                }
                TokenKind::Punct(b'(' | b'[' | b'{') => depth += 1,
                TokenKind::Punct(b')' | b']' | b'}') => depth -= 1,
                _ => {}
            }
            k += 1;
        }
        if k == step_from {
            return None; // empty step (`x |> |> f`, `x |>;`, trailing `|>`)
        }

        // A step starting with `.` + identifier is a method step; `?.` and
        // a `.` without an identifier are out (documented).
        let postfix = match &tokens[step_from].kind {
            TokenKind::Punct(b'.') => {
                if !matches!(
                    tokens.get(step_from + 1).map(|t| &t.kind),
                    Some(TokenKind::Ident)
                ) {
                    return None;
                }
                true
            }
            TokenKind::OptChain => return None,
            _ => false,
        };

        let span = Span {
            start: tokens[step_from].span.start,
            end: tokens[k - 1].span.end,
        };
        steps.push(PipeStep {
            span,
            postfix,
            body: parser.parse_tokens(&tokens[step_from..k], span.start, span.end),
        });
    }
    if steps.is_empty() {
        return None;
    }

    let head = (!is_flow_head(parser, tokens, head_idx, pipe_idx))
        .then(|| parser.parse_tokens(&tokens[head_idx..pipe_idx], head_span.start, head_span.end));
    Some((
        k,
        PipeExpr {
            head_span,
            head,
            steps,
        },
    ))
}
