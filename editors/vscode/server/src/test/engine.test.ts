/* Tests for the engine session — the semantic API every editor feature now
 * consumes (engine.ts → `rlc --server` → the rl engine). The questions are
 * positions in `.rl` sources and the answers come back in `.rl`
 * coordinates; nothing here touches emitted TypeScript.
 *
 * They drive the real compiler and, behind it, a real TypeScript language
 * server, so they skip when either is missing — the guard mirrors the
 * engine's own resolution, so a skip means "no toolchain", never "the
 * feature quietly answered nothing". */
import * as assert from "node:assert/strict";
import { after, test } from "node:test";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import * as engine from "../engine";
import { positionAt, sliceOf, spanOf } from "./positions";
import { COMPILER, compilerAvailable, findTsgo } from "./toolchain";

const skip = !compilerAvailable()
  ? "rlc not on PATH"
  : findTsgo() === null
    ? "no tsgo executable"
    : false;

after(() => engine.shutdownEngineServer());

/** A workspace with a hand-written `.ts` and an `.rl` file. The `.rl`
 * content here is plain passthrough TypeScript — what these tests exercise
 * is the session itself: buffers, navigation across files, rename safety,
 * diagnostics. (The rl constructs get their own suites in emitmap/
 * completion tests.) */
const RENDER = [
  "// nothing here is a rename target",
  'import { describe } from "./user";',
  "export function render(state: \"idle\" | \"loading\"): string {",
  "  const label = describe(state);",
  "  const bad: number = label;",
  "  return label;",
  "}",
  "",
].join("\n");

function workspace(): { dir: string; rl: string } {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "rl-engine-test-"));
  fs.mkdirSync(path.join(dir, "src"));
  fs.writeFileSync(
    path.join(dir, "tsconfig.json"),
    JSON.stringify({
      compilerOptions: {
        strict: true,
        target: "es2022",
        module: "preserve",
        moduleResolution: "bundler",
        skipLibCheck: true,
        noEmit: true,
      },
      include: ["src"],
    }),
  );
  fs.writeFileSync(
    path.join(dir, "src/user.ts"),
    'export type State = "idle" | "loading";\nexport function describe(s: State): string {\n  return s;\n}\n',
  );
  const rl = path.join(dir, "src/render.rl");
  fs.writeFileSync(rl, RENDER);
  return { dir, rl };
}

test("hover answers for a buffer the disk never saw", { skip }, async () => {
  const { rl } = workspace();
  // The disk copy never sees this text; the engine's overlay is the truth.
  const buffer = RENDER.replace("const label", "const tag = 1;\n  const label");
  engine.openDocument(COMPILER, rl, buffer);
  const info = await engine.hover(
    COMPILER,
    rl,
    positionAt(buffer, buffer.indexOf("tag = 1")),
  );
  assert.ok(info, "hover has an answer");
  assert.match(info!.signature, /tag: 1|tag: number/);
  engine.closeDocument(COMPILER, rl);
});

test("hover names the checker's view, at the source span", { skip }, async () => {
  const { rl } = workspace();
  const info = await engine.hover(
    COMPILER,
    rl,
    positionAt(RENDER, RENDER.indexOf("describe(state)")),
  );
  assert.ok(info, "hover has an answer");
  assert.match(info!.signature, /describe/);
  assert.match(info!.signature, /State/);
  assert.equal(sliceOf(RENDER, info!.range), "describe");
});

test("definition crosses into the hand-written file on disk", { skip }, async () => {
  const { dir, rl } = workspace();
  const locations = await engine.definition(
    COMPILER,
    rl,
    positionAt(RENDER, RENDER.indexOf("describe(state)")),
  );
  assert.equal(locations.length, 1, JSON.stringify(locations));
  assert.equal(locations[0].path, path.join(dir, "src/user.ts"));
  // The span names the declaration, in the target file's own coordinates.
  const target = fs.readFileSync(locations[0].path, "utf8");
  assert.equal(sliceOf(target, locations[0].range), "describe");
});

test("diagnostics come back at positions in the .rl source", { skip }, async () => {
  const { rl } = workspace();
  const diagnostics = await engine.tsDiagnostics(COMPILER, rl);
  const error = diagnostics.find((d) => d.code === 2322);
  assert.ok(error, JSON.stringify(diagnostics));
  assert.equal(sliceOf(RENDER, error!.range), "bad");
});

test("references find every use, in source coordinates", { skip }, async () => {
  const { rl } = workspace();
  const references = await engine.references(
    COMPILER,
    rl,
    positionAt(RENDER, RENDER.indexOf("label = describe")),
  );
  assert.ok(references.length >= 2, JSON.stringify(references));
  for (const reference of references) {
    assert.equal(reference.path, rl, "all of them in the .rl file");
    assert.equal(sliceOf(RENDER, reference.range), "label");
  }
});

test("rename names every place the name is written", { skip }, async () => {
  const { rl } = workspace();
  const edits = await engine.rename(
    COMPILER,
    rl,
    positionAt(RENDER, RENDER.indexOf("label = describe")),
  );
  assert.ok(edits, "the binding can be renamed");
  // Three uses of `label`; every span names the identifier, because the
  // engine refuses the rename whole if any edit cannot be mapped back.
  assert.equal(edits!.length, 3, JSON.stringify(edits));
  for (const edit of edits!) {
    assert.equal(edit.path, rl);
    assert.equal(sliceOf(RENDER, edit.range), "label");
  }
});

