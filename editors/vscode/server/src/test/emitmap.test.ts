/* End-to-end tests for the virtual-document pipeline (TASK-050): the real
 * `rlc --emit-map` output served to the TypeScript language service via
 * TsProject, with offsets translated through MappedDoc. These drive the
 * real compiler binary, so they skip when it is not on PATH — same rule as
 * the sidecar tests. */
import * as assert from "node:assert/strict";
import { test } from "node:test";
import { execFileSync } from "node:child_process";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import { runEmitMap, runEmitMapFileSync, stdModulePath } from "../rlc";
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

const SOURCE = [
  "enum Shape {",
  "  Circle(radius: number),",
  "  Rect(w: number, h: number),",
  "  Point,",
  "}",
  "",
  "declare function getShape(): Shape;",
  "const shape = getShape();",
  "",
  "const area = match (shape) {",
  "  Circle(radius) => Math.PI * radius * radius,",
  "  Rect(w, h) => w * h,",
  "  Point => 0,",
  "};",
  "",
].join("\n");

/** The source compiled and served as a virtual document. */
async function virtualProject(): Promise<{
  file: string;
  mapped: MappedDoc;
  ts: TsProject;
}> {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "rl-emitmap-test-"));
  const file = path.join(dir, "shapes.rl");
  fs.writeFileSync(file, SOURCE);
  const result = await runEmitMap(COMPILER, SOURCE, file);
  assert.ok(result, "rlc --emit-map failed");
  const mapped = new MappedDoc(SOURCE, result!.code, result!.mappings);
  const ts = new TsProject(
    (fileName) =>
      fileName === file ? { text: mapped.code, version: "1:emitted" } : null,
    () => [file],
    dir,
  );
  return { file, mapped, ts };
}

test("emitted virtual doc types the scrutinee exactly", { skip }, async () => {
  const { file, mapped, ts } = await virtualProject();
  const src = SOURCE.indexOf("shape) {");
  const at = mapped.srcToOut(src);
  assert.notEqual(at, null, "scrutinee must be mapped");
  const info = ts.typeAt(file, at!, { start: at!, end: at! + "shape".length });
  assert.ok(info, "expected a type");
  assert.equal(info!.name, "Shape");
  assert.equal(info!.declFile, file);
});

test(
  "hover inside a match arm body works on the virtual doc",
  { skip },
  async () => {
    // `radius` in the arm body is compiler-destructured in the emitted
    // switch — the raw text could never answer this, the virtual doc can.
    const { file, mapped, ts } = await virtualProject();
    const src = SOURCE.indexOf("radius * radius");
    const at = mapped.srcToOut(src);
    assert.notEqual(at, null, "arm body must be mapped");
    const info = ts.quickInfoAt(file, at!);
    assert.ok(info, "expected quick info");
    assert.ok(
      info!.signature.includes("radius: number"),
      `signature was: ${info!.signature}`,
    );
    // The hover span maps back into the source arm body.
    const back = mapped.outToSrc(info!.start);
    assert.equal(SOURCE.slice(back!, back! + "radius".length), "radius");
  },
);

test(
  "definition from an arm body maps back to source coordinates",
  { skip },
  async () => {
    // `Math.PI` inside the arm body resolves into lib.d.ts; the querying
    // side goes through the mapping, the result side is a plain TS file.
    const { file, mapped, ts } = await virtualProject();
    const src = SOURCE.indexOf("Math.PI");
    const at = mapped.srcToOut(src);
    assert.notEqual(at, null);
    const defs = ts.definitionsAt(file, at!);
    assert.ok(defs.length > 0, "expected a definition for Math");
    assert.ok(defs[0].fileName.endsWith(".d.ts"));
  },
);

/* --------------------------------------------------------------------------
 * TASK-055: the std module and imported `.rl` modules must reach the
 * language service with real types. Both used to arrive as `any` — the bare
 * `"@rl/std"` specifier resolved nowhere, and an imported `.rl` file was
 * served as raw source TypeScript can only error-recover through.
 * -------------------------------------------------------------------- */

