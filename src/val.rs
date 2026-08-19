//! `val` — the binding modifier and its compile-time mutation analysis.
//!
//! `val` marks a binding (a `const`/`let`/`var` declaration or a function
//! parameter) as **read-only through that binding**: rlc rejects every
//! mutation whose access path is rooted at it. It is not deep immutability
//! and not ownership — the same object reached through another, unmarked
//! binding stays mutable, exactly as in TypeScript. Nothing about `val`
//! survives into the output: the parser lifts the keyword out of the byte
//! stream ([`crate::ast::Segment::ValModifier`]) and codegen emits nothing
//! for it, so `val const x = 1;` compiles to `const x = 1;`.
//!
//! Two halves live here:
//!
//! - [`modifier_at`] — the *structural* rule the parser uses to decide
//!   whether a `val` identifier is a modifier at all. It is deliberately
//!   narrow so the passthrough contract holds: the two shapes it accepts
//!   (`val const|let|var` on one line, and `val <binding>` at the start of
//!   a parameter-list entry) cannot occur in valid TypeScript, so no
//!   working TypeScript file changes meaning.
//! - [`check`] — the *semantic* pass. Unlike [`crate::sema`] it works on
//!   the token stream rather than the AST, because the bindings and the
//!   mutations it reasons about live in passthrough TypeScript, which the
//!   AST deliberately keeps as opaque byte ranges. It tracks lexical
//!   scopes (so an inner declaration shadows an outer `val`), resolves the
//!   root identifier of every mutation path, and checks calls whose callee
//!   is a function declared in the same file.
//!
//! - [`method_calls`] — the *probe* half. A method call through a `val`
//!   path (`items.push(1)`) may or may not mutate: that depends on what
//!   the receiver is, which is a fact about a TypeScript type. rlc does
//!   not guess it from the method's name; it collects the calls as
//!   questions and `rlc --types` answers them with the real checker,
//!   exactly as literal-match exhaustiveness does ([`crate::probe`]).
//!
//! What it does *not* do is inference: no effect system, no borrow
//! checker, no analysis of what an arbitrary imported function does with
//! its argument, and no verdict on a method call it cannot resolve to a
//! built-in. The limits are normative and documented in
//! `docs/reference/language.md` §10.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::error::RlError;
use crate::lexer::{Token, TokenKind, TplPart};
use crate::parser::{dotted_at, find_close_at, is_reserved};

/// What the `val` keyword modifies at a given token index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ValModifier {
    /// `val const|let|var <binding> ...` — a variable declaration.
    Declaration,
    /// `val <binding>` at the start of a parameter-list entry (a function
    /// or method parameter, an arrow parameter, or a `catch` clause).
    Parameter,
}

/// Words that may sit between a parameter-list entry's start and its
/// binding — TypeScript's parameter property modifiers. `val` is allowed
/// after them (`constructor(private val x: X)`).
fn is_param_modifier(word: &str) -> bool {
    matches!(
        word,
        "public" | "private" | "protected" | "readonly" | "override"
    )
}

/// Words that turn `val <word>` into a valid TypeScript expression, so a
/// `val` in front of one is an ordinary identifier and never a modifier:
/// `(val as User)`, `for (val of items)`, `(val in obj)`, ...
fn is_operator_word(word: &str) -> bool {
    is_reserved(word)
        || matches!(
            word,
            "as" | "satisfies"
                | "is"
                | "keyof"
                | "infer"
                | "asserts"
                | "implements"
                | "readonly"
                | "out"
                | "unique"
        )
}

/// True when nothing but spaces and tabs separates the two byte offsets —
/// the "same line" rule. `val` only modifies what follows it on its own
/// line, so an ASI-separated `val\nconst x = 1;` (an expression statement
/// naming a variable `val`, then a declaration — valid TypeScript) keeps
/// its meaning.
fn same_line(src: &str, from: usize, to: usize) -> bool {
    !src[from..to].contains('\n')
}

/// Whether the identifier token at `idx` (which the caller has checked is
/// the word `val`) is an rl binding modifier — see the module docs for the
/// contract argument behind the two accepted shapes.
pub(crate) fn modifier_at(src: &str, tokens: &[Token], idx: usize) -> Option<ValModifier> {
    if dotted_at(tokens, 0, idx) {
        return None; // a property named `val`
    }
    let val = tokens.get(idx)?;
    let next = tokens.get(idx + 1)?;
    if !same_line(src, val.span.end, next.span.start) {
        return None;
    }

    // `val const|let|var ...`
    if let TokenKind::Ident = next.kind {
        let word = &src[next.span.start..next.span.end];
        if matches!(word, "const" | "let" | "var") {
            return Some(ValModifier::Declaration);
        }
    }

    // `( val <binding>` / `, val <binding>`, optionally after TypeScript's
    // parameter property modifiers.
    let binding_follows = match &next.kind {
        TokenKind::Ident => !is_operator_word(&src[next.span.start..next.span.end]),
        TokenKind::Punct(b'{' | b'[') => true,
        _ => false,
    };
    if !binding_follows {
        return None;
    }
    let mut k = idx;
    while k > 0 {
        match &tokens[k - 1].kind {
            TokenKind::Ident
                if is_param_modifier(&src[tokens[k - 1].span.start..tokens[k - 1].span.end]) =>
            {
                k -= 1;
            }
            TokenKind::Punct(b'(' | b',') => return Some(ValModifier::Parameter),
            _ => return None,
        }
    }
    None
}

