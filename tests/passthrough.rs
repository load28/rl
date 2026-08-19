//! Every valid TypeScript file is a valid .rl file and must compile to
//! itself, byte for byte.

use rlc::{Options, compile};

fn assert_passthrough(src: &str) {
    let out = compile(src, &Options::default()).expect("compile failed");
    assert_eq!(out, src);
}

#[test]
fn string_prototype_match() {
    assert_passthrough("const m = \"abc\".match(/b/);\n");
}

#[test]
fn optional_chaining_match() {
    assert_passthrough("const m = s?.match(re) ?? [];\n");
}

#[test]
fn class_method_named_match() {
    assert_passthrough(
        r#"
class Router {
  match(pathname: string): boolean {
    return this.routes.some((r) => r.test(pathname));
  }
}
"#,
    );
}

#[test]
fn object_method_named_match() {
    assert_passthrough(
        r#"
const matcher = {
  match(s: string) { return s.length > 0; },
};
"#,
    );
}

#[test]
fn interface_member_named_match() {
    assert_passthrough(
        r#"
interface Matcher {
  match(s: string): boolean;
}
"#,
    );
}

#[test]
fn function_named_match() {
    assert_passthrough(
        r#"
function match(a: number, b: number) {
  return a === b;
}
const ok = match(1, 1);
"#,
    );
}

#[test]
fn variable_named_variant() {
    assert_passthrough(
        r#"
const variant = { kind: "a" };
console.log(variant.kind, variant);
"#,
    );
}

#[test]
fn match_inside_string() {
    assert_passthrough("const s = \"match (x) { A => 1 }\";\n");
}

#[test]
fn match_inside_comment() {
    assert_passthrough("// match (x) { A => 1 }\n/* match (y) { B => 2 } */\nconst z = 1;\n");
}

#[test]
fn match_inside_template_chunk() {
    assert_passthrough("const s = `match (x) { A => 1 } and ${1 + 2}`;\n");
}

#[test]
fn regex_containing_braces() {
    assert_passthrough("const re = /match \\(x\\) \\{.*\\}/g;\n");
}

#[test]
fn generics_and_arrows() {
    assert_passthrough(
        r#"
const pick = <T,>(xs: T[], i: number): T | undefined => xs[i];
type Fn = (a: string, b: number) => Map<string, Array<number>>;
"#,
    );
}

#[test]
fn match_property_key() {
    assert_passthrough("const cfg = { match: true, mode: \"all\" };\n");
}

#[test]
fn misc_async_code() {
    assert_passthrough(
        r#"
export async function main(): Promise<void> {
  const data = await fetch("/api").then((r) => r.json());
  switch (data.kind) {
    case "a": break;
    default: break;
  }
}
"#,
    );
}

#[test]
fn ts_numeric_enum() {
    assert_passthrough("enum Direction {\n  Up = 1,\n  Down,\n  Left,\n  Right,\n}\n");
}

#[test]
fn ts_string_enum() {
    assert_passthrough("enum Level {\n  Info = \"INFO\",\n  Warn = \"WARN\",\n}\n");
}

#[test]
fn ts_unit_only_enum() {
    assert_passthrough("enum Color { Red, Green, Blue }\n");
}

#[test]
fn ts_exported_unit_only_enum() {
    assert_passthrough("export enum Color { Red, Green, Blue }\n");
}

#[test]
fn ts_const_enum() {
    assert_passthrough("const enum Flags { None, Read, Write }\n");
}

#[test]
fn ts_declare_enum() {
    assert_passthrough("declare enum Ambient { A, B }\n");
}

#[test]
fn ts_computed_member_enum() {
    assert_passthrough(
        "enum FileAccess {\n  Read = 1 << 1,\n  Write = 1 << 2,\n  ReadWrite = Read | Write,\n}\n",
    );
}

#[test]
fn multibyte_content_preserved() {
    assert_passthrough("const 인사말 = \"안녕하세요 🎉\"; // 한글 주석과 match (x) { A => 1 }\n");
}

#[test]
fn plain_ts_using_option_result_names_is_untouched() {
    // The built-in Option/Result enums must never affect pure TypeScript: a
    // file that works with these names on its own (import, constructors, a
    // switch over the tags) contains no rl syntax and passes through.
    assert_passthrough(
        r#"
import { Option, Result } from "./rl.js";
const o = Option.Some(1);
switch (o.kind) {
  case "Some":
    break;
  case "None":
    break;
}
const r: Result<number, string> = Result.Err("nope");
"#,
    );
}

