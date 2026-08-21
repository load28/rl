/* End-to-end tests of the language server itself, over LSP (TASK-062).
 *
 * The modules below it are unit-tested elsewhere; what only shows up here is
 * how the server *composes* them — which is where the reported bug lived:
 * `Result.` answered with the enum's two constructors instead of, rather
 * than alongside, everything the TypeScript language service knows about
 * the standard library namespace.
 *
 * The server is spawned as the editor spawns it (`--stdio`) and driven with
 * a minimal JSON-RPC client, so the assertions are on what an editor would
 * actually receive. It runs the real compiler, so these skip when it is not
 * on PATH.
 */
import * as assert from "node:assert/strict";
import { test } from "node:test";
import { execFileSync, spawn, ChildProcess } from "node:child_process";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { pathToFileURL } from "node:url";

import { findTsgo } from "./toolchain";

const SERVER = path.join(__dirname, "..", "server.js");
const COMPILER = "rlc";

function compilerAvailable(): boolean {
  try {
    execFileSync(COMPILER, ["-v"], { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
}

const skip = compilerAvailable() ? false : "rlc not on PATH";
/** Answers that need the TypeScript language service. A skip must mean a
 * tool is missing, never that a feature quietly answered nothing — so the
 * cases below that ask TypeScript guard on tsgo as well as on rlc. */
const skipTyped = skip || (findTsgo() ? false : "tsgo not installed");
/** Each case spawns a server and compiles through it; generous, and only
 * reached when something has hung. */
const timeout = 60_000;

interface Client {
  request(method: string, params: unknown): Promise<any>;
  notify(method: string, params: unknown): void;
  /** The next notification of `method` that satisfies `want` — the server
   * publishes diagnostics on its own schedule, so a test waits for one. */
  waitFor(method: string, want: (params: any) => boolean): Promise<any>;
  stop(): void;
}

/** The framing an LSP client speaks: `Content-Length` headers over stdio. */
function connect(): Client {
  const child: ChildProcess = spawn(process.execPath, [SERVER, "--stdio"], {
    stdio: ["pipe", "pipe", "ignore"],
  });
  const pending = new Map<number, (body: any) => void>();
  interface Waiter {
    method: string;
    want: (params: any) => boolean;
    resolve: (params: any) => void;
  }
  const waiters = new Map<number, Waiter>();
  let nextWaiter = 1;
  let nextId = 1;
  let buf = Buffer.alloc(0);

  child.stdout!.on("data", (chunk: Buffer) => {
    buf = Buffer.concat([buf, chunk]);
    for (;;) {
      const sep = buf.indexOf("\r\n\r\n");
      if (sep < 0) return;
      const length = /content-length: (\d+)/i.exec(
        buf.subarray(0, sep).toString(),
      );
      if (!length) return;
      const size = Number(length[1]);
      if (buf.length < sep + 4 + size) return;
      const body = JSON.parse(buf.subarray(sep + 4, sep + 4 + size).toString());
      buf = buf.subarray(sep + 4 + size);
      const resolve = body.id !== undefined ? pending.get(body.id) : undefined;
      if (resolve) {
        pending.delete(body.id);
        resolve(body);
        continue;
      }
      if (body.method !== undefined) {
        for (const [n, waiter] of [...waiters.entries()]) {
          if (waiter.method === body.method && waiter.want(body.params)) {
            waiters.delete(n);
            waiter.resolve(body.params);
          }
        }
      }
    }
  });

  const send = (message: unknown): void => {
    const text = JSON.stringify({ jsonrpc: "2.0", ...(message as object) });
    child.stdin!.write(
      `Content-Length: ${Buffer.byteLength(text)}\r\n\r\n${text}`,
    );
  };
  return {
    request: (method, params) =>
      new Promise((resolve) => {
        const id = nextId++;
        pending.set(id, resolve);
        send({ id, method, params });
      }),
    notify: (method, params) => send({ method, params }),
    waitFor: (method, want) =>
      new Promise((resolve) => {
        waiters.set(nextWaiter++, { method, want, resolve });
      }),
    stop: () => child.kill(),
  };
}

/** A server with `source` open as an .rl document, ready to be asked. */
async function open(source: string) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "rl-server-test-"));
  const file = path.join(dir, "main.rl");
  fs.writeFileSync(file, source);
  const uri = pathToFileURL(file).toString();
  const client = connect();
  await client.request("initialize", {
    processId: process.pid,
    rootUri: pathToFileURL(dir).toString(),
    workspaceFolders: [{ uri: pathToFileURL(dir).toString(), name: "test" }],
    // No `workspace.configuration`: the server then uses its defaults
    // instead of asking, which this client would not answer.
    capabilities: {},
  });
  client.notify("initialized", {});
  client.notify("textDocument/didOpen", {
    textDocument: { uri, languageId: "rl", version: 1, text: source },
  });

  /** Completion at the end of `marker`'s first occurrence. */
  const completion = async (marker: string) => {
    const offset = source.indexOf(marker) + marker.length;
    assert.notEqual(offset, marker.length - 1, `marker not found: ${marker}`);
    const before = source.slice(0, offset);
    const line = before.split("\n").length - 1;
    const response = await client.request("textDocument/completion", {
      textDocument: { uri },
      position: {
        line,
        character: before.length - (before.lastIndexOf("\n") + 1),
      },
      context: { triggerKind: 2, triggerCharacter: "." },
    });
    const items = (
      Array.isArray(response.result)
        ? response.result
        : (response.result?.items ?? [])
    ) as any[];
    return {
      items,
      labels: items.map((i) => i.label as string),
      /** The resolved form of one item, as the editor asks for it when the
       * user highlights it. */
      resolve: async (label: string) => {
        const item = items.find((i) => i.label === label);
        assert.ok(item, `no completion item ${label} in: ${items.map((i) => i.label)}`);
        return (await client.request("completionItem/resolve", item)).result;
      },
    };
  };

  return { client, uri, completion, stop: () => client.stop() };
}

