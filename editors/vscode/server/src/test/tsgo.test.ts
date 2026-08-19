/* Tests for TypeScript delegation through the compiler's own language
 * server. They drive a real `tsgo`, so they skip when one is not
 * resolvable — the guard starts the same server the feature starts, so a
 * skip means "no compiler", never "the delegation quietly answered
 * nothing". */
import * as assert from "node:assert/strict";
import { test } from "node:test";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import { TsgoProject, documentUri, fileNameOf } from "../tsgo";
import type { OpenDoc } from "../tstypes";
import { findTsgo } from "./toolchain";

const TSGO = findTsgo();
const skip = TSGO === null ? "no tsgo executable" : false;

/** A workspace with a hand-written `.ts` and the TypeScript an `.rl` file
 * lowers to — the second is served from memory, never written. */
function workspace(): { dir: string; rl: string; lowered: string } {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "rl-tsgo-test-"));
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
  fs.writeFileSync(rl, "// the source; what the server sees is the lowering below\n");
  const lowered = [
    "// nothing here is a rename target",
    'import { describe } from "./user";',
    "export function render(state: \"idle\" | \"loading\"): string {",
    "  const label = describe(state);",
    "  const bad: number = label;",
    "  return label;",
    "}",
    "",
  ].join("\n");
  return { dir, rl, lowered };
}

function project(dir: string, docs: Map<string, string>): TsgoProject {
  return new TsgoProject(TSGO as string, dir, (fileName) => {
    const text = docs.get(fileName);
    return text === undefined ? null : ({ text, version: 1 } as OpenDoc);
  });
}

test("a document is named the way the compiler names it", () => {
  assert.equal(documentUri("/p/src/x.rl"), "file:///p/src/x.rl.ts");
  assert.equal(documentUri("/p/src/x.ts"), "file:///p/src/x.ts");
  assert.equal(fileNameOf("file:///p/src/x.rl.ts"), "/p/src/x.rl");
  assert.equal(fileNameOf("file:///p/src/x.ts"), "/p/src/x.ts");
  // The compiler's own library lives inside the executable, under a URI no
  // editor can open. An answer about it is not an answer about a file.
  assert.equal(fileNameOf("bundled:///libs/lib.es5.d.ts"), null);
});

