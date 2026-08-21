//! Semantic tokens — the parser's own classification of the ambiguous
//! surface, for editors.
//!
//! The TextMate grammar (a regex approximation) colors rl constructs
//! structurally, but two families of decisions are the parser's alone:
//!
//! - **Claimed**: a `match`/`result`/`flow` the parser lifted really is rl,
//!   even where the grammar's same-line lookaheads miss it (a `flow` head
//!   with its first `|>` on the next line, a binding split across lines).
//! - **Not claimed**: an identifier that merely looks like rl — a function
//!   named `match` being called, a variable named `result` before a block —
//!   stays plain TypeScript, and the editor should color it that way.
//!
//! This module answers both from one structural parse ([`crate::parser`]),
//! so the editor's colors agree with the compiler by construction. Only the
//! ambiguous surface is reported — everything the grammar already gets
//! right (operators like `|>`, literals, TS-owned tokens) is left to it,
//! the same economy rust-analyzer applies. Consumers overlay these on the
//! grammar via LSP `textDocument/semanticTokens`.

use super::language::{Position, Range};
use crate::ast::{
    Binding, IfLetElse, Pattern, Program, ResultItem, Segment, TagPattern, TemplateChunk,
    TuplePattern,
};
use crate::lexer::{self, Token, TokenKind as Lex};
use crate::typescript::mapper;

/// What one token *is*, in the parser's judgement. Names follow the LSP
/// standard token types so an adapter maps them one to one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticTokenKind {
    /// An rl keyword the parser claimed: `match`, `result`, `flow`, `try`.
    Keyword,
    /// An rl enum's name, at its declaration.
    Enum,
    /// A case tag — in an enum declaration or in a pattern.
    EnumMember,
    /// A binding an rl pattern introduces (alias included).
    Variable,
    /// A field name in a pattern's `field: alias` binding.
    Property,
    /// An identifier that looks like an rl keyword but is a call —
    /// `match(...)` naming a plain function. Reported so the editor
    /// *un*-colors what the grammar over-approximated.
    Function,
    /// The `<-` of a claimed Result binding.
    Operator,
}

impl SemanticTokenKind {
    /// The LSP standard token-type string.
    pub fn as_str(self) -> &'static str {
        match self {
            SemanticTokenKind::Keyword => "keyword",
            SemanticTokenKind::Enum => "enum",
            SemanticTokenKind::EnumMember => "enumMember",
            SemanticTokenKind::Variable => "variable",
            SemanticTokenKind::Property => "property",
            SemanticTokenKind::Function => "function",
            SemanticTokenKind::Operator => "operator",
        }
    }
}

/// One classified token, in `.rl` coordinates (never spans lines).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticToken {
    /// Where, in the `.rl` source.
    pub range: Range,
    /// What the parser says it is.
    pub kind: SemanticTokenKind,
}

/// Classifies `source` — one structural parse, byte offsets out. Sorted by
/// start; single-line by construction (identifiers and fixed operators).
pub(crate) fn scan(source: &str) -> Vec<(usize, usize, SemanticTokenKind)> {
    let program = crate::parser::parse(source);
    let mut out = Vec::new();
    walk(source, &program, &mut out);
    out.sort_by_key(|&(start, _, _)| start);
    out
}

/// The semantic tokens of one source text, in `.rl` coordinates.
///
/// Parse-only and stateless — no project, no TypeScript toolchain — so a
/// consumer can ask on every pause, for any buffer, in any environment
/// (the same availability the grammar itself has).
pub fn semantic_tokens(source: &str) -> Vec<SemanticToken> {
    let lines = LineIndex::new(source);
    scan(source)
        .into_iter()
        .map(|(start, len, kind)| SemanticToken {
            range: Range {
                start: lines.position(source, start),
                end: lines.position(source, start + len),
            },
            kind,
        })
        .collect()
}

