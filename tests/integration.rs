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

/// Whether TypeScript resolves without a project-local copy — true when a
/// global install happens to be reachable, which changes what `--types` can
/// legitimately do.
fn global_typescript_resolvable() -> bool {
    let dir = tmpdir();
    let via_require = Command::new("node")
        .current_dir(&dir)
        .args(["-e", "require(\"typescript\")"])
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false);
    if via_require {
        return true;
    }
    // types_host.mjs also resolves the package that owns a `tsc` on PATH, so
    // a setup where only the binary is reachable succeeds too and must skip.
    std::env::var_os("PATH")
        .map(|path| {
            std::env::split_paths(&path)
                .any(|dir| dir.join("tsc").exists() || dir.join("tsc.cmd").exists())
        })
        .unwrap_or(false)
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

/* ------------------------------------------------------------------ */
/* symbol interface (--symbols)                                        */
/* ------------------------------------------------------------------ */

#[test]
fn symbols_reports_imports_and_positions_as_valid_json() {
    let dir = tmpdir();
    fs::write(dir.join("token.rl"), TOKEN_RL).unwrap();
    fs::write(
        dir.join("parser.rl"),
        "import { Token as Tok } from \"./token.rl\";\nimport { Gone } from \"./missing.rl\";\nenum Local { A(x: number) }\n",
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_rlc"))
        .current_dir(&dir)
        .args(["--symbols", "parser.rl"])
        .output()
        .expect("failed to run rlc");
    assert!(out.status.success());
    let json = String::from_utf8_lossy(&out.stdout).into_owned();

    // Shape: the local enum with its position, the resolved import with the
    // referenced file's exported declarations, and the unresolvable import
    // marked null.
    assert!(json.contains("\"file\":\"parser.rl\""), "{json}");
    assert!(json.contains("\"name\":\"Local\""), "{json}");
    assert!(
        json.contains("\"entries\":[{\"name\":\"Token\",\"alias\":\"Tok\"}]"),
        "{json}"
    );
    assert!(
        json.contains(
            "\"name\":\"Token\",\"exported\":true,\"generics\":\"\",\"line\":1,\"col\":13"
        ),
        "{json}"
    );
    assert!(
        json.contains("\"tag\":\"Eof\",\"line\":4,\"col\":3,\"fields\":null"),
        "{json}"
    );
    assert!(json.contains("\"specifier\":\"./missing.rl\""), "{json}");
    assert!(json.contains("\"resolved\":null,\"enums\":[]"), "{json}");

    // And it must be JSON a real parser accepts.
    if have("node") {
        let mut child = Command::new("node")
            .args([
                "-e",
                "let d='';process.stdin.on('data',c=>d+=c).on('end',()=>JSON.parse(d))",
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .spawn()
            .expect("failed to run node");
        use std::io::Write;
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(json.as_bytes())
            .unwrap();
        assert!(child.wait().unwrap().success(), "not valid JSON:\n{json}");
    }
}

/* ------------------------------------------------------------------ */
/* the unified pipeline through the CLI: build and --types             */
/* ------------------------------------------------------------------ */

const LEVEL_RL: &str = "export enum Level {\n  Low,\n  High(threshold: number),\n}\n";

const NOTICE_RL: &str = "import { Option } from \"@rl/std\";\nimport { Level } from \"./level.rl\";\n\nexport enum Notice {\n  Info(text: string),\n  Warn(text: string, code: number),\n}\n\nexport function render(n: Notice): string {\n  return match (n) {\n    Info(text) => `info: ${text}`,\n    Warn(text, code) => `warn[${code}]: ${text}`,\n  };\n}\n\nexport function gate(l: Level): number {\n  return match (l) {\n    Low => 0,\n    High(threshold) => threshold,\n  };\n}\n\nexport function first(list: Notice[]): Option<Notice> {\n  return list.length > 0 ? Option.Some(list[0]) : Option.None;\n}\n";

const CONSUMER_MAIN_TS: &str = "import { Option } from \"@rl/std\";\nimport { Notice, render, first } from \"./notice.rl\";\n\nconst items = [Notice.Info(\"hello\"), Notice.Warn(\"careful\", 7)];\nfor (const n of items) console.log(render(n));\nconsole.log(Option.isSome(first(items)));\n";

/// A mixed source tree: two `.rl` modules (one importing the other and the
/// standard library) plus a hand-written `.ts` entry that imports `.rl`.
/// Every file under `dir`, recursively.
fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // node_modules is the linked TypeScript, not project output.
        if path.file_name().is_some_and(|name| name == "node_modules") {
            continue;
        }
        if path.is_dir() {
            out.extend(walk(&path));
        } else {
            out.push(path);
        }
    }
    out
}

fn write_consumer_tree(dir: &std::path::Path) {
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join("src/level.rl"), LEVEL_RL).unwrap();
    fs::write(dir.join("src/notice.rl"), NOTICE_RL).unwrap();
    fs::write(dir.join("src/main.ts"), CONSUMER_MAIN_TS).unwrap();
    link_typescript(dir);
}

