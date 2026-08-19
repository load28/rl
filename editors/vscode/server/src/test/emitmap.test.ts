/* End-to-end tests for TypeScript answers over rl constructs, through the
 * engine session: hover inside match arms and pipelines, navigation out of
 * an arm body, imported unopened `.rl` modules, `@rl/std`, and type errors
 * mapped onto the source. The projection and every mapping live in the
 * engine now; what these lock is the observable result — the same results
 * the virtual-document pipeline (TASK-050/055/057/058/080) has always
 * produced, asked for and answered in `.rl` coordinates.
 *
 * They drive the real compiler *and* a real TypeScript language server, so
 * they skip when either is missing. */
import * as assert from "node:assert/strict";
import { after, test } from "node:test";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import * as engine from "../engine";
import { positionAt, sliceOf } from "./positions";
import { COMPILER, compilerAvailable, findTsgo } from "./toolchain";

const skip = !compilerAvailable()
  ? "rlc not on PATH"
  : findTsgo() === null
    ? "no tsgo executable"
    : false;

after(() => engine.shutdownEngineServer());

const SOURCE = [
  'import { disc } from "./helpers";',
  "",
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
  "  Circle(radius) => disc(radius * radius),",
  "  Rect(w, h) => w * h,",
  "  Point => 0,",
  "};",
  "",
].join("\n");

/** A hand-written TypeScript file the source imports, so a definition that
 * leaves an arm body lands somewhere an editor can actually open. */
const HELPERS = "export function disc(x: number): number {\n  return x * Math.PI;\n}\n";

function fixture(name: string, files: Record<string, string>): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), name));
  for (const [file, text] of Object.entries(files)) {
    fs.writeFileSync(path.join(dir, file), text);
  }
  return dir;
}

test(
  "hover inside a match arm body answers in source coordinates",
  { skip },
  async () => {
    // `radius` in the arm body is compiler-destructured in the emitted
    // switch — the raw text could never answer this; the engine's
    // projection does, and the span comes back in the arm body the user
    // wrote.
    const dir = fixture("rl-emitmap-test-", {
      "shapes.rl": SOURCE,
      "helpers.ts": HELPERS,
    });
    const file = path.join(dir, "shapes.rl");
    const info = await engine.hover(
      COMPILER,
      file,
      positionAt(SOURCE, SOURCE.indexOf("radius * radius")),
    );
    assert.ok(info, "expected quick info");
    assert.ok(
      info!.signature.includes("radius: number"),
      `signature was: ${info!.signature}`,
    );
    assert.equal(sliceOf(SOURCE, info!.range), "radius");
  },
);

