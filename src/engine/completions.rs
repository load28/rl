//! Completion where the checker has nothing to complete: pattern position.
//!
//! A case tag and a payload field name are written in places that lower to
//! a string literal and a destructuring key, so asking TypeScript "what can
//! go here?" asks about the wrong text — there *is* no text of rl's kind in
//! the output. The completion probe that mends an unfinished `x |> .`
//! cannot help either: splicing a placeholder into a pattern produces a
//! pattern, not a question TypeScript can answer.
//!
//! So rl answers, from the analysis' declaration table — the same table
//! that decides exhaustiveness and resolution, under the same shadowing.
//!
//! **Why the token stream and not the parser.** Completion is asked exactly
//! when the construct does not parse: `match (s) { Circle(r) => r, Po|` has
//! an arm with no body yet. The parser is all-or-nothing by contract (it
//! claims only complete constructs), so a parse-based answer would go
//! silent at the one moment it is wanted. The context is therefore read off
//! the token stream, which is defined for any text, and only the
//! *declarations* come from the parse — and a declaration elsewhere in the
//! file is complete even while a match is being typed.

use std::path::Path;

use crate::analysis::DeclaredEnum;
use crate::lexer::{Token, TokenKind, lex};

use super::language::Position;

/// What a completion item is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RlCompletionKind {
    /// A case tag.
    Case,
    /// A payload field name.
    Field,
    /// The wildcard arm `_`. Offered where an arm may be written, and
    /// nowhere else — `if let _ = x` is not rl syntax.
    Wildcard,
}

/// One thing that can be written at the asked-about position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RlCompletion {
    /// What to insert.
    pub label: String,
    /// Which name space it comes from.
    pub kind: RlCompletionKind,
    /// The declaration, rendered — shown beside the label.
    pub detail: String,
    /// Whether an arm already covers this case (an editor sorts these
    /// last, or dims them). Always false for fields.
    pub covered: bool,
}

/// What can be written at `position`, or an empty list when the position is
/// not a pattern position rl owns.
///
/// The answer never includes ordinary TypeScript completions: those are the
/// service's, and a consumer merges the two lists.
pub fn rl_completions_at(path: &Path, source: &str, position: Position) -> Vec<RlCompletion> {
    let offset = super::language::source_byte(source, position);
    let tokens = lex(source, 0, source.len());
    let declarations = super::language::analyses_for(path, source).declarations;
    match context(source, &tokens, offset) {
        Some(Context::Case { of: Some(tags) }) => {
            let mut items = match resolve(&declarations, &tags) {
                Some(declared) => cases(declared, &tags),
                None => Vec::new(),
            };
            // An arm position always admits the wildcard, whether or not
            // the subject resolved.
            items.push(RlCompletion {
                label: "_".to_string(),
                kind: RlCompletionKind::Wildcard,
                detail: "wildcard arm — every remaining case (must be last)".to_string(),
                covered: false,
            });
            items
        }
        // A pattern position with nothing to say which enum it is over —
        // an `if let` names its enum only by the tag being typed. Every
        // visible case is a candidate, and the editor filters by prefix.
        Some(Context::Case { of: None }) => declarations
            .iter()
            .flat_map(|declared| cases(declared, &[]))
            .collect(),
        Some(Context::Field { tag }) => {
            let Some(declared) = resolve(&declarations, std::slice::from_ref(&tag)) else {
                return Vec::new();
            };
            fields(declared, &tag)
        }
        Some(Context::Nested { tag, field }) => {
            let Some(declared) = resolve(&declarations, std::slice::from_ref(&tag)) else {
                return Vec::new();
            };
            let Some(inner) = declared
                .constructors
                .iter()
                .find(|c| c.tag == tag)
                .and_then(|c| c.fields.as_deref())
                .and_then(|fields| fields.iter().find(|f| f.name == field))
                .and_then(|f| type_enum(&declarations, &f.ty))
            else {
                return Vec::new();
            };
            cases(inner, &[])
        }
        None => Vec::new(),
    }
}

