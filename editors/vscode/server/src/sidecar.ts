/* --------------------------------------------------------------------------
 * On-save regeneration of the editor sidecars (`x.rl.d.ts` + `.map`).
 *
 * A `.ts` file importing `"./x.rl"` only type-checks when a declaration file
 * sits next to the module, and "go to definition" only lands in the original
 * when that declaration carries a map whose `sources` is the `.rl` file
 * (see `docs/reference/cli.md`, "에디터 사이드카").
 *
 * Both come from one `rlc` run: the compiler lowers the `.rl` files, hands
 * them to the real TypeScript project the workspace is configured with, and
 * writes what that project emits — declarations from the compiler, map from
 * rlc. Nothing here knows about TypeScript's API, so nothing here breaks
 * when that API changes.
 * ----------------------------------------------------------------------- */
import { execFile } from "node:child_process";
import * as fs from "node:fs";
import * as path from "node:path";

import { rlcSpawnEnv } from "./dev";

/** What to do when an `.rl` file is saved. */
export type SidecarMode = "off" | "refresh" | "always";

/** Outcome of one refresh, for logging and tests. */
export type SidecarResult =
  | { kind: "written"; files: string[] }
  | { kind: "skipped"; reason: string }
  | { kind: "failed"; detail: string };

/**
 * Rebuilds the sidecar for one `.rl` file.
 *
 * In `"refresh"` mode (the default) a sidecar is only rewritten when one is
 * already there — a project opts in by running `rlc --types` once, and
 * nothing appears in a workspace that never asked for it. `"always"` creates
 * it on first save.
 */
export async function refreshSidecar(
  compiler: string,
  rlPath: string,
  mode: SidecarMode,
  outDir?: string,
): Promise<SidecarResult> {
  if (mode === "off") return { kind: "skipped", reason: "disabled" };
  if (!rlPath.endsWith(".rl")) return { kind: "skipped", reason: "not an .rl file" };

  // Declarations either sit next to the source or in their own tree (which
  // TypeScript merges back with `rootDirs`); the refresh has to look where
  // they actually are.
  const base =
    outDir === undefined ? rlPath : path.join(outDir, path.basename(rlPath));
  const declarationTarget = `${base}.d.ts`;
  if (mode === "refresh" && !exists(declarationTarget)) {
    return { kind: "skipped", reason: "no sidecar to refresh" };
  }

  // `-o` is always passed: on its own `--types` writes to `.rl-types`, the
  // directory a project opts into, and the editor's sidecar goes wherever
  // the settings say — beside the source by default.
  const args = ["--types", rlPath, "-o", outDir ?? path.dirname(rlPath)];
  return run(compiler, args, [declarationTarget, `${base}.d.ts.map`]);
}

function exists(file: string): boolean {
  try {
    return fs.existsSync(file);
  } catch {
    return false;
  }
}

/**
 * One `rlc` run.
 *
 * The exit code carries three answers, and the middle one is the whole
 * point: a saved file mid-edit usually has type errors, and the sidecar is
 * written anyway (a stale one is worse than one built from code that does
 * not check yet).
 *
 * - `0` — checked clean, written.
 * - `1` — something was reported, and written all the same.
 * - `2` — the check could not run (an rl-level error left nothing to
 *   lower), so nothing was written and the last good sidecar stands.
 */
function run(compiler: string, args: string[], files: string[]): Promise<SidecarResult> {
  return new Promise((resolve) => {
    execFile(
      compiler,
      args,
      { timeout: 30000, maxBuffer: 8 * 1024 * 1024, env: rlcSpawnEnv(compiler) },
      (err, _stdout, stderr) => {
        const code = err === null ? 0 : ((err as { code?: number }).code ?? 1);
        if (code === 0 || code === 1) {
          resolve({ kind: "written", files });
          return;
        }
        resolve({ kind: "failed", detail: stderr.trim() || String(err) });
      },
    );
  });
}
