/* What the tests need on the machine, found the same way the server finds
 * it. Every guard here answers "is the tool there?" and nothing else — a
 * skip must mean the tool is missing, never that a feature quietly
 * answered nothing. */
import { execFileSync } from "node:child_process";

import { findTsgo as resolveTsgo } from "../rlc";

/** The compiler, as the extension invokes it. */
export const COMPILER = "rlc";

export function compilerAvailable(): boolean {
  try {
    execFileSync(COMPILER, ["-v"], { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
}

/** The `tsgo` executable, resolved exactly as the running server resolves
 * it, or null when there is none. */
export function findTsgo(): string | null {
  return resolveTsgo([]) || null;
}