/// The pattern position the cursor is in.
#[derive(Debug, PartialEq, Eq)]
enum Context {
    /// A tag is expected. `of` carries the tags already written in the same
    /// match, which is what says *which* enum — `None` when the position
    /// has no such evidence (an `if let`).
    Case { of: Option<Vec<String>> },
    /// A payload field name of `tag` is expected.
    Field { tag: String },
    /// A nested pattern's tag is expected, in `tag`'s field `field`.
    Nested { tag: String, field: String },
}

fn context(source: &str, tokens: &[Token], offset: usize) -> Option<Context> {
    // The identifier being typed is not context — step over it.
    let cursor = tokens
        .iter()
        .position(|t| t.span.start >= offset)
        .unwrap_or(tokens.len());
    // The identifier under the cursor is the prefix being typed: it is
    // neither context nor an arm already written.
    let prefix = if cursor < tokens.len() && is_prefix(tokens, cursor, offset) {
        Some(cursor)
    } else if cursor > 0 && is_prefix(tokens, cursor - 1, offset) {
        Some(cursor - 1)
    } else {
        None
    };
    let before = prefix.unwrap_or(cursor);
    if before == 0 {
        return None;
    }

    // Inside a pattern's parens? The innermost unclosed `(` decides.
    if let Some(open) = innermost_open(tokens, before)
        && open > 0
        && matches!(tokens[open - 1].kind, TokenKind::Ident)
    {
        let tag = text(source, &tokens[open - 1]).to_string();
        // `field:` right before the cursor means the nested pattern's tag.
        if before >= 2
            && matches!(tokens[before - 1].kind, TokenKind::Punct(b':'))
            && matches!(tokens[before - 2].kind, TokenKind::Ident)
        {
            return Some(Context::Nested {
                tag,
                field: text(source, &tokens[before - 2]).to_string(),
            });
        }
        // Otherwise a field name: at the start of the list or after a comma.
        if matches!(
            tokens[before - 1].kind,
            TokenKind::Punct(b'(') | TokenKind::Punct(b',')
        ) {
            return Some(Context::Field { tag });
        }
        return None;
    }

    // `if let <cursor>` — rl-only syntax, so no other reading is possible.
    if before >= 2
        && text(source, &tokens[before - 1]) == "let"
        && text(source, &tokens[before - 2]) == "if"
    {
        return Some(Context::Case { of: None });
    }

    // An arm's pattern: directly inside a match body, after `{` or `,`.
    if matches!(
        tokens[before - 1].kind,
        TokenKind::Punct(b'{') | TokenKind::Punct(b',') | TokenKind::Punct(b'|')
    ) && let Some(body) = enclosing_match_body(source, tokens, before)
    {
        return Some(Context::Case {
            of: Some(arm_tags(source, tokens, body, prefix)),
        });
    }
    None
}

/// Whether the token at `index` is the identifier the cursor sits in — the
/// prefix being typed rather than context.
fn is_prefix(tokens: &[Token], index: usize, offset: usize) -> bool {
    matches!(tokens[index].kind, TokenKind::Ident)
        && tokens[index].span.start <= offset
        && offset <= tokens[index].span.end
}

/// The index of the innermost `(` still open at `before`, if any. Braces
/// close the search: a `(` outside the enclosing block is not this
/// position's paren.
fn innermost_open(tokens: &[Token], before: usize) -> Option<usize> {
    let mut depth = 0usize;
    for index in (0..before).rev() {
        match tokens[index].kind {
            TokenKind::Punct(b')') => depth += 1,
            TokenKind::Punct(b'(') => {
                if depth == 0 {
                    return Some(index);
                }
                depth -= 1;
            }
            TokenKind::Punct(b'}') => return None,
            TokenKind::Punct(b'{') => return None,
            _ => {}
        }
    }
    None
}

