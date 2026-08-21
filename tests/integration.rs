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

/// Type-check a snippet that imports the standard library: the std module is
/// written next to it as `rl.ts` and both files go through tsc (`--noEmit`).
/// Returns (ok, tsc output + compiled source).
fn typecheck_with_std(src: &str) -> (bool, String) {
    let code = compile(&as_module(src), &Options::default()).expect("rl compile failed");
    let dir = tmpdir();
    fs::write(dir.join("rl.ts"), rlc::STD_SOURCE).unwrap();
    fs::write(dir.join("main.ts"), &code).unwrap();
    let out = Command::new("tsc")
        .arg(dir.join("main.ts"))
        .arg(dir.join("rl.ts"))
        .arg("--noEmit")
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

/// Compile a snippet that imports the standard library, emit JS for it and
/// the std module with tsc, execute with node, return stdout lines. The
/// two-file shape mirrors real projects: `./rl.js` in the source resolves to
/// `rl.ts` for tsc and to the emitted `rl.js` for node.
fn run_with_std(src: &str) -> Vec<String> {
    let code = compile(src, &Options::default()).expect("rl compile failed");
    let dir = tmpdir();
    fs::write(dir.join("rl.ts"), rlc::STD_SOURCE).unwrap();
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
    let lines = run_with_std(
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
    );
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
    let lines = run_with_std(
        r#"
import { Option, Result } from "./rl.js";

console.log(JSON.stringify(Option.zip(Option.Some(1), Option.Some("a"))));
console.log(JSON.stringify(Option.zip(Option.Some(1), Option.None)));
console.log(JSON.stringify(Option.flatten(Option.Some(Option.Some(2)))));
console.log(JSON.stringify(Option.collect([Option.Some(1), Option.Some(2)])));
console.log(JSON.stringify(Option.collect([Option.Some(1), Option.None])));
console.log(JSON.stringify(Option.transpose(Option.Some(Result.Ok<number>(3)))));
console.log(JSON.stringify(Result.collect([Result.Ok(1), Result.Ok(2)])));
console.log(JSON.stringify(Result.collect<number, string>([Result.Ok(1), Result.Err("x")])));
console.log(JSON.stringify(Result.flatten<number, string>(Result.Ok(Result.Ok(4)))));
const nested: Result<Option<number>, string> = Result.Ok(Option.None);
console.log(JSON.stringify(Result.transpose(nested)));
Result.fromPromise(Promise.resolve(5))
  .then((r) => console.log(JSON.stringify(r)))
  .then(() => Result.fromPromise(Promise.reject("boom")))
  .then((r) => console.log(JSON.stringify(r)));
"#,
    );
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
    let lines = run_with_std(
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
    );
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
fn runtime_or_patterns_in_let_else_and_if_let() {
    require_toolchain!();
    // tsc --strict must accept both shapes: the let-else guard narrows the
    // temporary to the alternatives' union for the shared destructuring,
    // and the if-let disjunction narrows inside the then-block.
    let lines = run(r#"
enum Shape { Circle(r: number), Square(r: number), Dot }

function side(s: Shape): number {
  const Circle(r) | Square(r) = s else { return 0; };
  return r;
}

function tell(s: Shape): string {
  if let Circle(r) | Square(r) = s {
    return "sized " + r;
  } else {
    return "dot";
  }
}

console.log(side(Shape.Circle(3)));
console.log(side(Shape.Square(4)));
console.log(side(Shape.Dot));
console.log(tell(Shape.Square(5)));
console.log(tell(Shape.Dot));
"#);
    assert_eq!(lines, vec!["3", "4", "0", "sized 5", "dot"]);
}

#[test]
fn runtime_try_inside_an_if_let_body_propagates_from_the_function() {
    require_toolchain!();
    // The if-let body is inline in the enclosing function, so the `try`
    // propagates from `f` — not from any construct in between.
    let lines = run_with_std(
        r#"
import { Option, Result } from "./rl.js";

function parseNum(raw: string): Result<number, string> {
  const n = Number(raw);
  return Number.isNaN(n) ? Result.Err("not a number: " + raw) : Result.Ok(n);
}

function f(o: Option<string>): Result<number, string> {
  if let Some(value) = o {
    const n = try parseNum(value);
    return Result.Ok(n * 10);
  }
  return Result.Ok(-1);
}

console.log(JSON.stringify(f(Option.Some("7"))));
console.log(JSON.stringify(f(Option.Some("x"))));
console.log(JSON.stringify(f(Option.None)));
"#,
    );
    assert_eq!(
        lines,
        vec![
            r#"{"kind":"Ok","value":70}"#,
            r#"{"kind":"Err","error":"not a number: x"}"#,
            r#"{"kind":"Ok","value":-1}"#,
        ]
    );
}

#[test]
fn runtime_try_inside_a_closure_propagates_from_the_closure() {
    require_toolchain!();
    // Rust's `?` inside a closure: the `try` inside the arrow written in a
    // match scrutinee returns from the *arrow*, and the match sees the
    // Result it produced.
    let lines = run_with_std(
        r#"
import { Result } from "./rl.js";

function parseNum(raw: string): Result<number, string> {
  const n = Number(raw);
  return Number.isNaN(n) ? Result.Err("not a number: " + raw) : Result.Ok(n);
}

function describe(raw: string): string {
  return match (((): Result<number, string> => {
    const n = try parseNum(raw);
    return Result.Ok(n * 2);
  })()) {
    Ok(value) => "doubled: " + value,
    Err(error) => "failed: " + error,
  };
}

console.log(describe("21"));
console.log(describe("x"));
"#,
    );
    assert_eq!(lines, vec!["doubled: 42", "failed: not a number: x"]);
}

#[test]
fn runtime_let_else_narrows_and_diverges() {
    require_toolchain!();
    // tsc --strict must accept the emitted destructuring: the diverging
    // else block narrows the temporary to the matched case.
    let lines = run_with_std(
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
    );
    assert_eq!(lines, vec!["hello, amy", "who?", "42", "-1"]);
}

#[test]
fn runtime_let_else_else_block_returns_an_object_literal() {
    require_toolchain!();
    // The natural shape for a `Result`-returning function: the else block
    // propagates an `Err` as an object literal. Its `}` ends no statement,
    // so the divergence check still sees a `return`.
    let lines = run_with_std(
        r#"
import { Option, Result } from "./rl.js";

function findUser(id: number): Option<string> {
  return id === 1 ? Option.Some("amy") : Option.None;
}

function greet(id: number): Result<string, string> {
  const Some(value: user) = findUser(id) else { return { kind: "Err", error: "no user " + id }; };
  return { kind: "Ok", value: "hello, " + user };
}

console.log(JSON.stringify(greet(1)));
console.log(JSON.stringify(greet(2)));
"#,
    );
    assert_eq!(
        lines,
        vec![
            r#"{"kind":"Ok","value":"hello, amy"}"#,
            r#"{"kind":"Err","error":"no user 2"}"#,
        ]
    );
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
fn std_result_constructors_type_only_their_own_variant() {
    require_toolchain!();
    // `Ok` carries no error type and `Err` carries no success type, so each
    // constructor is typed by its own variant — and both still fit a
    // `Result<T, E>` wherever one is expected.
    let (ok, out) = typecheck_with_std(
        r#"
import { Result } from "./rl.js";
import type { Ok, Err } from "./rl.js";

type Exact<A, B> = [A] extends [B] ? ([B] extends [A] ? true : false) : false;

const ok = Result.Ok(123);
const err = Result.Err("bad");
const okIsOkOfNumber: Exact<typeof ok, Ok<number>> = true;
const errIsErrOfString: Exact<typeof err, Err<string>> = true;

const fromOk: Result<number, string> = Result.Ok(1);
const fromErr: Result<number, string> = Result.Err("bad");

function parse(value: string): Result<number, string> {
  if (value.length === 0) {
    return Result.Err("empty");
  }
  return Result.Ok(Number(value));
}

console.log(okIsOkOfNumber, errIsErrOfString, fromOk, fromErr, parse("1"));
"#,
    );
    assert!(ok, "tsc rejected variant-typed constructors:\n{out}");
}

#[test]
fn try_error_types_infer_as_a_union_without_an_annotation() {
    require_toolchain!();
    // Two `try`s over results with different error types: the lowered early
    // returns plus `Result.Ok(...)` give tsc `Err<UserError> | Err<ConfigError>
    // | Ok<Data>`, which is exactly `Result<Data, UserError | ConfigError>`.
    // rlc collects no error types of its own — this is tsc's union inference.
    let (ok, out) = typecheck_with_std(
        r#"
import { Result } from "./rl.js";

type User = { id: number };
type Config = { port: number };
type UserError = { tag: "user" };
type ConfigError = { tag: "config" };

declare function getUser(): Result<User, UserError>;
declare function getConfig(): Result<Config, ConfigError>;

function load() {
  const user = try getUser();
  const config = try getConfig();
  return Result.Ok({ user, config });
}

const loaded: Result<{ user: User; config: Config }, UserError | ConfigError> = load();
console.log(loaded);
"#,
    );
    assert!(ok, "tsc lost the try error union:\n{out}");
}

#[test]
fn try_error_union_stays_checked_against_the_declared_return_type() {
    require_toolchain!();
    // The inference above is not a hole: an annotated function whose `Err`
    // type does not cover a propagated error is still a type error, reported
    // by tsc on the emitted early return.
    let (ok, out) = typecheck_with_std(
        r#"
import { Result } from "./rl.js";

declare function getUser(): Result<number, { tag: "user" }>;

function load(): Result<number, string> {
  const user = try getUser();
  return Result.Ok(user);
}

console.log(load());
"#,
    );
    assert!(!ok, "tsc accepted an uncovered error type:\n{out}");
}

/// Declarations shared by the `andThen` error-union tests: four steps, each
/// failing its own way, so a chain that loses an error type is visible in the
/// asserted union.
const ERROR_UNION_PRELUDE: &str = r#"
import { Result } from "./rl.js";

type Exact<A, B> = [A] extends [B] ? ([B] extends [A] ? true : false) : false;

type User = { id: number };
type Company = { name: string };
type Profile = { title: string };
type ConfigError = { tag: "config" };
type TokenError = { tag: "token" };
type FetchError = { tag: "fetch" };
type ValidationError = { tag: "validation" };

declare function loadConfig(): Result<string, ConfigError>;
declare function loadToken(config: string): Result<User, TokenError>;
declare function getCompany(user: User): Result<Company, FetchError>;
declare function fetchProfile(user: User): Result<Profile, FetchError>;
declare function validateProfile(profile: Profile): Result<Profile, ValidationError>;
"#;

#[test]
fn std_result_and_then_unions_the_two_error_types() {
    require_toolchain!();
    // `andThen` takes the chained function's error type as its own generic,
    // so chaining a `Result<User, TokenError>` with a step that fails with
    // `FetchError` keeps both — no `mapErr` to a common type first.
    let (ok, out) = typecheck_with_std(&format!(
        r#"{ERROR_UNION_PRELUDE}
declare const first: Result<User, TokenError>;

const chained = Result.andThen(first, (user) => getCompany(user));
const exact: Exact<typeof chained, Result<Company, TokenError | FetchError>> = true;

console.log(chained, exact);
"#
    ));
    assert!(ok, "andThen lost an error type:\n{out}");
}

#[test]
fn std_result_and_then_on_a_variant_typed_value_keeps_the_chained_error() {
    require_toolchain!();
    // A value typed as the `Ok<T>` variant alone (what `Result.Ok(...)` and a
    // never-failing function give) offers nothing to infer the incoming `E`
    // from. The `E = never` default is what keeps that case precise instead of
    // collapsing the union to `unknown`.
    let (ok, out) = typecheck_with_std(&format!(
        r#"{ERROR_UNION_PRELUDE}
const chained = Result.andThen(Result.Ok({{ id: 1 }}), (user: User) => fetchProfile(user));
const exact: Exact<typeof chained, Result<Profile, FetchError>> = true;

console.log(chained, exact);
"#
    ));
    assert!(
        ok,
        "andThen on an Ok value lost the chained error type:\n{out}"
    );
}

#[test]
fn std_result_and_then_p_accumulates_error_types_along_a_pipeline() {
    require_toolchain!();
    // The end-to-end shape from the design: `try` collects two error types
    // into the function's inferred return type, and every `andThenP` step
    // adds its own. rlc collects nothing — this is tsc's union inference.
    let (ok, out) = typecheck_with_std(&format!(
        r#"{ERROR_UNION_PRELUDE}
function loadUser() {{
  const config = try loadConfig();
  const token = try loadToken(config);
  return Result.Ok(token);
}}

const profile = loadUser()
  |> Result.andThenP(fetchProfile)
  |> Result.andThenP(validateProfile);

const exact: Exact<
  typeof profile,
  Result<Profile, ConfigError | TokenError | FetchError | ValidationError>
> = true;

console.log(profile, exact);
"#
    ));
    assert!(ok, "the pipeline lost an error type:\n{out}");
}

#[test]
fn std_result_map_p_keeps_the_error_type_it_was_given() {
    require_toolchain!();
    // `map`/`mapP` add no failure of their own, so they carry `E` through
    // unchanged — including a union an earlier `andThenP` accumulated.
    let (ok, out) = typecheck_with_std(&format!(
        r#"{ERROR_UNION_PRELUDE}
declare const first: Result<User, TokenError>;

const title = first
  |> Result.andThenP(fetchProfile)
  |> Result.mapP((profile) => profile.title);

const exact: Exact<typeof title, Result<string, TokenError | FetchError>> = true;

console.log(title, exact);
"#
    ));
    assert!(ok, "mapP changed the error type:\n{out}");
}

#[test]
fn std_result_and_then_p_composes_under_flow() {
    require_toolchain!();
    // `andThenP` returns a function still generic in `E`, so a `flow`
    // composition of two steps stays open at its input end: applying it to a
    // `Result<User, TokenError>` unions that error in too.
    let (ok, out) = typecheck_with_std(&format!(
        r#"{ERROR_UNION_PRELUDE}
declare const first: Result<User, TokenError>;

const pipeline = flow
  |> Result.andThenP(fetchProfile)
  |> Result.andThenP(validateProfile);

const profile = pipeline(first);
const exact: Exact<
  typeof profile,
  Result<Profile, TokenError | FetchError | ValidationError>
> = true;

console.log(profile, exact);
"#
    ));
    assert!(ok, "flow composition lost an error type:\n{out}");
}

#[test]
fn std_result_and_then_p_takes_an_annotated_inline_callback() {
    require_toolchain!();
    // The curried form reads `T` off the chained function, so an inline
    // callback carries its own parameter annotation. A named function (every
    // other test here) needs nothing.
    let (ok, out) = typecheck_with_std(&format!(
        r#"{ERROR_UNION_PRELUDE}
declare const first: Result<User, TokenError>;

const profile = first |> Result.andThenP((user: User) => fetchProfile(user));
const exact: Exact<typeof profile, Result<Profile, TokenError | FetchError>> = true;

console.log(profile, exact);
"#
    ));
    assert!(ok, "an annotated inline callback did not typecheck:\n{out}");
}

#[test]
fn std_result_block_output_pipes_into_and_then_p() {
    require_toolchain!();
    // A `result` block infers the same shape a `try` function does — one `Ok`
    // arm plus one `Err` arm per binding — so its value chains on with its
    // error types intact.
    let (ok, out) = typecheck_with_std(&format!(
        r#"{ERROR_UNION_PRELUDE}
const user = result {{
  const config <- loadConfig();
  const loaded <- loadToken(config);
  loaded
}};

const profile = user |> Result.andThenP(fetchProfile);
const exact: Exact<
  typeof profile,
  Result<Profile, ConfigError | TokenError | FetchError>
> = true;

console.log(profile, exact);
"#
    ));
    assert!(
        ok,
        "a result block lost its error types in a pipeline:\n{out}"
    );
}

#[test]
fn std_result_and_then_error_union_stays_checked_against_an_annotation() {
    require_toolchain!();
    // Accumulating errors is not a hole either: a declared return type that
    // covers only one of the two chained error types is still a tsc error.
    let (ok, out) = typecheck_with_std(&format!(
        r#"{ERROR_UNION_PRELUDE}
declare const first: Result<User, TokenError>;

function chain(): Result<Profile, TokenError> {{
  return Result.andThen(first, (user) => fetchProfile(user));
}}

console.log(chain());
"#
    ));
    assert!(
        !ok,
        "tsc accepted a return type missing an error case:\n{out}"
    );
}

#[test]
fn runtime_result_and_then_chain_short_circuits_on_the_first_err() {
    require_toolchain!();
    // The types changed; the emitted values did not. Both spellings still
    // return the first `Err` untouched and run the rest only on `Ok`.
    let lines = run_with_std(
        r#"
import { Result } from "./rl.js";

type Parsed = { n: number };
type ParseError = { tag: "parse"; raw: string };
type RangeError = { tag: "range"; n: number };

const parse = (raw: string): Result<Parsed, ParseError> =>
  Number.isNaN(Number(raw))
    ? Result.Err({ tag: "parse" as const, raw })
    : Result.Ok({ n: Number(raw) });

const inRange = (p: Parsed): Result<number, RangeError> =>
  p.n <= 10 ? Result.Ok(p.n) : Result.Err({ tag: "range" as const, n: p.n });

const check = (raw: string) => parse(raw) |> Result.andThenP(inRange);

console.log(JSON.stringify(check("4")));
console.log(JSON.stringify(check("40")));
console.log(JSON.stringify(check("x")));
console.log(JSON.stringify(Result.andThen(parse("4"), inRange)));
console.log(JSON.stringify(Result.andThen(parse("x"), inRange)));
"#,
    );
    assert_eq!(
        lines,
        vec![
            r#"{"kind":"Ok","value":4}"#,
            r#"{"kind":"Err","error":{"tag":"range","n":40}}"#,
            r#"{"kind":"Err","error":{"tag":"parse","raw":"x"}}"#,
            r#"{"kind":"Ok","value":4}"#,
            r#"{"kind":"Err","error":{"tag":"parse","raw":"x"}}"#,
        ]
    );
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
    run_rlc_env(dir, args, false)
}

/// [`run_rlc`], optionally with every TypeScript-toolchain variable cleared
/// so rlc resolves nothing — the only way to test what it says when there
/// is no compiler, on a machine that has one.
fn run_rlc_env(dir: &std::path::Path, args: &[&str], no_typescript: bool) -> (bool, String) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rlc"));
    command.current_dir(dir).args(args);
    if no_typescript {
        command
            .env_remove("RLC_TSGO_API")
            .env_remove("RLC_TSGO_BIN")
            .env_remove("RLC_TSGO_ROOT");
    }
    let out = command.output().expect("failed to run rlc");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// Whether rlc can resolve a TypeScript to drive *and* emit declarations
/// with it. Asked by running `--types` over a trivial project and looking
/// for the sidecar: the answer is rlc's own resolution, not a guess about
/// the machine. A released `typescript@7` can check but not emit, so it
/// answers `false` here and the `--types` success tests skip.
fn usable_typescript_for_types() -> bool {
    let dir = tmpdir();
    fs::write(dir.join("probe.rl"), "export const n: number = 1;\n").unwrap();
    let (ok, _) = run_rlc(&dir, &["--types", "probe.rl", "-o", "."]);
    ok && dir.join("probe.rl.d.ts").exists()
}

/// Skip a `--types` success test when no TypeScript that can emit
/// declarations is reachable.
macro_rules! require_types_typescript {
    () => {
        if !usable_typescript_for_types() {
            eprintln!("skipping: no TypeScript for rlc to drive, or it cannot emit declarations");
            return;
        }
    };
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
    require_types_typescript!();
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
    require_types_typescript!();
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
fn cli_types_reports_rl_type_errors_at_the_source_position() {
    require_toolchain!();
    require_types_typescript!();
    let dir = tmpdir();
    write_consumer_tree(&dir);
    // A type error *inside* rl syntax. The emitted TypeScript is a switch
    // IIFE that moves the offending expression far from where it was
    // written, and the file it lives in is never written to disk — the
    // diagnostic has to name `bad.rl` and the source line/column anyway.
    let bad = "import { Result } from \"@rl/std\";\n\
               \n\
               declare function evaluate(): Result<number, string>;\n\
               \n\
               export const bad = evaluate() |> Result.mapP((n) => n.length);\n";
    fs::write(dir.join("src/bad.rl"), bad).unwrap();

    let (ok, err) = run_rlc(&dir, &["--types", "src"]);
    assert!(!ok, "expected a failing exit code:\n{err}");

    let line = err
        .lines()
        .find(|line| line.contains("does not exist on type"))
        .unwrap_or_else(|| panic!("no type error reported:\n{err}"));
    // `length` sits at column 55 of line 5 of the source. The emitted code
    // puts it elsewhere entirely, and there is no `bad.ts` to open.
    assert!(
        line.starts_with("rlc: src/bad.rl:5:55: "),
        "diagnostic should point into the .rl source: {line}"
    );
    assert!(
        !err.contains("bad.ts"),
        "named a file that does not exist: {err}"
    );
}

#[test]
fn cli_types_without_typescript_says_so() {
    require_toolchain!();
    let dir = tmpdir();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join("src/level.rl"), LEVEL_RL).unwrap();
    // No TypeScript on purpose: the environment variables that would name
    // one are cleared, and a temporary directory has no `node_modules` and
    // no sibling typescript-go above it. So this runs everywhere, rather
    // than skipping on any machine that happens to have a compiler.
    let (ok, err) = run_rlc_env(&dir, &["--types", "src"], true);
    assert!(!ok, "expected failure:\n{err}");
    assert!(err.contains("no TypeScript compiler found"), "{err}");
}

#[test]
fn cli_types_sidecars_typecheck_the_source_tree() {
    require_toolchain!();
    require_types_typescript!();
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
fn flow_composition_infers_input_from_its_first_step() {
    require_toolchain!();
    // The composed function's parameter type comes from the first step,
    // and every later step (curried combinator, method step) infers from
    // the previous step's return type — no annotations anywhere.
    let (ok, out) = typecheck(&format!(
        "{PIPE_PRELUDE}\nconst label = flow |> half |> Option.mapP(x => x + 1) \
         |> Option.unwrapOrP(0) |> .toFixed(1);\nconst s: string = label(4);\n"
    ));
    assert!(ok, "{out}");
}

#[test]
fn flow_composition_keeps_the_first_step_arity() {
    require_toolchain!();
    // Composition is emitted with a rest-tuple parameter, so a multi-argument
    // first step stays multi-argument (a unary `flow` type would lose this).
    let (ok, out) = typecheck(
        "const add = (a: number, b: number) => a + b;\nconst f = flow |> add |> ((n: number) => n * 2);\nconst v: number = f(1, 2);\n",
    );
    assert!(ok, "{out}");
}

#[test]
fn flow_composition_input_mismatch_is_a_type_error_on_user_text() {
    require_toolchain!();
    // Calling the composed function with the wrong argument type is the
    // user's error — rlc emits no type tricks that could hide it.
    let (ok, out) = typecheck(
        "const parse = (s: string) => s.length;\nconst f = flow |> parse |> ((n: number) => n + 1);\nconst v = f(3);\n",
    );
    assert!(!ok, "{out}");
}

#[test]
fn flow_composition_runs_left_to_right_when_called() {
    require_toolchain!();
    let lines = run(r#"
const order: string[] = [];
const tap = <T,>(name: string) => (v: T): T => { order.push(name); return v; };
const f = flow |> tap<number>("s1") |> .toFixed(0) |> tap("s2");
console.log(order.join(","), "|", f(10), "|", order.join(","));
"#);
    assert_eq!(lines, [" | 10 | s1,s2"]); // nothing ran until the call
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

/* ------------------------------------------------------------------ */
/* if let                                                              */
/* ------------------------------------------------------------------ */

#[test]
fn runtime_if_let_chains_and_falls_back() {
    require_toolchain!();
    let lines = run(r#"
enum Opt { Some(value: number), None }

function pick(a: Opt, b: Opt): number {
  let out = -1;
  if let Some(value) = a {
    out = value;
  } else if let Some(value) = b {
    out = value * 10;
  } else {
    out = 0;
  }
  return out;
}

console.log(pick(Opt.Some(1), Opt.Some(2)));
console.log(pick(Opt.None, Opt.Some(2)));
console.log(pick(Opt.None, Opt.None));
"#);
    assert_eq!(lines, vec!["1", "20", "0"]);
}

#[test]
fn if_let_bindings_stay_narrowed_inside_closures() {
    require_toolchain!();
    // The binding materializes as a const, so the narrowed type survives
    // closure boundaries — the gap that motivated the feature (TASK-042 G5).
    let (ok, out) = typecheck(
        r#"
enum Opt { Some(value: string), None }
function f(o: Opt, xs: number[]): string[] {
  const collected: string[] = [];
  if let Some(value) = o {
    xs.forEach(() => collected.push(value.toUpperCase()));
  }
  return collected;
}
"#,
    );
    assert!(ok, "{out}");
}

/* ------------------------------------------------------------------ */
/* result computation block                                            */
/* ------------------------------------------------------------------ */

/// rl enums in exactly the shape `@rl/std`'s `Result` has, so the block
/// tests need no module setup.
const RESULT_PRELUDE: &str = r#"
enum Res<T, E> { Ok(value: T), Err(error: E) }
enum UserError { NoUser() }
enum CompanyError { NoCompany(id: number) }
type User = { id: number; name: string; companyId: number };
type Company = { id: number; name: string };
declare function getUser(id: number): Res<User, UserError>;
declare function getCompany(id: number): Res<Company, CompanyError>;
"#;

#[test]
fn result_block_unions_the_error_types_of_its_bindings() {
    require_toolchain!();
    // The whole error-type question: two bindings with different error
    // types must produce `Res<_, UserError | CompanyError>` with no help
    // from rlc and no change to the combinators' signatures.
    let (ok, out) = typecheck(&format!(
        r#"{RESULT_PRELUDE}
const view = (id: number): Res<string, UserError | CompanyError> => result {{
  const user <- getUser(id);
  const company <- getCompany(user.companyId);
  user.name + "@" + company.name
}};
"#
    ));
    assert!(ok, "{out}");
}

#[test]
fn result_block_missing_an_error_type_is_a_type_error() {
    require_toolchain!();
    // The other half: an annotation that forgets one binding's error type
    // is tsc's error, reported on the user's own annotation.
    let (ok, out) = typecheck(&format!(
        r#"{RESULT_PRELUDE}
const view = (id: number): Res<string, UserError> => result {{
  const user <- getUser(id);
  const company <- getCompany(user.companyId);
  user.name + "@" + company.name
}};
"#
    ));
    assert!(!ok, "{out}");
}

#[test]
fn result_block_bindings_are_narrowed_success_values() {
    require_toolchain!();
    // No annotations anywhere: each binding must be the `Ok` payload type,
    // and the block's value type must flow out of the block.
    let (ok, out) = typecheck(&format!(
        r#"{RESULT_PRELUDE}
const view = (id: number) => result {{
  const user <- getUser(id);
  const company <- getCompany(user.companyId);
  const label: string = user.name.toUpperCase() + company.name;
  {{ user, company, label }}
}};
const check = (id: number): string => match (view(id)) {{
  Ok(value) => value.label,
  Err(error) => match (error) {{
    NoUser => "no user",
    NoCompany(id: missing) => "no company " + missing,
  }},
}};
"#
    ));
    assert!(ok, "{out}");
}

#[test]
fn runtime_result_block_short_circuits_on_the_first_err() {
    require_toolchain!();
    let lines = run(r#"
enum Res<T, E> { Ok(value: T), Err(error: E) }

const steps: string[] = [];
const step = (name: string, ok: boolean): Res<string, string> => {
  steps.push(name);
  return ok ? Res.Ok(name) : Res.Err("failed:" + name);
};

const chain = (secondOk: boolean) => result {
  const a <- step("a", true);
  const b <- step("b", secondOk);
  const c <- step("c", true);
  a + b + c
};

console.log(JSON.stringify(chain(true)), steps.join(","));
steps.length = 0;
console.log(JSON.stringify(chain(false)), steps.join(","));
"#);
    assert_eq!(
        lines,
        vec![
            r#"{"kind":"Ok","value":"abc"} a,b,c"#,
            r#"{"kind":"Err","error":"failed:b"} a,b"#,
        ]
    );
}

#[test]
fn runtime_result_block_with_await_resolves_to_a_result() {
    require_toolchain!();
    let lines = run(r#"
enum Res<T, E> { Ok(value: T), Err(error: E) }

const fetchNum = async (n: number): Promise<Res<number, string>> =>
  n > 0 ? Res.Ok(n) : Res.Err("not positive");

const total = async (a: number, b: number) => result {
  const x <- await fetchNum(a);
  const y <- await fetchNum(b);
  x + y
};

total(2, 3).then((r) => console.log(JSON.stringify(r)));
total(2, -1).then((r) => console.log(JSON.stringify(r)));
"#);
    assert_eq!(
        lines,
        vec![
            r#"{"kind":"Ok","value":5}"#,
            r#"{"kind":"Err","error":"not positive"}"#,
        ]
    );
}

#[test]
fn runtime_result_block_replaces_nested_combinator_callbacks() {
    require_toolchain!();
    // The motivating shape: three dependent steps that all stay in scope,
    // written flat, against the real standard library.
    let lines = run_with_std(
        r#"
import { Result } from "./rl.js";

type User = { id: number; companyId: number; name: string };
type Company = { id: number; name: string };

const getUser = (id: number): Result<User, string> =>
  id === 1 ? Result.Ok({ id, companyId: 7, name: " Ada " }) : Result.Err("no user " + id);
const getCompany = (id: number): Result<Company, string> =>
  Result.Ok({ id, name: "Acme" });
const getPermission = (u: User, c: Company): Result<string, string> =>
  Result.Ok(u.name.trim() + "@" + c.name);

const view = (id: number) => result {
  const user <- getUser(id);
  const company <- getCompany(user.companyId);
  const normalized = user.name |> .trim() |> .toLowerCase();
  const permission <- getPermission(user, company);
  { user, company, permission, normalized }
};

console.log(JSON.stringify(view(1)));
console.log(JSON.stringify(view(2)));
"#,
    );
    assert_eq!(
        lines,
        vec![
            r#"{"kind":"Ok","value":{"user":{"id":1,"companyId":7,"name":" Ada "},"company":{"id":7,"name":"Acme"},"permission":"Ada@Acme","normalized":"ada"}}"#,
            r#"{"kind":"Err","error":"no user 2"}"#,
        ]
    );
}

/* ------------------------------------------------------------------ */
/* literal match patterns                                              */
/* ------------------------------------------------------------------ */

#[test]
fn runtime_literal_string_match() {
    require_toolchain!();
    let lines = run(r#"
type Direction = "north" | "south" | "east" | "west";

function short(dir: Direction) {
  return match (dir) {
    "north" => "N",
    "south" => "S",
    "east" => "E",
    "west" => "W",
  };
}

console.log(short("north"), short("south"), short("east"), short("west"));
"#);
    assert_eq!(lines, ["N S E W"]);
}

#[test]
fn runtime_literal_number_match_with_or_patterns() {
    require_toolchain!();
    let lines = run(r#"
function status(code: 200 | 201 | 404 | 500) {
  return match (code) {
    200 | 201 => "success",
    404 => "not found",
    500 => "server error",
  };
}

console.log(status(200), status(201), status(404), status(500));
"#);
    assert_eq!(lines, ["success success not found server error"]);
}

#[test]
fn runtime_literal_boolean_match() {
    require_toolchain!();
    let lines = run(r#"
function label(flag: boolean) {
  return match (flag) {
    true => "yes",
    false => "no",
  };
}

console.log(label(true), label(false));
"#);
    assert_eq!(lines, ["yes no"]);
}

#[test]
fn runtime_literal_match_keeps_number_spellings() {
    require_toolchain!();
    let lines = run(r#"
function pick(n: number) {
  return match (n) {
    0xff => "hex",
    1_000 => "sep",
    1.5e2 => "exp",
    -1 => "neg",
    _ => "other",
  };
}

console.log(pick(255), pick(1000), pick(150), pick(-1), pick(0));
"#);
    assert_eq!(lines, ["hex sep exp neg other"]);
}

#[test]
fn runtime_literal_match_evaluates_the_scrutinee_once() {
    require_toolchain!();
    let lines = run(r#"
let calls = 0;
function getValue(): string {
  calls += 1;
  return "b";
}

const picked = match (getValue()) {
  "a" => 1,
  "b" => 2,
  _ => 3,
};
console.log(picked, calls);
"#);
    assert_eq!(lines, ["2 1"]);
}

#[test]
fn runtime_literal_match_runtime_guard_throws() {
    require_toolchain!();
    let lines = run(r#"
function label(dir: string) {
  return match (dir as "a" | "b") {
    "a" => 1,
    "b" => 2,
  };
}

try {
  label("zzz");
  console.log("no throw");
} catch (e) {
  console.log((e as Error).message);
}
"#);
    assert_eq!(lines, [r#"rl match: unexpected literal "zzz""#]);
}

#[test]
fn runtime_literal_match_with_guard() {
    require_toolchain!();
    let lines = run(r#"
function classify(code: number, retry: boolean) {
  return match (code) {
    500 if retry => "retrying",
    500 => "failed",
    _ => "ok",
  };
}

console.log(classify(500, true), classify(500, false), classify(200, true));
"#);
    assert_eq!(lines, ["retrying failed ok"]);
}

#[test]
fn typecheck_literal_match_narrows_each_arm() {
    require_toolchain!();
    // The switch discriminates on the value itself, so tsc narrows the
    // scrutinee inside each arm with no type tricks.
    let (ok, out) = typecheck(
        r#"
type Size = "sm" | "md" | "lg";
const px: number = match ("sm" as Size) {
  "sm" => 12,
  "md" => 16,
  "lg" => 20,
};
"#,
    );
    assert!(ok, "{out}");
}

#[test]
fn typecheck_literal_match_block_bodies() {
    require_toolchain!();
    let (ok, out) = typecheck(
        r#"
const label: string = match ("a" as "a" | "b") {
  "a" => { return "first"; },
  "b" => { return "second"; },
};
"#,
    );
    assert!(ok, "{out}");
}

/* ------------------------------------------------------------------ */
/* val — binding modifier                                             */
/* ------------------------------------------------------------------ */

#[test]
fn typecheck_val_bindings_are_plain_typescript() {
    require_toolchain!();
    // `val` is compile-time only: what reaches tsc is an ordinary
    // declaration and an ordinary parameter, with no readonly types and
    // no runtime helper.
    let (ok, out) = typecheck(
        r#"
type User = { name: string; tags: string[] };

val const user: User = { name: "Kim", tags: ["dev"] };

function inspect(val u: User): string {
  return u.name + u.tags.length;
}

val let state = { count: 0 };
state = { ...state, count: state.count + 1 };

const label = inspect(user) + state.count;
"#,
    );
    assert!(ok, "{out}");
    assert!(
        !out.contains("val "),
        "the modifier leaked into the output: {out}"
    );
    assert!(!out.contains("readonly"), "{out}");
}

#[test]
fn run_val_program_behaves_exactly_like_the_typescript_it_erases_to() {
    require_toolchain!();
    let lines = run(r#"
val const config = { name: "rl", tags: ["dev"] };
val let state = { count: 0 };

function describe(val c: { name: string; tags: string[] }): string {
  return `${c.name}:${c.tags.length}`;
}

function bump(s: { count: number }) {
  s.count += 1;
  return s;
}

state = { count: state.count + 1 };
const mutable = { count: 0 };
bump(mutable);

console.log(describe(config));
console.log(String(state.count));
console.log(String(mutable.count));
"#);
    assert_eq!(lines, ["rl:1", "1", "1"]);
}
