/* Tests for on-save sidecar regeneration. These drive the real `rlc`
 * binary (the sidecar is written by `rlc --sidecar`), so they skip when the
 * compiler is not on PATH — same rule as the compiler's own integration
 * tests. */
import * as assert from "node:assert/strict";
import { test } from "node:test";
import { execFileSync } from "node:child_process";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import { refreshSidecar } from "../sidecar";

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
  "/** 알림 한 건. */",
  "export enum Notice {",
  "  Info(text: string),",
  "  Warn(text: string),",
  "}",
  "",
  "export function render(notice: Notice): string {",
  "  return match (notice) {",
  "    Info(text) => text,",
  "    Warn(text) => text.toUpperCase(),",
  "  };",
  "}",
  "",
].join("\n");

function workspace(): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "rl-sidecar-test-"));
  fs.writeFileSync(path.join(dir, "notice.rl"), SOURCE);
  return dir;
}

test("always mode writes both sidecar files", { skip }, async () => {
  const dir = workspace();
  const rl = path.join(dir, "notice.rl");

  const result = await refreshSidecar(COMPILER, rl, "always");
  assert.equal(result.kind, "written", JSON.stringify(result));

  const declarations = fs.readFileSync(`${rl}.d.ts`, "utf8");
  const map = fs.readFileSync(`${rl}.d.ts.map`, "utf8");

  // The declarations describe what rl emitted: a union type and a
  // constructor object under the same name.
  assert.match(declarations, /export type Notice/);
  assert.match(declarations, /export declare const Notice/);
  assert.match(declarations, /export declare function render/);
  assert.match(declarations, /\/\/# sourceMappingURL=notice\.rl\.d\.ts\.map/);

  // The map points back at the .rl file — this is what sends "go to
  // definition" to the original instead of the .d.ts.
  const parsed = JSON.parse(map) as { sources: string[]; mappings: string };
  assert.deepEqual(parsed.sources, ["notice.rl"]);
  assert.ok(parsed.mappings.length > 0);

  fs.rmSync(dir, { recursive: true, force: true });
});

test("declarations can live in their own tree", { skip }, async () => {
  const dir = workspace();
  const rl = path.join(dir, "notice.rl");
  const types = path.join(dir, ".rl-types");

  const result = await refreshSidecar(COMPILER, rl, "always", types);
  assert.equal(result.kind, "written", JSON.stringify(result));

  // The source tree stays clean; the declarations sit next to each other.
  assert.equal(fs.existsSync(`${rl}.d.ts`), false);
  assert.equal(fs.existsSync(path.join(types, "notice.rl.d.ts")), true);

  // `sources` has to cross the distance, or the map cannot find the source.
  const map = JSON.parse(
    fs.readFileSync(path.join(types, "notice.rl.d.ts.map"), "utf8"),
  ) as { sources: string[] };
  assert.deepEqual(map.sources, ["../notice.rl"]);

  fs.rmSync(dir, { recursive: true, force: true });
});

test("refresh mode looks for the sidecar where it is written", { skip }, async () => {
  const dir = workspace();
  const rl = path.join(dir, "notice.rl");
  const types = path.join(dir, ".rl-types");

  // Nothing there yet — refresh must not create it.
  assert.equal((await refreshSidecar(COMPILER, rl, "refresh", types)).kind, "skipped");

  await refreshSidecar(COMPILER, rl, "always", types);
  assert.equal((await refreshSidecar(COMPILER, rl, "refresh", types)).kind, "written");

  fs.rmSync(dir, { recursive: true, force: true });
});

test("refresh mode leaves a workspace that never opted in alone", { skip }, async () => {
  const dir = workspace();
  const rl = path.join(dir, "notice.rl");

  const result = await refreshSidecar(COMPILER, rl, "refresh");
  assert.equal(result.kind, "skipped");
  assert.equal(fs.existsSync(`${rl}.d.ts`), false);

  fs.rmSync(dir, { recursive: true, force: true });
});

test("refresh mode updates a sidecar that is already there", { skip }, async () => {
  const dir = workspace();
  const rl = path.join(dir, "notice.rl");

  await refreshSidecar(COMPILER, rl, "always");
  // Anything exported after the first generation must show up on the next
  // save.
  fs.writeFileSync(rl, `${SOURCE}export function count(items: Notice[]): number {\n  return items.length;\n}\n`);

  const result = await refreshSidecar(COMPILER, rl, "refresh");
  assert.equal(result.kind, "written", JSON.stringify(result));
  assert.match(fs.readFileSync(`${rl}.d.ts`, "utf8"), /export declare function count/);

  fs.rmSync(dir, { recursive: true, force: true });
});

test("a file that no longer compiles keeps its last good sidecar", { skip }, async () => {
  const dir = workspace();
  const rl = path.join(dir, "notice.rl");

  await refreshSidecar(COMPILER, rl, "always");
  const before = fs.readFileSync(`${rl}.d.ts`, "utf8");

  // Adding a case without an arm makes the match non-exhaustive, which is a
  // compile error — the editor should keep showing the last declarations
  // rather than lose them mid-edit.
  fs.writeFileSync(rl, SOURCE.replace("  Warn(text: string),", "  Warn(text: string),\n  Debug(),"));
  const result = await refreshSidecar(COMPILER, rl, "refresh");

  assert.equal(result.kind, "failed");
  assert.match((result as { detail: string }).detail, /not exhaustive/);
  assert.equal(fs.readFileSync(`${rl}.d.ts`, "utf8"), before);

  fs.rmSync(dir, { recursive: true, force: true });
});

test("off mode does nothing", { skip }, async () => {
  const dir = workspace();
  const rl = path.join(dir, "notice.rl");

  const result = await refreshSidecar(COMPILER, rl, "off");
  assert.equal(result.kind, "skipped");
  assert.equal(fs.existsSync(`${rl}.d.ts`), false);

  fs.rmSync(dir, { recursive: true, force: true });
});
