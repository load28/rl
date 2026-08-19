/* The local-development toolchain resolution (dev.ts): what `scripts/setup`
 * writes is what the server hands to every rlc it spawns — and nothing is
 * handed over when the setup is absent, chose npm, or went stale. All on
 * temp directories; no real toolchain is involved. */
import * as assert from "node:assert/strict";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { test } from "node:test";

import { devPackageCompiler, rlcSpawnEnv, toolchainEnv } from "../dev";

/** A fake RL repository whose setup chose the given toolchain config. */
function rlRepo(toolchain: object | null): string {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "rl-dev-test-"));
  fs.mkdirSync(path.join(root, "target", "release"), { recursive: true });
  if (toolchain !== null) {
    fs.mkdirSync(path.join(root, ".rl-dev"));
    fs.writeFileSync(
      path.join(root, ".rl-dev", "toolchain.json"),
      JSON.stringify(toolchain),
    );
  }
  return root;
}

/** A fake built typescript-go checkout (both artifacts present). */
function builtTsgo(): string {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "rl-tsgo-test-"));
  fs.mkdirSync(path.join(root, "built", "local"), { recursive: true });
  fs.writeFileSync(path.join(root, "built", "local", "tsgo"), "");
  const api = path.join(root, "_packages", "native-preview", "dist", "api", "sync");
  fs.mkdirSync(api, { recursive: true });
  fs.writeFileSync(path.join(api, "api.js"), "");
  return root;
}

test("a checkout toolchain above the compiler becomes RLC_TSGO_* variables", () => {
  const tsgo = builtTsgo();
  const rl = rlRepo({ kind: "checkout", root: tsgo });
  const env = toolchainEnv(path.join(rl, "target", "release", "rlc"));
  assert.deepEqual(env, {
    RLC_TSGO_ROOT: tsgo,
    RLC_TSGO_BIN: path.join(tsgo, "built", "local", "tsgo"),
    RLC_TSGO_API: path.join(
      tsgo,
      "_packages",
      "native-preview",
      "dist",
      "api",
      "sync",
      "api.js",
    ),
  });
});

test("a pre-'kind' config (root only) still reads as a checkout", () => {
  const tsgo = builtTsgo();
  const rl = rlRepo({ root: tsgo });
  const env = toolchainEnv(path.join(rl, "target", "release", "rlc"));
  assert.equal(env?.RLC_TSGO_ROOT, tsgo);
});

test("the npm toolchain adds nothing — rlc resolves the project's own", () => {
  const rl = rlRepo({ kind: "npm" });
  assert.equal(toolchainEnv(path.join(rl, "target", "release", "rlc")), null);
});

test("an unbuilt checkout adds nothing rather than a broken RLC_TSGO_ROOT", () => {
  const tsgo = fs.mkdtempSync(path.join(os.tmpdir(), "rl-tsgo-test-"));
  const rl = rlRepo({ kind: "checkout", root: tsgo });
  assert.equal(toolchainEnv(path.join(rl, "target", "release", "rlc")), null);
});

test("no config, a bare PATH compiler: inherit the environment untouched", () => {
  const rl = rlRepo(null);
  assert.equal(toolchainEnv(path.join(rl, "target", "release", "rlc")), null);
  assert.equal(toolchainEnv("rlc"), null);
  assert.equal(rlcSpawnEnv("rlc"), undefined);
});

test("rlcSpawnEnv layers the toolchain over the process environment", () => {
  const tsgo = builtTsgo();
  const rl = rlRepo({ kind: "checkout", root: tsgo });
  const env = rlcSpawnEnv(path.join(rl, "target", "release", "rlc"));
  assert.equal(env?.RLC_TSGO_ROOT, tsgo);
  assert.equal(env?.PATH, process.env.PATH);
});

test("devPackageCompiler finds the rlc of a file:-installed dev package", () => {
  const rl = rlRepo(null);
  const exe = process.platform === "win32" ? "rlc.exe" : "rlc";
  fs.writeFileSync(path.join(rl, "target", "release", exe), "");

  const workspace = fs.mkdtempSync(path.join(os.tmpdir(), "rl-ws-test-"));
  const pkg = path.join(workspace, "node_modules", "rl-lang");
  fs.mkdirSync(pkg, { recursive: true });
  fs.writeFileSync(
    path.join(pkg, "rl-dev.local.json"),
    JSON.stringify({ root: rl }),
  );

  assert.equal(
    devPackageCompiler([workspace]),
    path.join(rl, "target", "release", exe),
  );

  // A stale stamp (binary gone) resolves to nothing, not to a dead path.
  fs.rmSync(path.join(rl, "target", "release", exe));
  assert.equal(devPackageCompiler([workspace]), "");
});
