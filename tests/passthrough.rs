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