/// The byte span the parser drops for a `val` modifier: the keyword plus
/// the spaces and tabs right after it, so `val const x` emits `const x`
/// rather than ` const x`. A comment after the keyword is kept.
pub(crate) fn modifier_end(src: &str, keyword_end: usize) -> usize {
    let bytes = src.as_bytes();
    let mut end = keyword_end;
    while end < bytes.len() && (bytes[end] == b' ' || bytes[end] == b'\t') {
        end += 1;
    }
    end
}

/// Method names that *may* name a built-in mutator — the filter deciding
/// which calls are worth asking a type checker about.
///
/// It is a question filter, never a verdict: whether `q.set(k)` mutates
/// depends on what `q` is, and rlc has no types. The verdict is made in
/// `types_host.mjs` (`rlc --types`), which resolves the method's symbol and
/// only reports the one case it can prove — a method TypeScript itself
/// declares on `Array`/`Map`/`Set`/`WeakMap`/`WeakSet`/a TypedArray. A
/// user-defined `set` never reaches that verdict.
///
/// Keep this a **superset** of every name that host judges; a name missing
/// here is a mutation the typed path can no longer see.
fn is_builtin_mutator_name(name: &str) -> bool {
    matches!(
        name,
        "push"
            | "pop"
            | "shift"
            | "unshift"
            | "splice"
            | "sort"
            | "reverse"
            | "fill"
            | "copyWithin"
            | "set"
            | "delete"
            | "clear"
            | "add"
    )
}

/// True when the two tokens touch — how multi-byte operators are told from
/// separate ones (`x.a + = b` is not `+=`, and `x.a == b` is not an
/// assignment).
fn adjacent(a: &Token, b: &Token) -> bool {
    a.span.end == b.span.start
}

fn punct_at(tokens: &[Token], idx: usize, byte: u8) -> bool {
    matches!(tokens.get(idx).map(|t| &t.kind), Some(TokenKind::Punct(b)) if *b == byte)
}

/// The number of tokens forming an assignment operator at `idx`, or `None`
/// when what starts there is not one. Comparisons (`==`, `===`, `>=`,
/// `!=`) and the logical operators are deliberately excluded; the compound
/// forms (`+=`, `**=`, `>>>=`, `&&=`, `||=`, `??=`) are recognized from
/// their touching parts.
fn assignment_op_at(tokens: &[Token], idx: usize) -> Option<usize> {
    let first = tokens.get(idx)?;
    let touching = |k: usize, byte: u8| {
        punct_at(tokens, k, byte)
            && k > idx
            && adjacent(&tokens[k - 1], &tokens[k])
            && (k == idx + 1 || adjacent(&tokens[k - 2], &tokens[k - 1]))
    };
    match &first.kind {
        // `=` alone assigns; `==`/`===` compare.
        TokenKind::Punct(b'=') => {
            if touching(idx + 1, b'=') {
                None
            } else {
                Some(1)
            }
        }
        // `||=` and `??=` — the lexer fuses `||` and `??` into one token.
        TokenKind::OrOr | TokenKind::Coalesce => touching(idx + 1, b'=').then_some(2),
        TokenKind::Punct(op @ (b'+' | b'-' | b'*' | b'/' | b'%' | b'^' | b'&' | b'|')) => {
            // doubled forms: `**=`, `&&=`, `||=` (`||` is fused, so only
            // `&&` reaches here as a pair)
            if touching(idx + 1, *op) {
                return touching(idx + 2, b'=').then_some(3);
            }
            touching(idx + 1, b'=').then_some(2)
        }
        // shifts only — a bare `<=`/`>=` is a comparison
        TokenKind::Punct(op @ (b'<' | b'>')) => {
            if !touching(idx + 1, *op) {
                return None;
            }
            if touching(idx + 2, *op) {
                return touching(idx + 3, b'=').then_some(4); // `>>>=`
            }
            touching(idx + 2, b'=').then_some(3)
        }
        _ => None,
    }
}

/// True when a `++` or `--` operator starts at `idx`.
fn incdec_at(tokens: &[Token], idx: usize) -> bool {
    match tokens.get(idx).map(|t| &t.kind) {
        Some(TokenKind::Punct(op @ (b'+' | b'-'))) => {
            punct_at(tokens, idx + 1, *op) && adjacent(&tokens[idx], &tokens[idx + 1])
        }
        _ => false,
    }
}

/// One access path parsed forward from a root identifier.
struct Path<'a> {
    /// Token index just past the path.
    end: usize,
    /// Number of member steps (`.p`, `?.p`, `[i]`). Zero means the path is
    /// the bare binding, which `val` says nothing about — replacing the
    /// binding's value is `const`'s business, not `val`'s.
    steps: usize,
    /// The last `.p` / `?.p` property name, for the method-call probe.
    last_prop: Option<&'a str>,
    /// The token index that name sits at — the node `rlc --types` asks the
    /// checker to resolve.
    last_prop_tok: Option<usize>,
}

