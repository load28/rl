import type { UnpluginInstance } from "unplugin";
import type { Options } from "./index.js";

declare const plugin: UnpluginInstance<Options | undefined>["rollup"];
export default plugin;