test(
  "definition from an arm body lands in the hand-written file",
  { skip },
  async () => {
    const dir = fixture("rl-emitmap-test-", {
      "shapes.rl": SOURCE,
      "helpers.ts": HELPERS,
    });
    const file = path.join(dir, "shapes.rl");
    const defs = await engine.definition(
      COMPILER,
      file,
      positionAt(SOURCE, SOURCE.indexOf("disc(radius")),
    );
    assert.equal(defs.length, 1, JSON.stringify(defs));
    assert.equal(defs[0].path, path.join(dir, "helpers.ts"));
    assert.equal(sliceOf(HELPERS, defs[0].range), "disc");
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

test("a pipeline step over the std module is not `any`", { skip }, async () => {
  const dir = fixture("rl-std-test-", { "calc.rl": PIPE_SOURCE });
  const file = path.join(dir, "calc.rl");
  const info = await engine.hover(
    COMPILER,
    file,
    positionAt(PIPE_SOURCE, PIPE_SOURCE.indexOf("andThenP")),
  );
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
    const dir = fixture("rl-std-test-", { "calc.rl": PIPE_SOURCE });
    const file = path.join(dir, "calc.rl");
    const info = await engine.hover(
      COMPILER,
      file,
      positionAt(PIPE_SOURCE, PIPE_SOURCE.indexOf("trim")),
    );
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
    // The importer is the open buffer; `shapes.rl` is only on disk. The
    // engine projects and serves it on its own — served raw, TypeScript
    // would read `enum Shape` as a TS enum and the arm bindings would lose
    // their types.
    const dir = fixture("rl-import-test-", {
      "shapes.rl": IMPORTED_SHAPES,
      "main.rl": IMPORTER,
    });
    const file = path.join(dir, "main.rl");
    engine.openDocument(COMPILER, file, IMPORTER);
    const info = await engine.hover(
      COMPILER,
      file,
      positionAt(IMPORTER, IMPORTER.indexOf("radius * radius")),
    );
    assert.ok(info, "expected quick info");
    assert.ok(
      info!.signature.includes("radius: number"),
      `signature was: ${info!.signature}`,
    );
    engine.closeDocument(COMPILER, file);
  },
);

/* --------------------------------------------------------------------------
 * TASK-057: type errors inside rl syntax reach the editor, every span on
 * the source the user wrote — a span that would land in generated code is
 * dropped, never reported at a made-up position.
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
  const dir = fixture("rl-std-test-", { "calc.rl": BAD_PIPE });
  const file = path.join(dir, "calc.rl");
  const diagnostics = await engine.tsDiagnostics(COMPILER, file);
  assert.equal(diagnostics.length, 1, JSON.stringify(diagnostics));
  assert.equal(diagnostics[0].code, 2339); // property does not exist
  assert.match(diagnostics[0].message, /'length' does not exist on type/);
  assert.equal(sliceOf(BAD_PIPE, diagnostics[0].range), "length");
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
  const dir = fixture("rl-std-test-", { "calc.rl": BAD_ARM });
  const file = path.join(dir, "calc.rl");
  const diagnostics = await engine.tsDiagnostics(COMPILER, file);
  assert.equal(diagnostics.length, 1, JSON.stringify(diagnostics));
  assert.equal(diagnostics[0].code, 2339);
  assert.equal(sliceOf(BAD_ARM, diagnostics[0].range), "toUpperCase");
});

test("clean rl syntax reports nothing", { skip }, async () => {
  const dir = fixture("rl-std-test-", { "calc.rl": PIPE_SOURCE });
  assert.deepEqual(
    await engine.tsDiagnostics(COMPILER, path.join(dir, "calc.rl")),
    [],
  );
});

test("a buffer mid-edit is never invented errors for", { skip }, async () => {
  // An unfinished construct does not lower — the projection passes the raw
  // text through — and TypeScript cannot parse rl syntax, so its recovery
  // would invent errors all over the file. The parse-error guard drops
  // every one of them: degrade to silence, never to lies.
  const broken = [
    "declare const shape: { kind: string };",
    "const label = match (shape) {",
    "  Circle(radius) => radius.",
  ].join("\n");
  const dir = fixture("rl-rawdiag-test-", { "shapes.rl": broken });
  const file = path.join(dir, "shapes.rl");
  engine.openDocument(COMPILER, file, broken);
  assert.deepEqual(await engine.tsDiagnostics(COMPILER, file), []);
  engine.closeDocument(COMPILER, file);
});

/* --------------------------------------------------------------------------
 * TASK-058: the type environment itself must be sound before anything is
 * reported — no invented TS2488 on a tuple destructuring the user wrote.
 * -------------------------------------------------------------------- */

const TUPLE_SOURCE = [
  'import { Result } from "@rl/std";',
  "",
  "type Evaluated = Result<number, string>;",
  "type Operands = Result<[number, number], string>;",
  "",
  "export const applyP =",
  "  (f: (a: number, b: number) => number) =>",
  "  (ops: Operands): Evaluated =>",
  "    Result.map(ops, ([a, b]) => f(a, b));",
  "",
].join("\n");

test("tuple destructuring over the std module reports nothing", { skip }, async () => {
  const dir = fixture("rl-std-test-", { "calc.rl": TUPLE_SOURCE });
  assert.deepEqual(
    await engine.tsDiagnostics(COMPILER, path.join(dir, "calc.rl")),
    [],
  );
});

/* --------------------------------------------------------------------------
 * TASK-080: the names a construct *introduces* — a `try` declaration, a
 * let-else / if let / match pattern binding — are copied from the source
 * into the emitted declaration, so hovering one answers with its type
 * instead of nothing.
 * -------------------------------------------------------------------- */

const BINDING_SOURCE = [
  'import { Option, Result } from "@rl/std";',
  "",
  "declare function load(): Result<number, string>;",
  "declare function boxed(): Option<string>;",
  "",
  "export function run(): number {",
  "  const total = try load();",
  '  const Some(value: label) = boxed() else { throw new Error("none"); };',
  "  if let Some(value: shown) = boxed() {",
  "    console.log(shown);",
  "  }",
  "  const width = match (boxed()) {",
  "    Some(value: text) => text.length,",
  "    None => 0,",
  "  };",
  "  return total + label.length + width;",
  "}",
  "",
].join("\n");

/** Hovers the `needle` written inside `context` and returns the signature
 * TypeScript answers with. */
async function signatureOfBinding(
  file: string,
  context: string,
  needle: string,
): Promise<string> {
  const src = BINDING_SOURCE.indexOf(context) + context.indexOf(needle);
  const info = await engine.hover(
    COMPILER,
    file,
    positionAt(BINDING_SOURCE, src),
  );
  assert.ok(info, `expected quick info for ${needle}`);
  return info!.signature;
}

test("a try declaration's binding hovers with its Ok type", { skip }, async () => {
  const dir = fixture("rl-std-test-", { "calc.rl": BINDING_SOURCE });
  const file = path.join(dir, "calc.rl");
  const signature = await signatureOfBinding(file, "total = try load()", "total");
  assert.match(signature, /total: number/);
});

test("let-else and if let bindings hover with the extracted type", { skip }, async () => {
  const dir = fixture("rl-std-test-", { "calc.rl": BINDING_SOURCE });
  const file = path.join(dir, "calc.rl");
  assert.match(
    await signatureOfBinding(file, "value: label)", "label"),
    /label: string/,
  );
  assert.match(
    await signatureOfBinding(file, "value: shown)", "shown"),
    /shown: string/,
  );
  assert.match(
    await signatureOfBinding(file, "value: text)", "text"),
    /text: string/,
  );
});