/// `--types` emits declarations through TypeScript's API, which it resolves
/// from the project — the way ts-node, tsup and vite do. A fixture is a
/// project, so it needs its own `node_modules/typescript`; this links the
/// copy the repository already vendors for the language server.
fn link_typescript(dir: &std::path::Path) {
    let vendored = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("editors/vscode/server/node_modules/typescript");
    if !vendored.exists() {
        return; // the test's toolchain guard reports the skip
    }
    let modules = dir.join("node_modules");
    fs::create_dir_all(&modules).unwrap();
    #[cfg(unix)]
    let _ = std::os::unix::fs::symlink(&vendored, modules.join("typescript"));
    #[cfg(windows)]
    let _ = std::os::windows::fs::symlink_dir(&vendored, modules.join("typescript"));
}

#[test]
fn cli_build_emits_a_complete_tree_that_runs() {
    require_toolchain!();
    let dir = tmpdir();
    write_consumer_tree(&dir);

    let (ok, err) = run_rlc(&dir, &["-o", "build", "--no-banner", "src"]);
    assert!(ok, "build failed:\n{err}");

    // Hand-written TypeScript rides along byte-for-byte except for its
    // relative `.rl` (and `@rl/std`) specifiers.
    let main_ts = fs::read_to_string(dir.join("build/main.ts")).unwrap();
    assert_eq!(
        main_ts,
        CONSUMER_MAIN_TS
            .replace("./notice.rl", "./notice.js")
            .replace("@rl/std", "./rl.js")
    );
    assert!(dir.join("build/rl.ts").exists(), "std not materialized");

    // The emitted tree stands on its own: tsc compiles it, node runs it.
    fs::write(dir.join("build/package.json"), "{ \"type\": \"module\" }\n").unwrap();
    let out = Command::new("tsc")
        .current_dir(&dir)
        .args(["build/main.ts", "--outDir", "build"])
        .args(TSC_FLAGS)
        .output()
        .expect("failed to run tsc");
    assert!(
        out.status.success(),
        "tsc failed:\n{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let out = Command::new("node")
        .current_dir(&dir)
        .arg("build/main.js")
        .output()
        .expect("failed to run node");
    assert!(
        out.status.success(),
        "node failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        ["info: hello", "warn[7]: careful", "true"]
    );
}

#[test]
fn cli_refuses_to_overwrite_a_pass_through_input() {
    let dir = tmpdir();
    fs::write(dir.join("main.ts"), "export const x = 1;\n").unwrap();

    // In place, a pass-through `.ts` would land on top of itself.
    let (ok, err) = run_rlc(&dir, &["main.ts"]);
    assert!(!ok, "expected failure:\n{err}");
    assert!(err.contains("output would overwrite the input"), "{err}");
    let untouched = fs::read_to_string(dir.join("main.ts")).unwrap();
    assert_eq!(untouched, "export const x = 1;\n");

    // A separate output tree is fine.
    let (ok, err) = run_rlc(&dir, &["-o", "out", "main.ts"]);
    assert!(ok, "build failed:\n{err}");
}

#[test]
fn cli_types_leaves_nothing_but_the_sidecars() {
    require_toolchain!();
    let dir = tmpdir();
    write_consumer_tree(&dir);

    let (ok, err) = run_rlc(&dir, &["--types", "src"]);
    assert!(ok, "--types failed:\n{err}");

    // Declaration emit runs in memory: no cache tree, and above all no
    // copy of the hand-written TypeScript anywhere.
    assert!(!dir.join(".rl-build").exists(), "a cache tree was created");
    let copies: Vec<String> = walk(&dir)
        .into_iter()
        .filter(|path| {
            path.file_name().is_some_and(|name| name == "main.ts")
                && !path.starts_with(dir.join("src"))
        })
        .map(|path| path.display().to_string())
        .collect();
    assert!(
        copies.is_empty(),
        "hand-written source was copied: {copies:?}"
    );

    // What it does leave: one sidecar pair per .rl, plus the std types.
    assert!(dir.join(".rl-types/notice.rl.d.ts").exists());
    assert!(dir.join(".rl-types/notice.rl.d.ts.map").exists());
    assert!(dir.join(".rl-types/level.rl.d.ts").exists());
    assert!(dir.join(".rl-types/rl.d.ts").exists());
}

#[test]
fn cli_types_reports_type_errors_but_keeps_the_sidecars_fresh() {
    require_toolchain!();
    let dir = tmpdir();
    write_consumer_tree(&dir);
    // A type error in the consumer, not an rl-level one: declarations are
    // still emitted, so the sidecars must be written and the run must fail.
    fs::write(
        dir.join("src/main.ts"),
        format!("{CONSUMER_MAIN_TS}\nconst wrong: number = \"text\";\n"),
    )
    .unwrap();

    let (ok, err) = run_rlc(&dir, &["--types", "src"]);
    assert!(!ok, "expected a failing exit code:\n{err}");
    assert!(
        err.contains("main.ts"),
        "diagnostic should name the file: {err}"
    );
    assert!(
        dir.join(".rl-types/notice.rl.d.ts").exists(),
        "sidecars should still be written: {err}"
    );
}

#[test]
fn cli_types_without_typescript_says_so() {
    require_toolchain!();
    let dir = tmpdir();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join("src/level.rl"), LEVEL_RL).unwrap();
    // No node_modules/typescript on purpose. A machine with a globally
    // resolvable copy would (correctly) succeed, so skip there.
    if global_typescript_resolvable() {
        eprintln!("skipping: a global TypeScript is resolvable here");
        return;
    }
    let (ok, err) = run_rlc(&dir, &["--types", "src"]);
    assert!(!ok, "expected failure:\n{err}");
    assert!(err.contains("typescript not found"), "{err}");
}