const PIPE_SOURCE = [
  'import { Result } from "@rl/std";',
  "",
  "declare function tokenize(s: string): Result<string[], string>;",
  "declare function parse(t: string[]): Result<number, string>;",
  "",
  "export function calculate(input: string): Result<number, string> {",
  "  return input",
  "    |> .trim()",
  "    |> tokenize",
  "    |> Result.andThenP(parse);",
  "}",
  "",
].join("\n");

/** A buffer served as a virtual document, with the std fallback wired in
 * exactly as the server wires it. */
async function stdProject(source: string): Promise<{
  file: string;
  mapped: MappedDoc;
  ts: TsProject;
}> {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "rl-std-test-"));
  const file = path.join(dir, "calc.rl");
  fs.writeFileSync(file, source);
  const result = await runEmitMap(COMPILER, source, file);
  assert.ok(result, "rlc --emit-map failed");
  const mapped = new MappedDoc(source, result!.code, result!.mappings);
  const ts = new TsProject(
    (fileName) =>
      fileName === file ? { text: mapped.code, version: "1:emitted" } : null,
    () => [file],
    dir,
    () => stdModulePath(COMPILER),
  );
  return { file, mapped, ts };
}

test("a pipeline step over the std module is not `any`", { skip }, async () => {
  const { file, mapped, ts } = await stdProject(PIPE_SOURCE);
  const src = PIPE_SOURCE.indexOf("andThenP");
  const at = mapped.srcToOut(src);
  assert.notEqual(at, null, "pipeline step must be mapped");
  const info = ts.quickInfoAt(file, at!);
  assert.ok(info, "expected quick info");
  assert.ok(
    info!.signature.includes("Result<number, string>"),
    `signature was: ${info!.signature}`,
  );
});

test(
  "a postfix pipeline step keeps its receiver's type",
  { skip },
  async () => {
    const { file, mapped, ts } = await stdProject(PIPE_SOURCE);
    const src = PIPE_SOURCE.indexOf("trim");
    const at = mapped.srcToOut(src);
    assert.notEqual(at, null, "postfix step must be mapped");
    const info = ts.quickInfoAt(file, at!);
    assert.ok(info, "expected quick info");
    assert.ok(
      info!.signature.includes("String.trim(): string"),
      `signature was: ${info!.signature}`,
    );
  },
);

const IMPORTED_SHAPES = [
  "export enum Shape {",
  "  Circle(radius: number),",
  "  Rect(w: number, h: number),",
  "}",
  "",
].join("\n");

const IMPORTER = [
  'import { Shape } from "./shapes.rl";',
  "",
  "declare function getShape(): Shape;",
  "",
  "export const area = match (getShape()) {",
  "  Circle(radius) => Math.PI * radius * radius,",
  "  Rect(w, h) => w * h,",
  "};",
  "",
].join("\n");

test(
  "an imported .rl module is served emitted, not raw",
  { skip },
  async () => {
    // The importer is the open buffer; `shapes.rl` is only on disk, so it
    // reaches the service through the synchronous compile the server does
    // for imported modules. Served raw, TypeScript reads `enum Shape` as a
    // TS enum and the arm bindings lose their types.
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "rl-import-test-"));
    const shapes = path.join(dir, "shapes.rl");
    const main = path.join(dir, "main.rl");
    fs.writeFileSync(shapes, IMPORTED_SHAPES);
    fs.writeFileSync(main, IMPORTER);

    const result = await runEmitMap(COMPILER, IMPORTER, main);
    assert.ok(result, "rlc --emit-map failed");
    const mapped = new MappedDoc(IMPORTER, result!.code, result!.mappings);
    const onDisk = runEmitMapFileSync(COMPILER, shapes);
    assert.ok(onDisk, "rlc --emit-map on the imported module failed");
    const diskDoc = new MappedDoc(
      IMPORTED_SHAPES,
      onDisk!.code,
      onDisk!.mappings,
    );

    const ts = new TsProject(
      (fileName) => {
        if (fileName === main)
          return { text: mapped.code, version: "1:emitted" };
        if (fileName === shapes) {
          return { text: diskDoc.code, version: "disk-1:emitted" };
        }
        return null;
      },
      () => [main],
      dir,
      () => stdModulePath(COMPILER),
    );

    const src = IMPORTER.indexOf("radius * radius");
    const at = mapped.srcToOut(src);
    assert.notEqual(at, null, "arm body must be mapped");
    const info = ts.quickInfoAt(main, at!);
    assert.ok(info, "expected quick info");
    assert.ok(
      info!.signature.includes("radius: number"),
      `signature was: ${info!.signature}`,
    );
  },
);

