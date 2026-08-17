/* --------------------------------------------------------------------------
 * On-save regeneration of the editor sidecars (`x.rl.d.ts` + `.map`).
 *
 * A `.ts` file importing `"./x.rl"` only type-checks when a declaration file
 * sits next to the module, and "go to definition" only lands in the original
 * when that declaration carries a map whose `sources` is the `.rl` file
 * (see `docs/reference/cli.md`, "에디터 사이드카").
 *
 * `rlc --sidecar` builds both, but it needs the declaration *body*, which
 * requires type inference — tsc's job. So this module does the middle step:
 * compile the saved `.rl` with rlc, run TypeScript's declaration emit over
 * that output in memory, and hand the result back to `rlc --sidecar`.
 * ----------------------------------------------------------------------- */
import { execFile } from "node:child_process";
import * as crypto from "node:crypto";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import * as ts from "typescript";

/** What to do when an `.rl` file is saved. */
export type SidecarMode = "off" | "refresh" | "always";

/** Outcome of one refresh, for logging and tests. */
export type SidecarResult =
  | { kind: "written"; files: string[] }
  | { kind: "skipped"; reason: string }
  | { kind: "failed"; detail: string };

const DECLARATION_OPTIONS: ts.CompilerOptions = {
  target: ts.ScriptTarget.ES2022,
  module: ts.ModuleKind.Preserve,
  moduleResolution: ts.ModuleResolutionKind.Bundler,
  allowImportingTsExtensions: true,
  declaration: true,
  emitDeclarationOnly: true,
  skipLibCheck: true,
  strict: true,
};

/**
 * Rebuilds the sidecar for one `.rl` file.
 *
 * In `"refresh"` mode (the default) a sidecar is only rewritten when one is
 * already there — a project opts in by running `rlc --sidecar` once, and
 * nothing appears in a workspace that never asked for it. `"always"` creates
 * it on first save.
 */
export async function refreshSidecar(
  compiler: string,
  rlPath: string,
  mode: SidecarMode,
): Promise<SidecarResult> {
  if (mode === "off") return { kind: "skipped", reason: "disabled" };
  if (!rlPath.endsWith(".rl")) return { kind: "skipped", reason: "not an .rl file" };

  const declarationTarget = `${rlPath}.d.ts`;
  if (mode === "refresh" && !exists(declarationTarget)) {
    return { kind: "skipped", reason: "no sidecar to refresh" };
  }

  const compiled = await compileToTs(compiler, rlPath);
  if (compiled.kind !== "ok") return compiled;

  const declarations = emitDeclarations(rlPath, compiled.text);
  if (declarations === undefined) {
    return { kind: "failed", detail: "TypeScript emitted no declarations" };
  }

  const stem = path.basename(rlPath, ".rl");
  const scratch = path.join(
    os.tmpdir(),
    `rl-sidecar-${crypto.createHash("sha1").update(rlPath).digest("hex").slice(0, 12)}`,
  );
  try {
    fs.mkdirSync(scratch, { recursive: true });
    fs.writeFileSync(path.join(scratch, `${stem}.d.ts`), declarations);
  } catch (e) {
    return { kind: "failed", detail: String(e) };
  }

  const written = await runSidecar(compiler, scratch, rlPath);
  try {
    fs.rmSync(scratch, { recursive: true, force: true });
  } catch {
    // a leftover temp directory is harmless
  }
  return written;
}

function exists(file: string): boolean {
  try {
    return fs.existsSync(file);
  } catch {
    return false;
  }
}

type Compiled = { kind: "ok"; text: string } | SidecarResult;

/** `rlc -p --rewrite-imports ts <file>` — the TypeScript rlc would write. */
function compileToTs(compiler: string, rlPath: string): Promise<Compiled> {
  return new Promise((resolve) => {
    execFile(
      compiler,
      ["-p", "--rewrite-imports", "ts", rlPath],
      { timeout: 15000, maxBuffer: 8 * 1024 * 1024 },
      (err, stdout, stderr) => {
        if (err) {
          const detail = stderr.trim() || String(err);
          resolve({ kind: "failed", detail });
          return;
        }
        resolve({ kind: "ok", text: stdout });
      },
    );
  });
}

/**
 * Declaration emit over rlc's output, in memory.
 *
 * The compiled text is served at the path rlc would have written it to, so
 * its relative imports resolve against the real sibling files.
 */
function emitDeclarations(rlPath: string, compiled: string): string | undefined {
  const emittedPath = path.join(path.dirname(rlPath), `${path.basename(rlPath, ".rl")}.ts`);
  const host = ts.createCompilerHost(DECLARATION_OPTIONS, true);
  const readFile = host.readFile.bind(host);
  const fileExists = host.fileExists.bind(host);
  const getSourceFile = host.getSourceFile.bind(host);

  host.fileExists = (file) => (samePath(file, emittedPath) ? true : fileExists(file));
  host.readFile = (file) => (samePath(file, emittedPath) ? compiled : readFile(file));
  host.getSourceFile = (file, languageVersion, onError, shouldCreate) =>
    samePath(file, emittedPath)
      ? ts.createSourceFile(file, compiled, languageVersion, true, ts.ScriptKind.TS)
      : getSourceFile(file, languageVersion, onError, shouldCreate);

  let declarations: string | undefined;
  host.writeFile = (file, text) => {
    if (file.endsWith(".d.ts")) declarations = text;
  };

  const program = ts.createProgram([emittedPath], DECLARATION_OPTIONS, host);
  program.emit(undefined, undefined, undefined, true);
  return declarations;
}

function samePath(a: string, b: string): boolean {
  return path.resolve(a) === path.resolve(b);
}

/** `rlc --sidecar <dir> <file.rl>` — writes the two files next to the source. */
function runSidecar(
  compiler: string,
  declarationDir: string,
  rlPath: string,
): Promise<SidecarResult> {
  return new Promise((resolve) => {
    execFile(
      compiler,
      ["--sidecar", declarationDir, rlPath],
      { timeout: 15000, maxBuffer: 4 * 1024 * 1024 },
      (err, _stdout, stderr) => {
        if (err) {
          resolve({ kind: "failed", detail: stderr.trim() || String(err) });
          return;
        }
        resolve({
          kind: "written",
          files: [`${rlPath}.d.ts`, `${rlPath}.d.ts.map`],
        });
      },
    );
  });
}
