/* Tests for completion over rl buffers (TASK-062), through the engine
 * session. Two halves, both of which the editor used to lose:
 *
 *  - the standard library's combinators behind `Result.`/`Option.`, which
 *    the language service types perfectly well;
 *  - members at a `.` the user has just typed inside rl syntax, where no
 *    compiled form of the buffer exists yet and the engine's probe mends
 *    one.
 *
 * These drive the real compiler binary and a real TypeScript language
 * server, so they skip when either is missing. */
import * as assert from "node:assert/strict";
import { after, test } from "node:test";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import * as engine from "../engine";
import { positionAt } from "./positions";
import { COMPILER, compilerAvailable, findTsgo } from "./toolchain";

const skip = !compilerAvailable()
  ? "rlc not on PATH"
  : findTsgo() === null
    ? "no tsgo executable"
    : false;

after(() => engine.shutdownEngineServer());

/** A buffer in a workspace of its own, open in the engine. */
function project(source: string): { file: string; done: () => void } {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "rl-completion-"));
  const file = path.join(dir, "main.rl");
  fs.writeFileSync(file, source);
  engine.openDocument(COMPILER, file, source);
  return { file, done: () => engine.closeDocument(COMPILER, file) };
}

const STD_SOURCE = [
  'import type { TResult } from "@rl/std";',
  'import * as Result from "@rl/std/result";',
  "",
  "declare const r: TResult<number, string>;",
  "const doubled = Result.map(r, (n) => n * 2);",
  "",
].join("\n");

/* ------------------------------------------------- std namespace members */

test(
  "the std combinators are completions of `Result.`",
  { skip },
  async () => {
    const { file, done } = project(STD_SOURCE);
    const dot = STD_SOURCE.indexOf("Result.map(r") + "Result.".length;
    const answer = await engine.completion(
      COMPILER,
      file,
      positionAt(STD_SOURCE, dot),
      true,
    );
    assert.ok(answer, "expected a completion answer");
    assert.equal(answer!.member, true, "expected a member completion");
    const names = answer!.items.map((e) => e.label);
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
    done();
  },
);

test("a completion entry resolves to its type", { skip }, async () => {
  const { file, done } = project(STD_SOURCE);
  const dot = STD_SOURCE.indexOf("Result.map(r") + "Result.".length;
  const position = positionAt(STD_SOURCE, dot);
  const detail = await engine.completionResolve(
    COMPILER,
    file,
    position,
    "andThen",
    undefined,
  );
  assert.ok(detail, "expected details for andThen");
  // The engine resolves the entry against the position it was asked at, so
  // the signature comes back instantiated (`TResult<number, string>`) rather
  // than in the type parameters the declaration is written in.
  assert.ok(
    detail!.signature.includes("andThen:"),
    `signature was: ${detail!.signature}`,
  );
  assert.ok(
    detail!.signature.includes("TResult<number, string>"),
    `signature was: ${detail!.signature}`,
  );
  assert.ok(
    detail!.documentation.includes("Chains a computation"),
    `documentation was: ${detail!.documentation}`,
  );
  done();
});

test("signature help types a combinator's arguments", { skip }, async () => {
  const { file, done } = project(STD_SOURCE);
  // Inside `Result.map(r, ...)`, on the second argument.
  const inCall = STD_SOURCE.indexOf("(n) => n * 2");
  const help = await engine.signatureHelp(
    COMPILER,
    file,
    positionAt(STD_SOURCE, inCall),
  );
  assert.ok(help, "expected signature help");
  assert.equal(help!.activeParameter, 1);
  const sig = help!.signatures[help!.activeSignature];
  assert.ok(
    sig.label.includes("r: TResult<number, string>"),
    `label was: ${sig.label}`,
  );
  assert.equal(sig.parameters.length, 2);
  const [start, end] = sig.parameters[1].label;
  assert.ok(
    sig.label.slice(start, end).startsWith("f:"),
    `second parameter was: ${sig.label.slice(start, end)}`,
  );
  done();
});

/* ------------------------------------------------------------- probes -- */

/** Completions at `offset` both ways: the plain answer (no member policy),
 * and the member-access path where the engine probes when the plain path
 * cannot answer. Returns label lists so a test can assert the plain one
 * really was empty. */
async function probed(source: string, offset: number) {
  const { file, done } = project(source);
  const position = positionAt(source, offset);
  const plainAnswer = await engine.completion(COMPILER, file, position, false);
  const memberAnswer = await engine.completion(COMPILER, file, position, true);
  done();
  return {
    plain: (plainAnswer?.items ?? []).map((e) => e.label),
    withProbe: (memberAnswer?.items ?? []).map((e) => e.label),
    probe: memberAnswer?.probe ?? null,
  };
}

const PIPE_SOURCE = [
  "const n: number = 1;",
  "const s = n",
  "  |> .",
].join("\n");

test("a pipeline step's members need a probe", { skip }, async () => {
  const { plain, withProbe, probe } = await probed(
    PIPE_SOURCE,
    PIPE_SOURCE.length,
  );
  // The raw buffer is all TypeScript can see (`|> .` is not a member access
  // it can emit), and `|>` collapses the statement it sits in.
  assert.deepEqual(plain, [], "plain completions should be empty here");
  assert.notEqual(probe, null, "the members had to come from a probe");
  for (const member of ["toFixed", "toString", "toPrecision"]) {
    assert.ok(withProbe.includes(member), `missing ${member} in: ${withProbe}`);
  }
});

const PIPE_STD_SOURCE = [
  'import type { TResult } from "@rl/std";',
  'import * as Result from "@rl/std/result";',
  "",
  "declare const r: TResult<number, string>;",
  "const out = r",
  "  |> Result.mapP((n) => n + 1)",
  "  |> .",
].join("\n");

test(
  "a probe carries the pipeline's type through earlier steps",
  { skip },
  async () => {
    const { withProbe } = await probed(PIPE_STD_SOURCE, PIPE_STD_SOURCE.length);
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
  // (the buffer's projection) already answers and no probe is needed.
  const offset = MATCH_SOURCE.indexOf("radius.,") + "radius.".length;
  const { plain, withProbe, probe } = await probed(MATCH_SOURCE, offset);
  assert.equal(probe, null, "no probe should be needed here");
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
