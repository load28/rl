#!/usr/bin/env node
/* --------------------------------------------------------------------------
 * `rlc` launcher — thin veneer over the native binary from the platform
 * package. All arguments, stdio and the exit code pass through untouched,
 * so `npx rlc` behaves exactly like a natively installed rlc.
 * ----------------------------------------------------------------------- */
"use strict";

const { spawnSync } = require("node:child_process");

const { binaryPath } = require("../index.js");

let binary;
try {
  binary = binaryPath();
} catch (error) {
  console.error(error.message);
  process.exit(1);
}

const result = spawnSync(binary, process.argv.slice(2), { stdio: "inherit" });
if (result.error) {
  console.error(`rlc: failed to run ${binary}: ${result.error.message}`);
  process.exit(1);
}
if (result.signal) {
  // Re-raise so the parent observes the same termination signal.
  process.kill(process.pid, result.signal);
}
process.exit(result.status ?? 1);
