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
/** Each case spawns a server and compiles through it; generous, and only
 * reached when something has hung. */
const timeout = 60_000;

interface Client {
  request(method: string, params: unknown): Promise<any>;
  notify(method: string, params: unknown): void;
  stop(): void;
}

/** The framing an LSP client speaks: `Content-Length` headers over stdio. */
function connect(): Client {
  const child: ChildProcess = spawn(process.execPath, [SERVER, "--stdio"], {
    stdio: ["pipe", "pipe", "ignore"],
  });
  const pending = new Map<number, (body: any) => void>();
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

  return { client, completion, stop: () => client.stop() };
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
  { skip, timeout },
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
  { skip, timeout },
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
  { skip, timeout },
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
  { skip, timeout },
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
