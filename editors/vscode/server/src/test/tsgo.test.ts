/* Tests for TypeScript delegation through the compiler's own language
 * server. They drive a real `tsgo`, so they skip when one is not
 * resolvable — the guard starts the same server the feature starts, so a
 * skip means "no compiler", never "the delegation quietly answered
 * nothing". */
import * as assert from "node:assert/strict";
import { test } from "node:test";
import { execFileSync } from "node:child_process";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import { TsgoProject, documentUri, fileNameOf } from "../tsgo";
import type { OpenDoc } from "../tsproject";

/**
 * The `tsgo` executable, from the same places rlc looks: a built
 * typescript-go checkout, or the executable an installed package ships.
 */
function findTsgo(): string | null {
  const root = process.env.RLC_TSGO_ROOT;
  if (root) {
    const built = path.join(root, "built/local/tsgo");
    if (fs.existsSync(built)) return built;
  }
  for (const dir of [process.cwd(), path.join(process.cwd(), "../../..")]) {
    for (const pkg of ["@typescript/typescript-linux-x64", "@typescript/native-preview-linux-x64"]) {
      const exe = path.join(dir, "node_modules", pkg, "lib", "tsc");
      if (fs.existsSync(exe)) return exe;
    }
  }
  return null;
}

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
