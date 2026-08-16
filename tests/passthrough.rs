//! Every valid TypeScript file is a valid .rl file and must compile to
//! itself, byte for byte.

use rlc::{compile, Options};

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
fn multibyte_content_preserved() {
    assert_passthrough("const 인사말 = \"안녕하세요 🎉\"; // 한글 주석과 match (x) { A => 1 }\n");
}
