/* --------------------------------------------------------------------------
 * vite-plugin-rl — `.rl` modules for Vite and Rollup.
 *
 * The plugin resolves `.rl` specifiers itself and compiles each file with
 * `rlc` on the way in, so a project needs no intermediate `.ts` tree: the
 * bundler reads the sources directly.
 *
 * `rlc` is invoked with `--rewrite-imports off` on purpose. Rewriting exists
 * for the ahead-of-time pipeline (where a `.rl` neighbour has already become
 * a `.ts` file); here the specifier must stay `.rl` so this plugin resolves
 * it too.
 *
 * Editor support is separate: `rlc --sidecar` writes the declarations that
 * let a `.ts` file import `.rl` without the type checker complaining.
 * ----------------------------------------------------------------------- */
import { execFile } from "node:child_process";
import * as path from "node:path";
import { promisify } from "node:util";

const run = promisify(execFile);

/**
 * @typedef {object} RlPluginOptions
 * @property {string} [compiler] Path to the rlc binary (default: `"rlc"`).
 * @property {boolean} [verify] Run rlc's output self-check (default: true).
 */

/**
 * @param {RlPluginOptions} [options]
 * @returns {import("vite").Plugin}
 */
export function rl(options = {}) {
  const compiler = options.compiler ?? "rlc";
  const verify = options.verify ?? true;

  return {
    name: "vite-plugin-rl",
    // Ahead of the bundler's own resolution: `.rl` is not an extension it
    // knows, and the module has to become TypeScript before anything else
    // looks at it.
    enforce: "pre",

    resolveId(source, importer) {
      // The standard library has no file: rlc prints it on demand, so it
      // becomes a virtual module. Nothing lands in the project tree.
      if (source === STD_SPECIFIER) return stdId();
      if (!source.endsWith(".rl")) return null;
      const file = path.isAbsolute(source)
        ? source
        : importer === undefined
          ? null
          : path.resolve(path.dirname(importer), source);
      // The module id gets a `.ts` suffix so the host's TypeScript pass
      // picks the compiled output up. Keeping the plugin out of that job
      // means it needs no bundler API — the same code runs under Vite and
      // plain Rollup (with a TypeScript plugin).
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
        // the build error so the bundler shows the compiler's diagnostic.
        const detail = String(error.stderr ?? error.message).trim();
        this.error(detail.replace(/^rlc:\s*/, ""));
        return null;
      }
    },
  };
}

/** Virtual suffix that marks a compiled `.rl` module as TypeScript. */
const TS_SUFFIX = ".ts";

/** The bare specifier rl sources use for the standard library. */
const STD_SPECIFIER = "@rl/std";

/**
 * Virtual module id for the standard library.
 *
 * Deliberately *not* the conventional `\0`-prefixed form: hosts skip their
 * TypeScript pass on `\0` ids, and rlc emits TypeScript. A path-shaped id
 * ending in `.ts` gets transformed like any other module; nothing reads it
 * from disk because this plugin resolves and loads it first.
 */
function stdId() {
  return path.resolve(process.cwd(), `__rl_std__${TS_SUFFIX}`);
}

export default rl;
