/* --------------------------------------------------------------------------
 * A minimal LSP client, for talking to the TypeScript compiler's own server.
 *
 * TypeScript 7 has no JS compiler API; what it has is a language server, and
 * that server answers for documents it was *given* — a file that exists only
 * in the client's buffer is answered exactly like one on disk. That is what
 * lets rl ask about the TypeScript an `.rl` file lowers to without writing
 * that TypeScript anywhere.
 *
 * Only the parts rl uses are here: the framing, request/response, and
 * document sync. Two details are load-bearing and easy to miss:
 *
 * - **Server-initiated requests must be answered.** tsgo sends
 *   `client/registerCapability` during startup and waits for the reply; a
 *   client that ignores it hangs on every later request, with no error.
 * - **Diagnostics are pulled, not pushed.** The server advertises a
 *   `diagnosticProvider`, so `textDocument/diagnostic` is a request.
 * ----------------------------------------------------------------------- */
import { type ChildProcess, spawn } from "node:child_process";

/** A position in a document: zero-based line, UTF-16 code units. */
export interface LspPosition {
  line: number;
  character: number;
}

export interface LspRange {
  start: LspPosition;
  end: LspPosition;
}

export interface LspLocation {
  uri: string;
  range: LspRange;
}

export interface LspDiagnostic {
  range: LspRange;
  severity?: number;
  code?: number | string;
  message: string;
}

/** What the server answered, or the reason it did not. */
type Response = { result?: unknown; error?: { message: string } };

/** How long any one request may take before the client gives up on it. A
 * hung request must not hang the editor; the feature simply has no answer. */
const REQUEST_TIMEOUT_MS = 8000;

/**
 * A running language server, and the conversation with it.
 *
 * Documents are opened and updated through [`open`] and [`change`]; every
 * question is a [`request`]. The server is started lazily by the caller and
 * stopped with [`dispose`].
 */
export class LspClient {
  private readonly child: ChildProcess;
  private buffer: Buffer = Buffer.alloc(0);
  private readonly pending = new Map<number, (response: Response) => void>();
  private nextId = 1;
  private readonly opened = new Map<string, number>();
  private initialized: Promise<void>;
  private disposed = false;

  constructor(
    executable: string,
    private readonly rootUri: string,
    private readonly onLog?: (message: string) => void,
  ) {
    this.child = spawn(executable, ["--lsp", "-stdio"], {
      stdio: ["pipe", "pipe", "pipe"],
    });
    this.child.stdout?.on("data", (chunk: Buffer) => this.receive(chunk));
    this.child.stderr?.on("data", (chunk: Buffer) => this.onLog?.(String(chunk)));
    this.child.on("exit", (code) => {
      // Fail everything still waiting rather than leaving it hanging.
      for (const resolve of this.pending.values()) {
        resolve({ error: { message: `the TypeScript server exited (${code})` } });
      }
      this.pending.clear();
    });
    this.initialized = this.handshake();
  }

  /** Whether the server is still running. */
  get alive(): boolean {
    return !this.disposed && this.child.exitCode === null;
  }

  private async handshake(): Promise<void> {
    await this.request("initialize", {
      processId: process.pid,
      rootUri: this.rootUri,
      workspaceFolders: [{ uri: this.rootUri, name: "rl" }],
      capabilities: {
        textDocument: {
          synchronization: { dynamicRegistration: true },
          hover: { contentFormat: ["plaintext", "markdown"] },
          definition: {},
          references: {},
          completion: { completionItem: { labelDetailsSupport: true } },
          signatureHelp: {},
          rename: { prepareSupport: true },
          diagnostic: {},
        },
        workspace: { configuration: true, workspaceFolders: true },
      },
    });
    this.notify("initialized", {});
  }