fn walk(src: &str, program: &Program, out: &mut Vec<(usize, usize, SemanticTokenKind)>) {
    for segment in &program.segments {
        match segment {
            Segment::Verbatim(span) => deny_lookalikes(src, span.start, span.end, out),
            Segment::Enum(decl) => {
                out.push((decl.name_off, decl.name.len(), SemanticTokenKind::Enum));
                for case in &decl.cases {
                    out.push((case.tag_off, case.tag.len(), SemanticTokenKind::EnumMember));
                }
            }
            Segment::Match(m) => {
                out.push((m.keyword_off, 5, SemanticTokenKind::Keyword));
                walk(src, &m.scrutinee, out);
                for arm in &m.arms {
                    pattern(src, &arm.pattern, out);
                    if let Some(guard) = &arm.guard {
                        walk(src, &guard.expr, out);
                    }
                    walk(src, &arm.body, out);
                }
            }
            Segment::TupleMatch(m) => {
                out.push((m.keyword_off, 5, SemanticTokenKind::Keyword));
                for (_, scrutinee) in &m.scrutinees {
                    walk(src, scrutinee, out);
                }
                for arm in &m.arms {
                    if let TuplePattern::Elems(elems) = &arm.pattern {
                        for elem in elems {
                            pattern(src, elem, out);
                        }
                    }
                    if let Some(guard) = &arm.guard {
                        walk(src, &guard.expr, out);
                    }
                    walk(src, &arm.body, out);
                }
            }
            Segment::Try(t) => {
                // The bare form starts at `try`; in the declaration form the
                // statement starts at the declaration keyword and `try` sits
                // after `=`, where the grammar already colors it.
                if t.decl.is_none() {
                    out.push((t.keyword_off, 3, SemanticTokenKind::Keyword));
                }
                walk(src, &t.expr, out);
            }
            Segment::LetElse(stmt) => {
                for alt in &stmt.alternatives {
                    tag_pattern(alt, out);
                }
                walk(src, &stmt.expr, out);
                walk(src, &stmt.else_body, out);
            }
            Segment::IfLet(stmt) => if_let(src, stmt, out),
            Segment::RlImport(_) => {}
            Segment::ValModifier(_) => {
                // `val` keeps its grammar color (a storage modifier); the
                // parser and the grammar agree on every valid occurrence.
            }
            Segment::Template(template) => {
                for chunk in &template.chunks {
                    if let TemplateChunk::Interp(body) = chunk {
                        walk(src, body, out);
                    }
                }
            }
            Segment::Pipe(pipe) => {
                match &pipe.head {
                    // A composition's head is the `flow` keyword itself.
                    None => out.push((
                        pipe.head_span.start,
                        pipe.head_span.end - pipe.head_span.start,
                        SemanticTokenKind::Keyword,
                    )),
                    Some(head) => walk(src, head, out),
                }
                for step in &pipe.steps {
                    walk(src, &step.body, out);
                }
            }
            Segment::ResultBlock(block) => {
                out.push((block.keyword_off, 6, SemanticTokenKind::Keyword));
                for item in &block.items {
                    match item {
                        ResultItem::Stmts(stmts) => walk(src, stmts, out),
                        ResultItem::Bind(bind) => {
                            let span = bind.binding_span;
                            if is_identifier(&src[span.start..span.end]) {
                                out.push((
                                    span.start,
                                    span.end - span.start,
                                    SemanticTokenKind::Variable,
                                ));
                            }
                            if let Some(arrow_off) = expect_word(src, span.end, "<-") {
                                out.push((arrow_off, 2, SemanticTokenKind::Operator));
                            }
                            walk(src, &bind.expr, out);
                        }
                    }
                }
                walk(src, &block.value, out);
            }
        }
    }
}

fn if_let(
    src: &str,
    stmt: &crate::ast::IfLetStmt,
    out: &mut Vec<(usize, usize, SemanticTokenKind)>,
) {
    for alt in &stmt.alternatives {
        tag_pattern(alt, out);
    }
    walk(src, &stmt.expr, out);
    walk(src, &stmt.body, out);
    match &stmt.else_part {
        Some(IfLetElse::Block(body)) => walk(src, body, out),
        Some(IfLetElse::IfLet(chained)) => if_let(src, chained, out),
        None => {}
    }
}

fn pattern(src: &str, p: &Pattern, out: &mut Vec<(usize, usize, SemanticTokenKind)>) {
    let _ = src;
    match p {
        Pattern::Wildcard | Pattern::Literals(_) => {}
        Pattern::Tags(tags) => {
            for tag in tags {
                tag_pattern(tag, out);
            }
        }
    }
}

