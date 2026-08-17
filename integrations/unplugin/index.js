/* --------------------------------------------------------------------------
 * unplugin-rl — `.rl` modules for every bundler unplugin supports.
 *
 * The plugin resolves `.rl` specifiers itself and compiles each file with
 * `rlc` on the way in, so a project needs no intermediate `.ts` tree: the
 * bundler reads the sources directly.
 *
 * Two deliberate details:
 *
 * - `rlc` runs with `--rewrite-imports off`. Rewriting exists for the
 *   ahead-of-time pipeline (where a `.rl` neighbour has already become a
 *   `.ts` file); here the specifier must stay `.rl` so this plugin resolves
 *   it too.
 * - Module ids get a `.ts` suffix. rlc emits TypeScript, and the host's own
 *   TypeScript pass keys off the extension — this keeps the plugin out of
 *   that job entirely. esbuild is told the loader explicitly instead, since
 *   its `load` hook may only return JavaScript.
 *
 * Editor support is separate: `rlc --types` writes the declarations that let
 * a `.ts` file import `.rl` without the type checker complaining.
 * ----------------------------------------------------------------------- */
import { execFile } from "node:child_process";
import { createRequire } from "node:module";
import * as path from "node:path";
import { promisify } from "node:util";

import { createUnplugin } from "unplugin";

const run = promisify(execFile);

/**
 * Default compiler: the prebuilt binary from an installed `rl-lang` npm
 * package when present (spawned directly — no per-call node launcher),
 * otherwise `rlc` from PATH as before.
 */
function defaultCompiler() {
  try {
    const require = createRequire(import.meta.url);
    return require("rl-lang").binaryPath();
  } catch {
    return "rlc";
  }
}

/** Virtual suffix that marks a compiled `.rl` module as TypeScript. */
const TS_SUFFIX = ".ts";

/** The bare specifier rl sources use for the standard library. */
const STD_SPECIFIER = "@rl/std";

/** Virtual module id for the standard library, per working directory. */
const stdId = () => path.resolve(process.cwd(), `__rl_std__${TS_SUFFIX}`);

/**
 * @typedef {object} Options
 * @property {string} [compiler] Path to the rlc binary (default: the
 *   installed `rl-lang` package's binary, falling back to `"rlc"` on PATH).
 * @property {boolean} [verify] Run rlc's output self-check (default: true).
 */

/** @type {import("unplugin").UnpluginFactory<Options | undefined>} */
export const unpluginFactory = (options = {}) => {
  const compiler = options.compiler ?? defaultCompiler();
  const verify = options.verify ?? true;

  return {
    name: "unplugin-rl",
    // Ahead of the host's own resolution: `.rl` is not an extension it
    // knows. Rollup and esbuild ignore `enforce`, where plugin order is the
    // author's responsibility instead.
    enforce: "pre",

    resolveId(source, importer) {
      // The standard library has no file: rlc prints it on demand, so it
      // becomes a virtual module. Nothing lands in the project tree.
      if (source === STD_SPECIFIER) return stdId();
      if (!source.endsWith(".rl")) return null;

      const file = path.isAbsolute(source)
        ? source
        : importer === undefined || importer === null
          ? null
          : path.resolve(path.dirname(importer), source);
      return file === null ? null : `${file}${TS_SUFFIX}`;
    },

    async load(id) {
      if (id === stdId()) {
        const { stdout } = await run(compiler, ["--emit-std", "--no-banner"], {
          maxBuffer: 16 * 1024 * 1024,
        });
        return { code: stdout, map: null };
      }
      if (!id.endsWith(`.rl${TS_SUFFIX}`)) return null;
      const file = id.slice(0, -TS_SUFFIX.length);

      const args = ["-p", "--rewrite-imports", "off"];
      if (!verify) args.push("--no-verify");
      args.push(file);

      try {
        const { stdout } = await run(compiler, args, { maxBuffer: 16 * 1024 * 1024 });
        this.addWatchFile(file);
        return { code: stdout, map: null };
      } catch (error) {
        // rlc reports `file:line:col: message` on stderr; surface that as
        // the build error so the host shows the compiler's diagnostic.
        const detail = String(error.stderr ?? error.message).trim();
        this.error(detail.replace(/^rlc:\s*/, ""));
        return null;
      }
    },

    esbuild: {
      // esbuild resolves and loads through its own filters, and its `load`
      // may only return JavaScript — so narrow the filters to our ids and
      // name the loader for the TypeScript rlc emits.
      onResolveFilter: /(\.rl|^@rl\/std)$/,
      onLoadFilter: /(\.rl\.ts|__rl_std__\.ts)$/,
      loader: "ts",
    },
  };
};

export const unplugin = /* #__PURE__ */ createUnplugin(unpluginFactory);

export default unplugin;

export const vitePlugin = unplugin.vite;
export const rollupPlugin = unplugin.rollup;
export const rolldownPlugin = unplugin.rolldown;
export const webpackPlugin = unplugin.webpack;
export const rspackPlugin = unplugin.rspack;
export const esbuildPlugin = unplugin.esbuild;
export const farmPlugin = unplugin.farm;
