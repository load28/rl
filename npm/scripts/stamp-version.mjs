/* --------------------------------------------------------------------------
 * stamp-version.mjs — write the release version into npm/rl-lang.
 *
 *   node stamp-version.mjs <version>
 *
 * The repository keeps the placeholder 0.0.0-dev so there is exactly one
 * version source of truth (Cargo.toml); the release workflow stamps the tag
 * version into the main package and its optionalDependencies just before
 * publishing.
 * ----------------------------------------------------------------------- */
import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

const version = process.argv[2];
if (!version || !/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version)) {
  console.error("usage: stamp-version.mjs <semver>");
  process.exit(1);
}

const manifest = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "rl-lang", "package.json");
const pkg = JSON.parse(fs.readFileSync(manifest, "utf8"));
pkg.version = version;
for (const dep of Object.keys(pkg.optionalDependencies)) {
  pkg.optionalDependencies[dep] = version;
}
fs.writeFileSync(manifest, JSON.stringify(pkg, null, 2) + "\n");
console.log(`stamped ${version} into ${manifest}`);