/// Parses the access path rooted at the identifier token `root`.
fn parse_path<'a>(src: &'a str, tokens: &[Token], root: usize) -> Path<'a> {
    let mut path = Path {
        end: root + 1,
        steps: 0,
        last_prop: None,
        last_prop_tok: None,
    };
    loop {
        let j = path.end;
        let member = match tokens.get(j).map(|t| &t.kind) {
            Some(TokenKind::Punct(b'.') | TokenKind::OptChain) => true,
            Some(TokenKind::Punct(b'[')) => {
                let Some(close) = find_close_at(tokens, j) else {
                    return path;
                };
                path.end = close + 1;
                path.steps += 1;
                path.last_prop = None;
                path.last_prop_tok = None;
                continue;
            }
            // a non-null assertion continues the path; `!=` does not
            Some(TokenKind::Punct(b'!')) if !punct_at(tokens, j + 1, b'=') => {
                path.end = j + 1;
                continue;
            }
            _ => false,
        };
        if !member {
            return path;
        }
        match tokens.get(j + 1) {
            Some(t) if matches!(t.kind, TokenKind::Ident) => {
                path.last_prop = Some(&src[t.span.start..t.span.end]);
                path.last_prop_tok = Some(j + 1);
                path.end = j + 2;
                path.steps += 1;
            }
            _ => return path,
        }
    }
}

/// One in-scope binding. `val_at` carries the offset of the `val` keyword
/// that declared it, and `None` marks an ordinary (mutable) binding —
/// which is what makes an inner declaration shadow an outer `val`.
struct Var<'a> {
    name: &'a str,
    val_at: Option<usize>,
}

/// A lexical scope: the bindings it introduces, and the token index at
/// which it ends (its closing brace, or the end of an arrow's expression
/// body).
struct Frame<'a> {
    end: usize,
    vars: Vec<Var<'a>>,
}

/// One parameter of a collected function signature.
#[derive(PartialEq, Eq)]
struct ParamSig {
    /// `None` for a destructuring pattern, which has no single name.
    name: Option<String>,
    is_val: bool,
}

/// One method call made through a `val` binding's access path — a
/// *question*, not a verdict.
///
/// Whether `items.push(1)` mutates depends on what `items` is, so rlc
/// reports nothing on its own: [`crate::val_method_calls`] collects these
/// and `rlc --types` resolves the method against the receiver TypeScript
/// actually inferred, reporting only the calls it can prove land on a
/// built-in mutator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValMethodCall {
    /// Byte offset of the path's root identifier — where the diagnostic is
    /// reported (see [`crate::line_col`]).
    pub offset: usize,
    /// The `val` binding the path is rooted at, for the message.
    pub binding: String,
    /// The method called.
    pub method: String,
    /// Byte offset of the method name in the source. The name is copied
    /// verbatim into the output, so this maps through
    /// [`crate::EmitMapping`]s to the node the checker is asked about.
    pub name: usize,
    /// Byte offset just past the method name.
    pub name_end: usize,
}

/// Runs the `val` analysis over a whole file's token stream. Returns the
/// first violation, like every other rl-level check.
pub(crate) fn check(src: &str, tokens: &[Token]) -> Result<(), RlError> {
    run(src, tokens, None).map(|_| ())
}

/// Collects every candidate built-in mutator call made through a `val`
/// path, in source order — the typed half's input. Never reports an error.
pub(crate) fn method_calls(src: &str, tokens: &[Token]) -> Vec<ValMethodCall> {
    let sink = RefCell::new(Vec::new());
    let _ = run(src, tokens, Some(&sink));
    sink.into_inner()
}

/// The one walk both halves share. With a sink the walk is in probe mode:
/// it reports nothing and collects instead.
fn run(
    src: &str,
    tokens: &[Token],
    calls: Option<&RefCell<Vec<ValMethodCall>>>,
) -> Result<(), RlError> {
    // Files that do not use the modifier — the overwhelming majority —
    // pay one linear scan and nothing else.
    if !uses_val(src, tokens) {
        return Ok(());
    }
    let mut signatures: HashMap<&str, Option<Vec<ParamSig>>> = HashMap::new();
    collect_signatures(src, tokens, &mut signatures);
    let checker = Checker {
        src,
        signatures: &signatures,
        calls,
    };
    let mut frames = vec![Frame {
        end: usize::MAX,
        vars: Vec::new(),
    }];
    checker.walk(tokens, &mut frames)
}