const STD_SOURCE = [
  'import { Option, Result } from "@rl/std";',
  "",
  "declare const r: Result<number, string>;",
  "const out = Result.",
  "",
].join("\n");

test(
  "`Result.` completes the constructors and the combinators",
  { skip: skipTyped, timeout },
  async () => {
    const { completion, stop } = await open(STD_SOURCE);
    try {
      const { labels, resolve } = await completion("const out = Result.");
      // rl's own: the case constructors, first in the list.
      assert.deepEqual(labels.slice(0, 2), ["Ok", "Err"]);
      // TypeScript's: the standard library combinators that used to be
      // dropped on the floor by returning only the constructors.
      for (const member of ["map", "andThen", "unwrapOr", "mapErrP", "ok"]) {
        assert.ok(labels.includes(member), `missing ${member} in: ${labels}`);
      }

      const resolved = await resolve("map");
      assert.ok(
        String(resolved.detail).includes("Result<U, E>"),
        `detail was: ${resolved.detail}`,
      );
      assert.ok(
        String(resolved.documentation?.value).includes("Applies `f`"),
        `documentation was: ${JSON.stringify(resolved.documentation)}`,
      );
    } finally {
      stop();
    }
  },
);

const PIPE_SOURCE = ["const n: number = 1;", "const s = n", "  |> .", ""].join(
  "\n",
);

test(
  "a pipeline method step completes the piped value's members",
  { skip: skipTyped, timeout },
  async () => {
    const { completion, stop } = await open(PIPE_SOURCE);
    try {
      const { labels, resolve } = await completion("  |> .");
      for (const member of ["toFixed", "toString", "toPrecision"]) {
        assert.ok(labels.includes(member), `missing ${member} in: ${labels}`);
      }
      // A member access is members only — no enum names, no rl snippets.
      for (const noise of ["Option", "Result", "match", "enum"]) {
        assert.ok(!labels.includes(noise), `unexpected ${noise} in: ${labels}`);
      }
      const resolved = await resolve("toFixed");
      assert.ok(
        String(resolved.detail).includes("fractionDigits"),
        `detail was: ${resolved.detail}`,
      );
    } finally {
      stop();
    }
  },
);

test(
  "a member access in a step never falls back to the global scope",
  { skip: skipTyped, timeout },
  async () => {
    // Recovering from `|>`, TypeScript can lose the dot and answer with
    // every name in scope — the compiler's own `$rl_ap` helper included.
    // That answer is not a member list and must not be shown as one.
    const source = ["const n: number = 1;", "const s = n", "  |> n.", ""].join(
      "\n",
    );
    const { completion, stop } = await open(source);
    try {
      const { labels } = await completion("  |> n.");
      assert.ok(labels.includes("toFixed"), `members were: ${labels}`);
      for (const leaked of ["$rl_ap", "n", "s", "AbortController"]) {
        assert.ok(
          !labels.includes(leaked),
          `global scope leaked ${leaked} into: ${labels}`,
        );
      }
    } finally {
      stop();
    }
  },
);

test(
  "a pipeline of std combinators keeps completing at each step",
  { skip: skipTyped, timeout },
  async () => {
    const source = [
      'import { Result } from "@rl/std";',
      "",
      "declare const r: Result<number, string>;",
      "const out = r",
      "  |> Result.mapP((n) => n + 1)",
      "  |> Result.",
      "",
    ].join("\n");
    const { completion, stop } = await open(source);
    try {
      const { labels } = await completion("  |> Result.");
      for (const member of ["Ok", "Err", "mapP", "andThenP", "unwrapOrP"]) {
        assert.ok(labels.includes(member), `missing ${member} in: ${labels}`);
      }
    } finally {
      stop();
    }
  },
);

