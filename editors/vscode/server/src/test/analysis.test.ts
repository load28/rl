/* Unit tests for the text-shape utilities — run with `node --test` after
 * compiling (npm test). rl *semantics* (visible enums, match sites) are
 * the compiler's answer via the server's `declarations` method and are
 * covered by server.test.ts; what is pinned here is masking and cursor
 * context. */
import * as assert from "node:assert/strict";
import { test } from "node:test";

import { isIdent, maskNonCode, memberAccessAt, wordAt } from "../analysis";

test("maskNonCode blanks strings, comments and template text", () => {
  const src = 'const s = "enum A {}"; // enum B {}\nconst t = `match (${x}) {`;';
  const masked = maskNonCode(src);
  assert.equal(masked.length, src.length);
  assert.ok(!masked.includes("enum A"));
  assert.ok(!masked.includes("enum B"));
  // Code inside a template interpolation is kept.
  assert.ok(masked.includes("x"));
});

test("maskNonCode blanks regex literals but not division", () => {
  const src = "const r = /enum X/; const q = a / b;";
  const masked = maskNonCode(src);
  assert.ok(!masked.includes("enum X"));
  assert.ok(masked.includes("a / b"));
});

test("memberAccessAt finds the base identifier before a dot", () => {
  const src = "const v = Shape.";
  assert.equal(memberAccessAt(src, src.length), "Shape");
  assert.equal(memberAccessAt(src, 5), null);
});

test("wordAt covers the identifier under the cursor", () => {
  const src = "match (value)";
  const w = wordAt(src, 2)!;
  assert.equal(w.word, "match");
  assert.equal(w.start, 0);
  // At the word's end, still the word.
  assert.equal(wordAt(src, 5)!.word, "match");
  assert.equal(wordAt(src, 6), null);
});

test("isIdent rejects reserved words and bad starts", () => {
  assert.ok(isIdent("Shape"));
  assert.ok(!isIdent("enum"));
  assert.ok(!isIdent("1x"));
});