/// Whether the file uses `val` as a binding modifier anywhere.
fn uses_val(src: &str, tokens: &[Token]) -> bool {
    for (i, tok) in tokens.iter().enumerate() {
        match &tok.kind {
            TokenKind::Ident if &src[tok.span.start..tok.span.end] == "val" => {
                if modifier_at(src, tokens, i).is_some() {
                    return true;
                }
            }
            TokenKind::Template(parts) => {
                for part in parts.iter() {
                    if let TplPart::Interp { tokens: inner, .. } = part
                        && uses_val(src, inner)
                    {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

/// Collects the parameter signatures of the file's named functions —
/// `function f(...)`, `const f = (...) => ...`, `const f = function (...)`
/// — which is what makes the call-site capability check possible. A name
/// declared twice with different signatures maps to `None` and is not
/// checked.
fn collect_signatures<'a>(
    src: &'a str,
    tokens: &'a [Token],
    out: &mut HashMap<&'a str, Option<Vec<ParamSig>>>,
) {
    let ident_at = |idx: usize| match tokens.get(idx) {
        Some(t) if matches!(t.kind, TokenKind::Ident) => Some(&src[t.span.start..t.span.end]),
        _ => None,
    };
    for (i, tok) in tokens.iter().enumerate() {
        match &tok.kind {
            TokenKind::Template(parts) => {
                for part in parts.iter() {
                    if let TplPart::Interp { tokens: inner, .. } = part {
                        collect_signatures(src, inner, out);
                    }
                }
            }
            TokenKind::Ident if !dotted_at(tokens, 0, i) => {
                let word = &src[tok.span.start..tok.span.end];
                let found = match word {
                    "function" => {
                        let mut k = i + 1;
                        if punct_at(tokens, k, b'*') {
                            k += 1;
                        }
                        ident_at(k).zip(params_after_name(src, tokens, k + 1))
                    }
                    "const" | "let" | "var" => {
                        let params = declarator_eq(tokens, i + 2)
                            .and_then(|eq| function_value_at(src, tokens, eq));
                        ident_at(i + 1).zip(params)
                    }
                    _ => None,
                };
                if let Some((name, params)) = found {
                    match out.get(name) {
                        Some(Some(prev)) if *prev == params => {}
                        Some(_) => {
                            out.insert(name, None); // ambiguous — stop checking it
                        }
                        None => {
                            out.insert(name, Some(params));
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// The index of a declarator's `=`, when `idx` is the token right after
/// the declared name — the name may carry a type annotation
/// (`const f: Handler = ...`), which is skipped.
fn declarator_eq(tokens: &[Token], idx: usize) -> Option<usize> {
    if assignment_op_at(tokens, idx) == Some(1) {
        return Some(idx);
    }
    if !punct_at(tokens, idx, b':') {
        return None;
    }
    let mut k = idx + 1;
    while k < tokens.len() {
        match &tokens[k].kind {
            TokenKind::Punct(b'(' | b'[' | b'{') => k = find_close_at(tokens, k)? + 1,
            TokenKind::Punct(b')' | b']' | b'}' | b';' | b',') => return None,
            TokenKind::Punct(b'=') if assignment_op_at(tokens, k) == Some(1) => return Some(k),
            _ => k += 1,
        }
    }
    None
}

/// The parameter list of `function name<...>(...)`, when `idx` is the
/// token right after the name.
fn params_after_name(src: &str, tokens: &[Token], idx: usize) -> Option<Vec<ParamSig>> {
    let mut k = idx;
    if punct_at(tokens, k, b'<') {
        k = find_close_at(tokens, k)? + 1;
    }
    if !punct_at(tokens, k, b'(') {
        return None;
    }
    Some(parse_params(src, tokens, k))
}

/// The parameter list of a function *value* — `= (a, b) => ...` or
/// `= function (a, b) { ... }`, `async` included — when `idx` is the `=`
/// of the declarator.
fn function_value_at(src: &str, tokens: &[Token], idx: usize) -> Option<Vec<ParamSig>> {
    if assignment_op_at(tokens, idx) != Some(1) {
        return None;
    }
    let mut k = idx + 1;
    if matches!(tokens.get(k), Some(t) if matches!(t.kind, TokenKind::Ident)
        && &src[t.span.start..t.span.end] == "async")
    {
        k += 1;
    }
    match tokens.get(k) {
        Some(t) if matches!(t.kind, TokenKind::Ident) => {
            if &src[t.span.start..t.span.end] != "function" {
                return None;
            }
            let mut k = k + 1;
            if matches!(tokens.get(k), Some(t) if matches!(t.kind, TokenKind::Ident)) {
                k += 1;
            }
            params_after_name(src, tokens, k)
        }
        // an arrow: the parens must be followed by `=>` or a return type
        Some(t) if matches!(t.kind, TokenKind::Punct(b'(')) => {
            let close = find_close_at(tokens, k)?;
            let arrow = matches!(
                tokens.get(close + 1).map(|t| &t.kind),
                Some(TokenKind::Arrow)
            ) || punct_at(tokens, close + 1, b':');
            arrow.then(|| parse_params(src, tokens, k))
        }
        _ => None,
    }
}

/// Splits a parenthesized list into its top-level entries as token index
/// ranges. `open` is the `(` (or `[`/`{`); the ranges exclude the
/// delimiters.
fn list_entries(tokens: &[Token], open: usize) -> Vec<(usize, usize)> {
    let Some(close) = find_close_at(tokens, open) else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    let mut start = open + 1;
    let mut k = start;
    while k < close {
        match &tokens[k].kind {
            TokenKind::Punct(b'(' | b'[' | b'{') => {
                k = find_close_at(tokens, k).map_or(close, |c| c + 1);
                continue;
            }
            TokenKind::Punct(b',') => {
                if start < k {
                    entries.push((start, k));
                }
                start = k + 1;
            }
            _ => {}
        }
        k += 1;
    }
    if start < close {
        entries.push((start, close));
    }
    entries
}

/// The signature of a parameter list, `open` being its `(`.
fn parse_params(src: &str, tokens: &[Token], open: usize) -> Vec<ParamSig> {
    list_entries(tokens, open)
        .into_iter()
        .map(|(start, end)| {
            let mut k = start;
            let mut is_val = false;
            while k < end {
                match &tokens[k].kind {
                    TokenKind::Ident => {
                        let word = &src[tokens[k].span.start..tokens[k].span.end];
                        if word == "val" && modifier_at(src, tokens, k).is_some() {
                            is_val = true;
                            k += 1;
                            continue;
                        }
                        if is_param_modifier(word) {
                            k += 1;
                            continue;
                        }
                    }
                    // a rest parameter's dots
                    TokenKind::Punct(b'.') => {
                        k += 1;
                        continue;
                    }
                    _ => {}
                }
                break;
            }
            let name = match tokens.get(k) {
                Some(t) if matches!(t.kind, TokenKind::Ident) && k < end => {
                    Some(src[t.span.start..t.span.end].to_string())
                }
                _ => None,
            };
            ParamSig { name, is_val }
        })
        .collect()
}

/// Every name a binding target introduces, collected into `out`. Handles
/// plain identifiers, destructuring patterns (`{ a, b: c, d = 1, ...r }`,
/// `[x, , y]`) and rl let-else patterns (`Tag(a, b: c)`), and returns the
/// token index just past the target.
fn collect_pattern_names<'a>(
    src: &'a str,
    tokens: &[Token],
    start: usize,
    out: &mut Vec<&'a str>,
) -> usize {
    match tokens.get(start).map(|t| &t.kind) {
        // an rl let-else pattern (`const Tag(a, b: c) = ...`) binds the
        // names inside its parens, not the tag
        Some(TokenKind::Ident) if punct_at(tokens, start + 1, b'(') => {
            for (from, to) in list_entries(tokens, start + 1) {
                let aliased = matches!(tokens.get(from).map(|t| &t.kind), Some(TokenKind::Ident))
                    && punct_at(tokens, from + 1, b':')
                    && from + 2 < to;
                collect_pattern_names(src, tokens, if aliased { from + 2 } else { from }, out);
            }
            find_close_at(tokens, start + 1).map_or(start + 1, |c| c + 1)
        }
        Some(TokenKind::Ident) => {
            out.push(&src[tokens[start].span.start..tokens[start].span.end]);
            start + 1
        }
        Some(TokenKind::Punct(open @ (b'{' | b'['))) => {
            let array = *open == b'[';
            let Some(close) = find_close_at(tokens, start) else {
                return start + 1;
            };
            for (from, to) in list_entries(tokens, start) {
                let mut k = from;
                while k < to && punct_at(tokens, k, b'.') {
                    k += 1; // rest element
                }
                // `{ key: <target> }` binds the target, not the key
                if !array
                    && matches!(tokens.get(k).map(|t| &t.kind), Some(TokenKind::Ident))
                    && punct_at(tokens, k + 1, b':')
                {
                    k += 2;
                }
                collect_pattern_names(src, tokens, k, out);
            }
            close + 1
        }
        _ => start,
    }
}

/// The token index just past a declaration's binding targets, collecting
/// every name it introduces — `const a = 1, { b } = o;` introduces both.
/// The scan stops at the statement's `;`, at the end of the enclosing
/// group, or at an `of`/`in` of a `for` head.
/// Whether a declarator really starts at `idx` — the guard on the comma
/// scan in [`collect_decl_names`].
///
/// A `<...>` type-argument list is not a bracket the scanner matches, so a
/// comma inside one (`= new Map<string, number>()`, `: Map<string, number>`)
/// looks like a declarator separator. What follows tells them apart: a real
/// declarator continues with `=`, `:`, `,` or `;`, while a type argument is
/// followed by `>` (or by `[`, `|`, ...). Registering `number` as a binding
/// would be a false positive twice over — it would make a later
/// `number[] = ...` annotation read as a mutation.
fn declarator_at(tokens: &[Token], idx: usize) -> bool {
    match tokens.get(idx).map(|t| &t.kind) {
        // a destructuring pattern
        Some(TokenKind::Punct(b'{' | b'[')) => true,
        Some(TokenKind::Ident) => {
            tokens.get(idx + 1).is_none()
                || matches!(
                    tokens.get(idx + 1).map(|t| &t.kind),
                    Some(TokenKind::Punct(b':' | b',' | b';'))
                )
                || assignment_op_at(tokens, idx + 1) == Some(1)
        }
        _ => false,
    }
}

fn collect_decl_names<'a>(src: &'a str, tokens: &[Token], start: usize) -> Vec<&'a str> {
    let mut names = Vec::new();
    let mut k = start;
    let mut first = true;
    loop {
        if first || declarator_at(tokens, k) {
            k = collect_pattern_names(src, tokens, k, &mut names);
        }
        first = false;
        // skip this declarator's type annotation and initializer
        while k < tokens.len() {
            match &tokens[k].kind {
                TokenKind::Punct(b'(' | b'[' | b'{') => {
                    k = match find_close_at(tokens, k) {
                        Some(c) => c + 1,
                        None => return names,
                    };
                    continue;
                }
                TokenKind::Punct(b')' | b']' | b'}' | b';') => return names,
                TokenKind::Punct(b',') => break,
                _ => k += 1,
            }
        }
        if k >= tokens.len() {
            return names;
        }
        k += 1; // past the comma, on to the next declarator
    }
}

/// The token index just past a statement starting at `from` — a block, or
/// everything up to the next `;` at this bracket depth.
fn statement_end(tokens: &[Token], from: usize) -> usize {
    if punct_at(tokens, from, b'{') {
        return find_close_at(tokens, from).map_or(tokens.len(), |c| c + 1);
    }
    expression_end(tokens, from)
}

/// The token index just past an expression starting at `from`: the first
/// `,` or `;` at this bracket depth, or the closer of the enclosing group.
fn expression_end(tokens: &[Token], from: usize) -> usize {
    let mut k = from;
    while k < tokens.len() {
        match &tokens[k].kind {
            TokenKind::Punct(b'(' | b'[' | b'{') => {
                k = match find_close_at(tokens, k) {
                    Some(c) => c + 1,
                    None => return tokens.len(),
                };
            }
            TokenKind::Punct(b')' | b']' | b'}' | b',' | b';') => return k,
            _ => k += 1,
        }
    }
    tokens.len()
}

/// The `val` checker's walk state.
struct Checker<'a> {
    src: &'a str,
    signatures: &'a HashMap<&'a str, Option<Vec<ParamSig>>>,
    /// Probe mode: collect method calls instead of reporting violations.
    /// The violations are the same either way — the file has already been
    /// checked by the time its probes are collected.
    calls: Option<&'a RefCell<Vec<ValMethodCall>>>,
}

impl<'a> Checker<'a> {
    fn text(&self, tok: &Token) -> &'a str {
        &self.src[tok.span.start..tok.span.end]
    }

    /// The offset of the `val` keyword that declared `name`, or `None`
    /// when the innermost binding of that name is an ordinary one (or
    /// there is none). Innermost wins — that is the shadowing rule.
    fn lookup(&self, frames: &[Frame<'a>], name: &str) -> Option<usize> {
        for frame in frames.iter().rev() {
            if let Some(var) = frame.vars.iter().rev().find(|v| v.name == name) {
                return var.val_at;
            }
        }
        None
    }

    /// Walks one token stream (the file, or one template interpolation),
    /// pushing and popping scopes as it goes. Frames pushed here are
    /// dropped on the way out, so an interpolation cannot leak scopes into
    /// the stream that contains it.
    fn walk(&self, tokens: &'a [Token], frames: &mut Vec<Frame<'a>>) -> Result<(), RlError> {
        let base = frames.len();
        // Parameter scopes, activated when the walk reaches the function
        // body they belong to: (body start, body end, bindings).
        let mut pending: Vec<(usize, usize, Vec<Var<'a>>)> = Vec::new();
        let mut i = 0usize;
        let result = loop {
            if i >= tokens.len() {
                break Ok(());
            }
            while frames.len() > base && frames[frames.len() - 1].end <= i {
                frames.pop();
            }
            while let Some(pos) = pending.iter().position(|(start, _, _)| *start <= i) {
                let (_, end, vars) = pending.remove(pos);
                frames.push(Frame { end, vars });
            }

            let tok = &tokens[i];
            match &tok.kind {
                TokenKind::Template(parts) => {
                    let mut failed = None;
                    for part in parts.iter() {
                        if let TplPart::Interp { tokens: inner, .. } = part
                            && let Err(e) = self.walk(inner, frames)
                        {
                            failed = Some(e);
                            break;
                        }
                    }
                    if let Some(e) = failed {
                        break Err(e);
                    }
                }
                TokenKind::Punct(b'{') => {
                    let end = find_close_at(tokens, i).unwrap_or(tokens.len());
                    frames.push(Frame {
                        end,
                        vars: Vec::new(),
                    });
                }
                TokenKind::Punct(b'(') => {
                    if let Some((start, end, vars)) = self.param_scope(tokens, i) {
                        pending.push((start, end, vars));
                    }
                }
                // prefix `++x.foo` / `--x.foo`
                TokenKind::Punct(b'+' | b'-') if incdec_at(tokens, i) => {
                    if matches!(tokens.get(i + 2).map(|t| &t.kind), Some(TokenKind::Ident))
                        && !dotted_at(tokens, 0, i + 2)
                        && let Err(e) = self.check_mutation(tokens, i + 2, frames, true)
                    {
                        break Err(e);
                    }
                }
                TokenKind::Ident => {
                    match self.visit_ident(tokens, i, frames) {
                        Ok(next) => {
                            i = next;
                            continue;
                        }
                        Err(e) => break Err(e),
                    };
                }
                _ => {}
            }
            i += 1;
        };
        frames.truncate(base);
        result
    }

    /// Handles one identifier token: declarations register bindings, uses
    /// are checked for mutation and for call-site capability. Returns the
    /// index to continue from.
    fn visit_ident(
        &self,
        tokens: &'a [Token],
        i: usize,
        frames: &mut Vec<Frame<'a>>,
    ) -> Result<usize, RlError> {
        let word = self.text(&tokens[i]);
        if dotted_at(tokens, 0, i) {
            return Ok(i + 1);
        }

        if word == "val"
            && let Some(kind) = modifier_at(self.src, tokens, i)
        {
            return match kind {
                // the parameter's scope is registered at its `(`
                ValModifier::Parameter => Ok(i + 1),
                ValModifier::Declaration => {
                    let names = collect_decl_names(self.src, tokens, i + 2);
                    self.declare(frames, names, Some(tokens[i].span.start));
                    Ok(i + 2)
                }
            };
        }

        match word {
            "const" | "let" | "var" => {
                let names = collect_decl_names(self.src, tokens, i + 1);
                self.declare(frames, names, None);
                return Ok(i + 1);
            }
            "function" | "class" => {
                if let Some(t) = tokens.get(i + 1)
                    && matches!(t.kind, TokenKind::Ident)
                {
                    self.declare(frames, vec![self.text(t)], None);
                }
                return Ok(i + 1);
            }
            // a `for` head's bindings belong to the loop, not to the
            // enclosing block
            "for" => {
                if punct_at(tokens, i + 1, b'(')
                    && let Some(close) = find_close_at(tokens, i + 1)
                {
                    frames.push(Frame {
                        end: statement_end(tokens, close + 1),
                        vars: Vec::new(),
                    });
                }
                return Ok(i + 1);
            }
            "delete" => {
                if matches!(tokens.get(i + 1).map(|t| &t.kind), Some(TokenKind::Ident))
                    && !dotted_at(tokens, 0, i + 1)
                {
                    self.check_mutation(tokens, i + 1, frames, true)?;
                }
                return Ok(i + 1);
            }
            _ => {}
        }

        self.check_mutation(tokens, i, frames, false)?;
        if punct_at(tokens, i + 1, b'(')
            && let Some(Some(params)) = self.signatures.get(word)
        {
            self.check_call(tokens, i + 1, word, params, frames)?;
        }
        Ok(i + 1)
    }

    /// Registers bindings in the innermost scope.
    fn declare(&self, frames: &mut [Frame<'a>], names: Vec<&'a str>, val_at: Option<usize>) {
        if let Some(frame) = frames.last_mut() {
            frame
                .vars
                .extend(names.into_iter().map(|name| Var { name, val_at }));
        }
    }

    /// The scope a parameter list opens, when the `(` at `open` really is
    /// one: `(bindings' scope start, scope end, bindings)`. A parameter
    /// list is told from a call or a grouping by what follows its `)` — a
    /// body brace, an arrow, or a return type — and control-flow heads
    /// (`if (`, `while (`, ...) are excluded by the word in front of it.
    /// `catch (e)` *is* a binding form and is deliberately included.
    fn param_scope(
        &self,
        tokens: &'a [Token],
        open: usize,
    ) -> Option<(usize, usize, Vec<Var<'a>>)> {
        if open > 0
            && let TokenKind::Ident = tokens[open - 1].kind
        {
            // A control-flow head (`if (c) { ... }`) or an rl `match` is
            // not a parameter list even though a block follows it.
            // `function`/`async` do introduce one, and `catch (e)` really
            // is a binding form, so those three stay in.
            let word = self.text(&tokens[open - 1]);
            if word == "match"
                || (is_reserved(word) && !matches!(word, "function" | "async" | "catch"))
            {
                return None;
            }
        }
        let close = find_close_at(tokens, open)?;
        let body = self.body_after_params(tokens, close + 1)?;
        let vars = parse_params(self.src, tokens, open)
            .into_iter()
            .zip(list_entries(tokens, open))
            .flat_map(|(param, (start, end))| {
                let val_at = param.is_val.then(|| tokens[start].span.start);
                let mut names = Vec::new();
                let mut k = start;
                while k < end
                    && matches!(&tokens[k].kind, TokenKind::Ident
                        if self.text(&tokens[k]) == "val" || is_param_modifier(self.text(&tokens[k])))
                {
                    k += 1;
                }
                collect_pattern_names(self.src, tokens, k, &mut names);
                names.into_iter().map(move |name| Var { name, val_at })
            })
            .collect();
        Some((body.0, body.1, vars))
    }

    /// The `(start, end)` token range of a function body that follows a
    /// parameter list's `)`, or `None` when what follows is not one (so
    /// the parens were a call or a grouping).
    fn body_after_params(&self, tokens: &[Token], mut k: usize) -> Option<(usize, usize)> {
        if punct_at(tokens, k, b':') {
            // a return type annotation — skip to the body brace or arrow,
            // stepping over object types (`: { a: number } {`)
            k += 1;
            loop {
                match tokens.get(k)?.kind {
                    TokenKind::Arrow => break,
                    TokenKind::Punct(b'{') => {
                        let type_brace = k > 0
                            && matches!(
                                tokens[k - 1].kind,
                                TokenKind::Punct(b':' | b'|' | b'&' | b'<' | b'(' | b'[' | b',')
                            );
                        if !type_brace {
                            break;
                        }
                        k = find_close_at(tokens, k)? + 1;
                    }
                    TokenKind::Punct(b'(' | b'[') => k = find_close_at(tokens, k)? + 1,
                    TokenKind::Punct(b')' | b']' | b'}' | b';' | b',') => return None,
                    _ => k += 1,
                }
            }
        }
        if matches!(tokens.get(k)?.kind, TokenKind::Arrow) {
            k += 1;
        }
        if punct_at(tokens, k, b'{') {
            return Some((k, find_close_at(tokens, k)?));
        }
        // an arrow with an expression body: the scope runs to the end of
        // that expression
        if matches!(
            tokens.get(k.wrapping_sub(1)).map(|t| &t.kind),
            Some(TokenKind::Arrow)
        ) {
            return Some((k, expression_end(tokens, k)));
        }
        None
    }

    /// Reports a mutation through the access path rooted at the identifier
    /// token `root`, when that identifier resolves to a `val` binding.
    /// `mutates` is set by callers that already know the path is being
    /// mutated by an operator *in front* of it (`delete x.p`, `++x.p`);
    /// otherwise the operator after the path decides.
    fn check_mutation(
        &self,
        tokens: &[Token],
        root: usize,
        frames: &[Frame<'a>],
        mutates: bool,
    ) -> Result<(), RlError> {
        let name = self.text(&tokens[root]);
        if self.lookup(frames, name).is_none() {
            return Ok(());
        }
        let path = parse_path(self.src, tokens, root);
        if path.steps == 0 {
            // replacing the binding's value is `const`'s business
            return Ok(());
        }
        let offset = tokens[root].span.start;
        if mutates || assignment_op_at(tokens, path.end).is_some() || incdec_at(tokens, path.end) {
            if self.calls.is_some() {
                return Ok(());
            }
            return Err(RlError::at(
                offset,
                format!(
                    "cannot mutate through val binding `{name}` \
                     (the binding is declared with `val`, so every access path from it is read-only)"
                ),
            ));
        }
        // A method call is a *question*: `q.set(k)` mutates only if `q` is
        // a built-in with a mutating `set`, which needs the receiver's
        // type. rlc never answers it from the name — it records the call
        // for `rlc --types`, where the real checker decides.
        if let Some(sink) = self.calls
            && let (Some(method), Some(tok)) = (path.last_prop, path.last_prop_tok)
            && is_builtin_mutator_name(method)
            && punct_at(tokens, path.end, b'(')
        {
            sink.borrow_mut().push(ValMethodCall {
                offset,
                binding: name.to_string(),
                method: method.to_string(),
                name: tokens[tok].span.start,
                name_end: tokens[tok].span.end,
            });
        }
        Ok(())
    }

    /// Checks the arguments of a call to a function declared in this file:
    /// a `val` binding may only be passed to a parameter that is itself
    /// declared `val`. Only plain access-path arguments carry a decidable
    /// capability; anything computed is left alone.
    fn check_call(
        &self,
        tokens: &[Token],
        open: usize,
        callee: &str,
        params: &[ParamSig],
        frames: &[Frame<'a>],
    ) -> Result<(), RlError> {
        if self.calls.is_some() {
            return Ok(()); // probe mode reports nothing
        }
        for (idx, (start, end)) in list_entries(tokens, open).into_iter().enumerate() {
            if !matches!(tokens[start].kind, TokenKind::Ident) || dotted_at(tokens, 0, start) {
                continue;
            }
            let name = self.text(&tokens[start]);
            let Some(_) = self.lookup(frames, name) else {
                continue;
            };
            // only `x` / `x.y.z` — a computed argument is not a path
            let path = parse_path(self.src, tokens, start);
            if path.end != end || (path.steps > 0 && path.last_prop.is_none()) {
                continue;
            }
            let Some(param) = params.get(idx) else {
                continue;
            };
            if param.is_val {
                continue;
            }
            let described = match &param.name {
                Some(n) => format!("`{n}`"),
                None => format!("#{}", idx + 1),
            };
            return Err(RlError::at(
                tokens[start].span.start,
                format!(
                    "cannot pass val binding `{name}` to mutable parameter {described} of \
                     `{callee}` (the parameter is not declared with `val`, so the function \
                     may mutate through it)"
                ),
            ));
        }
        Ok(())
    }
}
