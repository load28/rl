//! The flow IR — rl's minimal control-flow analysis.
//!
//! This is the compiler core's flow layer (`docs/design/compiler-core.md`
//! §9): **not** a TypeScript MIR, just enough of a CFG to answer the
//! control-flow questions rl's own constructs pose. The first consumer is
//! let-else — Rust's rule is "the `else` block must diverge", and until now
//! rlc approximated it by looking at the last statement's first keyword,
//! which rejected genuinely diverging blocks (`if`/`else` with both
//! branches returning, a bare block ending in `return`, code after a
//! `return`). The CFG answers structurally.
//!
//! What the lowering understands is deliberately small: statement
//! boundaries, the four diverging statements (`return`/`throw`/`break`/
//! `continue`), `if`/`else` (chains included), and bare blocks. Everything
//! else — loops, `switch`, `try` — is conservatively fall-through: a
//! construct the lowering does not model can only make the answer "does
//! not diverge", never a false "diverges". Function bodies inside the
//! block are opaque (their `return` leaves *them*, not the enclosing
//! function).
//!
//! The block/expression brace distinction (an object literal's `}` ends no
//! statement) lives here too, moved from the let-else parser — one
//! implementation, shared by statement splitting wherever flow looks.

use crate::lexer::{Token, TokenKind};

/// One body's control-flow graph.
#[derive(Debug)]
pub struct FlowBody {
    /// The blocks; [`BlockId`] indexes this.
    pub blocks: Vec<BasicBlock>,
    /// Where execution enters.
    pub entry: BlockId,
}

/// Index into [`FlowBody::blocks`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockId(pub u32);

/// One basic block. The lowering keeps blocks minimal — the terminator is
/// the analysis; statements that neither branch nor diverge are a `Goto`.
#[derive(Debug)]
pub struct BasicBlock {
    /// How the block leaves.
    pub terminator: Terminator,
}

/// How control leaves a block.
///
/// The design's fuller shape (`Branch { condition: ExprId, .. }`,
/// `Match { subject, targets }`) arrives when bodies are lowered from the
/// HIR; today's consumer asks about token streams, so conditions and
/// subjects are not modeled yet — a `match` in statement position is
/// simply fall-through, which is conservative.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Terminator {
    /// Fall through to another block.
    Goto(BlockId),
    /// A conditional split (`if`/`else`).
    Branch {
        /// Where the then-branch enters.
        then_bb: BlockId,
        /// Where the else-branch enters — the join block when no `else`
        /// was written.
        else_bb: BlockId,
    },
    /// Leaves the enclosing function (`return`, `throw`).
    Return,
    /// Leaves the enclosing loop or switch (`break`, `continue`) — out of
    /// the analyzed block either way.
    Jump,
    /// Falls off the end of the analyzed body.
    End,
}

/// Whether every path through the body leaves it by `Return`/`Jump` —
/// i.e. no path reaches [`Terminator::End`]. This is Rust's "the else
/// block must diverge", answered on the graph instead of by the shape of
/// the last statement.
pub fn diverges(body: &FlowBody) -> bool {
    let mut seen = vec![false; body.blocks.len()];
    let mut stack = vec![body.entry];
    while let Some(BlockId(at)) = stack.pop() {
        let at = at as usize;
        if seen[at] {
            continue;
        }
        seen[at] = true;
        match body.blocks[at].terminator {
            Terminator::End => return false,
            Terminator::Return | Terminator::Jump => {}
            Terminator::Goto(next) => stack.push(next),
            Terminator::Branch { then_bb, else_bb } => {
                stack.push(then_bb);
                stack.push(else_bb);
            }
        }
    }
    true
}

/// Lowers one statement stream (a let-else `else` block's tokens) and
/// answers whether it diverges.
pub(crate) fn block_diverges(src: &str, tokens: &[Token]) -> bool {
    diverges(&lower_block(src, tokens))
}

