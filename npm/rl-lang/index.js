/* --------------------------------------------------------------------------
 * rl-lang — locate the platform-specific rlc binary.
 *
 * The actual compiler ships in per-platform packages (rl-lang-linux-x64,
 * rl-lang-darwin-arm64, ...) wired up as optionalDependencies: npm installs
 * only the one matching the host. This module finds that binary so tools
 * (the `rlc` bin launcher, unplugin-rl, editor servers) can spawn it
 * directly without paying for an extra node process per call.
 * ----------------------------------------------------------------------- */
"use strict";

const path = require("node:path");

/** Platforms a prebuilt rlc binary is published for. */
const SUPPORTED = ["linux-x64", "linux-arm64", "darwin-x64", "darwin-arm64", "win32-x64"];

/** `"linux-x64"`-style key for the current process, supported or not. */
function platformKey() {
  return `${process.platform}-${process.arch}`;
}

/**
 * Absolute path to the rlc binary for this platform.
 *
 * Resolution order: the `RLC_BINARY` environment variable (escape hatch for
 * local builds and tests), then the installed platform package. Throws with
 * install guidance when neither is available.
 *
 * @returns {string}
 */
function binaryPath() {
  const override = process.env.RLC_BINARY;
  if (override) return path.resolve(override);

  const key = platformKey();
  const exe = process.platform === "win32" ? "rlc.exe" : "rlc";
  const pkg = `rl-lang-${key}`;
  try {
    return require.resolve(`${pkg}/bin/${exe}`);
  } catch {
    const reason = SUPPORTED.includes(key)
      ? `the ${pkg} package is not installed — it is an optionalDependency of rl-lang, ` +
        `so this usually means optional dependencies were disabled (--no-optional / ` +
        `--omit=optional) or the lockfile was created on a different platform. ` +
        `Reinstalling rl-lang should fix it.`
      : `no prebuilt rlc binary is published for ${key}. Prebuilt platforms: ` +
        `${SUPPORTED.join(", ")}. You can build from source with ` +
        `"cargo install --git https://github.com/load28/rl" and point the ` +
        `RLC_BINARY environment variable at the resulting binary.`;
    throw new Error(`rl-lang: cannot find the rlc binary: ${reason}`);
  }
}

module.exports = { binaryPath, platformKey };