/* ---------------------------------------------------------------- semantic
 * tokens — the parser's classification, layered over the grammar (TASK-093).
 * Parse-only on the engine side, so unlike the completion cases these need
 * only the compiler, never a TypeScript toolchain.
 * -------------------------------------------------------------------------- */

/** The legend the server declares — mirrored here to decode the response. */
const TOKEN_TYPES = [
  "keyword",
  "enum",
  "enumMember",
  "variable",
  "property",
  "function",
  "operator",
];

/** Decodes the LSP delta-encoded quintuples into absolute tokens. */
function decodeTokens(
  data: number[],
): { line: number; character: number; length: number; type: string }[] {
  const out = [];
  let line = 0;
  let character = 0;
  for (let i = 0; i < data.length; i += 5) {
    line += data[i];
    character = data[i] === 0 ? character + data[i + 1] : data[i + 1];
    out.push({
      line,
      character,
      length: data[i + 2],
      type: TOKEN_TYPES[data[i + 3]],
    });
  }
  return out;
}

test(
  "semantic tokens carry the parser's own classification",
  { skip, timeout },
  async () => {
    const source = [
      "declare function match(n: number): number;",
      "const denied = match(1);",
      "const composed = flow",
      "  |> trim",
      "  |> parse;",
      "export function pick(shape: Shape): number {",
      "  return match (shape) {",
      "    Circle(r) => r,",
      "    _ => 0,",
      "  };",
      "}",
      "",
    ].join("\n");
    const { client, uri, stop } = await open(source);
    try {
      const response = await client.request("textDocument/semanticTokens/full", {
        textDocument: { uri },
      });
      const tokens = decodeTokens(response.result?.data ?? []);
      const at = (line: number, character: number) =>
        tokens.find((t) => t.line === line && t.character === character);

      // The call to a plain function named `match` is *denied* — reported
      // as the function it is, overriding the grammar's keyword color.
      assert.equal(at(1, 15)?.type, "function");
      // A `flow` head whose first `|>` sits on the next line is *claimed* —
      // the grammar's same-line lookahead cannot see it, the parser can.
      assert.equal(at(2, 17)?.type, "keyword");
      // The real match expression and its pattern.
      assert.equal(at(6, 9)?.type, "keyword");
      assert.equal(at(7, 4)?.type, "enumMember");
      assert.equal(at(7, 11)?.type, "variable");
    } finally {
      stop();
    }
  },
);

/* ---------------------------------------------------------------------- *
 * rl's own names (TASK-107): hover, definition and completion for the
 * three name spaces that exist only in `.rl` source. The engine answers
 * these from the compiler's declaration table, so — unlike everything
 * above — they need no TypeScript toolchain at all.
 * ---------------------------------------------------------------------- */

const SHAPE_SOURCE = [
  "enum Shape { Circle(radius: number), Rect(w: number, h: number), Point }",
  "declare const s: Shape;",
  "const area = match (s) {",
  "  Circle(radius) => radius,",
  "  Rect(w, h) => w * h,",
  "  Point => 0,",
  "};",
  "if let Circle(radius: r) = s { use(r); }",
  "",
].join("\n");

/** The position just past `marker`'s first occurrence, as LSP counts. */
function positionOf(source: string, marker: string) {
  const offset = source.indexOf(marker) + marker.length;
  const before = source.slice(0, offset);
  return {
    line: before.split("\n").length - 1,
    character: before.length - (before.lastIndexOf("\n") + 1),
  };
}

test("a case tag hovers as its declaration — in a match and in an if let", { skip, timeout }, async () => {
  const { client, uri, stop } = await open(SHAPE_SOURCE);
  try {
    for (const marker of ["  Circ", "if let Circ"]) {
      const answer = await client.request("textDocument/hover", {
        textDocument: { uri },
        position: positionOf(SHAPE_SOURCE, marker),
      });
      const value = String(answer.result?.contents?.value ?? "");
      assert.ok(
        value.includes("Shape.Circle(radius: number)"),
        `hover at ${marker} was: ${value}`,
      );
    }
  } finally {
    stop();
  }
});

test("a payload field hovers as its declaration", { skip, timeout }, async () => {
  const { client, uri, stop } = await open(SHAPE_SOURCE);
  try {
    const answer = await client.request("textDocument/hover", {
      textDocument: { uri },
      position: positionOf(SHAPE_SOURCE, "  Circle(rad"),
    });
    const value = String(answer.result?.contents?.value ?? "");
    assert.ok(value.includes("radius: number"), `hover was: ${value}`);
  } finally {
    stop();
  }
});