fn tag_pattern(tag: &TagPattern, out: &mut Vec<(usize, usize, SemanticTokenKind)>) {
    out.push((tag.tag_off, tag.tag.len(), SemanticTokenKind::EnumMember));
    if let Some(list) = &tag.bindings {
        bindings(list, out);
    }
}

fn bindings(list: &[Binding], out: &mut Vec<(usize, usize, SemanticTokenKind)>) {
    for binding in list {
        match (&binding.alias, &binding.nested) {
            // `field: alias` — the field names, the alias binds.
            (Some(_), _) => {
                out.push((
                    binding.name_span.start,
                    binding.name_span.end - binding.name_span.start,
                    SemanticTokenKind::Property,
                ));
                if let Some(span) = binding.alias_span {
                    out.push((
                        span.start,
                        span.end - span.start,
                        SemanticTokenKind::Variable,
                    ));
                }
            }
            // `field: Tag(...)` — the field names, the nested pattern matches.
            (None, Some(nested)) => {
                out.push((
                    binding.name_span.start,
                    binding.name_span.end - binding.name_span.start,
                    SemanticTokenKind::Property,
                ));
                tag_pattern(nested, out);
            }
            // Bare `name` — the field name doubles as the binding.
            (None, None) => out.push((
                binding.name_span.start,
                binding.name_span.end - binding.name_span.start,
                SemanticTokenKind::Variable,
            )),
        }
    }
}

/// The grammar colors `match (…)` and `result {` wherever they appear; the
/// parser knows which of those it did *not* claim. Reclassify the ones in
/// verbatim (plain TypeScript) ranges so the editor shows them as the
/// identifiers they are. Lexed, not searched — occurrences inside strings,
/// comments, templates and regexes never reach the token stream as
/// identifiers.
fn deny_lookalikes(
    src: &str,
    start: usize,
    end: usize,
    out: &mut Vec<(usize, usize, SemanticTokenKind)>,
) {
    let tokens = lexer::lex(src, start, end);
    deny_in(src, &tokens, out);
}

fn deny_in(src: &str, tokens: &[Token], out: &mut Vec<(usize, usize, SemanticTokenKind)>) {
    for (i, token) in tokens.iter().enumerate() {
        match &token.kind {
            Lex::Template(parts) => {
                for part in parts.iter() {
                    if let lexer::TplPart::Interp { tokens, .. } = part {
                        deny_in(src, tokens, out);
                    }
                }
            }
            Lex::Ident => {
                let text = &src[token.span.start..token.span.end];
                if text != "match" && text != "result" {
                    continue;
                }
                // A member access (`value.match(...)`) is out of the
                // grammar's reach already.
                if i > 0 && matches!(tokens[i - 1].kind, Lex::Punct(b'.') | Lex::OptChain) {
                    continue;
                }
                let next = tokens.get(i + 1);
                let denied = match text {
                    "match" => matches!(next.map(|t| &t.kind), Some(Lex::Punct(b'('))),
                    _ => matches!(next.map(|t| &t.kind), Some(Lex::Punct(b'{'))),
                };
                if denied {
                    let kind = if text == "match" {
                        SemanticTokenKind::Function
                    } else {
                        SemanticTokenKind::Variable
                    };
                    out.push((token.span.start, text.len(), kind));
                }
            }
            _ => {}
        }
    }
}

/// The offset of `word` after `at` when only ASCII whitespace separates
/// them; `None` otherwise (comments in between — rare — just skip the
/// token, never mis-place it).
fn expect_word(src: &str, at: usize, word: &str) -> Option<usize> {
    let bytes = src.as_bytes();
    let mut pos = at;
    while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
        pos += 1;
    }
    src[pos..].starts_with(word).then_some(pos)
}

fn is_identifier(text: &str) -> bool {
    let mut chars = text.bytes();
    chars
        .next()
        .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_' || b == b'$')
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'$')
}

