/* --------------------------------------------------------------------------
 * TypeScript delegation through the compiler's own language server.
 *
 * The questions are the same ones [`TsProject`](./tsproject.ts) answers — the
 * symbols an `.rl` file does not own are TypeScript's — but they are asked of
 * TypeScript 7's server rather than of an in-process language service, which
 * TypeScript 7 no longer ships.
 *
 * What the server is given for an `.rl` file is the TypeScript it lowers to,
 * served at `<path>.rl.ts`: the same name the compiler uses in its own
 * project graph, and the name an `import "./x.rl"` resolves to. Nothing is
 * written to disk. Answers come back in the coordinates of *that* text, and
 * the caller maps them to the `.rl` source with the emit map — exactly as it
 * already does.
 * ----------------------------------------------------------------------- */
import { pathToFileURL, fileURLToPath } from "node:url";

import {
  LspClient,
  type LspDiagnostic,
  type LspLocation,
  type LspRange,
  offsetAt,
  positionAt,
} from "./lsp";
import type {
  OpenDoc,
  TsCompletion,
  TsCompletionList,
  TsDefinition,
  TsDiagnostic,
  TsQuickInfo,
  TsReference,
} from "./tsproject";

/** The document name a file takes in the server's world. An `.rl` file is
 * served as the TypeScript it lowers to, under the name that TypeScript
 * resolution would find for `"./x.rl"`. */
export function documentUri(fileName: string): string {
  return pathToFileURL(fileName.endsWith(".rl") ? `${fileName}.ts` : fileName).toString();
}

/** The inverse: which file an answer is about. */
export function fileNameOf(uri: string): string {
  const path = fileURLToPath(uri);
  return path.endsWith(".rl.ts") ? path.slice(0, -".ts".length) : path;
}

/**
 * A project served by the TypeScript language server.
 *
 * Every query serves the document first, so the server always answers about
 * the text the editor currently has rather than whatever is on disk.
 */
export class TsgoProject {
  private readonly client: LspClient;
  /** The text last served for a file, needed to turn offsets into positions
   * and answers back into offsets. */
  private readonly served = new Map<string, string>();

  constructor(
    executable: string,
    rootDir: string,
    private readonly getOpenDoc: (fileName: string) => OpenDoc | null,
    onLog?: (message: string) => void,
  ) {
    this.client = new LspClient(executable, pathToFileURL(rootDir).toString(), onLog);
  }

  dispose(): void {
    this.client.dispose();
  }

  /** Whether the server is running — a dead one answers nothing, and the
   * caller should say so rather than report "no results". */
  get alive(): boolean {
    return this.client.alive;
  }

  /** The text the server was given for a file, if any. */
  fileText(fileName: string): string | null {
    return this.served.get(fileName) ?? null;
  }

  /** Serves the current text of `fileName`, and returns it. */
  private async serve(fileName: string): Promise<string | null> {
    const open = this.getOpenDoc(fileName);
    if (!open) return this.served.get(fileName) ?? null;
    if (this.served.get(fileName) !== open.text) {
      this.served.set(fileName, open.text);
      await this.client.open(documentUri(fileName), open.text);
    }
    return open.text;
  }

  /** Hover: the signature TypeScript shows, and the span it covers. */
  async quickInfoAt(fileName: string, offset: number): Promise<TsQuickInfo | null> {
    const text = await this.serve(fileName);
    if (text === null) return null;
    const hover = (await this.client.ask("textDocument/hover", {
      textDocument: { uri: documentUri(fileName) },
      position: positionAt(text, offset),
    })) as { contents?: { value?: string } | string; range?: LspRange } | null;
    if (!hover) return null;

    const contents =
      typeof hover.contents === "string" ? hover.contents : (hover.contents?.value ?? "");
    if (!contents) return null;
    // The server renders a signature and any documentation as one block; the
    // first fenced or bare paragraph is the signature.
    const { signature, documentation } = splitHover(contents);
    const span = hover.range
      ? spanOf(text, hover.range)
      : { start: offset, length: 0 };
    return { signature, documentation, ...span };
  }

  /** Go to definition. */
  async definitionsAt(fileName: string, offset: number): Promise<TsDefinition[]> {
    const locations = await this.locations(fileName, offset, "textDocument/definition", {});
    return locations;
  }

  /** Find references, definition included. */
  async referencesAt(fileName: string, offset: number): Promise<TsReference[]> {
    const locations = await this.locations(fileName, offset, "textDocument/references", {
      context: { includeDeclaration: true },
    });
    // The server does not mark which location is the declaration; the caller
    // only uses the flag for presentation, so the first one stands in.
    return locations.map((location, index) => ({ ...location, isDefinition: index === 0 }));
  }