test("a pattern tag goes to its declaration", { skip, timeout }, async () => {
  const { client, uri, stop } = await open(SHAPE_SOURCE);
  try {
    const answer = await client.request("textDocument/definition", {
      textDocument: { uri },
      position: positionOf(SHAPE_SOURCE, "  Circ"),
    });
    const location = Array.isArray(answer.result)
      ? answer.result[0]
      : answer.result;
    assert.equal(location?.range?.start?.line, 0, "the declaration is line 0");
    assert.equal(location?.uri, uri);
  } finally {
    stop();
  }
});

test("pattern positions complete cases and fields", { skip, timeout }, async () => {
  const { completion, stop } = await open(SHAPE_SOURCE);
  try {
    // An arm position: every case, with the wildcard.
    const arm = await completion("  Rect(w, h) => w * h,\n  ");
    for (const label of ["Circle", "Rect", "Point", "_"]) {
      assert.ok(arm.labels.includes(label), `missing ${label} in: ${arm.labels}`);
    }
    // A payload position: that case's fields, and nothing else.
    const payload = await completion("  Rect(");
    assert.deepEqual(payload.labels, ["w", "h"]);
    // An `if let` — a position this server could not complete at all before.
    const conditional = await completion("if let ");
    assert.ok(
      conditional.labels.includes("Circle"),
      `missing Circle in: ${conditional.labels}`,
    );
  } finally {
    stop();
  }
});

/* ------------------------------------------------------------------ */
/* diagnostic ranges (TASK-116)                                        */
/* ------------------------------------------------------------------ */

/** The text a published range covers, as the editor would underline it. */
function covered(source: string, range: any): string {
  const lines = source.split("\n");
  const at = (p: { line: number; character: number }) =>
    lines.slice(0, p.line).reduce((n, l) => n + l.length + 1, 0) + p.character;
  return source.slice(at(range.start), at(range.end));
}

/** The diagnostics the server publishes for `source`, once it has some. */
async function published(source: string): Promise<any[]> {
  const { client, uri, stop } = await open(source);
  try {
    const params = await client.waitFor(
      "textDocument/publishDiagnostics",
      (p) => p.uri === uri && p.diagnostics.length > 0,
    );
    return params.diagnostics;
  } finally {
    stop();
  }
}

test(
  "a diagnostic underlines the construct it is about",
  { skip, timeout },
  async () => {
    const source = [
      "enum Shape { Circle(r: number), Square(s: number), Tri(a: number) }",
      "export function area(shape: Shape): number {",
      "  return match (shape) {",
      "    Circle(r) => r * r,",
      "    Square(s) => s * s,",
      "  };",
      "}",
      "",
    ].join("\n");
    const diagnostics = await published(source);
    const missing = diagnostics.find((d: any) =>
      d.message.includes("is not exhaustive"),
    );
    assert.ok(missing, `no exhaustiveness error in: ${JSON.stringify(diagnostics)}`);
    // The whole head, not just the word the position lands on.
    assert.equal(covered(source, missing.range), "match (shape)");
  },
);

test(
  "a construct that did not parse is reported where it is written",
  { skip, timeout },
  async () => {
    // Parser recovery owns this unambiguous rl near miss and keeps its
    // stable rule identity through the compiler protocol and LSP adapter.
    const source = [
      "enum Shape { Circle(r: number), Square(s: number) }",
      "export function area(shape: Shape): number {",
      "  return match shape {",
      "    Circle(r) => r,",
      "    Square(s) => s,",
      "  };",
      "}",
      "",
    ].join("\n");
    const diagnostics = await published(source);
    const failed = diagnostics.find((d: any) => d.code === "malformed-match");
    assert.ok(failed, `no parse report in: ${JSON.stringify(diagnostics)}`);
    assert.match(failed.message, /wrap the scrutinee in parentheses/);
    assert.equal(failed.range.start.line, 2);
    assert.equal(covered(source, failed.range), "match");
  },
);

test(
  "return try is diagnosed as an expression placement error",
  { skip, timeout },
  async () => {
    const source = [
      "const a = () => Result.Err(10);",
      "",
      "function Test(): Result<string, string> {",
      "  return try a();",
      "}",
      "",
    ].join("\n");
    const diagnostics = await published(source);
    const failed = diagnostics.find((d: any) => d.code === "try-placement");
    assert.ok(failed, `no placement error in: ${JSON.stringify(diagnostics)}`);
    assert.match(failed.message, /statement, not an expression/);
    assert.equal(covered(source, failed.range), "try a()");
  },
);
