/* Tests for completion over rl buffers (TASK-062).
 *
 * Two halves, both of which the editor used to lose:
 *
 *  - the standard library's combinators behind `Result.`/`Option.`, which
 *    the language service types perfectly well — the server was returning
 *    the enum's case constructors *instead of* its answer, not alongside;
 *  - members at a `.` the user has just typed inside rl syntax, where no
 *    compiled form of the buffer exists yet and a probe has to make one.
 *
 * These drive the real compiler binary, so they skip when it is not on
 * PATH — same rule as the sidecar and emit-map tests.
 */
import * as assert from "node:assert/strict";
import { test } from "node:test";
import { execFileSync } from "node:child_process";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import { runEmitMap, stdModulePath } from "../rlc";
import { buildProbe } from "../probe";
import { TsProject } from "../tsproject";
import { MappedDoc } from "../virtual";

const COMPILER = "rlc";

function compilerAvailable(): boolean {
  try {
    execFileSync(COMPILER, ["-v"], { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
}

const skip = compilerAvailable() ? false : "rlc not on PATH";

/** A buffer served to the language service the way the server serves it:
 * the emitted TypeScript when the source compiles into one, otherwise the
 * raw text. `text` is what the service is given; `at` maps a source offset
 * into it. */
async function project(source: string) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "rl-completion-"));
  const file = path.join(dir, "main.rl");
  fs.writeFileSync(file, source);

  const result = await runEmitMap(COMPILER, source, file);
  assert.ok(result, "rlc --emit-map failed");
  const mapped = new MappedDoc(source, result!.code, result!.mappings);
  const text = mapped.code;

  let served = { text, version: "1" };
  const ts = new TsProject(
    (fileName) => (fileName === file ? served : null),
    () => [file],
    dir,
    () => stdModulePath(COMPILER),
  );
  return {
    file,
    ts,
    at: (offset: number) => mapped.srcToOut(offset),
    /** Serve different text for the file — how the server installs a probe. */
    serve(probeText: string, version: string) {
      served = { text: probeText, version };
    },
  };
}

const STD_SOURCE = [
  'import { Result } from "@rl/std";',
  "",
  "declare const r: Result<number, string>;",
  "const doubled = Result.map(r, (n) => n * 2);",
  "",
].join("\n");

/* ------------------------------------------------- std namespace members */

test(
  "the std combinators are completions of `Result.`",
  { skip },
  async () => {
    const { file, ts, at } = await project(STD_SOURCE);
    const dot = STD_SOURCE.indexOf("Result.map(r") + "Result.".length;
    const offset = at(dot);
    assert.notEqual(offset, null);
    const answer = ts.completionsAt(file, offset!);
    assert.equal(answer.member, true, "expected a member completion");
    const names = answer.entries.map((e) => e.name);
    // The constructors the server contributes itself, and the combinators
    // it used to hide by returning only those.
    for (const member of [
      "Ok",
      "Err",
      "map",
      "mapErr",
      "andThen",
      "unwrapOr",
      "fromPromise",
      "andThenP",
      "mapErrP",
    ]) {
      assert.ok(names.includes(member), `missing ${member} in: ${names}`);
    }
  },
);

test("a completion entry resolves to its type", { skip }, async () => {
  const { file, ts, at } = await project(STD_SOURCE);
  const dot = STD_SOURCE.indexOf("Result.map(r") + "Result.".length;
  const detail = ts.completionDetail(file, at(dot)!, "andThen");
  assert.ok(detail, "expected details for andThen");
  assert.ok(
    detail!.signature.includes("Result<U, E>"),
    `signature was: ${detail!.signature}`,
  );
  assert.ok(
    detail!.documentation.includes("Chains a computation"),
    `documentation was: ${detail!.documentation}`,
  );
});