/* --------------------------------------------------------------------------
 * TASK-057: type errors inside rl syntax reach the editor. The service only
 * sees them over a virtual document — the raw text is not TypeScript — and
 * every span has to come back through the mapping, or it would point into
 * generated code.
 * -------------------------------------------------------------------- */

const BAD_PIPE = [
  'import { Result } from "@rl/std";',
  "",
  "declare function evaluate(): Result<number, string>;",
  "",
  "// `n` is a number; `n.length` is not a thing.",
  "export const bad = evaluate() |> Result.mapP((n) => n.length);",
  "",
].join("\n");

test("a type error inside a pipeline is reported", { skip }, async () => {
  const { file, mapped, ts } = await stdProject(BAD_PIPE);
  const diagnostics = ts.diagnosticsFor(file);
  assert.equal(diagnostics.length, 1, JSON.stringify(diagnostics));
  assert.equal(diagnostics[0].code, 2339); // property does not exist
  assert.match(diagnostics[0].message, /'length' does not exist on type/);

  // The span maps back onto the source text the user wrote.
  const start = mapped.outToSrc(diagnostics[0].start);
  assert.notEqual(start, null, "the span must map back to the source");
  assert.equal(BAD_PIPE.slice(start!, start! + "length".length), "length");
});

const BAD_ARM = [
  "enum Shape {",
  "  Circle(radius: number),",
  "  Point,",
  "}",
  "",
  "declare function getShape(): Shape;",
  "",
  "export const area = match (getShape()) {",
  "  Circle(radius) => radius.toUpperCase(),",
  "  Point => 0,",
  "};",
  "",
].join("\n");

test("a type error inside a match arm is reported", { skip }, async () => {
  const { file, mapped, ts } = await stdProject(BAD_ARM);
  const diagnostics = ts.diagnosticsFor(file);
  assert.equal(diagnostics.length, 1, JSON.stringify(diagnostics));
  assert.equal(diagnostics[0].code, 2339);
  const start = mapped.outToSrc(diagnostics[0].start);
  assert.notEqual(start, null, "the span must map back to the source");
  assert.equal(
    BAD_ARM.slice(start!, start! + "toUpperCase".length),
    "toUpperCase",
  );
});

test("clean rl syntax reports nothing", { skip }, async () => {
  const { file, ts } = await stdProject(PIPE_SOURCE);
  assert.deepEqual(ts.diagnosticsFor(file), []);
});

test("raw .rl text is never type-checked", { skip }, async () => {
  // The same buffer served raw instead of emitted: TypeScript cannot parse
  // `match`, so its recovery would invent errors all over the file. The
  // syntactic guard drops every one of them.
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "rl-rawdiag-test-"));
  const file = path.join(dir, "shapes.rl");
  fs.writeFileSync(file, SOURCE);
  const ts = new TsProject(
    (fileName) =>
      fileName === file ? { text: SOURCE, version: "1:raw" } : null,
    () => [file],
    dir,
  );
  assert.deepEqual(ts.diagnosticsFor(file), []);
});