#[test]
fn bitwise_or_arguments_untouched() {
    // `|` in ordinary expression positions (including a method named
    // `match`) never becomes an or-pattern.
    assert_passthrough("const m = matcher.match(a | b);\nconst flags = READ | WRITE;\n");
}

#[test]
fn ts_try_catch_finally_block() {
    assert_passthrough(
        "try {\n  risky();\n} catch (e) {\n  handle(e);\n} finally {\n  done();\n}\n",
    );
}

#[test]
fn class_field_and_method_named_try() {
    assert_passthrough(
        "class Guard {\n  try = 5;\n  run() {\n    try {\n      this.try += 1;\n    } catch {}\n  }\n}\n",
    );
}

#[test]
fn interface_members_named_try() {
    // Signatures named `try` — including generic and annotation-free ones —
    // must never be taken for an rl try statement.
    assert_passthrough(
        "interface Retryable {\n  try(times: number): void;\n  try2?: () => void;\n}\ninterface Generic {\n  try<T>(x: T);\n}\n",
    );
}

#[test]
fn object_property_and_method_named_try() {
    assert_passthrough(
        "const machine = { try(x: number) { return x + 1; } };\nconst spec = { try: 1 };\nmachine.try(spec.try);\n",
    );
}

#[test]
fn plain_if_else_statement() {
    assert_passthrough(
        "function f(a: boolean): number {\n  if (a) {\n    return 1;\n  } else {\n    return 2;\n  }\n}\n",
    );
}

#[test]
fn function_named_some_called_after_const() {
    // `const Some = ...` has no pattern parens, so it is never a let-else.
    assert_passthrough("const Some = (x: number) => x + 1;\nconst y = Some(2);\n");
}

#[test]
fn object_method_named_const() {
    // A method *named* `const` is followed by `(`, not `<ident>(`.
    assert_passthrough("const machine = { const(x: number) { return x; } };\nmachine.const(1);\n");
}

#[test]
fn import_specifiers_without_rl_extension_untouched() {
    assert_passthrough(
        r#"
import { a } from "./mod.js";
import def from "../other";
import * as ns from "pkg";
export { b } from "./re.ts";
import "polyfill";
"#,
    );
}

#[test]
fn rl_specifier_in_string_comment_and_template_untouched() {
    assert_passthrough(
        "const s = \"import x from './a.rl'\";\n// import y from \"./b.rl\";\nconst t = `from \"./c.rl\"`;\n",
    );
}

#[test]
fn dynamic_import_of_rl_path_untouched() {
    // Dynamic import is out of scope for specifier rewriting.
    assert_passthrough("const m = import(\"./x.rl\");\n");
}

#[test]
fn export_declarations_are_not_reexports() {
    // `export` followed by a declaration must never be scanned for a
    // module specifier, even if a `from` + string appears later.
    assert_passthrough("export const from = 1;\nexport function f() { return \"./x.rl\"; }\n");
}

#[test]
fn bitwise_or_and_unions_are_not_pipelines() {
    assert_passthrough("const a = x | y;\nconst b = x || y;\nconst c = x | y > z;\n");
    assert_passthrough("type U = A | B;\nlet v: string | number = 1;\n");
    assert_passthrough("function f<T extends A | B>(x: T): T | null { return x; }\n");
}

#[test]
fn flow_is_an_ordinary_identifier_in_typescript() {
    // `flow` only means composition at a pipeline head, and a pipeline
    // needs a `|>` — which valid TypeScript cannot contain.
    assert_passthrough("import { flow } from \"fp-ts/function\";\nconst f = flow(g, h);\n");
    assert_passthrough("const flow = 1;\nconst a = flow | mask;\nconst b = o.flow;\n");
    assert_passthrough("function flow<T>(x: T): T { return x; }\ntype flow = number;\n");
}

#[test]
fn pipe_bytes_in_strings_comments_regexes_and_templates_pass_through() {
    assert_passthrough(
        "const s = \"a |> b\";\n// c |> d\n/* e |> f */\nconst r = /\\|>/;\nconst t = `g |> h`;\n",
    );
}

#[test]
fn match_shaped_calls_with_two_arguments_pass_through() {
    // A real function named `match` called with a comma list — no braces
    // follow, so it can never be claimed.
    assert_passthrough("const x = match(a, b);\nobj.match(a, (b, c));\n");
}

#[test]
fn if_statements_and_if_shaped_members_pass_through() {
    assert_passthrough("if (c) { a(); } else if (d) { b(); } else { e(); }\n");
    assert_passthrough("const o = { if: 1 };\ninterface I { if: number }\nobj.if(x);\n");
}

