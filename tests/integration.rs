//! End-to-end tests: compile rl → TypeScript, then run `tsc` to type-check
//! (including the exhaustiveness guarantee) and `node` to execute.
//!
//! These tests skip silently when `tsc` or `node` is not installed.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use rlc::{compile, Options};

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
    (out.status.success(), format!("{text}\n---compiled---\n{code}"))
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
fn runtime_variant_construction_and_match() {
    require_toolchain!();
    let lines = run(r#"
variant Shape {
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
variant Msg {
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
fn runtime_generic_variant() {
    require_toolchain!();
    let lines = run(r#"
variant Option<T> {
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
variant Job {
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
fn runtime_unhandled_variant_throws() {
    require_toolchain!();
    let lines = run(r#"
variant AB { A, B }
function f(x: AB): number {
  return match (x) {
    A => 1,
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
    assert_eq!(lines, vec![r#"threw: rl match: unhandled variant {"kind":"C"}"#]);
}

/* ------------------------------------------------------------------ */
/* exhaustiveness checking via tsc                                     */
/* ------------------------------------------------------------------ */

#[test]
fn typecheck_exhaustive_match_passes() {
    require_toolchain!();
    let (ok, out) = typecheck(r#"
variant Shape { Circle(radius: number), Point }
const f = (s: Shape) => match (s) {
  Circle(radius) => radius,
  Point => 0,
};
"#);
    assert!(ok, "{out}");
}

#[test]
fn typecheck_non_exhaustive_match_fails() {
    require_toolchain!();
    let (ok, out) = typecheck(r#"
variant Shape { Circle(radius: number), Rect(w: number, h: number), Point }
const f = (s: Shape) => match (s) {
  Circle(radius) => radius,
  Point => 0,
};
"#);
    assert!(!ok, "expected tsc to reject the non-exhaustive match");
    assert!(out.contains("never"), "{out}");
}

#[test]
fn typecheck_wildcard_makes_partial_match_exhaustive() {
    require_toolchain!();
    let (ok, out) = typecheck(r#"
variant Shape { Circle(radius: number), Rect(w: number, h: number), Point }
const f = (s: Shape) => match (s) {
  Circle(radius) => radius,
  _ => 0,
};
"#);
    assert!(ok, "{out}");
}

#[test]
fn typecheck_match_on_handwritten_discriminated_union() {
    require_toolchain!();
    let (ok, out) = typecheck(r#"
type AppEvent =
  | { kind: "click"; x: number; y: number }
  | { kind: "key"; code: string };
const f = (e: AppEvent) => match (e) {
  click(x, y) => x + y,
  key(code) => code.length,
};
"#);
    assert!(ok, "{out}");
}
