/* --------------------------------------------------------------------------
 * Diagnostics come from the real compiler: through the engine session
 * (`rlc --server`, see engine.ts) when one is available, and through the
 * one-shot commands otherwise — the two produce the same diagnostics, so
 * the fallback is invisible. Editor errors are byte-for-byte the compiler's
 * own (`file:line:col: message` — docs/reference/errors.md).
 * ----------------------------------------------------------------------- */
import { execFile } from "child_process";
import * as crypto from "crypto";
import * as fs from "fs";
import * as os from "os";
import * as path from "path";

import { devPackageCompiler, rlcSpawnEnv } from "./dev";
import { engineRequest } from "./engine";

/** A typed check opens a project and starts a compiler; it is slower than
 * `--check` by that much, and a project that will not open must not hold the
 * editor forever. */
const TYPED_CHECK_TIMEOUT_MS = 30000;

export interface RlcDiagnostic {
  /** 1-based; 0 means "no position" (output-verification errors). */
  line: number;
  /** 1-based; 0 means "no position". */
  col: number;
  /** End of the range the diagnostic covers, past its last character;
   * 1-based, 0 (or absent) when the compiler reported a position only —
   * the squiggle then falls back to the word at the position. The compiler
   * sends this for a construct it knows the extent of, so the underline
   * covers `try parse(text)` or `match (shape)` as a whole. */
  endLine?: number;
  /** See {@link RlcDiagnostic.endLine}. */
  endCol?: number;
  message: string;
  /** Stable rl rule identity; older/one-shot compilers may omit it. */
  code?: string;
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
 * workspace root > the rlc of a `file:`-installed local rl-lang package
 * (a test project set up via `scripts/setup`, see dev.ts) > `rlc` on PATH.
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
  const dev = devPackageCompiler(workspaceRoots);
  if (dev !== "") return dev;
  return "rlc";
}

/**
 * Whether a TypeScript language toolchain is around, from the same places
 * the engine looks: a built typescript-go checkout named by
 * `RLC_TSGO_ROOT`, or the executable an installed TypeScript package ships
 * beside its own files. The engine resolves its own; this mirror exists so
 * the tests can tell "the feature answered nothing" from "the toolchain is
 * missing" — and skip, not fail, on the latter.
 */