/// The token range of the `match` body the position sits directly in —
/// `(open brace index, close brace index or end)` — or `None` when it does
/// not sit in one.
fn enclosing_match_body(source: &str, tokens: &[Token], before: usize) -> Option<(usize, usize)> {
    // Walk back to the `{` this position is directly inside.
    let mut depth = 0usize;
    let mut open = None;
    for index in (0..before).rev() {
        match tokens[index].kind {
            TokenKind::Punct(b'}') => depth += 1,
            TokenKind::Punct(b'{') => {
                if depth == 0 {
                    open = Some(index);
                    break;
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    let open = open?;
    // `match ( ... ) {` — the scrutinee parens sit between the keyword and
    // the brace, so step back over them.
    let close_paren = open.checked_sub(1)?;
    if !matches!(tokens[close_paren].kind, TokenKind::Punct(b')')) {
        return None;
    }
    let mut depth = 0usize;
    let mut keyword = None;
    for index in (0..close_paren).rev() {
        match tokens[index].kind {
            TokenKind::Punct(b')') => depth += 1,
            TokenKind::Punct(b'(') => {
                if depth == 0 {
                    keyword = index.checked_sub(1);
                    break;
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    let keyword = keyword?;
    if text(source, &tokens[keyword]) != "match" {
        return None;
    }
    // The body ends at its matching brace, or at the end of the token
    // stream when the user has not typed it yet.
    let mut depth = 0usize;
    let mut close = tokens.len();
    for (index, token) in tokens.iter().enumerate().skip(open + 1) {
        match token.kind {
            TokenKind::Punct(b'{') => depth += 1,
            TokenKind::Punct(b'}') => {
                if depth == 0 {
                    close = index;
                    break;
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    Some((open, close))
}

/// The tags the arms of a match body already write, in source order.
///
/// Read off the token stream at the body's own nesting level: the first
/// identifier after `{`, `,` or `|` is a tag, and anything deeper (an arm
/// body, a pattern's payload) is skipped.
fn arm_tags(
    source: &str,
    tokens: &[Token],
    (open, close): (usize, usize),
    prefix: Option<usize>,
) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    let mut depth = 0usize;
    let mut expect = true;
    let end = close.min(tokens.len());
    for (index, token) in tokens.iter().enumerate().take(end).skip(open + 1) {
        match token.kind {
            TokenKind::Punct(b'(') | TokenKind::Punct(b'{') | TokenKind::Punct(b'[') => {
                depth += 1;
                expect = false;
            }
            TokenKind::Punct(b')') | TokenKind::Punct(b'}') | TokenKind::Punct(b']') => {
                depth = depth.saturating_sub(1);
            }
            TokenKind::Punct(b',') | TokenKind::Punct(b'|') if depth == 0 => expect = true,
            TokenKind::Ident if depth == 0 && expect => {
                if prefix == Some(index) {
                    expect = false;
                    continue;
                }
                let tag = text(source, token);
                if !tags.iter().any(|t| t == tag) {
                    tags.push(tag.to_string());
                }
                expect = false;
            }
            _ if depth == 0 => expect = false,
            _ => {}
        }
    }
    tags
}

/// The enum the table resolves a tag set to: the first declaration holding
/// every one of them — the shadowing order the table is already in, and the
/// same rule pattern resolution uses.
fn resolve<'a>(declarations: &'a [DeclaredEnum], tags: &[String]) -> Option<&'a DeclaredEnum> {
    declarations.iter().find(|declared| {
        tags.iter()
            .all(|tag| declared.constructors.iter().any(|c| c.tag == *tag))
    })
}

/// The enum a declared field type names, when it names one plainly.
fn type_enum<'a>(declarations: &'a [DeclaredEnum], ty: &str) -> Option<&'a DeclaredEnum> {
    let trimmed = ty.trim();
    let base: String = trimmed
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '$' | '.'))
        .collect();
    declarations.iter().find(|d| d.name == base)
}

fn cases(declared: &DeclaredEnum, covered: &[String]) -> Vec<RlCompletion> {
    declared
        .constructors
        .iter()
        .map(|constructor| RlCompletion {
            label: constructor.tag.clone(),
            kind: RlCompletionKind::Case,
            detail: super::names::case_signature(&declared.name, constructor),
            covered: covered.contains(&constructor.tag),
        })
        .collect()
}

fn fields(declared: &DeclaredEnum, tag: &str) -> Vec<RlCompletion> {
    declared
        .constructors
        .iter()
        .find(|c| c.tag == tag)
        .and_then(|c| c.fields.as_deref())
        .unwrap_or_default()
        .iter()
        .map(|field| RlCompletion {
            label: field.name.clone(),
            kind: RlCompletionKind::Field,
            detail: super::names::field_signature(field),
            covered: false,
        })
        .collect()
}

fn text<'a>(source: &'a str, token: &Token) -> &'a str {
    &source[token.span.start..token.span.end]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(source: &str, needle: &str) -> Position {
        let offset = source.find(needle).expect("needle") + needle.len();
        let before = &source[..offset];
        Position {
            line: before.matches('\n').count() as u32,
            character: (offset - before.rfind('\n').map_or(0, |n| n + 1)) as u32,
        }
    }

    fn labels(source: &str, needle: &str) -> Vec<String> {
        rl_completions_at(Path::new("/p/a.rl"), source, at(source, needle))
            .into_iter()
            .map(|item| item.label)
            .collect()
    }

    const DECL: &str = "enum Shape { Circle(radius: number), Rect(w: number, h: number), Point }\n";

    #[test]
    fn an_arm_position_offers_the_subjects_cases() {
        // The arm being typed has no body yet — which is exactly when the
        // question is asked, and when the parser claims nothing.
        let src = format!("{DECL}const a = match (s) {{ Circle(r) => r, Po }};\n");
        assert_eq!(
            labels(&src, "Circle(r) => r, "),
            ["Circle", "Rect", "Point", "_"]
        );
        let items = rl_completions_at(Path::new("/p/a.rl"), &src, at(&src, "Circle(r) => r, "));
        assert_eq!(
            items
                .iter()
                .filter(|i| i.covered)
                .map(|i| i.label.as_str())
                .collect::<Vec<_>>(),
            ["Circle"],
            "an arm already written is marked, not hidden"
        );
    }

    #[test]
    fn a_payload_position_offers_the_cases_fields() {
        let src = format!("{DECL}const a = match (s) {{ Rect(w) => w, Point => 0 }};\n");
        assert_eq!(labels(&src, "Rect("), ["w", "h"]);
        // ...and after a comma inside the list.
        let src = format!("{DECL}const a = match (s) {{ Rect(w, ) => w }};\n");
        assert_eq!(labels(&src, "Rect(w, "), ["w", "h"]);
    }

    #[test]
    fn an_if_let_offers_every_visible_case() {
        let src = format!("{DECL}if let  = s {{ }}\n");
        let found = labels(&src, "if let ");
        assert!(found.contains(&"Circle".to_string()), "{found:?}");
        // The built-ins are visible without a declaration.
        assert!(found.contains(&"Some".to_string()), "{found:?}");
    }

    #[test]
    fn a_nested_position_offers_the_fields_enum() {
        let src = "enum Inner { Yes(n: number), No }\n\
                   enum Outer { Wrap(inner: Inner), Bare }\n\
                   const a = match (o) { Wrap(inner: ) => 0 };\n";
        assert_eq!(labels(src, "Wrap(inner: "), ["Yes", "No"]);
    }

    #[test]
    fn payload_positions_work_in_every_construct_that_has_a_pattern() {
        // The paren logic is construct-agnostic: what decides is that the
        // innermost open paren follows a case tag.
        let src = format!("{DECL}if let Rect() = s {{ }}\n");
        assert_eq!(labels(&src, "if let Rect("), ["w", "h"]);

        let src = format!("{DECL}const Rect() = s else {{ return; }};\n");
        assert_eq!(labels(&src, "const Rect("), ["w", "h"]);

        let src = "enum Inner { Yes(n: number), No }\n\
                   enum Outer { Wrap(inner: Inner), Bare }\n\
                   if let Wrap(inner: ) = o { }\n";
        assert_eq!(labels(src, "Wrap(inner: "), ["Yes", "No"]);
    }

    #[test]
    fn the_wildcard_is_an_arm_position_only() {
        let src = format!("{DECL}const a = match (s) {{  }};\n");
        assert!(labels(&src, "match (s) { ").contains(&"_".to_string()));
        let src = format!("{DECL}if let  = s {{ }}\n");
        assert!(!labels(&src, "if let ").contains(&"_".to_string()));
    }

    #[test]
    fn nothing_is_offered_outside_a_pattern() {
        let src = format!("{DECL}const a = match (s) {{ Circle(r) => r }};\nconst b = ;\n");
        assert!(labels(&src, "const b = ").is_empty());
        // The scrutinee is an expression, not a pattern.
        assert!(labels(&src, "match (").is_empty());
        // An arm body is an expression too.
        assert!(labels(&src, "Circle(r) => ").is_empty());
    }
}
