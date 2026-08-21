/* Unit tests for the structural analysis — run with `node --test` after
 * compiling (npm test). These pin the mirrored compiler rules: masking,
 * rl/TS enum disambiguation, match contexts, builtin shadowing. */
import * as assert from "node:assert/strict";
import { test } from "node:test";

import {
  BUILTIN_ENUMS,
  caseSignature,
  maskNonCode,
  memberAccessAt,
  parseEnums,
  parseMatches,
  visibleEnums,
} from "../analysis";

const analyzeAll = (src: string) => {
  const masked = maskNonCode(src);
  return {
    masked,
    enums: parseEnums(src, masked),
    matches: parseMatches(masked),
  };
};

test("maskNonCode blanks strings, comments and template text", () => {
  const src = 'const s = "enum A {}"; // enum B {}\nconst t = `match (${x}) {`;';
  const masked = maskNonCode(src);
  assert.equal(masked.length, src.length);
  assert.ok(!masked.includes("enum A"));
  assert.ok(!masked.includes("enum B"));
  assert.ok(masked.includes("${") === false);
  assert.ok(masked.includes("x")); // interpolation code survives
  assert.equal(parseEnums(src, masked).length, 0);
});

test("maskNonCode keeps newlines so offsets are stable", () => {
  const src = "/* a\nb */\nenum E { A() }";
  const masked = maskNonCode(src);
  assert.equal(
    [...masked].filter((c) => c === "\n").length,
    [...src].filter((c) => c === "\n").length,
  );
  assert.equal(parseEnums(src, masked).length, 1);
});

test("maskNonCode blanks regex literals but not division", () => {
  const src = "const r = /enum{/g; const q = a / b / c;";
  const masked = maskNonCode(src);
  assert.ok(!masked.includes("enum{"));
  assert.ok(masked.includes("a / b / c"));
});

test("parseEnums recognizes payload cases and fields", () => {
  const src =
    "export enum Shape {\n  Circle(radius: number),\n  Rect(width: number, height: number),\n  Point,\n}";
  const { enums } = analyzeAll(src);
  assert.equal(enums.length, 1);
  const e = enums[0];
  assert.equal(e.name, "Shape");
  assert.ok(e.exported);
  assert.deepEqual(
    e.cases.map((c) => c.tag),
    ["Circle", "Rect", "Point"],
  );
  assert.deepEqual(
    e.cases[1].fields.map((f) => f.name),
    ["width", "height"],
  );
  assert.equal(e.cases[2].hasParens, false);
});

test("plain TS enums are not rl enums", () => {
  for (const src of [
    "enum Color { Red, Green, Blue }",
    "const enum C { A(x: number) }",
    "declare enum D { A(x: number) }",
    "enum N { A = 1, B = 2 }",
  ]) {
    const { enums } = analyzeAll(src);
    assert.equal(enums.length, 0, src);
  }
});

test("generics alone make an enum an rl enum", () => {
  const { enums } = analyzeAll("enum Option<T> { Some(value: T), None }");
  assert.equal(enums.length, 1);
  assert.equal(enums[0].generics, "<T>");
});

test("empty payload parens make an enum an rl enum", () => {
  const { enums } = analyzeAll("enum Status { Active(), Inactive }");
  assert.equal(enums.length, 1);
  assert.ok(enums[0].cases[0].hasParens);
  assert.ok(!enums[0].cases[1].hasParens);
});

test("reserved words disqualify a declaration", () => {
  const { enums } = analyzeAll("enum E { true(x: number) }");
  assert.equal(enums.length, 0);
});

test("function types in fields parse (arrow inside parens)", () => {
  const { enums } = analyzeAll(
    "enum Cb { Handler(fn: (s: string) => number), Idle }",
  );
  assert.equal(enums.length, 1);
  assert.equal(enums[0].cases[0].fields[0].type, "(s: string) => number");
});

test("parseMatches finds match and skips str.match", () => {
  const src =
    'const a = "x".match(/y/); const b = match (v) { A => 1, B => 2 };';
  const { matches } = analyzeAll(src);
  assert.equal(matches.length, 1);
  assert.equal(src.slice(matches[0].start, matches[0].start + 5), "match");
});








test("builtin Option/Result are visible and shadowed by local decls", () => {
  assert.deepEqual(
    visibleEnums([]).map((e) => e.name),
    ["Option", "Result"],
  );
  const { enums } = analyzeAll("enum Option<T> { Some(value: T), None }");
  const visible = visibleEnums(enums);
  assert.equal(visible.filter((e) => e.name === "Option").length, 1);
  assert.ok(!visible.find((e) => e.name === "Option")!.builtin);
  assert.ok(visible.find((e) => e.name === "Result")!.builtin);
});

test("memberAccessAt resolves the base identifier", () => {
  const src = "const v = Shape.Cir";
  const masked = maskNonCode(src);
  assert.equal(memberAccessAt(masked, src.length), "Shape");
  assert.equal(memberAccessAt(masked, src.indexOf("Shape")), null);
});




test("template interpolation code is analyzed", () => {
  const src =
    "enum S { A(), B }\nconst t = `v=${match (s) { A => 1, B => 2 }}`;";
  const { matches } = analyzeAll(src);
  assert.equal(matches.length, 1);
});

/* ---- imported enums (rlc --symbols, module graph phase 3) ---- */

import type { EnumInfo } from "../analysis";

const importedEnum = (name: string, tags: string[]): EnumInfo => ({
  name,
  nameStart: -1,
  nameEnd: -1,
  generics: "",
  exported: true,
  builtin: false,
  start: -1,
  end: -1,
  cases: tags.map((tag) => ({
    tag,
    tagStart: -1,
    tagEnd: -1,
    fields: [],
    hasParens: true,
  })),
  imported: {
    path: "/proj/token.rl",
    specifier: "./token.rl",
    name,
    line: 1,
    col: 13,
    cases: Object.fromEntries(tags.map((t) => [t, { line: 2, col: 3 }])),
  },
});

test("visibleEnums shadows local > imported > builtin", () => {
  const src = "enum Token { A() }\n";
  const { masked } = { masked: maskNonCode(src) };
  const locals = parseEnums(src, masked);
  const imported = [
    importedEnum("Token", ["X", "Y"]), // shadowed by the local Token
    importedEnum("Option", ["Some", "None", "Maybe"]), // shadows the builtin
    importedEnum("Extra", ["E"]),
  ];
  const visible = visibleEnums(locals, imported);
  const token = visible.find((e) => e.name === "Token")!;
  assert.equal(token.imported, undefined); // the local one won
  const option = visible.find((e) => e.name === "Option")!;
  assert.ok(option.imported); // the imported one shadows the builtin
  assert.equal(option.cases.length, 3);
  assert.ok(visible.some((e) => e.name === "Extra"));
  assert.ok(visible.some((e) => e.name === "Result" && e.builtin));
});

