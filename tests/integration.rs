//! End-to-end tests: compile rl → TypeScript, then run `tsc` to type-check
//! (exhaustiveness is checked by rlc itself; tsc sees plain TypeScript) and `node` to execute.
//!
//! These tests skip silently when `tsc` or `node` is not installed.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use rlc::{Options, compile};

const TSC_FLAGS: &[&str] = &[
    "--strict",
    "--target",
    "es2022",
    "--module",
    "esnext",
    "--moduleResolution",
    "bundler",
    "--skipLibCheck",
];

fn have(cmd: &str) -> bool {
    Command::new(cmd)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

static DIR_SEQ: AtomicUsize = AtomicUsize::new(0);

fn tmpdir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rl-test-{}-{}",
        std::process::id(),
        DIR_SEQ.fetch_add(1, Ordering::SeqCst)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Appended to every snippet so it is a module (like real rl files with
/// exports) — otherwise script-scope names collide with DOM globals
/// such as `Option`.
fn as_module(src: &str) -> String {
    format!("{src}\nexport {{}};\n")
}

/// Compile rl source and type-check the output with tsc. Returns (ok, tsc output).
fn typecheck(src: &str) -> (bool, String) {
    let code = compile(&as_module(src), &Options::default()).expect("rl compile failed");
    let dir = tmpdir();
    let ts = dir.join("main.ts");
    fs::write(&ts, &code).unwrap();
    let out = Command::new("tsc")
        .arg(&ts)
        .arg("--noEmit")
        .args(TSC_FLAGS)
        .output()
        .expect("failed to run tsc");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    (
        out.status.success(),
        format!("{text}\n---compiled---\n{code}"),
    )
}

/// Compile rl source, emit JS with tsc, execute with node, return stdout lines.
fn run(src: &str) -> Vec<String> {
    let code = compile(&as_module(src), &Options::default()).expect("rl compile failed");
    let dir = tmpdir();
    let ts = dir.join("main.ts");
    fs::write(&ts, &code).unwrap();
    // the emitted .js contains `export {}` — run it as an ES module
    fs::write(dir.join("package.json"), "{ \"type\": \"module\" }\n").unwrap();
    let out = Command::new("tsc")
        .arg(&ts)
        .arg("--outDir")
        .arg(&dir)
        .args(TSC_FLAGS)
        .output()
        .expect("failed to run tsc");
    assert!(
        out.status.success(),
        "tsc failed:\n{}\n---compiled---\n{code}",
        String::from_utf8_lossy(&out.stdout)
    );
    let out = Command::new("node")
        .arg(dir.join("main.js"))
        .output()
        .expect("failed to run node");
    assert!(
        out.status.success(),
        "node failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_string)
        .collect()
}

macro_rules! require_toolchain {
    () => {
        if !have("tsc") || !have("node") {
            eprintln!("skipping: tsc/node not available");
            return;
        }
    };
}

/* ------------------------------------------------------------------ */
/* runtime behavior                                                    */
/* ------------------------------------------------------------------ */

#[test]
fn runtime_enum_construction_and_match() {
    require_toolchain!();
    let lines = run(r#"
enum Shape {
  Circle(radius: number),
  Rect(width: number, height: number),
  Point,
}

function area(s: Shape): number {
  return match (s) {
    Circle(radius) => Math.PI * radius * radius,
    Rect(width, height) => width * height,
    Point => 0,
  };
}

console.log(JSON.stringify([area(Shape.Circle(1)), area(Shape.Rect(3, 4)), area(Shape.Point)]));
console.log(JSON.stringify(Shape.Circle(2)));
console.log(JSON.stringify(Shape.Point));
"#);
    assert_eq!(
        lines,
        vec![
            format!("[{},12,0]", std::f64::consts::PI),
            r#"{"kind":"Circle","radius":2}"#.to_string(),
            r#"{"kind":"Point"}"#.to_string(),
        ]
    );
}

#[test]
fn runtime_binding_aliases_and_block_bodies() {
    require_toolchain!();
    let lines = run(r#"
enum Msg {
  Quit,
  Move(x: number, y: number),
  Write(text: string),
}

function describe(m: Msg): string {
  return match (m) {
    Move(x: px, y: py) => {
      const sum = px + py;
      return "move:" + sum;
    },
    Write(text) => "write:" + text,
    Quit => "quit",
  };
}

console.log(describe(Msg.Move(2, 3)));
console.log(describe(Msg.Write("hi")));
console.log(describe(Msg.Quit));
"#);
    assert_eq!(lines, vec!["move:5", "write:hi", "quit"]);
}

#[test]
fn runtime_or_patterns_share_one_body() {
    require_toolchain!();
    let lines = run(r#"
enum Key {
  Enter(),
  Escape,
  Tab,
  Char(ch: string),
}

function action(k: Key): string {
  return match (k) {
    Enter => "submit",
    Escape | Tab => "cancel",
    Char(ch) => "type:" + ch,
  };
}

console.log(action(Key.Enter()));
console.log(action(Key.Escape));
console.log(action(Key.Tab));
console.log(action(Key.Char("z")));
"#);
    assert_eq!(lines, vec!["submit", "cancel", "cancel", "type:z"]);
}

#[test]
fn runtime_match_guards_fall_through_top_to_bottom() {
    require_toolchain!();
    let lines = run(r#"
enum Score {
  Graded(points: number),
  Pending,
}

function grade(s: Score): string {
  return match (s) {
    Graded(points) if points >= 90 => "A",
    Graded(points) if points >= 80 => "B",
    Graded(points) => "F",
    Pending => "-",
  };
}

function tally(s: Score): number {
  return match (s) {
    Graded(points) if points > 0 => {
      const doubled = points * 2;
      return doubled;
    },
    _ => 0,
  };
}

console.log(grade(Score.Graded(95)));
console.log(grade(Score.Graded(85)));
console.log(grade(Score.Graded(10)));
console.log(grade(Score.Pending));
console.log(tally(Score.Graded(3)));
console.log(tally(Score.Graded(-1)));
"#);
    assert_eq!(lines, vec!["A", "B", "F", "-", "6", "0"]);
}

#[test]
fn runtime_generic_enum() {
    require_toolchain!();
    let lines = run(r#"
enum Option<T> {
  Some(value: T),
  None,
}

function unwrapOr<T>(o: Option<T>, fallback: T): T {
  return match (o) {
    Some(value) => value,
    None => fallback,
  };
}

console.log(unwrapOr(Option.Some(7), 0));
console.log(unwrapOr<number>(Option.None, 42));
"#);
    assert_eq!(lines, vec!["7", "42"]);
}

#[test]
fn runtime_async_match_with_await() {
    require_toolchain!();
    let lines = run(r#"
enum Job {
  Fetch(n: number),
  Idle,
}

async function double(n: number): Promise<number> {
  return n * 2;
}

async function runJob(j: Job): Promise<number> {
  return match (j) {
    Fetch(n) => await double(n),
    Idle => 0,
  };
}

runJob(Job.Fetch(21)).then((a) => {
  console.log(a);
  return runJob(Job.Idle);
}).then((b) => {
  console.log(b);
});
"#);
    assert_eq!(lines, vec!["42", "0"]);
}

#[test]
fn runtime_unexpected_case_throws() {
    require_toolchain!();
    // The emitted default branch is a plain runtime guard — it protects when
    // the type system was bypassed (e.g. data from the outside world).
    let lines = run(r#"
enum AB { A(n: number), B }
function f(x: AB): number {
  return match (x) {
    A(n) => n,
    B => 2,
  };
}
const g = f as unknown as (x: { kind: string }) => number;
try {
  g({ kind: "C" });
} catch (e) {
  console.log("threw: " + (e as Error).message);
}
"#);
    assert_eq!(
        lines,
        vec![r#"threw: rl match: unexpected case {"kind":"C"}"#]
    );
}

#[test]
fn runtime_plain_typescript_enum_coexists() {
    require_toolchain!();
    // A unit-only enum is TypeScript's own enum, untouched by rlc.
    let lines = run(r#"
enum Color { Red, Green, Blue }
enum Shape { Circle(radius: number), Point }

console.log(Color.Green);
console.log(Color[Color.Blue]);
console.log(JSON.stringify(Shape.Circle(1)));
"#);
    assert_eq!(lines, vec!["1", "Blue", r#"{"kind":"Circle","radius":1}"#]);
}

#[test]
fn runtime_std_option_result_functional_pipeline() {
    require_toolchain!();
    // Two-file setup: the standard library module next to a compiled rl file
    // that imports it. nodenext resolution so the emitted JS runs under node
    // unchanged (`./rl.js` → tsc reads rl.ts, node loads rl.js).
    let dir = tmpdir();
    fs::write(dir.join("rl.ts"), rlc::STD_SOURCE).unwrap();
    let code = compile(
        r#"
import { Option, Result } from "./rl.js";

function parseNum(raw: string): Result<number, string> {
  const n = Number(raw);
  return Number.isNaN(n) ? Result.Err("not a number: " + raw) : Result.Ok(n);
}

const half = (n: number): Option<number> =>
  n % 2 === 0 ? Option.Some(n / 2) : Option.None;

const describe = (raw: string): string =>
  match (parseNum(raw)) {
    Ok(value) => match (half(value)) {
      Some(value: h) => "half=" + h,
      None => "odd:" + value,
    },
    Err(error) => "error:" + error,
  };

console.log(describe("42"));
console.log(describe("7"));
console.log(describe("x"));
console.log(Option.unwrapOr(Option.map(Option.fromNullable([1, 2].find((n) => n > 1)), (n) => n * 2), -1));
console.log(Result.unwrapOr(Result.andThen(parseNum("10"), (n): Result<number, string> => n > 5 ? Result.Ok(n * 2) : Result.Err("small")), -1));
console.log(Result.isErr(Result.fromThrowable(() => JSON.parse("{"))));
"#,
        &Options::default(),
    )
    .expect("rl compile failed");
    fs::write(dir.join("main.ts"), &code).unwrap();
    fs::write(dir.join("package.json"), "{ \"type\": \"module\" }\n").unwrap();
    let out = Command::new("tsc")
        .arg(dir.join("main.ts"))
        .arg(dir.join("rl.ts"))
        .arg("--outDir")
        .arg(&dir)
        .args([
            "--strict",
            "--target",
            "es2022",
            "--module",
            "nodenext",
            "--moduleResolution",
            "nodenext",
        ])
        .output()
        .expect("failed to run tsc");
    assert!(
        out.status.success(),
        "tsc failed:\n{}\n---compiled---\n{code}",
        String::from_utf8_lossy(&out.stdout)
    );
    let out = Command::new("node")
        .arg(dir.join("main.js"))
        .output()
        .expect("failed to run node");
    assert!(
        out.status.success(),
        "node failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let lines: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(
        lines,
        vec![
            "half=21",
            "odd:7",
            "error:not a number: x",
            "4",
            "20",
            "true"
        ]
    );
}

#[test]
fn runtime_std_new_combinators() {
    require_toolchain!();
    let dir = tmpdir();
    fs::write(dir.join("rl.ts"), rlc::STD_SOURCE).unwrap();
    let code = compile(
        r#"
import { Option, Result } from "./rl.js";

console.log(JSON.stringify(Option.zip(Option.Some(1), Option.Some("a"))));
console.log(JSON.stringify(Option.zip(Option.Some(1), Option.None)));
console.log(JSON.stringify(Option.flatten(Option.Some(Option.Some(2)))));
console.log(JSON.stringify(Option.collect([Option.Some(1), Option.Some(2)])));
console.log(JSON.stringify(Option.collect([Option.Some(1), Option.None])));
console.log(JSON.stringify(Option.transpose(Option.Some(Result.Ok<number, string>(3)))));
console.log(JSON.stringify(Result.collect([Result.Ok(1), Result.Ok(2)])));
console.log(JSON.stringify(Result.collect([Result.Ok<number, string>(1), Result.Err<number, string>("x")])));
console.log(JSON.stringify(Result.flatten(Result.Ok<Result<number, string>, string>(Result.Ok(4)))));
const nested: Result<Option<number>, string> = Result.Ok(Option.None);
console.log(JSON.stringify(Result.transpose(nested)));
Result.fromPromise(Promise.resolve(5))
  .then((r) => console.log(JSON.stringify(r)))
  .then(() => Result.fromPromise(Promise.reject("boom")))
  .then((r) => console.log(JSON.stringify(r)));
"#,
        &Options::default(),
    )
    .expect("rl compile failed");
    fs::write(dir.join("main.ts"), &code).unwrap();
    fs::write(dir.join("package.json"), "{ \"type\": \"module\" }\n").unwrap();
    let out = Command::new("tsc")
        .arg(dir.join("main.ts"))
        .arg(dir.join("rl.ts"))
        .arg("--outDir")
        .arg(&dir)
        .args([
            "--strict",
            "--target",
            "es2022",
            "--module",
            "nodenext",
            "--moduleResolution",
            "nodenext",
        ])
        .output()
        .expect("failed to run tsc");
    assert!(
        out.status.success(),
        "tsc failed:\n{}\n---compiled---\n{code}",
        String::from_utf8_lossy(&out.stdout)
    );
    let out = Command::new("node")
        .arg(dir.join("main.js"))
        .output()
        .expect("failed to run node");
    assert!(
        out.status.success(),
        "node failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let lines: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(
        lines,
        vec![
            r#"{"kind":"Some","value":[1,"a"]}"#,
            r#"{"kind":"None"}"#,
            r#"{"kind":"Some","value":2}"#,
            r#"{"kind":"Some","value":[1,2]}"#,
            r#"{"kind":"None"}"#,
            r#"{"kind":"Ok","value":{"kind":"Some","value":3}}"#,
            r#"{"kind":"Ok","value":[1,2]}"#,
            r#"{"kind":"Err","error":"x"}"#,
            r#"{"kind":"Ok","value":4}"#,
            r#"{"kind":"None"}"#,
            r#"{"kind":"Ok","value":5}"#,
            r#"{"kind":"Err","error":"boom"}"#,
        ]
    );
}

#[test]
fn runtime_try_error_propagation() {
    require_toolchain!();
    let dir = tmpdir();
    fs::write(dir.join("rl.ts"), rlc::STD_SOURCE).unwrap();
    let code = compile(
        r#"
import { Result } from "./rl.js";

function parseNum(raw: string): Result<number, string> {
  const n = Number(raw);
  return Number.isNaN(n) ? Result.Err("not a number: " + raw) : Result.Ok(n);
}

function sumList(raws: string[]): Result<number, string> {
  let total = 0;
  for (const raw of raws) {
    const n = try parseNum(raw);
    total += n;
  }
  return Result.Ok(total);
}

function checked(raw: string): Result<number, string> {
  try parseNum(raw);
  let big: number = try parseNum(raw);
  return Result.Ok(big * 10);
}

console.log(JSON.stringify(sumList(["1", "2", "3"])));
console.log(JSON.stringify(sumList(["1", "x"])));
console.log(JSON.stringify(checked("4")));
"#,
        &Options::default(),
    )
    .expect("rl compile failed");
    fs::write(dir.join("main.ts"), &code).unwrap();
    fs::write(dir.join("package.json"), "{ \"type\": \"module\" }\n").unwrap();
    let out = Command::new("tsc")
        .arg(dir.join("main.ts"))
        .arg(dir.join("rl.ts"))
        .arg("--outDir")
        .arg(&dir)
        .args([
            "--strict",
            "--target",
            "es2022",
            "--module",
            "nodenext",
            "--moduleResolution",
            "nodenext",
        ])
        .output()
        .expect("failed to run tsc");
    assert!(
        out.status.success(),
        "tsc failed:\n{}\n---compiled---\n{code}",
        String::from_utf8_lossy(&out.stdout)
    );
    let out = Command::new("node")
        .arg(dir.join("main.js"))
        .output()
        .expect("failed to run node");
    assert!(
        out.status.success(),
        "node failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let lines: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(
        lines,
        vec![
            r#"{"kind":"Ok","value":6}"#,
            r#"{"kind":"Err","error":"not a number: x"}"#,
            r#"{"kind":"Ok","value":40}"#,
        ]
    );
}

#[test]
fn runtime_let_else_narrows_and_diverges() {
    require_toolchain!();
    // tsc --strict must accept the emitted destructuring: the diverging
    // else block narrows the temporary to the matched case.
    let dir = tmpdir();
    fs::write(dir.join("rl.ts"), rlc::STD_SOURCE).unwrap();
    let code = compile(
        r#"
import { Option, Result } from "./rl.js";

function findUser(id: number): Option<string> {
  return id === 1 ? Option.Some("amy") : Option.None;
}

function greet(id: number): string {
  const Some(value: user) = findUser(id) else { return "who?"; };
  return "hello, " + user;
}

function parseNum(raw: string): Result<number, string> {
  const n = Number(raw);
  return Number.isNaN(n) ? Result.Err("bad") : Result.Ok(n);
}

function double(raw: string): number {
  const Ok(value) = parseNum(raw) else { return -1; };
  return value * 2;
}

console.log(greet(1));
console.log(greet(2));
console.log(double("21"));
console.log(double("x"));
"#,
        &Options::default(),
    )
    .expect("rl compile failed");
    fs::write(dir.join("main.ts"), &code).unwrap();
    fs::write(dir.join("package.json"), "{ \"type\": \"module\" }\n").unwrap();
    let out = Command::new("tsc")
        .arg(dir.join("main.ts"))
        .arg(dir.join("rl.ts"))
        .arg("--outDir")
        .arg(&dir)
        .args([
            "--strict",
            "--target",
            "es2022",
            "--module",
            "nodenext",
            "--moduleResolution",
            "nodenext",
        ])
        .output()
        .expect("failed to run tsc");
    assert!(
        out.status.success(),
        "tsc failed:\n{}\n---compiled---\n{code}",
        String::from_utf8_lossy(&out.stdout)
    );
    let out = Command::new("node")
        .arg(dir.join("main.js"))
        .output()
        .expect("failed to run node");
    assert!(
        out.status.success(),
        "node failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let lines: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(lines, vec!["hello, amy", "who?", "42", "-1"]);
}

/* ------------------------------------------------------------------ */
/* the generated output is plain TypeScript: tsc accepts it            */
/* ------------------------------------------------------------------ */

#[test]
fn typecheck_exhaustive_match_passes() {
    require_toolchain!();
    let (ok, out) = typecheck(
        r#"
enum Shape { Circle(radius: number), Point }
const f = (s: Shape) => match (s) {
  Circle(radius) => radius,
  Point => 0,
};
"#,
    );
    assert!(ok, "{out}");
}

#[test]
fn typecheck_wildcard_makes_partial_match_exhaustive() {
    require_toolchain!();
    let (ok, out) = typecheck(
        r#"
enum Shape { Circle(radius: number), Rect(w: number, h: number), Point }
const f = (s: Shape) => match (s) {
  Circle(radius) => radius,
  _ => 0,
};
"#,
    );
    assert!(ok, "{out}");
}

#[test]
fn typecheck_match_on_handwritten_discriminated_union() {
    require_toolchain!();
    let (ok, out) = typecheck(
        r#"
type AppEvent =
  | { kind: "click"; x: number; y: number }
  | { kind: "key"; code: string };
const f = (e: AppEvent) => match (e) {
  click(x, y) => x + y,
  key(code) => code.length,
};
"#,
    );
    assert!(ok, "{out}");
}

/* ------------------------------------------------------------------ */
/* import specifier rewriting                                          */
/* ------------------------------------------------------------------ */

const ERROR_RL: &str = "export enum CalcError { DivByZero, Overflow(limit: number) }\n";
const MAIN_RL: &str = r#"import { CalcError } from "./error.rl";
const e = CalcError.Overflow(9);
const msg = match (e) {
  Overflow(limit) => `over ${limit}`,
  _ => "other",
};
console.log(msg);
export {};
"#;

#[test]
fn cross_file_rl_import_typechecks_and_runs() {
    require_toolchain!();
    let dir = tmpdir();
    let error_ts = compile(ERROR_RL, &Options::default()).expect("rl compile failed");
    let main_ts = compile(MAIN_RL, &Options::default()).expect("rl compile failed");
    assert!(main_ts.contains("\"./error.js\""), "{main_ts}");
    fs::write(dir.join("error.ts"), &error_ts).unwrap();
    fs::write(dir.join("main.ts"), &main_ts).unwrap();
    fs::write(dir.join("package.json"), "{ \"type\": \"module\" }\n").unwrap();
    let out = Command::new("tsc")
        .arg(dir.join("main.ts"))
        .arg("--outDir")
        .arg(&dir)
        .args(TSC_FLAGS)
        .output()
        .expect("failed to run tsc");
    assert!(
        out.status.success(),
        "tsc failed:\n{}\n---main.ts---\n{main_ts}",
        String::from_utf8_lossy(&out.stdout)
    );
    let out = Command::new("node")
        .arg(dir.join("main.js"))
        .output()
        .expect("failed to run node");
    assert!(
        out.status.success(),
        "node failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "over 9");
}

#[test]
fn cross_file_rl_import_bare_mode_typechecks() {
    require_toolchain!();
    let dir = tmpdir();
    let opts = Options {
        rewrite_imports: rlc::ImportRewrite::Bare,
        ..Options::default()
    };
    let error_ts = compile(ERROR_RL, &opts).expect("rl compile failed");
    let main_ts = compile(MAIN_RL, &opts).expect("rl compile failed");
    assert!(main_ts.contains("\"./error\""), "{main_ts}");
    fs::write(dir.join("error.ts"), &error_ts).unwrap();
    fs::write(dir.join("main.ts"), &main_ts).unwrap();
    // extensionless specifiers need `moduleResolution: bundler` (already in
    // TSC_FLAGS); type-check only — Node ESM cannot resolve them at runtime.
    let out = Command::new("tsc")
        .arg(dir.join("main.ts"))
        .arg("--noEmit")
        .args(TSC_FLAGS)
        .output()
        .expect("failed to run tsc");
    assert!(
        out.status.success(),
        "tsc failed:\n{}\n---main.ts---\n{main_ts}",
        String::from_utf8_lossy(&out.stdout)
    );
}

/* ------------------------------------------------------------------ */
/* project-wide exhaustiveness through the CLI                         */
/* ------------------------------------------------------------------ */

const TOKEN_RL: &str =
    "export enum Token {\n  Num(value: number),\n  Ident(name: string),\n  Eof,\n}\n";

/// Runs the rlc binary itself — declaration collection across files lives
/// in the CLI, not in `compile`. No tsc/node needed.
fn run_rlc(dir: &std::path::Path, args: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_rlc"))
        .current_dir(dir)
        .args(args)
        .output()
        .expect("failed to run rlc");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn cli_checks_exhaustiveness_across_rl_imports() {
    let dir = tmpdir();
    fs::write(dir.join("token.rl"), TOKEN_RL).unwrap();
    fs::write(
        dir.join("parser.rl"),
        "import { Token } from \"./token.rl\";\nconst show = (t: Token) =>\n  match (t) {\n    Num(value) => value,\n    Ident(name) => 0,\n  };\n",
    )
    .unwrap();
    let (ok, err) = run_rlc(&dir, &["--check", "parser.rl"]);
    assert!(!ok, "expected failure:\n{err}");
    assert!(
        err.contains("parser.rl:3:3: match on enum Token (imported from \"./token.rl\") is not exhaustive: missing \"Eof\""),
        "{err}"
    );

    fs::write(
        dir.join("parser.rl"),
        "import { Token } from \"./token.rl\";\nconst show = (t: Token) =>\n  match (t) {\n    Num(value) => value,\n    Ident(name) => 0,\n    Eof => -1,\n  };\n",
    )
    .unwrap();
    let (ok, err) = run_rlc(&dir, &["--check", "parser.rl"]);
    assert!(ok, "expected success:\n{err}");
}

#[test]
fn cli_skips_unresolvable_imports_silently() {
    // A missing module is tsc's problem (TS2307); the match simply stays
    // unchecked, as before phase 2.
    let dir = tmpdir();
    fs::write(
        dir.join("main.rl"),
        "import { Gone } from \"./missing.rl\";\nconst x = match (g) { A(v) => v, B => 0 };\n",
    )
    .unwrap();
    let (ok, err) = run_rlc(&dir, &["--check", "main.rl"]);
    assert!(ok, "expected success:\n{err}");
}

#[test]
fn cli_cross_file_match_runs_end_to_end() {
    require_toolchain!();
    let dir = tmpdir();
    fs::write(dir.join("token.rl"), TOKEN_RL).unwrap();
    fs::write(
        dir.join("main.rl"),
        "import { Token } from \"./token.rl\";\nconst t = Token.Ident(\"x\");\nconsole.log(match (t) {\n  Num(value) => `n${value}`,\n  Ident(name) => `i${name}`,\n  Eof => \"eof\",\n});\nexport {};\n",
    )
    .unwrap();
    let (ok, err) = run_rlc(&dir, &["token.rl", "main.rl"]);
    assert!(ok, "rlc failed:\n{err}");
    fs::write(dir.join("package.json"), "{ \"type\": \"module\" }\n").unwrap();
    let out = Command::new("tsc")
        .arg(dir.join("main.ts"))
        .arg("--outDir")
        .arg(&dir)
        .args(TSC_FLAGS)
        .output()
        .expect("failed to run tsc");
    assert!(
        out.status.success(),
        "tsc failed:\n{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let out = Command::new("node")
        .arg(dir.join("main.js"))
        .output()
        .expect("failed to run node");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "ix");
}
