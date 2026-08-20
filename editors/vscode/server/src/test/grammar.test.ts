/* --------------------------------------------------------------------------
 * TextMate 문법 테스트 — syntaxes/rl.tmLanguage.json.
 *
 * 문법은 TypeScript 문법의 완전 확장(TSX 방식)으로 build.mjs가 생성한다.
 * 여기서 지키는 계약:
 *
 * 1. rl 구문은 **중첩 컨텍스트에서도** rl 스코프를 받는다 — 함수 본문,
 *    초기화식, 매개변수 목록. (기존 문법은 최상위에서만 동작해서 함수 안의
 *    모든 rl 구문이 순수 TS로 오해석됐고, `result { ... }`는 객체 리터럴로
 *    파싱되어 뒤 코드까지 연쇄 오염됐다.)
 * 2. 순수 TypeScript는 TypeScript 문법과 **동일한 스코프**를 받는다 —
 *    "유효한 TS는 그대로 유효한 rl" 통과 계약의 하이라이팅판.
 * 3. 생성물은 소스(src/)와 일치한다 — 손으로 고친 rl.tmLanguage.json은
 *    여기서 잡힌다.
 *
 * VS Code와 같은 엔진(vscode-textmate + oniguruma)으로 토크나이즈한다.
 * ----------------------------------------------------------------------- */
import { test } from "node:test";
import * as assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import * as fs from "node:fs";
import * as path from "node:path";
import * as oniguruma from "vscode-oniguruma";
import * as vsctm from "vscode-textmate";

const syntaxesDir = path.resolve(__dirname, "..", "..", "..", "syntaxes");

let registryPromise: Promise<vsctm.Registry> | undefined;
function registry(): Promise<vsctm.Registry> {
  registryPromise ??= (async () => {
    const wasmPath = path.join(
      path.dirname(require.resolve("vscode-oniguruma")),
      "onig.wasm",
    );
    const wasm = fs.readFileSync(wasmPath);
    await oniguruma.loadWASM(
      wasm.buffer.slice(wasm.byteOffset, wasm.byteOffset + wasm.byteLength),
    );
    return new vsctm.Registry({
      onigLib: Promise.resolve({
        createOnigScanner: (sources) => new oniguruma.OnigScanner(sources),
        createOnigString: (s) => new oniguruma.OnigString(s),
      }),
      loadGrammar: async (scopeName) => {
        const file =
          scopeName === "source.rl"
            ? path.join(syntaxesDir, "rl.tmLanguage.json")
            : scopeName === "source.ts"
              ? path.join(syntaxesDir, "src", "typescript.tmLanguage.json")
              : undefined;
        if (!file) return null;
        return vsctm.parseRawGrammar(fs.readFileSync(file, "utf8"), file);
      },
    });
  })();
  return registryPromise;
}

interface Token {
  text: string;
  scopes: string[];
}

async function tokenize(scopeName: string, source: string): Promise<Token[][]> {
  const grammar = await (await registry()).loadGrammar(scopeName);
  assert.ok(grammar, `grammar for ${scopeName}`);
  const lines: Token[][] = [];
  let stack = vsctm.INITIAL;
  for (const line of source.split("\n")) {
    const result = grammar.tokenizeLine(line, stack);
    lines.push(
      result.tokens.map((t) => ({
        text: line.slice(t.startIndex, t.endIndex),
        scopes: t.scopes,
      })),
    );
    stack = result.ruleStack;
  }
  return lines;
}

/** 1-기반 행 번호의 줄에서 text가 일치하는 첫 토큰을 찾는다. */
function tokenAt(lines: Token[][], line: number, text: string): Token {
  const hit = lines[line - 1]?.find((t) => t.text === text);
  assert.ok(
    hit,
    `no token ${JSON.stringify(text)} on line ${line}: ` +
      JSON.stringify(lines[line - 1]?.map((t) => t.text)),
  );
  return hit;
}

function assertScope(lines: Token[][], line: number, text: string, scope: string): void {
  const token = tokenAt(lines, line, text);
  assert.ok(
    token.scopes.includes(scope),
    `expected ${JSON.stringify(text)} on line ${line} to carry ${scope}, got: ${token.scopes.join(" ")}`,
  );
}

// 하이라이팅이 깨졌던 실제 리포트를 그대로 옮긴 픽스처.
const REPORTED = `export function endpoint(store: Record<string, string>): Result<string, ConfigError> {
  return result {
    const host <- get(store, "host");
    const portText <- get(store, "port");
    const port <- toPort(portText);
    \`https://\${host}:\${port}\`
  };
}

export function withTag(input: string[], tag: string): string {
  val const source = input;
  const next = [...source, tag];
  return next.join(", ");
}

function widen(val box: { width: number; height: number }): number {
  return box.width * box.height;
}
`;