#[test]
fn cli_types_sidecars_typecheck_the_source_tree() {
    require_toolchain!();
    let dir = tmpdir();
    write_consumer_tree(&dir);

    let (ok, err) = run_rlc(&dir, &["--types", "src"]);
    assert!(ok, "--types failed:\n{err}");

    // The declarations keep the *source* specifiers — that is what resolves
    // in the consumer's merged view.
    let sidecar = fs::read_to_string(dir.join(".rl-types/notice.rl.d.ts")).unwrap();
    assert!(sidecar.contains("from \"@rl/std\""), "{sidecar}");
    assert!(sidecar.contains("from \"./level.rl\""), "{sidecar}");
    assert!(
        sidecar.contains("export declare function render"),
        "{sidecar}"
    );
    assert!(dir.join(".rl-types/notice.rl.d.ts.map").exists());
    assert!(dir.join(".rl-types/level.rl.d.ts").exists());
    assert!(dir.join(".rl-types/rl.d.ts").exists(), "std types missing");

    // Round trip: the untouched source tree typechecks once the sidecars
    // are merged in (`rootDirs`) and `@rl/std` is mapped (`paths`).
    fs::write(
        dir.join("tsconfig.json"),
        r#"{
  "compilerOptions": {
    "target": "es2022",
    "module": "preserve",
    "moduleResolution": "bundler",
    "strict": true,
    "skipLibCheck": true,
    "noEmit": true,
    "rootDirs": ["./src", "./.rl-types"],
    "paths": { "@rl/std": ["./.rl-types/rl.d.ts"] }
  },
  "include": ["src"]
}
"#,
    )
    .unwrap();
    let out = Command::new("tsc")
        .current_dir(&dir)
        .args(["-p", "tsconfig.json"])
        .output()
        .expect("failed to run tsc");
    assert!(
        out.status.success(),
        "consumer typecheck failed:\n{}\n---sidecar---\n{sidecar}",
        String::from_utf8_lossy(&out.stdout)
    );
}

