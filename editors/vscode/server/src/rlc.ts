/* --------------------------------------------------------------------------
 * Diagnostics come from the real compiler: the buffer is written to a temp
 * file and `rlc --check` is run on it, so editor errors are byte-for-byte
 * the compiler's own (`file:line:col: message` — docs/reference/errors.md).
 * ----------------------------------------------------------------------- */
import { execFile } from "child_process";
import * as crypto from "crypto";
import * as fs from "fs";
import * as os from "os";
import * as path from "path";

export interface RlcDiagnostic {
  /** 1-based; 0 means "no position" (output-verification errors). */
  line: number;
  /** 1-based; 0 means "no position". */
  col: number;
  message: string;
}

export type RlcResult =
  | { kind: "ok"; diagnostics: RlcDiagnostic[] }
  | { kind: "not-found"; compiler: string }
  | { kind: "failed"; detail: string };

let tmpDir: string | null = null;
function tempDir(): string {
  if (tmpDir === null) {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "rl-lsp-"));
  }
  return tmpDir;
}

const CANDIDATE_PATHS = [
  path.join("target", "release", "rlc"),
  path.join("target", "debug", "rlc"),
  path.join("target", "release", "rlc.exe"),
  path.join("target", "debug", "rlc.exe"),
];

/**
 * Resolve the compiler to run: explicit setting > locally built binary in a
 * workspace root > `rlc` on PATH.
 */
export function findCompiler(
  configuredPath: string,
  workspaceRoots: string[],
): string {
  if (configuredPath.trim() !== "") return configuredPath.trim();
  for (const root of workspaceRoots) {
    for (const rel of CANDIDATE_PATHS) {
      const candidate = path.join(root, rel);
      try {
        if (fs.existsSync(candidate)) return candidate;
      } catch {
        // ignore and keep looking
      }
    }
  }
  return "rlc";
}

/** Run `rlc --check` on the buffer contents and parse stderr diagnostics. */
export function runCheck(
  compiler: string,
  text: string,
  docName: string,
  verify: boolean,
): Promise<RlcResult> {
  const rawBase = path.basename(docName).replace(/[^\w.-]/g, "_") || "buffer";
  const base = rawBase.endsWith(".rl") ? rawBase : `${rawBase}.rl`;
  const hash = crypto.createHash("sha1").update(docName).digest("hex");
  const file = path.join(tempDir(), `${hash.slice(0, 8)}-${base}`);

  try {
    fs.writeFileSync(file, text);
  } catch (e) {
    return Promise.resolve({ kind: "failed", detail: String(e) });
  }

  const args = ["--check"];
  if (!verify) args.push("--no-verify");
  args.push(file);

  return new Promise((resolve) => {
    execFile(
      compiler,
      args,
      { timeout: 15000, maxBuffer: 4 * 1024 * 1024 },
      (err, _stdout, stderr) => {
        if (err && (err as NodeJS.ErrnoException).code === "ENOENT") {
          resolve({ kind: "not-found", compiler });
          return;
        }
        const diagnostics = parseStderr(String(stderr), file);
        if (err && diagnostics.length === 0) {
          // Crashed or timed out without a parseable diagnostic.
          resolve({
            kind: "failed",
            detail: `${compiler} exited abnormally: ${String(stderr).trim() || err.message}`,
          });
          return;
        }
        resolve({ kind: "ok", diagnostics });
      },
    );
  });
}

/* ----------------------------------------------------------------------
 * Symbol interface (`rlc --symbols`, docs/reference/cli.md): the compiler
 * reports a file's rl enum declarations (with 1-based positions) and its
 * direct relative `.rl` imports, including each referenced file's exported
 * declarations — the server consumes this for cross-file features instead
 * of re-implementing import resolution.
 * -------------------------------------------------------------------- */

export interface SymbolsField {
  name: string;
  optional: boolean;
  type: string;
}

export interface SymbolsCase {
  tag: string;
  line: number;
  col: number;
  /** null for a unit case without parens. */
  fields: SymbolsField[] | null;
}

export interface SymbolsEnum {
  name: string;
  exported: boolean;
  generics: string;
  line: number;
  col: number;
  cases: SymbolsCase[];
}

