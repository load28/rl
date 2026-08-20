#!/usr/bin/env node
/* --------------------------------------------------------------------------
 * rl.tmLanguage.json 생성기 — TSX 방식(완전 문법 확장)의 유지비를 흡수한다.
 *
 * 입력:
 *   src/typescript.tmLanguage.json  업스트림 TypeScript 문법, 무수정 vendoring
 *                                   (tm-grammars 1.32.5의 typescript.json —
 *                                   microsoft/TypeScript-TmLanguage 파생, MIT)
 *   src/rl.rules.json               rl 규칙(repository)과 접합 명세(splices)
 *
 * 출력:
 *   rl.tmLanguage.json              scopeName source.rl의 독립 문법.
 *                                   TS repository 전체 + rl 규칙, 접합 지점의
 *                                   patterns 맨 앞에 rl include를 삽입.
 *
 * 업스트림 갱신 = src/typescript.tmLanguage.json 교체 후 재생성.
 * 새 rl 구문 = src/rl.rules.json에 규칙과 접합 지점 추가 후 재생성.
 *
 *   node build.mjs           생성
 *   node build.mjs --check   생성물이 소스와 일치하는지 검사 (CI/테스트용)
 * ----------------------------------------------------------------------- */
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const tsGrammar = JSON.parse(
  readFileSync(join(here, "src", "typescript.tmLanguage.json"), "utf8"),
);
const rl = JSON.parse(readFileSync(join(here, "src", "rl.rules.json"), "utf8"));

// --- 검증: rl 규칙 이름은 rl- 접두사, TS repository와 충돌 금지 -----------
for (const name of Object.keys(rl.repository)) {
  if (!name.startsWith("rl-")) {
    throw new Error(`rl rule "${name}" must be prefixed with "rl-"`);
  }
  if (name in tsGrammar.repository) {
    throw new Error(`rl rule "${name}" collides with a TypeScript rule`);
  }
}

// --- 검증: 접합 지점은 실제 TS repository 항목이어야 한다 ------------------
// (업스트림 갱신으로 항목이 사라지거나 모양이 바뀌면 여기서 곧바로 실패한다.)
// 접합 대상 표기: "이름" 또는 "이름:i:j..." — repository[이름]에서 시작해
// patterns[i], patterns[j], ...로 내려간 지점의 patterns 맨 앞에 삽입한다.
const resolveSpliceTarget = (repository, target) => {
  const [name, ...indices] = target.split(":");
  let entry = repository[name];
  if (!entry) {
    throw new Error(`splice target "${target}" is not in the TypeScript grammar`);
  }
  for (const idx of indices) {
    if (!Array.isArray(entry.patterns) || !entry.patterns[Number(idx)]) {
      throw new Error(`splice target "${target}" has no patterns[${idx}]`);
    }
    entry = entry.patterns[Number(idx)];
  }
  if (!Array.isArray(entry.patterns)) {
    throw new Error(`splice target "${target}" has no patterns array`);
  }
  return entry;
};
for (const [target, includes] of Object.entries(rl.splices)) {
  resolveSpliceTarget(tsGrammar.repository, target);
  for (const inc of includes) {
    if (!(inc.slice(1) in rl.repository)) {
      throw new Error(`splice "${inc}" into "${target}" names no rl rule`);
    }
  }
}

// --- 검증: rl 규칙이 참조하는 include가 전부 해석돼야 한다 -----------------
const known = new Set([
  ...Object.keys(tsGrammar.repository),
  ...Object.keys(rl.repository),
]);
const walk = (node, where) => {
  if (Array.isArray(node)) return node.forEach((n) => walk(n, where));
  if (node && typeof node === "object") {
    if (typeof node.include === "string" && node.include.startsWith("#")) {
      if (!known.has(node.include.slice(1))) {
        throw new Error(`unresolved include "${node.include}" in ${where}`);
      }
    }
    for (const v of Object.values(node)) walk(v, where);
  }
};
for (const [name, rule] of Object.entries(rl.repository)) walk(rule, `rl rule "${name}"`);

// --- 합성 ------------------------------------------------------------------
const out = {
  name: "rl",
  scopeName: "source.rl",
  patterns: tsGrammar.patterns,
  repository: { ...tsGrammar.repository, ...rl.repository },
};
for (const [target, includes] of Object.entries(rl.splices)) {
  const entry = resolveSpliceTarget(out.repository, target);
  entry.patterns = [...includes.map((include) => ({ include })), ...entry.patterns];
}

const rendered = JSON.stringify(out, null, 2) + "\n";
const outPath = join(here, "rl.tmLanguage.json");

if (process.argv.includes("--check")) {
  let existing = "";
  try {
    existing = readFileSync(outPath, "utf8");
  } catch {
    /* 파일 없음 = 불일치 */
  }
  if (existing !== rendered) {
    console.error(
      "rl.tmLanguage.json is stale — run `node syntaxes/build.mjs` and commit the result",
    );
    process.exit(1);
  }
  console.log("rl.tmLanguage.json is up to date");
} else {
  writeFileSync(outPath, rendered);
  console.log(`wrote ${outPath}`);
}
