/* --------------------------------------------------------------------------
 * rl-lang local development mode — created by `scripts/setup` in the RL
 * repository, absent from published installs.
 *
 * `rl-dev.local.json` beside this file (written by setup, gitignored) marks
 * the package as a local `file:` install and names the RL repository root.
 * From that one value everything else is derived, never stored twice:
 *
 *   rlc        <root>/target/release/rlc            (the release build)
 *   toolchain  <root>/.rl-dev/toolchain.json        (setup's toolchain choice)
 *
 * A "checkout" toolchain names a typescript-go tree; its tsgo binary and API
 * client paths are computed here and handed to the child rlc process as
 * RLC_TSGO_* environment variables — the user's shell is never touched. An
 * "npm" toolchain injects nothing: rlc resolves TypeScript 7 from the
 * consuming project's own node_modules, exactly as a published rlc would.
 *
 * This whole layer is temporary (docs/tasks/TASK-090): once RL ships a
 * verified TypeScript 7 inside its own package, delete this module together
 * with scripts/setup and .rl-dev/.
 * ----------------------------------------------------------------------- */
"use strict";

const fs = require("node:fs");
const path = require("node:path");

const DEV_CONFIG = path.join(__dirname, "rl-dev.local.json");

/** tsgo's path inside a built typescript-go tree (src/typescript/native.rs). */
const BIN_IN_TREE = "built/local/tsgo";
/** The JS API client's path inside a built typescript-go tree. */
const API_IN_TREE = "_packages/native-preview/dist/api/sync/api.js";

/** The parsed JSON object of `file`, or null when missing/unreadable. */
function readConfig(file) {
  try {
    const parsed = JSON.parse(fs.readFileSync(file, "utf8"));
    return parsed && typeof parsed === "object" ? parsed : null;
  } catch {
    return null;
  }
}

/**
 * The local development environment `scripts/setup` configured, or null when
 * this is a regular published install (no `rl-dev.local.json`).
 *
 * Throws when the config exists but the binary it names is gone — running
 * some other rlc silently would be worse than failing with the fix.
 *
 * @returns {{ binary: string, env: Record<string, string> } | null}
 */
function devEnvironment() {
  const config = readConfig(DEV_CONFIG);
  const root = config && typeof config.root === "string" ? config.root : null;
  if (root === null) return null;

  const exe = process.platform === "win32" ? "rlc.exe" : "rlc";
  const binary = path.join(root, "target", "release", exe);
  if (!fs.existsSync(binary)) {
    throw new Error(
      `rl-lang: this is a local development install (${DEV_CONFIG}), ` +
        `but ${binary} does not exist. Run ./scripts/setup in ${root}, ` +
        `then reinstall this package.`,
    );
  }
  return { binary, env: toolchainEnv(root) };
}

/**
 * Environment variables pointing rlc at the toolchain `scripts/setup` chose,
 * read from `<rlRoot>/.rl-dev/toolchain.json`. Empty for the "npm" toolchain
 * (rlc resolves the consuming project's TypeScript itself), for a missing
 * config, and for a checkout whose artifacts are gone — an unbuilt tree
 * named via RLC_TSGO_ROOT would stop rlc's own fallback resolution cold.
 *
 * @param {string} rlRoot
 * @returns {Record<string, string>}
 */
function toolchainEnv(rlRoot) {
  const config = readConfig(path.join(rlRoot, ".rl-dev", "toolchain.json"));
  if (config === null) return {};
  // A config predating the "kind" field named a checkout root only.
  const kind = typeof config.kind === "string" ? config.kind : "checkout";
  const root = typeof config.root === "string" ? config.root : null;
  if (kind !== "checkout" || root === null) return {};

  const bin = path.join(root, ...BIN_IN_TREE.split("/"));
  const api = path.join(root, ...API_IN_TREE.split("/"));
  if (!fs.existsSync(bin) || !fs.existsSync(api)) return {};
  return { RLC_TSGO_ROOT: root, RLC_TSGO_BIN: bin, RLC_TSGO_API: api };
}

module.exports = { devEnvironment };
