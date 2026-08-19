/* --------------------------------------------------------------------------
 * The engine session — one persistent `rlc --server` conversation.
 *
 * The language server is a protocol adapter; the language engine is rlc's
 * (docs/design/lsp-architecture.md). This module is the client half of that
 * split: it holds the one child process, forwards the editor's document
 * lifecycle (open/change/close) so the engine's project state is the
 * authoritative one, and asks the engine's semantic API — hover, definition,
 * references, completion (probe included), rename, signature help, type
 * diagnostics — all answered in `.rl` coordinates. No projection, mapping,
 * TypeScript project or probe logic lives on this side of the pipe.
 *
 * Every caller degrades gracefully: when the server is unavailable (an
 * older rlc, a crash), requests resolve to null and the feature simply has
 * no answer — exactly how a missing TypeScript toolchain has always
 * behaved. Two consecutive losses without a single answer disable the
 * server for the process (an rlc without `--server` is not going to grow
 * one), and `rlc.ts`'s check/typedCheck callers then fall back to their
 * one-shot commands.
 * ----------------------------------------------------------------------- */
import { ChildProcess, spawn } from "child_process";

/** The name a rename asks the engine for, standing in for the new name in
 * every edit's `newText` (see `onRenameRequest`). */
export const RENAME_PLACEHOLDER = "rlRenamePlaceholder";

/** A position/range as both LSP and the engine protocol speak them. */
export interface EnginePosition {
  line: number;
  character: number;
}

export interface EngineRange {
  start: EnginePosition;
  end: EnginePosition;
}

export interface EngineLocation {
  path: string;
  range: EngineRange;
}

export interface EngineHover {
  signature: string;
  documentation: string;
  range: EngineRange;
}

export interface EngineCompletionItem {
  label: string;
  /** The element-kind string the editor has always mapped. */
  kind: string;
  sortText: string;
}

export interface EngineCompletionList {
  items: EngineCompletionItem[];
  member: boolean;
  /** Set when the list was answered from a completion probe; carried back
   * to `completionResolve` so the entry resolves against the same text. */
  probe: number | null;
}

export interface EngineCompletionDetail {
  signature: string;
  documentation: string;
}

export interface EngineRenameEdit extends EngineLocation {
  /** What the engine wants written, with RENAME_PLACEHOLDER standing in
   * for the new name; null for the bare name. */
  newText: string | null;
}

export interface EngineSignatureHelp {
  signatures: {
    label: string;
    documentation: string;
    parameters: { label: [number, number]; documentation: string }[];
  }[];
  activeSignature: number;
  activeParameter: number;
}

export interface EngineDiagnostic {
  range: EngineRange;
  message: string;
  code: number;
  warning: boolean;
}

/** How a request ended: an engine result, an engine error (the session is
 * fine, the request failed), or null — the server itself is unavailable. */
export type EngineAnswer = { result: unknown } | { error: string } | null;

interface EngineServer {
  child: ChildProcess;
  pending: Map<
    number,
    { resolve: (v: EngineAnswer) => void; timer: NodeJS.Timeout }
  >;
  nextId: number;
  buffer: string;
  alive: boolean;
  /** Whether any request was ever answered — a server that dies before its
   * first answer is an rlc without `--server`, and retrying is pointless. */
  answered: boolean;
}

let engineServer: EngineServer | null = null;
let engineServerCompiler: string | null = null;
/** Consecutive server losses without a single answer; two disable it. */
let engineServerStrikes = 0;

/** Called whenever a fresh server comes up (first spawn or respawn after a
 * crash), so the owner can re-send the documents it holds open — a new
 * engine session starts with the disk's view of the world. */
let onSessionStart: (() => void) | null = null;

export function setOnSessionStart(callback: (() => void) | null): void {
  onSessionStart = callback;
}

function engineServerFor(compiler: string): EngineServer | null {
  if (engineServerStrikes >= 2) return null;
  if (engineServer && engineServer.alive && engineServerCompiler === compiler) {
    return engineServer;
  }
  shutdownEngineServer();
  let child: ChildProcess;
  try {
    child = spawn(compiler, ["--server"], {
      stdio: ["pipe", "pipe", "ignore"],
    });
  } catch {
    engineServerStrikes += 1;
    return null;
  }
  const server: EngineServer = {
    child,
    pending: new Map(),
    nextId: 1,
    buffer: "",
    alive: true,
    answered: false,
  };
  const fail = () => {
    if (!server.alive) return;
    server.alive = false;
    if (!server.answered) engineServerStrikes += 1;
    for (const [, entry] of server.pending) {
      clearTimeout(entry.timer);
      entry.resolve(null);
    }
    server.pending.clear();
  };
  child.on("error", fail);
  child.on("exit", fail);
  child.stdout?.setEncoding("utf8");
  child.stdout?.on("data", (chunk: string) => {
    server.buffer += chunk;
    let newline: number;
    while ((newline = server.buffer.indexOf("\n")) !== -1) {
      const line = server.buffer.slice(0, newline);
      server.buffer = server.buffer.slice(newline + 1);
      let message: { id?: number; result?: unknown; error?: string };
      try {
        message = JSON.parse(line);
      } catch {
        continue;
      }
      const entry =
        typeof message.id === "number"
          ? server.pending.get(message.id)
          : undefined;
      if (!entry) continue;
      server.pending.delete(message.id as number);
      clearTimeout(entry.timer);
      server.answered = true;
      engineServerStrikes = 0;
      entry.resolve(
        typeof message.error === "string"
          ? { error: message.error }
          : { result: message.result },
      );
    }
  });
  // The server must never keep the host process alive: it exits on its own
  // when this process's end of stdin closes.
  child.unref();
  (child.stdin as unknown as { unref?: () => void })?.unref?.();
  (child.stdout as unknown as { unref?: () => void })?.unref?.();
  engineServer = server;
  engineServerCompiler = compiler;
  onSessionStart?.();
  return server;
}