  /** Serves `text` as `uri`, whether or not anything is there on disk. */
  async open(uri: string, text: string): Promise<void> {
    await this.initialized;
    const version = (this.opened.get(uri) ?? 0) + 1;
    this.opened.set(uri, version);
    if (version === 1) {
      this.notify("textDocument/didOpen", {
        textDocument: { uri, languageId: "typescript", version, text },
      });
    } else {
      // Whole-document sync: the lowered text is regenerated as a whole, so
      // there is no incremental edit to describe.
      this.notify("textDocument/didChange", {
        textDocument: { uri, version },
        contentChanges: [{ text }],
      });
    }
  }

  /** Stops serving `uri`. */
  close(uri: string): void {
    if (!this.opened.delete(uri)) return;
    this.notify("textDocument/didClose", { textDocument: { uri } });
  }

  /** Asks the server something, or gives up after [`REQUEST_TIMEOUT_MS`]. */
  async request(method: string, params: unknown): Promise<Response> {
    if (!this.alive) return { error: { message: "the TypeScript server is not running" } };
    const id = this.nextId++;
    return new Promise<Response>((resolve) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        resolve({ error: { message: `${method} timed out` } });
      }, REQUEST_TIMEOUT_MS);
      this.pending.set(id, (response) => {
        clearTimeout(timer);
        resolve(response);
      });
      this.send({ id, method, params });
    });
  }

  /** A request whose answer is only useful after the document is served. */
  async ask(method: string, params: unknown): Promise<unknown> {
    await this.initialized;
    const response = await this.request(method, params);
    if (response.error) {
      this.onLog?.(`rl: ${method}: ${response.error.message}`);
      return null;
    }
    return response.result ?? null;
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.child.kill();
  }

  private notify(method: string, params: unknown): void {
    this.send({ method, params });
  }

  private send(message: Record<string, unknown>): void {
    if (!this.alive) return;
    const text = JSON.stringify({ jsonrpc: "2.0", ...message });
    this.child.stdin?.write(`Content-Length: ${Buffer.byteLength(text)}\r\n\r\n${text}`);
  }

  private receive(chunk: Buffer): void {
    this.buffer = Buffer.concat([this.buffer, chunk]);
    while (true) {
      const header = this.buffer.indexOf("\r\n\r\n");
      if (header < 0) return;
      const length = /Content-Length: (\d+)/.exec(this.buffer.subarray(0, header).toString());
      if (!length) return;
      const size = Number(length[1]);
      if (this.buffer.length < header + 4 + size) return;
      const body = this.buffer.subarray(header + 4, header + 4 + size).toString();
      this.buffer = this.buffer.subarray(header + 4 + size);
      this.dispatch(JSON.parse(body) as Record<string, unknown>);
    }
  }

  private dispatch(message: Record<string, unknown>): void {
    const id = message.id as number | string | undefined;
    if (id !== undefined && message.method === undefined) {
      const resolve = this.pending.get(id as number);
      if (resolve) {
        this.pending.delete(id as number);
        resolve(message as Response);
      }
      return;
    }
    if (message.method === undefined) return;
    // A request from the server. Answering is not optional: tsgo registers
    // capabilities during startup and waits for the reply before serving
    // anything else.
    if (id !== undefined) {
      this.send({
        id,
        result: message.method === "workspace/configuration" ? [{}] : null,
      });
    }
  }
}

/** The offset a zero-based `line`/`character` names in `text`. */
export function offsetAt(text: string, position: LspPosition): number {
  let offset = 0;
  for (let line = 0; line < position.line; line++) {
    const next = text.indexOf("\n", offset);
    if (next < 0) return text.length;
    offset = next + 1;
  }
  return Math.min(offset + position.character, text.length);
}

/** The `line`/`character` an offset names in `text` — LSP's own form. */
export function positionAt(text: string, offset: number): LspPosition {
  const clamped = Math.max(0, Math.min(offset, text.length));
  const before = text.slice(0, clamped);
  const line = before.split("\n").length - 1;
  return { line, character: clamped - (before.lastIndexOf("\n") + 1) };
}
