/* End-to-end tests for the virtual-document pipeline (TASK-048): the real
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

import { runEmitMap } from "../rlc";
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