  private async locations(
    fileName: string,
    offset: number,
    method: string,
    extra: Record<string, unknown>,
  ): Promise<TsDefinition[]> {
    const text = await this.serve(fileName);
    if (text === null) return [];
    const answer = await this.client.ask(method, {
      textDocument: { uri: documentUri(fileName) },
      position: positionAt(text, offset),
      ...extra,
    });
    const raw: LspLocation[] = Array.isArray(answer)
      ? (answer as LspLocation[])
      : answer
        ? [answer as LspLocation]
        : [];
    const out: TsDefinition[] = [];
    for (const location of raw) {
      const target = fileNameOf(location.uri);
      const targetText = await this.textOf(target);
      if (targetText === null) continue;
      out.push({ fileName: target, fileText: targetText, ...spanOf(targetText, location.range) });
    }
    return out;
  }

  /** Completions at a position. */
  async completionsAt(fileName: string, offset: number): Promise<TsCompletionList> {
    const text = await this.serve(fileName);
    if (text === null) return { entries: [], member: false };
    const answer = (await this.client.ask("textDocument/completion", {
      textDocument: { uri: documentUri(fileName) },
      position: positionAt(text, offset),
    })) as { items?: unknown[] } | unknown[] | null;
    const items = (Array.isArray(answer) ? answer : (answer?.items ?? [])) as {
      label: string;
      kind?: number;
      sortText?: string;
      data?: unknown;
    }[];
    return {
      entries: items.map(
        (item): TsCompletion => ({
          name: item.label,
          kind: completionKind(item.kind),
          sortText: item.sortText ?? item.label,
          data: item.data,
        }),
      ),
      // A member completion is one the server answered for a `.` — the
      // caller uses this to tell a real member list from the global scope.
      member: isMemberContext(text, offset),
    };
  }

  /** Type errors, in the coordinates of the text the server was given. */
  async diagnosticsFor(fileName: string): Promise<TsDiagnostic[]> {
    const text = await this.serve(fileName);
    if (text === null) return [];
    // Pulled, not pushed: the server advertises a diagnosticProvider.
    const answer = (await this.client.ask("textDocument/diagnostic", {
      textDocument: { uri: documentUri(fileName) },
    })) as { items?: LspDiagnostic[] } | null;
    return (answer?.items ?? [])
      .filter((item) => (item.severity ?? 1) <= 2)
      .map((item) => ({
        ...spanOf(text, item.range),
        message: item.message,
        code: typeof item.code === "number" ? item.code : 0,
        warning: item.severity === 2,
      }));
  }

  /** The text of a file as the server sees it: the buffer if the editor has
   * one, otherwise what was served before. Files only on disk are the
   * server's own to read, and their text is read here for offset maths. */
  private async textOf(fileName: string): Promise<string | null> {
    const open = this.getOpenDoc(fileName);
    if (open) return open.text;
    const served = this.served.get(fileName);
    if (served !== undefined) return served;
    try {
      const { readFileSync } = await import("node:fs");
      const text = readFileSync(fileName, "utf8");
      this.served.set(fileName, text);
      return text;
    } catch {
      return null;
    }
  }
}

/** An LSP range as the offset span the rest of the server speaks in. */
function spanOf(text: string, range: LspRange): { start: number; length: number } {
  const start = offsetAt(text, range.start);
  return { start, length: Math.max(0, offsetAt(text, range.end) - start) };
}

/** Splits hover contents into the signature and the prose under it. */
function splitHover(contents: string): { signature: string; documentation: string } {
  const fenced = /^```[a-z]*\n([\s\S]*?)\n```\s*([\s\S]*)$/.exec(contents.trim());
  if (fenced) {
    return { signature: fenced[1].trim(), documentation: fenced[2].trim() };
  }
  const [first, ...rest] = contents.trim().split("\n\n");
  return { signature: first.trim(), documentation: rest.join("\n\n").trim() };
}

/** Whether the position follows a `.`, which is what makes an answer a
 * member list rather than everything in scope. */
function isMemberContext(text: string, offset: number): boolean {
  let i = Math.min(offset, text.length) - 1;
  while (i >= 0 && /[A-Za-z0-9_$]/.test(text[i])) i--;
  return i >= 0 && text[i] === ".";
}

/** LSP completion kinds, as the ScriptElementKind strings the server maps
 * to LSP kinds of its own. Only the ones rl's mapping distinguishes are
 * named; anything else is a plain property. */
function completionKind(kind: number | undefined): string {
  switch (kind) {
    case 3:
      return "function";
    case 2:
    case 4:
      return "method";
    case 5:
      return "property";
    case 6:
      return "var";
    case 7:
    case 22:
      return "class";
    case 8:
      return "interface";
    case 9:
      return "module";
    case 13:
      return "enum";
    case 14:
      return "keyword";
    case 21:
      return "const";
    case 25:
      return "type";
    default:
      return "property";
  }
}
