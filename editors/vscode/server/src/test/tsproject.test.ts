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

test("completions include an imported function and object members", () => {
  const { main, ts } = project();
  // General position at end of file.
  const general = ts.completionsAt(main, MAIN_RL.length);
  assert.ok(general.some((c) => c.name === "add"), "missing `add`");
  assert.ok(general.some((c) => c.name === "local"), "missing `local`");

  // Member position: after `console.` the members of console appear.
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "rl-tsproject-"));
  const file = path.join(dir, "m.rl");
  const src = "console.\n";
  fs.writeFileSync(file, src);
  const p = new TsProject(() => null, () => [file], dir);
  const members = p.completionsAt(file, src.indexOf(".") + 1);
  assert.ok(members.some((c) => c.name === "log"), "missing console.log");
});

test("references cross the .rl import in both directions", () => {
  const { main, util, ts } = project();
  const refs = ts.referencesAt(main, MAIN_RL.indexOf("add(1"));
  const files = refs.map((r) => r.fileName).sort();
  assert.ok(files.includes(util), "declaration reference missing");
  assert.ok(files.includes(main), "usage reference missing");
  assert.ok(refs.some((r) => r.isDefinition));
});

test("rename returns every location of a local symbol", () => {
  const { main, ts } = project();
  const locations = ts.renameAt(main, MAIN_RL.indexOf("local);"));
  assert.ok(locations);
  assert.equal(locations!.length, 2); // declaration + use
  for (const l of locations!) {
    assert.equal(
      l.fileText.slice(l.start, l.start + l.length),
      "local",
    );
  }
});

test("typeAt reports the scrutinee's enum type across an .rl import", () => {
  // Raw .rl serving (the pre-virtual fallback path): error recovery still
  // lets TS infer the annotated/returned type name and its declaring file.
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "rl-tsproject-"));
  const shape = path.join(dir, "shape.rl");
  const file = path.join(dir, "use.rl");
  fs.writeFileSync(
    shape,
    "export enum Shape { Circle(radius: number), Point }\n",
  );
  const src = [
    'import { Shape } from "./shape.rl";',
    "declare function getShape(): Shape;",
    "const shape = getShape();",
    "const r = match (shape) {",
    "",
    "};",
    "",
  ].join("\n");
  fs.writeFileSync(file, src);
  const ts = new TsProject(() => null, () => [file], dir);

  const at = src.indexOf("shape) {");
  const info = ts.typeAt(file, at, { start: at, end: at + "shape".length });
  assert.ok(info, "expected a type");
  assert.equal(info!.name, "Shape");
  assert.equal(info!.declFile, shape);
});

test("typeAt with a span reports the whole expression's type", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "rl-tsproject-"));
  const file = path.join(dir, "chain.rl");
  const src = [
    "type Wrapped = { kind: \"A\" } | { kind: \"B\" };",
    "declare function get(): Wrapped;",
    "const c = [get()].map((s) => s)[0];",
    "console.log(c);",
    "",
  ].join("\n");
  fs.writeFileSync(file, src);
  const ts = new TsProject(() => null, () => [file], dir);

  const start = src.indexOf("c);");
  const info = ts.typeAt(file, start, { start, end: start + 1 });
  assert.ok(info, "expected a type");
  assert.equal(info!.name, "Wrapped");
});

test("typeAt strips generic arguments from the type name", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "rl-tsproject-"));
  const file = path.join(dir, "opt.rl");
  const src = [
    "type Option<T> = { kind: \"Some\"; value: T } | { kind: \"None\" };",
    "declare function find(): Option<string>;",
    "const found = find();",
    "console.log(found);",
    "",
  ].join("\n");
  fs.writeFileSync(file, src);
  const ts = new TsProject(() => null, () => [file], dir);

  const at = src.indexOf("found);");
  const info = ts.typeAt(file, at, { start: at, end: at + "found".length });
  assert.ok(info, "expected a type");
  assert.equal(info!.name, "Option");
});
