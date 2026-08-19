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

  const args = ["--native-sidecar", rlPath];
  if (outDir !== undefined) args.push("-o", outDir);
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
 * One `rlc` run. Type errors in the saved file do not fail it — the sidecar
 * is written either way, and a stale one would be worse than one built from
 * code that does not check yet.
 */
function run(compiler: string, args: string[], files: string[]): Promise<SidecarResult> {
  return new Promise((resolve) => {
    execFile(
      compiler,
      args,
      { timeout: 30000, maxBuffer: 8 * 1024 * 1024 },
      (err, _stdout, stderr) => {
        if (err) {
          resolve({ kind: "failed", detail: stderr.trim() || String(err) });
          return;
        }
        resolve({ kind: "written", files });
      },
    );
  });
}
