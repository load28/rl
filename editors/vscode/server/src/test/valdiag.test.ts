import * as assert from "node:assert/strict";
import { test } from "node:test";

import { valMethodProbes } from "../valdiag";

test("collects built-in mutator candidates through val declarations", () => {
  const src = [
    "val const map = new Map<string, number>();",
    'map.set("a", 1);',
    "map.get('a');",
    "",
  ].join("\n");
  const probes = valMethodProbes(src);
  assert.equal(probes.length, 1);
  assert.equal(probes[0].binding, "map");
  assert.equal(probes[0].method, "set");
  assert.equal(src.slice(probes[0].nameStart, probes[0].nameEnd), "set");
});

test("does not collect after a later ordinary binding shadows the val name", () => {
  const src = [
    "val const map = readonlyMap();",
    "const map = new Map<string, number>();",
    'map.set("a", 1);',
    "",
  ].join("\n");
  assert.deepEqual(valMethodProbes(src), []);
});

test("collects val parameter candidates and ignores strings", () => {
  const src = [
    "function update(val items: string[]) {",
    '  "items.push(1)";',
    '  items.push("x");',
    "}",
    "",
  ].join("\n");
  const probes = valMethodProbes(src);
  assert.equal(probes.length, 1);
  assert.equal(probes[0].binding, "items");
  assert.equal(probes[0].method, "push");
});