/* ------------------------------------------------------------------ */
/* pipeline                                                            */
/* ------------------------------------------------------------------ */

// Inline the curried std combinators so the snippets need no module
// resolution (the std source itself is covered by tests/stdlib.rs).
const PIPE_PRELUDE: &str = r#"
type Option<T> = { kind: "Some"; value: T } | { kind: "None" };
const Option = {
  Some: <T>(value: T): Option<T> => ({ kind: "Some", value }),
  None: { kind: "None" } as const,
  mapP:
    <T, U>(f: (value: T) => U) =>
    (o: Option<T>): Option<U> =>
      o.kind === "Some" ? { kind: "Some", value: f(o.value) } : { kind: "None" },
  unwrapOrP:
    <T>(fallback: T) =>
    (o: Option<T>): T =>
      o.kind === "Some" ? o.value : fallback,
};
const half = (n: number): Option<number> =>
  n % 2 === 0 ? Option.Some(n / 2) : Option.None;
"#;

#[test]
fn pipeline_curried_combinator_steps_infer_without_annotations() {
    require_toolchain!();
    // The whole point of the $rl_ap emission: `x` in the curried step must
    // infer as number (a direct-application emission collapses it to
    // `unknown` — TS18046).
    let (ok, out) = typecheck(&format!(
        "{PIPE_PRELUDE}\nconst label: string = half(4) |> Option.mapP(x => x + 1) |> Option.unwrapOrP(0) |> .toFixed(1);\n"
    ));
    assert!(ok, "{out}");
}

#[test]
fn pipeline_generic_user_functions_instantiate() {
    require_toolchain!();
    // Composing generic functions is where pipe() libraries lose inference;
    // step-by-step application must keep it.
    let (ok, out) = typecheck(
        "const wrap = <T,>(v: T): T[] => [v];\nconst arr: number[][] = 3 |> wrap |> wrap;\n",
    );
    assert!(ok, "{out}");
}

#[test]
fn pipeline_type_error_in_a_step_is_reported_on_user_text() {
    require_toolchain!();
    // A step that is not a unary function is the user's type error — tsc
    // must reject it (rlc emits it untouched).
    let (ok, out) = typecheck("const n: number = 1 |> ((a: string) => a.length);\n");
    assert!(!ok, "{out}");
}