test("a definition is only offered when a file is behind it", { skip }, async () => {
  const { dir, rl, lowered } = workspace();
  // `length` is declared in the standard library, which a built
  // typescript-go carries *inside* the executable and names
  // `bundled:///libs/lib.es5.d.ts` — there is no document for an editor to
  // open. A toolchain that reads its libraries off disk answers with a real
  // path instead. Either is fine; what must never happen is a location an
  // editor is told to open and cannot.
  const text = lowered.replace("return label;", "return label.length;");
  const ts = project(dir, new Map([[rl, text]]));
  try {
    for (const definition of await ts.definitionsAt(rl, text.indexOf("length;"))) {
      assert.ok(fs.existsSync(definition.fileName), definition.fileName);
      assert.equal(
        definition.fileText.slice(
          definition.start,
          definition.start + definition.length,
        ),
        "length",
      );
    }
  } finally {
    ts.dispose();
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("hover answers for a buffer the disk never saw", { skip }, async () => {
  const { dir, rl, lowered } = workspace();
  const docs = new Map([[rl, lowered]]);
  const ts = project(dir, docs);
  try {
    const info = await ts.quickInfoAt(rl, lowered.indexOf("describe(state)"));
    assert.ok(info, "hover has an answer");
    assert.match(info.signature, /describe/);
    assert.match(info.signature, /State/);
  } finally {
    ts.dispose();
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("definition crosses into the hand-written file on disk", { skip }, async () => {
  const { dir, rl, lowered } = workspace();
  const ts = project(dir, new Map([[rl, lowered]]));
  try {
    const definitions = await ts.definitionsAt(rl, lowered.indexOf("describe(state)"));
    assert.equal(definitions.length, 1, JSON.stringify(definitions));
    assert.equal(definitions[0].fileName, path.join(dir, "src/user.ts"));
    // The span names the declaration, in the target file's own coordinates.
    const target = definitions[0];
    assert.equal(
      target.fileText.slice(target.start, target.start + target.length),
      "describe",
    );
  } finally {
    ts.dispose();
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("diagnostics are pulled for the served text", { skip }, async () => {
  const { dir, rl, lowered } = workspace();
  const ts = project(dir, new Map([[rl, lowered]]));
  try {
    const diagnostics = await ts.diagnosticsFor(rl);
    const error = diagnostics.find((d) => d.code === 2322);
    assert.ok(error, JSON.stringify(diagnostics));
    // The span is in the lowered text's coordinates — the caller maps it
    // back to the `.rl` source with the emit map.
    assert.equal(lowered.slice(error.start, error.start + error.length), "bad");
  } finally {
    ts.dispose();
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("completions come from the real project", { skip }, async () => {
  const { dir, rl, lowered } = workspace();
  const ts = project(dir, new Map([[rl, lowered]]));
  try {
    const completions = await ts.completionsAt(rl, lowered.indexOf("label;"));
    assert.ok(completions.entries.length > 0, "the list is not empty");
    assert.ok(
      completions.entries.some((entry) => entry.name === "describe"),
      "and it holds what the file imported",
    );
  } finally {
    ts.dispose();
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("references find the uses in the served text", { skip }, async () => {
  const { dir, rl, lowered } = workspace();
  const ts = project(dir, new Map([[rl, lowered]]));
  try {
    const references = await ts.referencesAt(rl, lowered.indexOf("label = describe"));
    assert.ok(references.length >= 2, JSON.stringify(references));
    for (const reference of references) {
      assert.equal(reference.fileName, rl, "all of them in the .rl file");
      assert.equal(
        reference.fileText.slice(reference.start, reference.start + reference.length),
        "label",
      );
    }
  } finally {
    ts.dispose();
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("rename names every place the name is written", { skip }, async () => {
  const { dir, rl, lowered } = workspace();
  const ts = project(dir, new Map([[rl, lowered]]));
  try {
    const locations = await ts.renameAt(rl, lowered.indexOf("label = describe"));
    assert.ok(locations, "the binding can be renamed");
    // Three uses of `label` in the lowered text; every span has to name the
    // identifier, because the caller maps each one back to the .rl source
    // and refuses the rename if any of them cannot be mapped.
    assert.equal(locations.length, 3, JSON.stringify(locations));
    for (const location of locations) {
      assert.equal(
        location.fileText.slice(location.start, location.start + location.length),
        "label",
      );
    }
  } finally {
    ts.dispose();
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("what cannot be renamed answers null rather than nothing", { skip }, async () => {
  const { dir, rl, lowered } = workspace();
  const ts = project(dir, new Map([[rl, lowered]]));
  try {
    // A comment is not a rename target; the server says so, and answering
    // "no locations" instead would look like a rename that changed nothing.
    assert.equal(await ts.renameAt(rl, lowered.indexOf("rename target")), null);
  } finally {
    ts.dispose();
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("signature help describes the call being written", { skip }, async () => {
  const { dir, rl, lowered } = workspace();
  const ts = project(dir, new Map([[rl, lowered]]));
  try {
    const help = await ts.signatureHelpAt(rl, lowered.indexOf("describe(state)") + "describe(".length);
    assert.ok(help, "the call site has help");
    assert.match(help.signatures[0].label, /describe/);
    assert.equal(help.signatures[0].parameters.length, 1);
  } finally {
    ts.dispose();
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("an edit is answered against the new text", { skip }, async () => {
  const { dir, rl, lowered } = workspace();
  const docs = new Map([[rl, lowered]]);
  const ts = project(dir, docs);
  try {
    assert.ok((await ts.diagnosticsFor(rl)).some((d) => d.code === 2322));
    docs.set(rl, lowered.replace("  const bad: number = label;\n", ""));
    assert.equal(
      (await ts.diagnosticsFor(rl)).filter((d) => d.code === 2322).length,
      0,
      "the fix is seen without restarting anything",
    );
  } finally {
    ts.dispose();
    fs.rmSync(dir, { recursive: true, force: true });
  }
});