/// Builds the CFG of one token stream treated as a statement sequence.
pub(crate) fn lower_block(src: &str, tokens: &[Token]) -> FlowBody {
    let stmts = parse_statements(src, tokens);
    let mut builder = Builder { blocks: Vec::new() };
    let end = builder.block(Terminator::End);
    let entry = builder.seq(&stmts, end);
    FlowBody {
        blocks: builder.blocks,
        entry,
    }
}

/// One statement, as far as the flow lowering cares.
#[derive(Debug)]
enum Stmt {
    /// `return`/`throw` — leaves the function.
    Return,
    /// `break`/`continue` — leaves the loop/switch, and the block with it.
    Jump,
    /// `if (…) then [else else]`, chains flattened into the else.
    If {
        then: Vec<Stmt>,
        else_: Option<Vec<Stmt>>,
    },
    /// A bare `{ … }` statement.
    Block(Vec<Stmt>),
    /// Anything else — falls through.
    Other,
}

struct Builder {
    blocks: Vec<BasicBlock>,
}

impl Builder {
    fn block(&mut self, terminator: Terminator) -> BlockId {
        let id = BlockId(u32::try_from(self.blocks.len()).expect("block count fits u32"));
        self.blocks.push(BasicBlock { terminator });
        id
    }

    /// The entry block of `stmts` followed by `follow`.
    fn seq(&mut self, stmts: &[Stmt], follow: BlockId) -> BlockId {
        let Some((first, rest)) = stmts.split_first() else {
            return follow;
        };
        let rest_entry = self.seq(rest, follow);
        match first {
            Stmt::Return => self.block(Terminator::Return),
            Stmt::Jump => self.block(Terminator::Jump),
            Stmt::Other => self.block(Terminator::Goto(rest_entry)),
            Stmt::Block(inner) => self.seq(inner, rest_entry),
            Stmt::If { then, else_ } => {
                let then_bb = self.seq(then, rest_entry);
                let else_bb = match else_ {
                    Some(else_) => self.seq(else_, rest_entry),
                    None => rest_entry,
                };
                self.block(Terminator::Branch { then_bb, else_bb })
            }
        }
    }
}

/// Splits a token stream into top-level statements and classifies each.
///
/// Statements are separated by a top-level `;` or by the `}` of a block
/// statement; an object literal's or arrow body's `}` separates nothing
/// ([`brace_opens_statement`]) — which is what keeps `return { … };` one
/// `return` statement.
fn parse_statements(src: &str, tokens: &[Token]) -> Vec<Stmt> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;
    // Whether the outermost `{` currently open began a statement.
    let mut in_block_stmt = false;
    for (k, t) in tokens.iter().enumerate() {
        match t.kind {
            TokenKind::Punct(b'{') => {
                if depth == 0 {
                    in_block_stmt = brace_opens_statement(src, tokens, start, k);
                }
                depth += 1;
            }
            TokenKind::Punct(b'(' | b'[') => depth += 1,
            TokenKind::Punct(b')' | b']') => depth = depth.saturating_sub(1),
            TokenKind::Punct(b'}') => {
                depth = depth.saturating_sub(1);
                if depth == 0 && in_block_stmt {
                    // An `else` after the `}` continues the same `if`
                    // statement — splitting there would orphan the branch.
                    let continues = matches!(
                        tokens.get(k + 1),
                        Some(t) if matches!(t.kind, TokenKind::Ident)
                            && &src[t.span.start..t.span.end] == "else"
                    );
                    if !continues {
                        out.push(classify(src, tokens, start, k + 1));
                        start = k + 1;
                    }
                }
            }
            TokenKind::Punct(b';') if depth == 0 => {
                // Same continuation rule for a braceless then:
                // `if (c) return 1; else …` is one statement.
                let continues = matches!(
                    tokens.get(k + 1),
                    Some(t) if matches!(t.kind, TokenKind::Ident)
                        && &src[t.span.start..t.span.end] == "else"
                );
                if !continues {
                    out.push(classify(src, tokens, start, k + 1));
                    start = k + 1;
                }
            }
            _ => {}
        }
    }
    if start < tokens.len() {
        out.push(classify(src, tokens, start, tokens.len()));
    }
    out
}