/// Line starts, for byte-offset → line / UTF-16-column conversion.
struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    fn new(text: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(
            text.bytes()
                .enumerate()
                .filter(|&(_, b)| b == b'\n')
                .map(|(i, _)| i + 1),
        );
        LineIndex { starts }
    }

    fn position(&self, text: &str, byte: usize) -> Position {
        let line = self.starts.partition_point(|&start| start <= byte) - 1;
        let line_start = self.starts[line];
        Position {
            line: line as u32,
            character: mapper::to_utf16(&text[line_start..], byte - line_start) as u32,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds_at(source: &str) -> Vec<(String, SemanticTokenKind)> {
        scan(source)
            .into_iter()
            .map(|(start, len, kind)| (source[start..start + len].to_string(), kind))
            .collect()
    }

    #[test]
    fn claimed_constructs_report_their_tokens() {
        let src = r#"function demo(shape: Shape) {
  enum Shape { Circle(r: number), Dot }
  return match (shape) {
    Circle(r) => r,
    Some(value: user) => user,
    Pair(left: Circle(r)) => r,
    _ => 0,
  };
}
"#;
        let tokens = kinds_at(src);
        assert!(tokens.contains(&("Shape".into(), SemanticTokenKind::Enum)));
        assert!(tokens.contains(&("Circle".into(), SemanticTokenKind::EnumMember)));
        assert!(tokens.contains(&("Dot".into(), SemanticTokenKind::EnumMember)));
        assert!(tokens.contains(&("match".into(), SemanticTokenKind::Keyword)));
        assert!(tokens.contains(&("r".into(), SemanticTokenKind::Variable)));
        assert!(tokens.contains(&("value".into(), SemanticTokenKind::Property)));
        assert!(tokens.contains(&("user".into(), SemanticTokenKind::Variable)));
        assert!(tokens.contains(&("left".into(), SemanticTokenKind::Property)));
    }

    #[test]
    fn lookalike_calls_are_reclassified() {
        let src = "const a = match(x);\nconst b = value.match(x);\n";
        let tokens = kinds_at(src);
        assert_eq!(
            tokens
                .iter()
                .filter(|(text, _)| text == "match")
                .collect::<Vec<_>>(),
            vec![&("match".into(), SemanticTokenKind::Function)],
            "the bare call is denied once; the member call is not reported"
        );
    }

    #[test]
    fn lookalikes_inside_strings_and_comments_stay_silent() {
        let src = "const s = \"match(x)\";\n// match(x) result { }\nconst t = `${\"result {\"}`;\n";
        assert!(kinds_at(src).is_empty());
    }

    #[test]
    fn unclaimed_result_block_is_a_variable() {
        // No `<-` binding, so the parser passes it through — and the
        // identifier should be shown as one.
        let src = "const r = result\n{ a: 1 };\n";
        assert!(kinds_at(src).contains(&("result".into(), SemanticTokenKind::Variable)));
    }

    #[test]
    fn multi_line_flow_and_result_bind_are_claimed() {
        let src = "const f = flow\n  |> trim\n  |> parse;\nconst r = result {\n  const n <- get();\n  n\n};\n";
        let tokens = kinds_at(src);
        assert!(tokens.contains(&("flow".into(), SemanticTokenKind::Keyword)));
        assert!(tokens.contains(&("result".into(), SemanticTokenKind::Keyword)));
        assert!(tokens.contains(&("n".into(), SemanticTokenKind::Variable)));
        assert!(tokens.contains(&("<-".into(), SemanticTokenKind::Operator)));
    }

    #[test]
    fn let_else_and_if_let_tags_are_enum_members() {
        let src = "function f(o: O) {\n  let Some(value: v) = o else { return 0; };\n  if let Ok(value) = ask() {\n    use(value);\n  }\n  return v;\n}\n";
        let tokens = kinds_at(src);
        assert!(tokens.contains(&("Some".into(), SemanticTokenKind::EnumMember)));
        assert!(tokens.contains(&("Ok".into(), SemanticTokenKind::EnumMember)));
    }

    #[test]
    fn positions_are_utf16() {
        let src = "const 이름 = 1;\nconst a = match(x);\n";
        let tokens = semantic_tokens(src);
        assert_eq!(tokens.len(), 1);
        let token = tokens[0];
        assert_eq!(token.kind, SemanticTokenKind::Function);
        assert_eq!(token.range.start.line, 1);
        assert_eq!(token.range.start.character, 10);
        assert_eq!(token.range.end.character, 15);
    }
}