export function engineRequest(
  compiler: string,
  method: string,
  params: unknown,
  timeoutMs: number,
): Promise<EngineAnswer> {
  const server = engineServerFor(compiler);
  if (!server || !server.child.stdin?.writable) {
    return Promise.resolve(null);
  }
  const id = server.nextId++;
  return new Promise((resolve) => {
    // The timeout stays ref'd on purpose: while a request is in flight it
    // is the one handle keeping the event loop alive (the child and its
    // pipes are unref'd so an *idle* server never holds the process open).
    const timer = setTimeout(() => {
      server.pending.delete(id);
      resolve(null);
    }, timeoutMs);
    server.pending.set(id, { resolve, timer });
    server.child.stdin?.write(
      JSON.stringify({ id, method, params }) + "\n",
      (err) => {
        if (err) {
          const entry = server.pending.get(id);
          if (entry) {
            server.pending.delete(id);
            clearTimeout(entry.timer);
            resolve(null);
          }
        }
      },
    );
  });
}

/** Ends the engine server, if one is running. Tests call this so the
 * process can exit; the language server just dies with its client. */
export function shutdownEngineServer(): void {
  if (engineServer) {
    try {
      engineServer.child.kill();
    } catch {
      // already gone
    }
    engineServer.alive = false;
    engineServer = null;
    engineServerCompiler = null;
  }
}

/* ------------------------------------------------------------ documents --
 * The editor's open buffers, mirrored into the engine so its project state
 * is the authoritative one. Sends are synchronous writes on one pipe, so
 * a request written after a change is answered after it too — the queue is
 * the ordering (the same guarantee tsgo's LSP gets from its own).
 * ---------------------------------------------------------------------- */

const SYNC_TIMEOUT_MS = 15000;
const SEMANTIC_TIMEOUT_MS = 15000;

export function openDocument(compiler: string, path: string, text: string): void {
  void engineRequest(compiler, "openDocument", { path, text }, SYNC_TIMEOUT_MS);
}

export function updateDocument(compiler: string, path: string, text: string): void {
  void engineRequest(compiler, "updateDocument", { path, text }, SYNC_TIMEOUT_MS);
}

export function closeDocument(compiler: string, path: string): void {
  void engineRequest(compiler, "closeDocument", { path }, SYNC_TIMEOUT_MS);
}

/* ------------------------------------------------------------ semantics -- */

async function semantic<T>(
  compiler: string,
  method: string,
  params: unknown,
  onError?: (message: string) => void,
): Promise<T | null> {
  const answer = await engineRequest(compiler, method, params, SEMANTIC_TIMEOUT_MS);
  if (!answer) return null;
  if ("error" in answer) {
    onError?.(`rl: ${method}: ${answer.error}`);
    return null;
  }
  return (answer.result ?? null) as T | null;
}

export function hover(
  compiler: string,
  path: string,
  position: EnginePosition,
  onError?: (message: string) => void,
): Promise<EngineHover | null> {
  return semantic(compiler, "hover", { path, position }, onError);
}

export async function definition(
  compiler: string,
  path: string,
  position: EnginePosition,
  onError?: (message: string) => void,
): Promise<EngineLocation[]> {
  const result = await semantic<{ locations: EngineLocation[] }>(
    compiler,
    "definition",
    { path, position },
    onError,
  );
  return result?.locations ?? [];
}

export async function references(
  compiler: string,
  path: string,
  position: EnginePosition,
  onError?: (message: string) => void,
): Promise<(EngineLocation & { isDefinition: boolean })[]> {
  const result = await semantic<{
    locations: (EngineLocation & { isDefinition: boolean })[];
  }>(compiler, "references", { path, position }, onError);
  return result?.locations ?? [];
}

export function completion(
  compiler: string,
  path: string,
  position: EnginePosition,
  member: boolean,
  onError?: (message: string) => void,
): Promise<EngineCompletionList | null> {
  return semantic(compiler, "completion", { path, position, member }, onError);
}

export function completionResolve(
  compiler: string,
  path: string,
  position: EnginePosition,
  label: string,
  probe: number | undefined,
  onError?: (message: string) => void,
): Promise<EngineCompletionDetail | null> {
  return semantic(
    compiler,
    "completionResolve",
    { path, position, label, probe: probe ?? null },
    onError,
  );
}

export async function rename(
  compiler: string,
  path: string,
  position: EnginePosition,
  onError?: (message: string) => void,
): Promise<EngineRenameEdit[] | null> {
  const result = await semantic<{ edits: EngineRenameEdit[] | null }>(
    compiler,
    "rename",
    { path, position },
    onError,
  );
  return result?.edits ?? null;
}

export function signatureHelp(
  compiler: string,
  path: string,
  position: EnginePosition,
  onError?: (message: string) => void,
): Promise<EngineSignatureHelp | null> {
  return semantic(compiler, "signatureHelp", { path, position }, onError);
}

export async function tsDiagnostics(
  compiler: string,
  path: string,
  onError?: (message: string) => void,
): Promise<EngineDiagnostic[]> {
  const result = await semantic<{ diagnostics: EngineDiagnostic[] }>(
    compiler,
    "tsDiagnostics",
    { path },
    onError,
  );
  return result?.diagnostics ?? [];
}