export type SymbolsNames =
  | { kind: "namespace"; name: string }
  | { kind: "named"; entries: { name: string; alias: string | null }[] }
  | { kind: "none" };

export interface SymbolsImport {
  specifier: string;
  names: SymbolsNames;
  /** Path the specifier resolved to, or null if unreadable. */
  resolved: string | null;
  enums: SymbolsEnum[];
}

export interface SymbolsFile {
  file: string;
  enums: SymbolsEnum[];
  imports: SymbolsImport[];
}

/**
 * Run `rlc --symbols` on a file on disk. Returns null when the compiler is
 * missing, predates `--symbols`, or the output is unparseable — callers
 * degrade to single-file behavior.
 */
export function runSymbols(
  compiler: string,
  file: string,
): Promise<SymbolsFile | null> {
  return new Promise((resolve) => {
    execFile(
      compiler,
      ["--symbols", file],
      { timeout: 15000, maxBuffer: 16 * 1024 * 1024 },
      (err, stdout) => {
        if (err) {
          resolve(null);
          return;
        }
        try {
          const parsed = JSON.parse(String(stdout)) as SymbolsFile[];
          resolve(parsed[0] ?? null);
        } catch {
          resolve(null);
        }
      },
    );
  });
}

/* ----------------------------------------------------------------------
 * Emit map (`rlc --emit-map`, docs/reference/cli.md): the compiler emits
 * the buffer's TypeScript (parse + emit only — no rl-level checks, `.rl`
 * specifiers untouched) plus byte mappings for every chunk copied verbatim
 * from the source. The server serves this as the buffer's virtual document
 * to the TypeScript language service (TASK-048).
 * -------------------------------------------------------------------- */

export interface EmitMapResult {
  code: string;
  mappings: { src: number; out: number; len: number }[];
}

/**
 * Run `rlc --emit-map` on the buffer contents. Returns null when the
 * compiler is missing, predates `--emit-map`, or the output is
 * unparseable — callers degrade to serving the raw text.
 */
export function runEmitMap(
  compiler: string,
  text: string,
  docName: string,
): Promise<EmitMapResult | null> {
  const rawBase = path.basename(docName).replace(/[^\w.-]/g, "_") || "buffer";
  const base = rawBase.endsWith(".rl") ? rawBase : `${rawBase}.rl`;
  const hash = crypto.createHash("sha1").update(docName).digest("hex");
  const file = path.join(tempDir(), `em-${hash.slice(0, 8)}-${base}`);

  try {
    fs.writeFileSync(file, text);
  } catch {
    return Promise.resolve(null);
  }

  return new Promise((resolve) => {
    execFile(
      compiler,
      ["--emit-map", file],
      { timeout: 15000, maxBuffer: 64 * 1024 * 1024 },
      (err, stdout) => {
        if (err) {
          resolve(null);
          return;
        }
        try {
          const parsed = JSON.parse(String(stdout)) as (EmitMapResult & {
            file: string;
          })[];
          const entry = parsed[0];
          resolve(
            entry && typeof entry.code === "string"
              ? { code: entry.code, mappings: entry.mappings ?? [] }
              : null,
          );
        } catch {
          resolve(null);
        }
      },
    );
  });
}

/** Parse `rlc: <file>:<line>:<col>: <msg>` / `rlc: <file>: <msg>` lines. */
export function parseStderr(stderr: string, file: string): RlcDiagnostic[] {
  const diagnostics: RlcDiagnostic[] = [];
  for (const line of stderr.split("\n")) {
    if (!line.startsWith("rlc: ")) continue;
    const rest = line.slice(5);
    if (!rest.startsWith(file)) continue; // progress logs, other files
    const tail = rest.slice(file.length);
    let m = /^:(\d+):(\d+): (.*)$/.exec(tail);
    if (m) {
      diagnostics.push({
        line: Number(m[1]),
        col: Number(m[2]),
        message: m[3],
      });
      continue;
    }
    m = /^: (.*)$/.exec(tail);
    if (m) diagnostics.push({ line: 0, col: 0, message: m[1] });
  }
  return diagnostics;
}
