/* Tests for the TypeScript language-service delegation: definitions and
 * hover for ordinary TS symbols in .rl files, across `.rl` imports. */
import * as assert from "node:assert/strict";
import { test } from "node:test";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import { TsProject, positionAt } from "../tsproject";

const UTIL_RL = [
  "export function add(a: number, b: number): number {",
  "  return a + b;",
  "}",
  "export enum St { A(x: number), B }",
  "",
].join("\n");

const MAIN_RL = [
  'import { add } from "./util.rl";',
  "const n = add(1, 2);",
  "const local = n + 1;",
  "console.log(local);",
  "",
].join("\n");

function project(): { dir: string; main: string; util: string; ts: TsProject } {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "rl-tsproject-"));
  const util = path.join(dir, "util.rl");
  const main = path.join(dir, "main.rl");
  fs.writeFileSync(util, UTIL_RL);
  fs.writeFileSync(main, MAIN_RL);
  const ts = new TsProject(() => null, () => [main], dir);
  return { dir, main, util, ts };
}

test("definition of an imported function crosses the .rl import", () => {
  const { main, util, ts } = project();
  const offset = MAIN_RL.indexOf("add(1");
  const defs = ts.definitionsAt(main, offset);
  assert.equal(defs.length, 1);
  assert.equal(defs[0].fileName, util);
  assert.equal(
    defs[0].fileText.slice(defs[0].start, defs[0].start + defs[0].length),
    "add",
  );
  // The span points at the declaration on line 1.
  assert.deepEqual(positionAt(defs[0].fileText, defs[0].start), {
    line: 0,
    character: UTIL_RL.indexOf("add"),
  });
});

test("definition of a local variable stays in the file", () => {
  const { main, ts } = project();
  const offset = MAIN_RL.indexOf("local);");
  const defs = ts.definitionsAt(main, offset);
  assert.equal(defs.length, 1);
  assert.equal(defs[0].fileName, main);
  assert.equal(
    positionAt(defs[0].fileText, defs[0].start).line,
    2, // `const local = ...`
  );
});

test("quick info renders an imported function's signature", () => {
  const { main, ts } = project();
  const offset = MAIN_RL.indexOf("add(1");
  const info = ts.quickInfoAt(main, offset);
  assert.ok(info);
  assert.ok(
    info!.signature.includes("add(a: number, b: number): number"),
    info!.signature,
  );
});

test("rl constructs in the file do not break nearby TS resolution", () => {
  // A match expression above the usage: TS error recovery must still
  // resolve the passthrough-region symbol below it.
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "rl-tsproject-"));
  const file = path.join(dir, "mixed.rl");
  const src = [
    "const flag = 1;",
    "const r = match (flag) { A => 1, _ => 0 };",
    "const use = flag + 1;",
    "",
  ].join("\n");
  fs.writeFileSync(file, src);
  const ts = new TsProject(() => null, () => [file], dir);
  const defs = ts.definitionsAt(file, src.indexOf("flag + 1"));
  assert.equal(defs.length, 1);
  assert.equal(positionAt(defs[0].fileText, defs[0].start).line, 0);
});