export function findTsgo(workspaceRoots: string[]): string {
  const root = process.env.RLC_TSGO_ROOT;
  if (root) {
    const built = path.join(root, "built/local/tsgo");
    if (exists(built)) return built;
  }
  const platform = `${process.platform}-${process.arch}`;
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
export async function runCheck(
  compiler: string,
  text: string,
  docName: string,
  verify: boolean,
): Promise<RlcResult> {
  // The engine server answers with the same diagnostics and no process
  // spawn; the one-shot below is the fallback and the reference.
  const answer = await engineRequest(
    compiler,
    "check",
    { text, filename: path.basename(docName), verify },
    15000,
  );
  if (answer && "result" in answer) {
    const result = answer.result as { diagnostics?: RlcDiagnostic[] };
    return {
      kind: "ok",
      diagnostics: (result.diagnostics ?? []).map((d) => ({
        line: d.line,
        col: d.col,
        endLine: d.endLine,
        endCol: d.endCol,
        message: d.message,
        code: d.code,
      })),
    };
  }
  return runCheckOnce(compiler, text, docName, verify);
}

/** The one-shot `rlc --check`, via a temp file. */
function runCheckOnce(
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
      { timeout: 15000, maxBuffer: 4 * 1024 * 1024, env: rlcSpawnEnv(compiler) },
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
      { timeout: 15000, maxBuffer: 16 * 1024 * 1024, env: rlcSpawnEnv(compiler) },
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

/**
 * The typed half of the rl diagnostics, for the buffer as it stands.
 *
 * `rlc --check` answers everything rl can decide from the text alone. What
 * it cannot decide is what a TypeScript *type* says — whether `m.set(...)`
 * calls a built-in mutator through a `val` binding, whether a scrutinee's
 * type still allows a case (`language.md` §10.4). Those answers come from
 * the engine's typed pass over the live project, or — as the fallback —
 * from `rlc --check-types --rl-only --overlay`.
 *
 * Every message is the compiler's own, verbatim: the editor decides nothing
 * about `val`, it relays what rlc said (CLAUDE.md, error layers).
 */
export type ValCheckResult =
  | { kind: "ok"; diagnostics: RlcDiagnostic[] }
  /** The check could not run (no toolchain, crash, or nothing to check).
   * Distinct from "ran and found nothing": the caller keeps what it has. */
  | { kind: "unavailable"; detail: string };

export async function runTypedCheck(
  compiler: string,
  text: string,
  fsPath: string,
  includeTypes = false,
): Promise<ValCheckResult> {
  // The overlay stands in for a file of the project, so there has to be one:
  // a buffer that was never saved has no place in the project graph yet.
  if (!exists(fsPath)) {
    return { kind: "unavailable", detail: "not on disk yet" };
  }
  // The engine session keeps the project — and the TypeScript compiler
  // behind it — alive between checks, so this answers in milliseconds
  // where the one-shot pays a project open every time. Same diagnostics
  // either way; the outcome mapping below mirrors the one-shot's exactly.
  const answer = await engineRequest(
    compiler,
    "typedCheck",
    { path: fsPath, text, includeTypes },
    TYPED_CHECK_TIMEOUT_MS,
  );
  if (answer && "error" in answer) {
    // The session ran and the request failed (no toolchain, a backend
    // crash) — what the one-shot reports as "could not run".
    return { kind: "unavailable", detail: answer.error };
  }
  if (answer && "result" in answer) {
    const result = answer.result as {
      blocked?: boolean;
      diagnostics?: {
        path: string;
        line: number;
        col: number;
        endLine?: number;
        endCol?: number;
        message: string;
        code?: string;
      }[];
    };
    const all = result.diagnostics ?? [];
    let real = fsPath;
    try {
      real = fs.realpathSync(fsPath);
    } catch {
      // keep the raw path; the comparison below still has a chance
    }
    const mine = all.filter((d) => d.path === real || d.path === fsPath);
    // The one-shot parses only this file's lines off stderr, so a failing
    // run whose findings are all in *other* files reads as "could not
    // run" there — and therefore here.
    if (mine.length === 0 && (result.blocked || all.length > 0)) {
      return {
        kind: "unavailable",
        detail: "the check reported only outside this file",
      };
    }
    return {
      kind: "ok",
      diagnostics: mine.map((d) => ({
        line: d.line,
        col: d.col,
        endLine: d.endLine,
        endCol: d.endCol,
        message: d.message,
        code: d.code,
      })),
    };
  }
  return runTypedCheckOnce(compiler, text, fsPath, includeTypes);
}

/** The one-shot `rlc --check-types --overlay` fallback. */
function runTypedCheckOnce(
  compiler: string,
  text: string,
  fsPath: string,
  includeTypes: boolean,
): Promise<ValCheckResult> {
  // Run from the file's own directory so the compiler's paths print as the
  // bare file name (it shows paths relative to the working directory), and
  // a file of the same name elsewhere in the project stays distinguishable.
  const cwd = path.dirname(fsPath);
  const shown = path.basename(fsPath);
  const args = ["--check-types"];
  if (!includeTypes) args.push("--rl-only");
  args.push("--overlay", fsPath, fsPath);

  return new Promise((resolve) => {
    let child: ReturnType<typeof execFile>;
    try {
      child = execFile(
        compiler,
        args,
        {
          cwd,
          timeout: TYPED_CHECK_TIMEOUT_MS,
          maxBuffer: 4 * 1024 * 1024,
          env: rlcSpawnEnv(compiler),
        },
        (err, _stdout, stderr) => {
          const diagnostics = parseStderr(String(stderr), shown);
          // Exit code 2 is "could not run, nothing was checked" — an rl-level
          // error left nothing to lower. Anything else with no parseable
          // diagnostic is a missing toolchain or a crash. Both keep whatever
          // the caller already had rather than clearing it.
          const code = (err as { code?: number | string } | null)?.code;
          if (diagnostics.length === 0 && err) {
            resolve({
              kind: "unavailable",
              detail: `${compiler} (${String(code)}): ${String(stderr).trim() || err.message}`,
            });
            return;
          }
          resolve({ kind: "ok", diagnostics });
        },
      );
    } catch (e) {
      resolve({ kind: "unavailable", detail: String(e) });
      return;
    }
    // The buffer is the overlay; the compiler reads it from stdin.
    child.stdin?.end(text);
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