test("signature help types a combinator's arguments", { skip }, async () => {
  const { file, ts, at } = await project(STD_SOURCE);
  // Inside `Result.map(r, ...)`, on the second argument.
  const inCall = STD_SOURCE.indexOf("(n) => n * 2");
  const help = ts.signatureHelpAt(file, at(inCall)!);
  assert.ok(help, "expected signature help");
  assert.equal(help!.activeParameter, 1);
  const sig = help!.signatures[help!.activeSignature];
  assert.ok(
    sig.label.includes("r: Result<number, string>"),
    `label was: ${sig.label}`,
  );
  assert.equal(sig.parameters.length, 2);
  const [start, end] = sig.parameters[1].label;
  assert.ok(
    sig.label.slice(start, end).startsWith("f:"),
    `second parameter was: ${sig.label.slice(start, end)}`,
  );
});

/* ------------------------------------------------------------- probes -- */

/** Completions at `offset` the way the server asks for them: the plain
 * path first, then a probe when it came back empty. Returns both answers
 * so a test can assert the plain one really was empty. */
async function probed(source: string, offset: number) {
  const { file, ts, at, serve } = await project(source);
  const plain = ts
    .completionsAt(file, at(offset) ?? offset)
    .entries.map((e) => e.name);

  const built = await buildProbe(source, offset, (probeSource) =>
    runEmitMap(COMPILER, probeSource, `${file}.probe`),
  );
  assert.ok(built, "probe did not compile");
  serve(built!.code, "probe-1");
  const withProbe = ts
    .completionsAt(file, built!.offset)
    .entries.map((e) => e.name);
  return { plain, withProbe };
}

const PIPE_SOURCE = [
  "const n: number = 1;",
  "const s = n",
  "  |> .",
].join("\n");

test("a pipeline step's members need a probe", { skip }, async () => {
  const { plain, withProbe } = await probed(PIPE_SOURCE, PIPE_SOURCE.length);
  // The raw buffer is all TypeScript can see (`|> .` is not a member access
  // it can emit), and `|>` collapses the statement it sits in.
  assert.deepEqual(plain, [], "plain completions should be empty here");
  for (const member of ["toFixed", "toString", "toPrecision"]) {
    assert.ok(withProbe.includes(member), `missing ${member} in: ${withProbe}`);
  }
});

const PIPE_STD_SOURCE = [
  'import { Result } from "@rl/std";',
  "",
  "declare const r: Result<number, string>;",
  "const out = r",
  "  |> Result.mapP((n) => n + 1)",
  "  |> .",
].join("\n");

test(
  "a probe carries the pipeline's type through earlier steps",
  { skip },
  async () => {
    const { withProbe } = await probed(
      PIPE_STD_SOURCE,
      PIPE_STD_SOURCE.length,
    );
    // The value at the last step is a `Result`, not the head's type.
    assert.ok(withProbe.includes("kind"), `members were: ${withProbe}`);
  },
);

const MATCH_SOURCE = [
  "enum Shape {",
  "  Circle(radius: number),",
  "  Point,",
  "}",
  "",
  "declare const shape: Shape;",
  "",
  "const label = match (shape) {",
  "  Circle(radius) => radius.,",
  "  Point => 0,",
  "};",
].join("\n");

test("a match arm binding's members come from the emit", { skip }, async () => {
  // `radius` is a pattern binding — it exists only in the emitted switch —
  // but a trailing `.` in an arm body still compiles, so the ordinary path
  // (the buffer's virtual document) already answers and no probe is built.
  const offset = MATCH_SOURCE.indexOf("radius.,") + "radius.".length;
  const { plain, withProbe } = await probed(MATCH_SOURCE, offset);
  for (const member of ["toFixed", "toString"]) {
    assert.ok(plain.includes(member), `missing ${member} in: ${plain}`);
    assert.ok(withProbe.includes(member), `missing ${member} in: ${withProbe}`);
  }
});

test("a probe answers nothing for an unmendable buffer", { skip }, async () => {
  // A `match` with no closing brace is not a whole construct with the
  // placeholder either: the compiler passes the file through, so the probe
  // is still raw rl text. The point is that this degrades to no answer —
  // never to members of something else.
  const broken = [
    "declare const shape: { kind: string };",
    "const label = match (shape) {",
    "  Circle(radius) => radius.",
  ].join("\n");
  const { withProbe } = await probed(broken, broken.length);
  assert.deepEqual(withProbe, []);
});
