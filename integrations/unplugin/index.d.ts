import type { UnpluginFactory, UnpluginInstance } from "unplugin";

/** Options accepted by every `unplugin-rl` entry. */
export interface Options {
  /** Path to the rlc binary (default: the installed `rl-lang`, else `"rlc"`). */
  compiler?: string;
  /** Run rlc's output self-check (default: true). */
  verify?: boolean;
}

export declare const unpluginFactory: UnpluginFactory<Options | undefined>;
export declare const unplugin: UnpluginInstance<Options | undefined>;
export default unplugin;

export declare const vitePlugin: UnpluginInstance<Options | undefined>["vite"];
export declare const rollupPlugin: UnpluginInstance<Options | undefined>["rollup"];
export declare const rolldownPlugin: UnpluginInstance<Options | undefined>["rolldown"];
export declare const webpackPlugin: UnpluginInstance<Options | undefined>["webpack"];
export declare const rspackPlugin: UnpluginInstance<Options | undefined>["rspack"];
export declare const esbuildPlugin: UnpluginInstance<Options | undefined>["esbuild"];
export declare const farmPlugin: UnpluginInstance<Options | undefined>["farm"];