/// Classifies the statement in `tokens[start..end]`.
fn classify(src: &str, tokens: &[Token], start: usize, end: usize) -> Stmt {
    let word = |i: usize| match tokens.get(i) {
        Some(t) if matches!(t.kind, TokenKind::Ident) => Some(&src[t.span.start..t.span.end]),
        _ => None,
    };
    match word(start) {
        Some("return" | "throw") => Stmt::Return,
        Some("break" | "continue") => Stmt::Jump,
        Some("if") => parse_if(src, tokens, start, end).unwrap_or(Stmt::Other),
        _ => {
            // A bare block statement: recurse into it. (An `if`/`for`/…
            // body never lands here — its statement starts at the keyword.)
            if matches!(
                tokens.get(start).map(|t| &t.kind),
                Some(TokenKind::Punct(b'{'))
            ) && let Some(close) = find_close(tokens, start)
                && close < end
            {
                return Stmt::Block(parse_statements(src, &tokens[start + 1..close]));
            }
            Stmt::Other
        }
    }
}

/// Parses `if (…) <embedded> [else <embedded>]` inside `tokens[start..end]`
/// — `start` is the `if`. `None` when the shape is not recognized (then the
/// statement is conservatively [`Stmt::Other`]).
fn parse_if(src: &str, tokens: &[Token], start: usize, end: usize) -> Option<Stmt> {
    if !matches!(
        tokens.get(start + 1).map(|t| &t.kind),
        Some(TokenKind::Punct(b'('))
    ) {
        return None;
    }
    let cond_close = find_close(tokens, start + 1)?;
    let (then, after_then) = parse_embedded(src, tokens, cond_close + 1, end)?;
    let else_ = match tokens.get(after_then) {
        Some(t)
            if after_then < end
                && matches!(t.kind, TokenKind::Ident)
                && &src[t.span.start..t.span.end] == "else" =>
        {
            let (else_stmts, _) = parse_embedded(src, tokens, after_then + 1, end)?;
            Some(else_stmts)
        }
        _ => None,
    };
    Some(Stmt::If { then, else_ })
}

/// One embedded statement position (an `if`'s then or else): a block, a
/// chained `if`, or a single statement running to its top-level `;`.
/// Returns the statements and the index just past them.
fn parse_embedded(
    src: &str,
    tokens: &[Token],
    start: usize,
    end: usize,
) -> Option<(Vec<Stmt>, usize)> {
    if start >= end {
        return None;
    }
    match tokens[start].kind {
        TokenKind::Punct(b'{') => {
            let close = find_close(tokens, start)?;
            if close >= end {
                return None;
            }
            Some((parse_statements(src, &tokens[start + 1..close]), close + 1))
        }
        TokenKind::Ident if &src[tokens[start].span.start..tokens[start].span.end] == "if" => {
            // `else if …` — the chained if runs to the statement's end.
            let stmt = parse_if(src, tokens, start, end)?;
            Some((vec![stmt], end))
        }
        _ => {
            // A single statement: to the first top-level `;`.
            let mut depth = 0usize;
            let mut i = start;
            while i < end {
                match tokens[i].kind {
                    TokenKind::Punct(b'(' | b'[' | b'{') => depth += 1,
                    TokenKind::Punct(b')' | b']' | b'}') => depth = depth.saturating_sub(1),
                    TokenKind::Punct(b';') if depth == 0 => {
                        return Some((vec![classify(src, tokens, start, i + 1)], i + 1));
                    }
                    _ => {}
                }
                i += 1;
            }
            Some((vec![classify(src, tokens, start, end)], end))
        }
    }
}

/// The index of the token closing the bracket at `open`.
fn find_close(tokens: &[Token], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (k, t) in tokens.iter().enumerate().skip(open) {
        match t.kind {
            TokenKind::Punct(b'(' | b'[' | b'{') => depth += 1,
            TokenKind::Punct(b')' | b']' | b'}') => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(k);
                }
            }
            _ => {}
        }
    }
    None
}