#[test]
fn result_is_an_ordinary_identifier_in_typescript() {
    // `result { ... }` is only claimed when the block carries a Result
    // binding (`const x <- ...;`), which valid TypeScript cannot contain.
    assert_passthrough("const result = compute();\nconsole.log(result);\n");
    assert_passthrough("class result { }\ninterface result { a: number }\ntype result = number;\n");
    assert_passthrough("const o = { result: 1 };\nobj.result(x);\nfoo(result, { a: 1 });\n");
}

#[test]
fn identifier_statement_followed_by_a_block_passes_through() {
    // The ASI shape `result` + newline + block statement is valid (dead)
    // TypeScript — without a binding inside, nothing is claimed.
    assert_passthrough("result\n{\n  const y = 2;\n  console.log(y);\n}\n");
    assert_passthrough("result\n{\n  const c = a < -b;\n}\n");
}

#[test]
fn less_than_negation_passes_through() {
    assert_passthrough("const c = a < -b;\nif (x <-1) { f(); }\nwhile (i <-n) { g(); }\n");
    assert_passthrough("const d = result.a < -1;\nconst e = (a) < (-b);\n");
}

#[test]
fn negative_literal_type_arguments_pass_through() {
    // `let x: Foo<-1>;` is the one valid-TypeScript shape that puts `<-`
    // after a declaration keyword — its tail carries the generic's closing
    // `>`, which an expression cannot, so it is never a Result binding.
    assert_passthrough(
        "type result = { ok: boolean };\nfunction f(): result {\n  let x: Foo<-1>;\n  let y: Map<-1, string>, z: number;\n  return { ok: true };\n}\n",
    );
}

/* ------------------------------------------------------------------ */
/* literal patterns must not claim ordinary TypeScript                 */
/* ------------------------------------------------------------------ */

#[test]
fn switch_over_string_literals() {
    assert_passthrough(
        r#"
function short(dir: "north" | "south") {
  switch (dir) {
    case "north":
      return "N";
    case "south":
      return "S";
  }
}
"#,
    );
}

#[test]
fn call_named_match_followed_by_a_block() {
    assert_passthrough("match(x)\n{ 1 }\n");
}

#[test]
fn object_literal_with_numeric_and_string_keys() {
    assert_passthrough("const table = { 200: \"ok\", \"404\": \"missing\", true: 1 };\n");
}

#[test]
fn arrow_functions_returning_literals() {
    assert_passthrough("const f = (x: number) => 1;\nconst g = () => \"a\";\n");
}

#[test]
fn numeric_literals_of_every_form() {
    assert_passthrough(
        "const a = 0xff;\nconst b = 1_000;\nconst c = 1.5e2;\nconst d = 0b1010;\n\
         const e = 0o17;\nconst f = 10n;\nconst g = -1;\nconst h = .5;\n",
    );
}

#[test]
fn boolean_literals_in_ordinary_positions() {
    assert_passthrough("const t = true;\nconst f = false;\nconst u = { a: true };\n");
}

/* ---- `val` as an ordinary identifier ---- */

#[test]
fn variable_named_val() {
    assert_passthrough("const val = { a: 1 };\nval.a = 2;\nconst n = val.a + 1;\n");
}

#[test]
fn property_and_parameter_named_val() {
    assert_passthrough("const o = { val: 1 };\no.val = 2;\n");
    assert_passthrough("function f(val: number) { return val + 1; }\n");
    assert_passthrough("const g = (val: string) => val.length;\n");
    assert_passthrough("interface I { val: string }\ntype T = { val?: number };\n");
    assert_passthrough("class C { val = 1; getVal() { return this.val; } }\n");
}

#[test]
fn val_followed_by_a_declaration_on_the_next_line() {
    // Two statements separated by ASI: an expression statement naming the
    // variable `val`, then a declaration. `val` only modifies what follows
    // it on the same line, so this keeps its meaning.
    assert_passthrough("let x = 0;\nx = val\nconst y = 1;\n");
    assert_passthrough("val\nconst y = 1;\n");
    assert_passthrough("val;\nconst y = 1;\n");
}

#[test]
fn val_in_front_of_an_operator_word() {
    assert_passthrough("const u = (val as User);\nconst v = (val satisfies User);\n");
    assert_passthrough("for (val of items) { log(val); }\n");
    assert_passthrough("if (val in obj) { log(1); }\nif (val instanceof C) { log(2); }\n");
}

#[test]
fn val_as_a_call_argument_or_element() {
    assert_passthrough("f(val, other);\nconst xs = [val, other];\n");
    assert_passthrough("const m = new Map([[val, 1]]);\n");
    assert_passthrough("arr.reduce((acc, val) => acc + val, 0);\n");
}
