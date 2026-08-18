/* Tests for the virtual-document offset mapping (TASK-050): byte↔UTF-16
 * conversion and bidirectional chunk lookup over `rlc --emit-map` output. */
import * as assert from "node:assert/strict";
import { test } from "node:test";

import { MappedDoc } from "../virtual";

test("identity mapping converts offsets both ways", () => {
  const src = "const n = 1;\n";
  const doc = new MappedDoc(src, src, [{ src: 0, out: 0, len: src.length }]);
  assert.equal(doc.srcToOut(0), 0);
  assert.equal(doc.srcToOut(7), 7);
  assert.equal(doc.outToSrc(7), 7);
  // the end offset belongs to the chunk (completion sits there)
  assert.equal(doc.srcToOut(src.length), src.length);
});

test("offsets in compiler glue map to null", () => {
  // source:  match (shape) { A => x }   (schematic)
  // output:  GLUE(shape)GLUE(x)GLUE
  const src = "match (shape) { A => x }";
  const code = "((() => { const $rl_m = (shape); return (x); })())";
  const mappings = [
    { src: src.indexOf("shape"), out: code.indexOf("shape"), len: 5 },
    { src: src.indexOf("x"), out: code.lastIndexOf("(x)") + 1, len: 1 },
  ];
  const doc = new MappedDoc(src, code, mappings);

  // scrutinee round-trips
  const s = doc.srcToOut(src.indexOf("shape") + 2);
  assert.equal(code.slice(s! - 2, s! + 3), "shape");
  assert.equal(doc.outToSrc(s!), src.indexOf("shape") + 2);

  // the `match` keyword has no output counterpart
  assert.equal(doc.srcToOut(0), null);
  // IIFE scaffolding has no source counterpart
  assert.equal(doc.outToSrc(code.indexOf("$rl_m")), null);
});

test("multi-byte characters convert between bytes and UTF-16", () => {
  // The compiler speaks UTF-8 bytes; "한글" is 6 bytes / 2 UTF-16 units.
  const src = 'const s = "한글";\nconst n = 1;\n';
  const code = src; // passthrough
  const byteLen = Buffer.byteLength(src, "utf8");
  const doc = new MappedDoc(src, code, [{ src: 0, out: 0, len: byteLen }]);

  const nAt = src.indexOf("n =");
  assert.equal(doc.srcToOut(nAt), nAt);
  assert.equal(doc.outToSrc(nAt), nAt);
  // inside the string, before and after the multi-byte run
  const quote = src.indexOf('";');
  assert.equal(doc.srcToOut(quote), quote);
});

test("multi-byte source with ASCII-shifted output chunk", () => {
  // A mapped chunk after a multi-byte region: byte offsets differ from
  // UTF-16 offsets on the source side, and the output places the chunk
  // elsewhere entirely.
  const src = '// 주석\nconst value = 42;\n';
  const chunkU16 = src.indexOf("const");
  const chunkByte = Buffer.byteLength(src.slice(0, chunkU16), "utf8");
  const chunkLen = Buffer.byteLength(src.slice(chunkU16), "utf8");
  const code = "/* glue */ const value = 42;\n";
  const outAt = code.indexOf("const");
  const doc = new MappedDoc(src, code, [
    { src: chunkByte, out: outAt, len: chunkLen },
  ]);

  const valueU16 = src.indexOf("value");
  const mapped = doc.srcToOut(valueU16);
  assert.equal(code.slice(mapped!, mapped! + 5), "value");
  assert.equal(doc.outToSrc(mapped!), valueU16);
});

test("spans crossing a chunk boundary refuse to map", () => {
  const src = "abcdef";
  const code = "ab__ef";
  const doc = new MappedDoc(src, code, [
    { src: 0, out: 0, len: 2 },
    { src: 4, out: 4, len: 2 },
  ]);
  assert.equal(doc.srcToOut(1), 1);
  assert.equal(doc.srcToOut(3), null);
  assert.equal(doc.srcToOut(5), 5);
  assert.equal(doc.outToSrc(3), null);
});
