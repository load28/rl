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
import { dirname, resolve } from "node:path";
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
  TsCompletionDetail,
  TsCompletionList,
  TsDefinition,
  TsDiagnostic,
  TsQuickInfo,
  TsReference,
  TsSignatureHelp,
} from "./tstypes";

/** The document name a file takes in the server's world. An `.rl` file is
 * served as the TypeScript it lowers to, under the name that TypeScript
 * resolution would find for `"./x.rl"`. */
export function documentUri(fileName: string): string {
  return pathToFileURL(fileName.endsWith(".rl") ? `${fileName}.ts` : fileName).toString();
}

/** The inverse: which file an answer is about, or null when the answer is
 * not about a file at all. The compiler carries its standard library inside
 * the executable and names it `bundled:///libs/lib.es5.d.ts`; there is no
 * document behind such a URI for an editor to open. */
export function fileNameOf(uri: string): string | null {
  if (!uri.startsWith("file:")) return null;
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
  /** The raw items of the last completion answers, so one can be resolved
   * later: the server resolves the item it produced, not a name. */
  private readonly lastCompletion = new Map<string, unknown>();

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

  /**
   * Serves the current text of `fileName`, and returns it.
   *
   * The `.rl` modules it imports are served too, transitively. That is not
   * an optimization: the server resolves `"./x.rl"` to `x.rl.ts`, and that
   * name only exists as a document *this client serves* — an `.rl` module
   * nobody served is a module the server cannot find, and everything
   * flowing across the import becomes `any` behind a TS2307.
   */
  private async serve(fileName: string): Promise<string | null> {
    const text = await this.serveOne(fileName);
    if (text !== null) await this.serveImports(fileName, text, new Set([fileName]));
    return text;
  }

  private async serveOne(fileName: string): Promise<string | null> {
    const open = this.getOpenDoc(fileName);
    if (!open) return this.served.get(fileName) ?? null;
    if (this.served.get(fileName) !== open.text) {
      this.served.set(fileName, open.text);
      await this.client.open(documentUri(fileName), open.text);
    }
    return open.text;
  }

  private async serveImports(
    fileName: string,
    text: string,
    seen: Set<string>,
  ): Promise<void> {
    for (const specifier of rlImports(text)) {
      const target = resolve(dirname(fileName), specifier);
      if (seen.has(target)) continue;
      seen.add(target);
      const imported = await this.serveOne(target);
      if (imported !== null) await this.serveImports(target, imported, seen);
    }
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
      if (target === null) continue;
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
    this.lastCompletion.clear();
    for (const item of items) {
      this.lastCompletion.set(`${fileName}\u0000${offset}\u0000${item.label}`, item);
    }
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

  /**
   * The signature and documentation behind one completion entry, which the
   * server fills in only when asked — a list of a thousand entries carries
   * no types until one is looked at.
   */
  async completionDetail(
    fileName: string,
    offset: number,
    name: string,
  ): Promise<TsCompletionDetail | null> {
    const key = `${fileName}\u0000${offset}\u0000${name}`;
    // The server resolves the item *it* produced, not a name, so the list
    // has to have been asked for first. A caller that wants one entry's
    // type without having shown a list has not asked; ask for it here.
    if (!this.lastCompletion.has(key)) await this.completionsAt(fileName, offset);
    const item = this.lastCompletion.get(key);
    if (!item) return null;
    const resolved = (await this.client.ask("completionItem/resolve", item)) as {
      detail?: string;
      documentation?: string | { value?: string };
    } | null;
    if (!resolved) return null;
    return {
      signature: resolved.detail ?? "",
      documentation: textOfDocs(resolved.documentation),
    };
  }

  /**
   * Rename: every place the name is written, or `null` when the rename
   * cannot be done.
   *
   * The answer is a `WorkspaceEdit` in the coordinates of the *lowered*
   * text. The caller maps each span back to the `.rl` source, and **an edit
   * that does not map back must fail the whole rename**: it would land in
   * code rlc generated, which the user cannot edit, and applying the rest
   * would leave the program renamed by halves. That check belongs to the
   * caller, which owns the mapping; what is guaranteed here is that every
   * location is reported, so the caller can see the one it cannot map.
   */
  async renameAt(fileName: string, offset: number): Promise<TsReference[] | null> {
    const text = await this.serve(fileName);
    if (text === null) return null;
    const position = positionAt(text, offset);
    const uri = documentUri(fileName);

    // `prepareRename` is the server's own "can this be renamed?" — a
    // keyword or a literal answers null, and forcing it would rename
    // nothing while looking like it worked.
    const prepared = await this.client.ask("textDocument/prepareRename", {
      textDocument: { uri },
      position,
    });
    if (prepared === null) return null;

    const edit = (await this.client.ask("textDocument/rename", {
      textDocument: { uri },
      position,
      newName: "rlRenamePlaceholder",
    })) as { changes?: Record<string, { range: LspRange }[]> } | null;
    if (!edit?.changes) return null;

    const out: TsReference[] = [];
    for (const [editedUri, edits] of Object.entries(edit.changes)) {
      const target = fileNameOf(editedUri);
      // A file whose text cannot be read — or that is not a file at all —
      // cannot be renamed in safely.
      if (target === null) return null;
      const targetText = await this.textOf(target);
      if (targetText === null) return null;
      for (const one of edits) {
        out.push({
          fileName: target,
          fileText: targetText,
          isDefinition: false,
          ...spanOf(targetText, one.range),
        });
      }
    }
    return out.length > 0 ? out : null;
  }

  /** Signature help at a call site. */
  async signatureHelpAt(fileName: string, offset: number): Promise<TsSignatureHelp | null> {
    const text = await this.serve(fileName);
    if (text === null) return null;
    const help = (await this.client.ask("textDocument/signatureHelp", {
      textDocument: { uri: documentUri(fileName) },
      position: positionAt(text, offset),
    })) as {
      signatures?: {
        label: string;
        documentation?: string | { value?: string };
        parameters?: { label: string | [number, number]; documentation?: string }[];
      }[];
      activeSignature?: number;
      activeParameter?: number;
    } | null;
    if (!help?.signatures?.length) return null;
    return {
      signatures: help.signatures.map((signature) => ({
        label: signature.label,
        documentation: textOfDocs(signature.documentation),
        parameters: (signature.parameters ?? []).map((parameter) => ({
          // The LSP allows a label to be either the substring or the span
          // inside the signature; rl's presentation needs the span.
          label: Array.isArray(parameter.label)
            ? parameter.label
            : spanOfLabel(signature.label, parameter.label),
          documentation: textOfDocs(parameter.documentation),
        })),
      })),
      activeSignature: help.activeSignature ?? 0,
      activeParameter: help.activeParameter ?? 0,
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
    const items = answer?.items ?? [];
    // TypeScript numbers its parse errors 1000–1999 and its checker errors
    // from 2000 up. A text with a parse error is not TypeScript at all —
    // raw `.rl` source, or a buffer mid-edit — and the checker's recovery
    // invents errors all through it, so none of them is reported.
    if (items.some((item) => typeof item.code === "number" && item.code < 2000)) {
      return [];
    }
    return items
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

/** Documentation as plain text, whichever shape the server used. */
function textOfDocs(documentation: string | { value?: string } | undefined): string {
  if (!documentation) return "";
  return (typeof documentation === "string" ? documentation : (documentation.value ?? "")).trim();
}

/** Where a parameter's label sits inside its signature. */
function spanOfLabel(signature: string, label: string): [number, number] {
  const start = signature.indexOf(label);
  return start < 0 ? [0, 0] : [start, start + label.length];
}

/** The relative `.rl` specifiers a text imports. Static forms only — the
 * same shapes the compiler rewrites, and the only ones whose target is
 * known without evaluating anything. */
function rlImports(text: string): string[] {
  const out: string[] = [];
  const pattern = /(?:\bfrom|\bimport|\brequire\s*\()\s*["'](\.[^"']*\.rl)["']/g;
  for (const match of text.matchAll(pattern)) out.push(match[1]);
  return out;
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
