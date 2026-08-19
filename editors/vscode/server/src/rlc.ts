/* --------------------------------------------------------------------------
 * Diagnostics come from the real compiler: the buffer is written to a temp
 * file and `rlc --check` is run on it, so editor errors are byte-for-byte
 * the compiler's own (`file:line:col: message` — docs/reference/errors.md).
 * ----------------------------------------------------------------------- */
import {
  execFile,
  execFileSync,
  ExecFileSyncOptionsWithStringEncoding,
} from "child_process";
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

/**
 * The `tsgo` executable the TypeScript delegation runs, from the same places
 * the compiler looks: a built typescript-go checkout named by
 * `RLC_TSGO_ROOT`, or the executable an installed `typescript` package ships
 * beside its own files. Empty when there is none — the delegation then has
 * no answers, which the caller reports rather than hides.
 */
export function findTsgo(workspaceRoots: string[]): string {
  const root = process.env.RLC_TSGO_ROOT;
  if (root) {
    const built = path.join(root, "built/local/tsgo");
    if (exists(built)) return built;
  }
  const platform = `${process.platform}-${process.arch}`;
  // The workspace first — a project that pins a TypeScript pins the
  // compiler its own code is checked by — and the server's own directory
  // after it, which is where a toolchain installed for the editor sits.
  for (const start of [...workspaceRoots, process.cwd()]) {
    let dir = start;
    while (true) {
      for (const pkg of [
        `@typescript/typescript-${platform}`,
        `@typescript/native-preview-${platform}`,
      ]) {
        const exe = path.join(dir, "node_modules", pkg, "lib", "tsc");
        if (exists(exe)) return exe;
      }
      const parent = path.dirname(dir);
      if (parent === dir) break;
      dir = parent;
    }
  }
  return "";
}

function exists(file: string): boolean {
  try {
    return fs.existsSync(file);
  } catch {
    return false;
  }
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
 * to the TypeScript language service (TASK-050).
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

/* --------------------------------------------------------------------------
 * Synchronous compiler calls for the TypeScript language-service host
 * (TASK-055). The host's `readFile`/`fileExists` are synchronous, so the two
 * things it needs from the compiler — the standard library module and the
 * emitted TypeScript of an imported `.rl` file — are produced with
 * `execFileSync` and cached. Both are keyed so a call happens at most once
 * per compiler path (std) or per file mtime (modules).
 * -------------------------------------------------------------------- */

const EXEC_SYNC_OPTIONS: ExecFileSyncOptionsWithStringEncoding = {
  encoding: "utf8",
  timeout: 15000,
  maxBuffer: 64 * 1024 * 1024,
  // stderr stays out of the server's own stderr; only stdout is read.
  stdio: ["ignore", "pipe", "ignore"],
};

const stdModules = new Map<string, string | null>();

/**
 * The standard library module (`rlc --emit-std`) materialized in the temp
 * directory, so `"@rl/std"` resolves to a real file even in a project that
 * has never run `rlc --types`. Null when the compiler is missing or
 * predates `--emit-std` — the caller then leaves the specifier unresolved.
 */
/**
 * Makes `"@rl/std"` resolvable in `root`, by putting the standard library
 * where TypeScript looks for a package of that name.
 *
 * The compiler serves the library from memory; a language server cannot —
 * module resolution reads the file system, not the buffers a client has
 * opened. So for the editor the library has to *be* there. Only our own
 * scoped package is ever written, and never over one that already exists:
 * a project that installs `@rl/std` itself keeps its own copy.
 *
 * Returns the package directory, or null when the library could not be
 * produced (no compiler) or already resolves on its own.
 */
export function ensureStdModule(compiler: string, root: string): string | null {
  const pkg = path.join(root, "node_modules", "@rl", "std");
  const entry = path.join(pkg, "index.ts");
  if (exists(entry)) return pkg;
  if (exists(pkg)) return null; // something else owns the name

  const source = stdModuleSource(compiler);
  if (source === null) return null;
  try {
    fs.mkdirSync(pkg, { recursive: true });
    fs.writeFileSync(entry, source);
    fs.writeFileSync(
      path.join(pkg, "package.json"),
      JSON.stringify({ name: "@rl/std", version: "0.0.0", types: "index.ts" }, null, 2) + "\n",
    );
    return pkg;
  } catch {
    return null;
  }
}

/** The standard library's source, as the compiler writes it. */
function stdModuleSource(compiler: string): string | null {
  try {
    const code = String(execFileSync(compiler, ["--emit-std"], EXEC_SYNC_OPTIONS));
    return code.trim() === "" ? null : code;
  } catch {
    return null;
  }
}

export function stdModulePath(compiler: string): string | null {
  const cached = stdModules.get(compiler);
  if (cached !== undefined) return cached;

  let resolved: string | null = null;
  try {
    const code = String(
      execFileSync(compiler, ["--emit-std"], EXEC_SYNC_OPTIONS),
    );
    if (code.trim() !== "") {
      const hash = crypto.createHash("sha1").update(compiler).digest("hex");
      const file = path.join(tempDir(), `std-${hash.slice(0, 8)}.ts`);
      fs.writeFileSync(file, code);
      resolved = file;
    }
  } catch {
    resolved = null;
  }
  stdModules.set(compiler, resolved);
  return resolved;
}

/**
 * `rlc --emit-map` for an `.rl` file on disk — an imported module the editor
 * does not have open. Serving its emitted TypeScript (instead of the raw
 * source, which TypeScript can only error-recover through) is what makes an
 * imported rl enum a tagged union rather than a misparsed TS `enum`.
 * Null when the compiler is missing, too old, or the output is unparseable.
 */
export function runEmitMapFileSync(
  compiler: string,
  file: string,
): EmitMapResult | null {
  let stdout: string;
  try {
    stdout = String(
      execFileSync(compiler, ["--emit-map", file], EXEC_SYNC_OPTIONS),
    );
  } catch {
    return null;
  }
  try {
    const parsed = JSON.parse(stdout) as (EmitMapResult & { file: string })[];
    const entry = parsed[0];
    return entry && typeof entry.code === "string"
      ? { code: entry.code, mappings: entry.mappings ?? [] }
      : null;
  } catch {
    return null;
  }
}