test("rl constructs inside a function body get rl scopes (reported regression)", async () => {
  const lines = await tokenize("source.rl", REPORTED);

  // result 블록: 키워드, 바인딩, `<-` 전부 함수 본문 안에서.
  assertScope(lines, 2, "result", "keyword.control.result.rl");
  assertScope(lines, 3, "const", "storage.type.ts");
  assertScope(lines, 3, "host", "variable.other.readwrite.ts");
  assertScope(lines, 3, "<-", "keyword.operator.result-bind.rl");
  assertScope(lines, 3, "get", "entity.name.function.ts");
  assertScope(lines, 3, "host", "variable.other.readwrite.ts");
  // 블록의 마지막 값 식(템플릿)은 문자열로 남는다 — 객체 키가 아니라.
  assertScope(lines, 6, "https://", "string.template.ts");

  // result 블록이 객체 리터럴로 오해석되면 뒤 코드가 연쇄 오염된다 —
  // 다음 함수 시그니처가 온전하면 오염이 없는 것.
  assertScope(lines, 10, "withTag", "entity.name.function.ts");
  assertScope(lines, 10, "export", "keyword.control.export.ts");
  for (const token of lines.flat()) {
    assert.ok(
      !token.scopes.includes("meta.objectliteral.ts"),
      `object-literal misparse leaked into: ${JSON.stringify(token.text)}`,
    );
  }

  // val — 선언 수식자와 매개변수 수식자, 둘 다 중첩 위치에서.
  assertScope(lines, 11, "val", "storage.modifier.val.rl");
  assertScope(lines, 11, "source", "variable.other.constant.ts");
  assertScope(lines, 16, "val", "storage.modifier.val.rl");
  assertScope(lines, 16, "box", "variable.parameter.ts");
});

// 나머지 구문 전부를 함수 본문(중첩 컨텍스트)에 몰아넣은 픽스처.
const NESTED = `function demo(items: Item[]) {
  enum Shape<T> { Circle(r: number, meta?: T), Dot }
  const area = match (shape) {
    Circle(r) => r * r,
    Rect(w: width, h) if width > 0 => width * h,
    Dot | Empty => 0,
    "north" | "south" => 1,
    true => 2,
    Some(value: Circle(r)) => r,
    _ => -1,
  };
  const out = raw |> trim |> parse;
  const compose = flow |> trim |> parse;
  try cleanup();
  const parsed = try parse(raw);
  let Some(value: user) = findUser(id) else { return none; };
  if let Some(value: cached) = cache.get(id) {
    greet(cached);
  } else {
    prompt();
  }
  for (val const item of items) { log(item.id); }
  try { work(); } catch (val error: unknown) { log(error); }
  const r = result {
    const n: number <- toPort(raw);
    const { x, y } <- point();
    n + x + y
  };
  return r;
}
`;