test(
  "renaming a destructuring shorthand keeps TypeScript's expansion",
  { skip },
  async () => {
    // What an rl pattern binding compiles to. TypeScript cannot rewrite
    // `{ value }` to the new name alone — it expands the shorthand — and
    // the engine carries that expansion in `newText`, or the rename would
    // silently rebind a different field.
    const { rl } = workspace();
    const source = [
      'import { describe } from "./user";',
      'export function render(o: { value: "idle" | "loading" }): string {',
      "  const { value } = o;",
      "  return describe(value);",
      "}",
      "",
    ].join("\n");
    engine.openDocument(COMPILER, rl, source);
    const edits = await engine.rename(
      COMPILER,
      rl,
      positionAt(source, source.indexOf("value } = o")),
    );
    assert.ok(edits, "the binding can be renamed");
    const declaration = edits!.find(
      (e) => spanOf(source, e.range).start === source.indexOf("value } = o"),
    );
    assert.ok(declaration, JSON.stringify(edits));
    assert.equal(declaration!.newText, "value: rlRenamePlaceholder");
    // An ordinary use is replaced by the bare name.
    const use = edits!.find(
      (e) => spanOf(source, e.range).start === source.indexOf("value)"),
    );
    assert.ok(use, JSON.stringify(edits));
    assert.ok(
      use!.newText === null || use!.newText === "rlRenamePlaceholder",
      JSON.stringify(use),
    );
    engine.closeDocument(COMPILER, rl);
  },
);

test("what cannot be renamed answers null rather than nothing", { skip }, async () => {
  const { rl } = workspace();
  // A comment is not a rename target; the engine says so, and answering
  // "no edits" instead would look like a rename that changed nothing.
  assert.equal(
    await engine.rename(
      COMPILER,
      rl,
      positionAt(RENDER, RENDER.indexOf("rename target")),
    ),
    null,
  );
});

test("signature help describes the call being written", { skip }, async () => {
  const { rl } = workspace();
  const help = await engine.signatureHelp(
    COMPILER,
    rl,
    positionAt(RENDER, RENDER.indexOf("describe(state)") + "describe(".length),
  );
  assert.ok(help, "the call site has help");
  assert.match(help!.signatures[0].label, /describe/);
  assert.equal(help!.signatures[0].parameters.length, 1);
});

test("an edit is answered against the new text", { skip }, async () => {
  const { rl } = workspace();
  engine.openDocument(COMPILER, rl, RENDER);
  assert.ok(
    (await engine.tsDiagnostics(COMPILER, rl)).some((d) => d.code === 2322),
  );
  engine.updateDocument(
    COMPILER,
    rl,
    RENDER.replace("  const bad: number = label;\n", ""),
  );
  assert.equal(
    (await engine.tsDiagnostics(COMPILER, rl)).filter((d) => d.code === 2322)
      .length,
    0,
    "the fix is seen without restarting anything",
  );
  engine.closeDocument(COMPILER, rl);
});

test("a closed document is the disk's again", { skip }, async () => {
  const { rl } = workspace();
  engine.openDocument(
    COMPILER,
    rl,
    RENDER.replace("  const bad: number = label;\n", ""),
  );
  assert.equal(
    (await engine.tsDiagnostics(COMPILER, rl)).filter((d) => d.code === 2322)
      .length,
    0,
  );
  engine.closeDocument(COMPILER, rl);
  // The disk copy still has the error; closing dropped the overlay.
  assert.ok(
    (await engine.tsDiagnostics(COMPILER, rl)).some((d) => d.code === 2322),
  );
});

/* Hints need no TypeScript at all — they are the parse-only surface, like
 * semantic tokens — so they answer wherever rlc itself is available. */
const skipRlOnly = compilerAvailable() ? false : "rlc not on PATH";

const DEAD_ARM = [
  "enum Shape {",
  "  Circle(radius: number),",
  "  Rect(width: number, height: number),",
  "}",
  "",
  "declare const shape: Shape;",
  "",
  "const area = match (shape) {",
  "  Circle(radius) => radius,",
  "  Rect(width) => width,",
  "  Circle(radius: r) => r,",
  "};",
  "",
].join("\n");

test("an unreachable arm comes back as a hint, not a diagnostic", { skip: skipRlOnly }, async () => {
  const { dir } = workspace();
  const file = path.join(dir, "src/dead.rl");
  fs.writeFileSync(file, DEAD_ARM);
  const hints = await engine.rlHints(COMPILER, file, DEAD_ARM);
  assert.equal(hints.length, 1, JSON.stringify(hints));
  assert.equal(hints[0].kind, "unreachableArm");
  assert.equal(sliceOf(DEAD_ARM, hints[0].range), "Circle(radius: r) => r");
});

test("a live match has no hints", { skip: skipRlOnly }, async () => {
  const { dir, rl } = workspace();
  assert.deepEqual(await engine.rlHints(COMPILER, rl, RENDER), []);
  const live = path.join(dir, "src/live.rl");
  const text = DEAD_ARM.replace("  Circle(radius: r) => r,\n", "");
  fs.writeFileSync(live, text);
  assert.deepEqual(await engine.rlHints(COMPILER, live, text), []);
});