/// Words that head a statement whose braces are a *body* (their `}` ends
/// the statement).
const BLOCK_STMT_WORDS: &[&str] = &[
    "if",
    "else",
    "for",
    "while",
    "do",
    "try",
    "catch",
    "finally",
    "switch",
    "function",
    "class",
    "async",
    "declare",
    "namespace",
    "module",
    "interface",
    "enum",
    "with",
];

/// Words a `{` may directly follow while still being an *expression*:
/// `return { … }`, `case { … }`, `await { … }`. Without this,
/// `if (c) return { k: 1 };` would read its object literal as the `if`'s
/// block.
const EXPR_BRACE_WORDS: &[&str] = &[
    "return",
    "throw",
    "case",
    "typeof",
    "instanceof",
    "in",
    "of",
    "new",
    "delete",
    "void",
    "await",
    "yield",
];

/// True when the top-level `{` at `k` opens a statement — a bare block or
/// the body of the statement starting at `last` — rather than an
/// expression's braces. Only the first kind ends a statement when it
/// closes: an object literal or an arrow body leaves its statement running
/// until the `;`.
pub(crate) fn brace_opens_statement(src: &str, tokens: &[Token], last: usize, k: usize) -> bool {
    if k == last {
        return true; // the statement *is* a block
    }
    let word = |i: usize| &src[tokens[i].span.start..tokens[i].span.end];
    if !matches!(tokens[last].kind, TokenKind::Ident) || !BLOCK_STMT_WORDS.contains(&word(last)) {
        return false;
    }
    // Inside such a statement the body brace follows its head: `) {` for
    // the parenthesized ones, a name or the keyword itself for the rest.
    match tokens[k - 1].kind {
        TokenKind::Punct(b')') => true,
        TokenKind::Ident => !EXPR_BRACE_WORDS.contains(&word(k - 1)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(body: &str) -> bool {
        let tokens = crate::lexer::lex(body, 0, body.len());
        block_diverges(body, &tokens)
    }

    #[test]
    fn the_four_keywords_diverge_as_before() {
        assert!(check("return 0;"));
        assert!(check("throw new Error(\"x\");"));
        assert!(check("break;"));
        assert!(check("continue;"));
        assert!(check("log(\"x\"); return 0;"));
        assert!(check("return { k: 1 };"));
    }

    #[test]
    fn plain_statements_do_not() {
        assert!(!check("log(\"x\");"));
        assert!(!check(""));
        assert!(!check("const o = { n: 1 };"));
    }

    #[test]
    fn an_if_needs_both_branches_to_diverge() {
        assert!(!check("if (c) { return 1; }"));
        assert!(check("if (c) { return 1; } else { return 2; }"));
        assert!(check("if (c) return 1; else return 2;"));
        assert!(!check("if (c) { return 1; } else { log(\"x\"); }"));
    }

    #[test]
    fn else_if_chains_are_walked() {
        assert!(check(
            "if (a) { return 1; } else if (b) { throw e; } else { return 2; }"
        ));
        assert!(!check("if (a) { return 1; } else if (b) { throw e; }"));
    }

    #[test]
    fn a_bare_block_diverges_when_its_body_does() {
        assert!(check("{ return 1; }"));
        assert!(!check("{ log(\"x\"); }"));
    }

    #[test]
    fn code_after_a_diverging_statement_is_unreachable_not_a_hole() {
        assert!(check("return 0; log(\"never\");"));
    }

    #[test]
    fn a_functions_return_does_not_leave_the_enclosing_block() {
        assert!(!check("function g() { return 1; }"));
        assert!(!check("const g = () => { return 1; };"));
        // …but a diverging statement after one still counts.
        assert!(check("function g() { return 1; } return g();"));
    }

    #[test]
    fn loops_and_try_are_conservatively_fall_through() {
        assert!(!check("for (;;) { return 1; }"));
        assert!(!check("while (c) { return 1; }"));
        assert!(!check("try { return 1; } catch (e) { return 2; }"));
    }
}