test("every rl construct works in nested contexts", async () => {
  const lines = await tokenize("source.rl", NESTED);

  // enum — 함수 안 선언, 페이로드 필드와 타입.
  assertScope(lines, 2, "enum", "storage.type.enum.ts");
  assertScope(lines, 2, "Shape", "entity.name.type.enum.ts");
  assertScope(lines, 2, "Circle", "variable.other.enummember.ts");
  assertScope(lines, 2, "r", "variable.other.property.rl");
  assertScope(lines, 2, "number", "support.type.primitive.ts");
  assertScope(lines, 2, "Dot", "variable.other.enummember.ts");

  // match — 초기화식 위치, 패턴/가드/or-패턴/리터럴/와일드카드.
  assertScope(lines, 3, "match", "keyword.control.match.rl");
  assertScope(lines, 4, "Circle", "variable.other.enummember.ts");
  assertScope(lines, 4, "=>", "storage.type.function.arrow.ts");
  assertScope(lines, 5, "w", "variable.other.property.rl");
  assertScope(lines, 5, "width", "variable.other.readwrite.ts");
  assertScope(lines, 5, "if", "keyword.control.conditional.ts");
  assertScope(lines, 6, "|", "keyword.operator.or-pattern.rl");
  assertScope(lines, 7, "north", "string.quoted.double.ts");
  assertScope(lines, 8, "true", "constant.language.boolean.ts");
  assertScope(lines, 9, "Circle", "variable.other.enummember.ts");
  assertScope(lines, 10, "_", "keyword.control.wildcard.rl");

  // 파이프라인과 flow.
  assertScope(lines, 12, "|>", "keyword.operator.pipeline.rl");
  assertScope(lines, 13, "flow", "keyword.control.flow.rl");

  // try 문(식 형태)과 값 바인딩 형태.
  assertScope(lines, 14, "try", "keyword.control.trycatch.ts");
  assertScope(lines, 15, "try", "keyword.control.trycatch.ts");

  // let-else와 if let — 패턴, else, 본문.
  assertScope(lines, 16, "Some", "variable.other.enummember.ts");
  assertScope(lines, 16, "value", "variable.other.property.rl");
  assertScope(lines, 16, "user", "variable.other.readwrite.ts");
  assertScope(lines, 16, "else", "keyword.control.conditional.ts");
  assertScope(lines, 16, "return", "keyword.control.flow.ts");
  assertScope(lines, 17, "if", "keyword.control.conditional.ts");
  assertScope(lines, 17, "let", "storage.type.ts");
  assertScope(lines, 17, "Some", "variable.other.enummember.ts");
  assertScope(lines, 18, "greet", "entity.name.function.ts");

  // val — for 헤드와 catch 매개변수.
  assertScope(lines, 22, "val", "storage.modifier.val.rl");
  assertScope(lines, 23, "val", "storage.modifier.val.rl");
  assertScope(lines, 23, "error", "variable.parameter.ts");
  assertScope(lines, 23, "unknown", "support.type.primitive.ts");

  // result — 타입 주석 바인딩과 구조 분해 바인딩.
  assertScope(lines, 24, "result", "keyword.control.result.rl");
  assertScope(lines, 25, "n", "variable.other.readwrite.ts");
  assertScope(lines, 25, "number", "support.type.primitive.ts");
  assertScope(lines, 25, "<-", "keyword.operator.result-bind.rl");
  assertScope(lines, 26, "x", "variable.other.readwrite.ts");
  assertScope(lines, 26, "<-", "keyword.operator.result-bind.rl");
});

// rl 구문과 형태가 겹치는 순수 TypeScript — rl로 오인하면 안 된다.
const LOOKALIKES = `const enum Flags { A = 1, B = 2 }
function passthrough(a: number, b: number) {
  const cmp = a < -b;
  const box = { result: 1 };
  return cmp;
}
`;

test("TypeScript look-alikes are not claimed by rl rules", async () => {
  const lines = await tokenize("source.rl", LOOKALIKES);
  // const enum은 TS enum 규칙의 것 — rl enum 스코프가 없어야 한다.
  // (루트 스코프 source.rl은 모든 토큰이 가지므로 제외.)
  for (const token of lines[0]) {
    assert.ok(
      !token.scopes.slice(1).some((s) => s.endsWith(".rl")),
      `const enum leaked an rl scope on ${JSON.stringify(token.text)}`,
    );
  }
  // `a < -b`는 비교 — result 바인딩이 아니다.
  assertScope(lines, 3, "<", "keyword.operator.relational.ts");
  // 객체 키 result는 키워드가 아니다.
  const key = tokenAt(lines, 4, "result");
  assert.ok(!key.scopes.includes("keyword.control.result.rl"));
});

// 순수 TypeScript 픽스처 — rl 문법과 TS 문법의 스코프가 같아야 한다.
const PURE_TS = `import { readFile } from "node:fs/promises";

export interface User { name: string; tags: string[]; }
export type Outcome<T, E> = { ok: true; value: T } | { ok: false; error: E };

export class Repo<T extends User> {
  private cache = new Map<string, T>();
  constructor(private readonly root: string) {}
  async load(id: string): Promise<T | undefined> {
    try {
      const raw = await readFile(\`\${this.root}/\${id}.json\`, "utf8");
      return JSON.parse(raw) as T;
    } catch (error: unknown) {
      console.error("load failed", error);
      return undefined;
    }
  }
}

export function totals(xs: number[]): number {
  let sum = 0;
  for (const x of xs) sum += x;
  const doubled = xs.map((x) => x * 2).filter((x) => x > 0);
  return doubled.reduce((a, b) => a + b, sum);
}

const label = totals([1, 2, 3]) > 3 ? "big" : "small";
export default { label };
`;

test("pure TypeScript tokenizes identically to the TypeScript grammar", async () => {
  const rl = await tokenize("source.rl", PURE_TS);
  const ts = await tokenize("source.ts", PURE_TS);
  const strip = (lines: Token[][]) =>
    lines.map((line) =>
      line.map((t) => ({ text: t.text, scopes: t.scopes.slice(1).join(" ") })),
    );
  assert.deepEqual(strip(rl), strip(ts));
});

test("generated grammar matches its sources (build.mjs --check)", () => {
  execFileSync(process.execPath, [path.join(syntaxesDir, "build.mjs"), "--check"], {
    stdio: "pipe",
  });
});