#[test]
fn pipeline_runs_left_to_right() {
    require_toolchain!();
    let lines = run(r#"
const order: string[] = [];
const tap = <T,>(name: string) => (v: T): T => { order.push(name); return v; };
const out = (order.push("head"), 10) |> tap("s1") |> .toFixed(0) |> tap("s2");
console.log(order.join(","), out);
"#);
    assert_eq!(lines, ["head,s1,s2 10"]);
}

#[test]
fn pipeline_await_in_head_runs_in_the_surrounding_async_context() {
    require_toolchain!();
    let lines = run(r#"
const upper = (s: string) => s.toUpperCase();
async function main() {
  const v = await Promise.resolve("ok") |> upper |> .concat("!");
  console.log(v);
}
await main();
"#);
    assert_eq!(lines, ["OK!"]);
}

/* ------------------------------------------------------------------ */
/* tuple match                                                         */
/* ------------------------------------------------------------------ */

#[test]
fn runtime_tuple_match_dispatches_on_the_combination() {
    require_toolchain!();
    let lines = run(r#"
enum Conn { Online(latency: number), Offline }
enum Mode { Auto(), Manual(level: number) }

function decide(c: Conn, m: Mode): number {
  return match (c, m) {
    (Online(latency), Auto) if latency < 50 => 10,
    (Online, Auto) => 5,
    (Online, Manual(level)) => level,
    (Offline, _) => 0,
  };
}

console.log(decide(Conn.Online(10), Mode.Auto()));
console.log(decide(Conn.Online(80), Mode.Auto()));
console.log(decide(Conn.Online(10), Mode.Manual(7)));
console.log(decide(Conn.Offline, Mode.Auto()));
"#);
    assert_eq!(lines, vec!["10", "5", "7", "0"]);
}

#[test]
fn tuple_match_bindings_typecheck_per_position() {
    require_toolchain!();
    let (ok, out) = typecheck(
        r#"
enum Left { A(n: number), B }
enum Right { C(s: string), D }
function f(l: Left, r: Right): string {
  return match (l, r) {
    (A(n), C(s)) => s.repeat(n),
    (A(n), D) => n.toFixed(0),
    (B, C(s)) => s,
    (B, D) => "",
  };
}
"#,
    );
    assert!(ok, "{out}");
}

#[test]
fn tuple_match_scrutinees_evaluate_once_each_left_to_right() {
    require_toolchain!();
    let lines = run(r#"
enum Coin { Heads(), Tails }
const order: string[] = [];
function heads(name: string): Coin { order.push(name); return Coin.Heads(); }
const r = match (heads("a"), heads("b")) {
  (Heads, Heads) => 1,
  _ => 0,
};
console.log(order.join(","), r);
"#);
    assert_eq!(lines, vec!["a,b 1"]);
}

/* ------------------------------------------------------------------ */
/* nested patterns                                                     */
/* ------------------------------------------------------------------ */

#[test]
fn runtime_nested_pattern_falls_through_on_inner_mismatch() {
    require_toolchain!();
    let lines = run(r#"
enum Opt { Some(value: number), None }
enum Res { Ok(value: Opt), Err(error: string) }

function grade(r: Res): string {
  return match (r) {
    Ok(value: Some(value: v)) if v > 9000 => "over",
    Ok(value: Some(value: v)) => "num:" + v,
    Ok(value: None()) => "empty",
    Err(error) => "err:" + error,
    // v1 exhaustiveness: nested arms cover nothing, so `Ok` counts as
    // uncovered without a final wildcard (documented, like guards).
    _ => "unreachable",
  };
}

console.log(grade(Res.Ok(Opt.Some(9001))));
console.log(grade(Res.Ok(Opt.Some(3))));
console.log(grade(Res.Ok(Opt.None)));
console.log(grade(Res.Err("boom")));
"#);
    assert_eq!(lines, vec!["over", "num:3", "empty", "err:boom"]);
}

#[test]
fn nested_pattern_bindings_typecheck_through_the_paths() {
    require_toolchain!();
    // The emitted condition chain must narrow $rl_m.value for the
    // destructuring — no type tricks, plain control-flow analysis.
    let (ok, out) = typecheck(
        r#"
enum Opt { Some(value: number), None }
enum Res { Ok(value: Opt), Err(error: string) }
function f(r: Res): number {
  return match (r) {
    Ok(value: Some(value: v)) => v + 1,
    _ => 0,
  };
}
"#,
    );
    assert!(ok, "{out}");
}
