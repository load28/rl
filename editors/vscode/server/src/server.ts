/* --------------------------------------------------------------------------
 * rl language server — LSP over Node IPC (official VS Code LSP pattern).
 *
 * Diagnostics run the real compiler (`rlc --check`); completion, hover,
 * definitions, symbols and quick fixes come from the lightweight structural
 * analysis in analysis.ts.
 * ----------------------------------------------------------------------- */
import {
  CodeAction,
  CodeActionKind,
  CompletionItem,
  CompletionItemKind,
  createConnection,
  Diagnostic,
  DiagnosticSeverity,
  DocumentSymbol,
  InitializeParams,
  InitializeResult,
  InsertTextFormat,
  Location,
  MarkupKind,
  ParameterInformation,
  ProposedFeatures,
  Range,
  SignatureHelp,
  SignatureInformation,
  SymbolKind,
  TextDocuments,
  TextDocumentSyncKind,
  TextEdit,
} from "vscode-languageserver/node";
import { TextDocument } from "vscode-languageserver-textdocument";
import { URI } from "vscode-uri";

import * as analysis from "./analysis";
import * as rlc from "./rlc";
import * as fs from "node:fs";
import * as path from "node:path";

import * as probe from "./probe";
import * as sidecar from "./sidecar";
import { positionAt as positionOf } from "./lsp";
import type * as tsproject from "./tstypes";
import { TsgoProject } from "./tsgo";
import * as virtual from "./virtual";

const connection = createConnection(ProposedFeatures.all);
const documents = new TextDocuments(TextDocument);

let hasConfigurationCapability = false;
let workspaceRoots: string[] = [];
let warnedCompilerMissing = false;

connection.onInitialize((params: InitializeParams): InitializeResult => {
  hasConfigurationCapability = Boolean(
    params.capabilities.workspace?.configuration,
  );
  workspaceRoots = (params.workspaceFolders ?? [])
    .map((f) => URI.parse(f.uri))
    .filter((u) => u.scheme === "file")
    .map((u) => u.fsPath);

  return {
    capabilities: {
      textDocumentSync: {
        openClose: true,
        change: TextDocumentSyncKind.Incremental,
        // Saving is when the sidecars are rebuilt; the text is already on
        // disk by then, so the notification does not need to carry it.
        save: { includeText: false },
      },
      completionProvider: {
        triggerCharacters: [".", "(", "|"],
        // Signatures and documentation are fetched per entry, when the
        // editor asks for the one the user highlighted (onCompletionResolve).
        resolveProvider: true,
      },
      signatureHelpProvider: {
        triggerCharacters: ["(", ","],
        retriggerCharacters: [")"],
      },
      hoverProvider: true,
      definitionProvider: true,
      referencesProvider: true,
      renameProvider: true,
      documentSymbolProvider: true,
      codeActionProvider: { codeActionKinds: [CodeActionKind.QuickFix] },
    },
  };
});

connection.onInitialized(() => {
  // Nothing further; settings are pulled per-request when supported.
});

// ---------------------------------------------------------------- settings

interface RlSettings {
  compilerPath: string;
  verify: boolean;
  typeDiagnostics: boolean;
  typedChecks: boolean;
  sidecar: sidecar.SidecarMode;
  sidecarDir: string;
}

const DEFAULT_SETTINGS: RlSettings = {
  compilerPath: "",
  verify: true,
  typeDiagnostics: true,
  typedChecks: true,
  sidecar: "refresh",
  sidecarDir: "",
};

async function getSettings(uri: string): Promise<RlSettings> {
  if (!hasConfigurationCapability) return DEFAULT_SETTINGS;
  const conf = (await connection.workspace.getConfiguration({
    scopeUri: uri,
    section: "rl",
  })) as Partial<RlSettings> | null;
  return {
    compilerPath: conf?.compilerPath ?? DEFAULT_SETTINGS.compilerPath,
    verify: conf?.verify ?? DEFAULT_SETTINGS.verify,
    typeDiagnostics: conf?.typeDiagnostics ?? DEFAULT_SETTINGS.typeDiagnostics,
    typedChecks: conf?.typedChecks ?? DEFAULT_SETTINGS.typedChecks,
    sidecar: conf?.sidecar ?? DEFAULT_SETTINGS.sidecar,
    sidecarDir: conf?.sidecarDir ?? DEFAULT_SETTINGS.sidecarDir,
  };
}

connection.onDidChangeConfiguration(() => {
  warnedCompilerMissing = false;
  for (const doc of documents.all()) scheduleValidation(doc);
});

connection.onDidChangeWatchedFiles(() => {
  // A freshly built rlc appeared (or changed) — try validating again.
  warnedCompilerMissing = false;
  for (const doc of documents.all()) scheduleValidation(doc);
});

// ---------------------------------------------------------------- analysis

interface Analyzed {
  version: number;
  text: string;
  masked: string;
  enums: analysis.EnumInfo[];
  matches: analysis.MatchInfo[];
}

const analysisCache = new Map<string, Analyzed>();

function analyze(doc: TextDocument): Analyzed {
  const cached = analysisCache.get(doc.uri);
  if (cached && cached.version === doc.version) return cached;
  const text = doc.getText();
  const masked = analysis.maskNonCode(text);
  const result: Analyzed = {
    version: doc.version,
    text,
    masked,
    enums: analysis.parseEnums(text, masked),
    matches: analysis.parseMatches(masked),
  };
  analysisCache.set(doc.uri, result);
  return result;
}

// ----------------------------------------- virtual documents (TASK-050)

/* The compiler's emitted TypeScript for each open .rl buffer, refreshed on
 * the validation cadence (`rlc --emit-map`). When current, it is what the
 * TypeScript language service sees instead of the raw .rl text — the
 * emitted code is plain TypeScript, so inference works inside match arms,
 * scrutinees and the other rl constructs. Offsets are translated through
 * the emit mappings in both directions; while the entry lags the buffer
 * (or the compiler is missing) the raw text is served and offsets pass
 * through unchanged — the pre-TASK-050 error-recovery behavior. */

interface VirtualEntry {
  version: number;
  doc: virtual.MappedDoc;
}

const virtualDocs = new Map<string, VirtualEntry>();

/** The buffer's mapped virtual document, when it matches `version`. */
function activeVirtual(
  fsPath: string,
  version: number,
): virtual.MappedDoc | null {
  const entry = virtualDocs.get(fsPath);
  return entry && entry.version === version ? entry.doc : null;
}

/* An imported `.rl` module the editor does not have open is served the same
 * way (TASK-055) — compiled synchronously on first use and cached until its
 * mtime changes. Serving the raw source instead would hand TypeScript a file
 * it can only error-recover through: an rl `enum` reads as a misparsed TS
 * `enum`, so every value flowing across the import loses its type. */

interface DiskEntry {
  /** Compiler path and mtime the entry was compiled from — a failed compile
   * is cached too, so keying on the compiler lets a session that only later
   * resolves one (settings arriving after the first request) try again. */
  key: string;
  /** null: the compiler could not produce an emit map for it. */
  doc: virtual.MappedDoc | null;
}

const diskVirtuals = new Map<string, DiskEntry>();

/** The compiler path last resolved from settings — the synchronous paths
 * (disk modules, the std module) have no place to await settings. */
let servedCompiler: string | null = null;

function currentCompiler(): string {
  return servedCompiler ?? rlc.findCompiler("", workspaceRoots);
}

/** The emitted document of an `.rl` file on disk, or null when it is not a
 * compilable `.rl` file (the caller then serves the disk text as-is). */
function diskVirtualEntry(fsPath: string): DiskEntry | null {
  if (!fsPath.endsWith(".rl")) return null;
  const compiler = currentCompiler();
  let key: string;
  try {
    key = `${compiler}@${fs.statSync(fsPath).mtimeMs}`;
  } catch {
    diskVirtuals.delete(fsPath);
    return null;
  }
  const cached = diskVirtuals.get(fsPath);
  if (cached && cached.key === key) return cached;

  let doc: virtual.MappedDoc | null = null;
  const result = rlc.runEmitMapFileSync(compiler, fsPath);
  if (result) {
    try {
      doc = new virtual.MappedDoc(
        fs.readFileSync(fsPath, "utf8"),
        result.code,
        result.mappings,
      );
    } catch {
      doc = null;
    }
  }
  const entry: DiskEntry = { key, doc };
  diskVirtuals.set(fsPath, entry);
  return entry;
}

function diskVirtual(fsPath: string): virtual.MappedDoc | null {
  return diskVirtualEntry(fsPath)?.doc ?? null;
}

/** Bring the buffer's virtual document up to date and wait for it. The
 * validation cadence refreshes it on a debounce; a request that arrives
 * inside that window would otherwise be answered off the raw text, where
 * TypeScript cannot parse a pipeline or a match arm at all and answers
 * `any`. Concurrent requests for the same version share one compiler run. */
const pendingVirtual = new Map<string, Promise<void>>();

async function ensureVirtual(doc: TextDocument): Promise<void> {
  const uri = URI.parse(doc.uri);
  if (uri.scheme !== "file") return;
  if (activeVirtual(uri.fsPath, doc.version)) return;

  const key = `${doc.uri}@${doc.version}`;
  const existing = pendingVirtual.get(key);
  if (existing) {
    await existing;
    return;
  }
  const run = (async () => {
    const settings = await getSettings(doc.uri);
    const compiler = rlc.findCompiler(settings.compilerPath, workspaceRoots);
    servedCompiler = compiler;
    await refreshVirtual(doc, compiler);
  })().finally(() => pendingVirtual.delete(key));
  pendingVirtual.set(key, run);
  await run;
}

async function refreshVirtual(
  doc: TextDocument,
  compiler: string,
): Promise<void> {
  const uri = URI.parse(doc.uri);
  if (uri.scheme !== "file") return;
  const version = doc.version;
  const text = doc.getText();
  const result = await rlc.runEmitMap(compiler, text, uri.fsPath);
  const current = documents.get(doc.uri);
  if (!current || current.version !== version) return; // stale
  if (result) {
    virtualDocs.set(uri.fsPath, {
      version,
      doc: new virtual.MappedDoc(text, result.code, result.mappings),
    });
  } else {
    virtualDocs.delete(uri.fsPath);
  }
}

/** The open document for a filesystem path, if any. */
function openDocByPath(fsPath: string): TextDocument | null {
  for (const doc of documents.all()) {
    const uri = URI.parse(doc.uri);
    if (uri.scheme === "file" && uri.fsPath === fsPath) return doc;
  }
  return null;
}

/** A source offset translated into the text the service currently sees
 * for this buffer — identity while the raw text is served, mapped when
 * the virtual document is. Null: the position is compiler glue. */
function toServiceOffset(
  fsPath: string,
  doc: TextDocument,
  offset: number,
): number | null {
  const mapped = activeVirtual(fsPath, doc.version);
  return mapped ? mapped.srcToOut(offset) : offset;
}

/** A service-space span start translated back to source coordinates, with
 * the text LSP positions should be computed against. Null: the span has no
 * source counterpart (compiler glue). */
function fromServiceOffset(
  fileName: string,
  fileText: string,
  offset: number,
): { text: string; offset: number } | null {
  const doc = openDocByPath(fileName);
  if (!doc) {
    const disk = diskVirtual(fileName);
    if (!disk) return { text: fileText, offset }; // served raw
    const src = disk.outToSrc(offset);
    return src === null ? null : { text: disk.srcText, offset: src };
  }
  const mapped = activeVirtual(fileName, doc.version);
  if (!mapped) return { text: doc.getText(), offset };
  const src = mapped.outToSrc(offset);
  return src === null ? null : { text: doc.getText(), offset: src };
}

/** [`fromServiceOffset`] over a span; null when either end is unmapped. */
function fromServiceSpan(
  fileName: string,
  fileText: string,
  start: number,
  length: number,
): { text: string; start: number; end: number } | null {
  const s = fromServiceOffset(fileName, fileText, start);
  const e = fromServiceOffset(fileName, fileText, start + length);
  if (!s || !e || e.offset < s.offset) return null;
  return { text: s.text, start: s.offset, end: e.offset };
}

// -------------------------------------------- TypeScript language service

/* An .rl file is TypeScript plus six constructs, so symbols the rl
 * analysis does not own — variables, functions, types, imported values —
 * are answered by the real TypeScript language service (tsproject.ts) over
 * the virtual documents above (or the raw sources while one lags). Used as
 * the fallback for definitions and hover, and — over a virtual document
 * only — for the type diagnostics `validate` merges into rlc's own. */

let tsProjectInstance: TsgoProject | null = null;

function getTsProject(): TsgoProject {
  if (tsProjectInstance === null) {
    // The language server reads modules from disk, so the standard library
    // has to be somewhere it can find — see `ensureStdModule`.
    const root = workspaceRoots[0];
    if (root) rlc.ensureStdModule(currentCompiler(), root);
  }
  tsProjectInstance ??= new TsgoProject(
    rlc.findTsgo(workspaceRoots),
    workspaceRoots[0] ?? process.cwd(),
    (fileName) => {
      // A probe stands in for the buffer while one is installed — the
      // completion path below serves the emitted TypeScript of a source
      // the user has not finished typing (see `probeFor`).
      if (installedProbe && installedProbe.fsPath === fileName) {
        return { text: installedProbe.code, version: installedProbe.version };
      }
      for (const doc of documents.all()) {
        const uri = URI.parse(doc.uri);
        if (uri.scheme === "file" && uri.fsPath === fileName) {
          const mapped = activeVirtual(fileName, doc.version);
          if (mapped) {
            return { text: mapped.code, version: `${doc.version}:emitted` };
          }
          return { text: doc.getText(), version: `${doc.version}:raw` };
        }
      }
      const disk = diskVirtualEntry(fileName);
      if (disk?.doc) {
        return { text: disk.doc.code, version: `disk-${disk.key}:emitted` };
      }
      return null;
    },
    (message) => connection.console.warn(message),
  );
  return tsProjectInstance;
}

/* ------------------------------------------------------------- probes --
 *
 * Completion at a trailing `.` needs a type for what precedes it, and inside
 * an rl construct those types exist only over the emitted TypeScript — a
 * `|>` pipeline or a `match` arm binding is nothing the TS parser can make
 * sense of in the raw source. But at that exact moment the buffer is not a
 * member access the compiler can emit either: `x |> .` has no member yet, so
 * the file passes through unchanged, the service is handed raw rl text, and
 * it offers nothing at all. That is the whole of the reported symptom — and
 * since the editor caches the list it got when `.` was typed, the members do
 * not appear later either.
 *
 * probe.ts builds the mended source (`x |> .$rl_probe`) and locates the
 * placeholder in the emitted output; here it is compiled and installed —
 * around the synchronous language-service call that reads it, and never any
 * longer, so no diagnostic can be computed from text the user did not write.
 * ---------------------------------------------------------------------- */

interface InstalledProbe extends probe.Probe {
  fsPath: string;
  /** Snapshot version, unique per probe so the service re-reads the text. */
  version: string;
}

/** Non-null only inside [`withProbe`]. */
let installedProbe: InstalledProbe | null = null;
let probeCount = 0;

/** The probe the last completion list was answered from, kept so resolving
 * one of its items can install it again — an item's detail has to come from
 * the same text and position the item was listed from. */
let lastProbe: InstalledProbe | null = null;

/**
 * A probe for the cursor, or null when there is nothing to mend: the cursor
 * is not at a member access without a member, or the compiler cannot emit
 * from the probe source either.
 *
 * Note what does *not* gate this: whether a virtual document exists. An
 * unfinished pipeline is not an emit error — `--emit-map` passes the file
 * through unchanged — so the buffer does have a virtual document, one whose
 * text is the raw source. That is exactly the case the probe is for.
 */
async function probeFor(
  doc: TextDocument,
  offset: number,
): Promise<InstalledProbe | null> {
  const uri = URI.parse(doc.uri);
  if (uri.scheme !== "file") return null;

  const version = doc.version;
  const built = await probe.buildProbe(doc.getText(), offset, (source) =>
    // A doc name of its own: `runEmitMap` derives the temp file it compiles
    // from this, and sharing it with the buffer's own runs would let the
    // two overwrite each other.
    rlc.runEmitMap(currentCompiler(), source, `${uri.fsPath}.probe`),
  );
  if (!built) return null;
  // The buffer may have moved on while rlc ran; its offsets rule.
  const current = documents.get(doc.uri);
  if (!current || current.version !== version) return null;

  return { ...built, fsPath: uri.fsPath, version: `probe-${++probeCount}` };
}

/** Runs `query` with `p` standing in for its buffer. Synchronous by
 * construction — nothing but `query` may see the probe's text. */
/** Queries run one at a time: a probe is installed for the duration of one,
 * and the buffer it stands in for must not change under another. */
let queries: Promise<unknown> = Promise.resolve();

function serialize<T>(query: () => Promise<T>): Promise<T> {
  const next = queries.then(query, query);
  queries = next.catch(() => undefined);
  return next;
}

function withProbe<T>(p: InstalledProbe, query: () => Promise<T>): Promise<T> {
  return serialize(async () => {
    installedProbe = p;
    try {
      return await query();
    } finally {
      installedProbe = null;
    }
  });
}

async function tsDefinitions(
  doc: TextDocument,
  offset: number,
): Promise<Location[] | null> {
  const uri = URI.parse(doc.uri);
  if (uri.scheme !== "file") return null;
  const at = toServiceOffset(uri.fsPath, doc, offset);
  if (at === null) return null;
  const definitions = await getTsProject().definitionsAt(uri.fsPath, at);
  const locations: Location[] = [];
  for (const d of definitions) {
    const span = fromServiceSpan(d.fileName, d.fileText, d.start, d.length);
    if (!span) continue;
    locations.push(
      Location.create(URI.file(d.fileName).toString(), {
        start: positionOf(span.text, span.start),
        end: positionOf(span.text, span.end),
      }),
    );
  }
  return locations.length > 0 ? locations : null;
}

/** TypeScript ScriptElementKind strings → LSP completion kinds. */
const TS_COMPLETION_KINDS: Record<string, CompletionItemKind> = {
  var: CompletionItemKind.Variable,
  let: CompletionItemKind.Variable,
  const: CompletionItemKind.Variable,
  "local var": CompletionItemKind.Variable,
  parameter: CompletionItemKind.Variable,
  alias: CompletionItemKind.Reference,
  function: CompletionItemKind.Function,
  "local function": CompletionItemKind.Function,
  method: CompletionItemKind.Method,
  property: CompletionItemKind.Property,
  getter: CompletionItemKind.Property,
  setter: CompletionItemKind.Property,
  class: CompletionItemKind.Class,
  interface: CompletionItemKind.Interface,
  type: CompletionItemKind.TypeParameter,
  enum: CompletionItemKind.Enum,
  "enum member": CompletionItemKind.EnumMember,
  module: CompletionItemKind.Module,
  keyword: CompletionItemKind.Keyword,
  string: CompletionItemKind.Constant,
};

/** What a TS-delegated completion item carries so its signature and
 * documentation can be fetched when the editor asks for that one entry
 * (`completionItem/resolve`). Positions are in source coordinates and
 * re-mapped at resolve time — the buffer may have moved on since. */
interface TsCompletionData {
  uri: string;
  offset: number;
  name: string;
  source?: string;
  entry?: unknown;
  /** Version of the probe the entry was listed from, when it came from one
   * — the detail must be fetched against that same text. */
  probe?: string;
}

/** TS completions, sorted after the rl-specific items (`2` prefix; rl
 * items use `0`/`1`). Signature and docs are left to the resolve step:
 * TypeScript computes them per entry, so asking eagerly would type-check
 * every member of the type on every keystroke. */
async function tsCompletions(
  doc: TextDocument,
  offset: number,
  activeProbe: InstalledProbe | null = null,
): Promise<{ items: CompletionItem[]; member: boolean }> {
  const uri = URI.parse(doc.uri);
  if (uri.scheme !== "file") return { items: [], member: false };

  let list: tsproject.TsCompletionList;
  if (activeProbe) {
    list = await withProbe(activeProbe, () =>
      getTsProject().completionsAt(activeProbe.fsPath, activeProbe.offset),
    );
    lastProbe = activeProbe;
  } else {
    const at = toServiceOffset(uri.fsPath, doc, offset);
    if (at === null) return { items: [], member: false };
    list = await getTsProject().completionsAt(uri.fsPath, at);
  }

  return {
    member: list.member,
    items: list.entries.map((entry) => ({
      label: entry.name,
      kind: TS_COMPLETION_KINDS[entry.kind] ?? CompletionItemKind.Text,
      sortText: `2${entry.sortText}`,
      data: {
        uri: doc.uri,
        offset,
        name: entry.name,
        source: entry.source,
        entry: entry.data,
        probe: activeProbe?.version,
      } satisfies TsCompletionData,
    })),
  };
}

/**
 * TS completions, with a probe behind them when the cursor is at a member
 * access (`atMember`) that the ordinary path cannot answer.
 *
 * "Cannot answer" is not just an empty list. Recovering from rl syntax,
 * TypeScript may lose the dot and answer with the *global scope* instead —
 * `x |> n.` offers every name in scope, the compiler's own `$rl_ap` helper
 * among them. At a member access only a member answer means anything, so a
 * non-member one is discarded rather than shown, whether it came from the
 * buffer or from the probe.
 */
async function tsCompletionsProbed(
  doc: TextDocument,
  offset: number,
  atMember: boolean,
): Promise<CompletionItem[]> {
  const plain = await tsCompletions(doc, offset);
  if (!atMember) return plain.items;
  if (plain.member && plain.items.length > 0) return plain.items;

  const built = await probeFor(doc, offset);
  if (!built) return plain.member ? plain.items : [];
  const probed = await tsCompletions(doc, offset, built);
  return probed.member ? probed.items : [];
}

async function tsHover(doc: TextDocument, offset: number) {
  const uri = URI.parse(doc.uri);
  if (uri.scheme !== "file") return null;
  const at = toServiceOffset(uri.fsPath, doc, offset);
  if (at === null) return null;
  const info = await getTsProject().quickInfoAt(uri.fsPath, at);
  if (!info || info.signature === "") return null;
  const mapped = activeVirtual(uri.fsPath, doc.version);
  let start = info.start;
  let end = info.start + info.length;
  if (mapped) {
    const s = mapped.outToSrc(start);
    const e = mapped.outToSrc(end);
    if (s === null || e === null || e < s) return null;
    start = s;
    end = e;
  }
  const value =
    "```ts\n" +
    info.signature +
    "\n```" +
    (info.documentation ? `\n${info.documentation}` : "");
  return {
    contents: { kind: MarkupKind.Markdown, value },
    range: {
      start: doc.positionAt(start),
      end: doc.positionAt(end),
    },
  };
}

// ------------------------------------------------- imported declarations

/* Cross-file declarations come from the compiler (`rlc --symbols`,
 * cli.md): the file's direct `.rl` imports and the referenced files'
 * exported enums, with positions. Only named imports (aliases applied)
 * are merged — mirroring the compiler's collection — and the compiler is
 * run on the saved file, so a not-yet-saved edit to the import lines can
 * lag one save behind. */

const importedCache = new Map<
  string,
  { version: number; enums: analysis.EnumInfo[] }
>();

async function importedEnums(doc: TextDocument): Promise<analysis.EnumInfo[]> {
  const cached = importedCache.get(doc.uri);
  if (cached && cached.version === doc.version) return cached.enums;
  let enums: analysis.EnumInfo[] = [];
  const uri = URI.parse(doc.uri);
  if (uri.scheme === "file") {
    const settings = await getSettings(doc.uri);
    const compiler = rlc.findCompiler(settings.compilerPath, workspaceRoots);
    const symbols = await rlc.runSymbols(compiler, uri.fsPath);
    if (symbols) enums = toImportedEnumInfos(symbols);
  }
  importedCache.set(doc.uri, { version: doc.version, enums });
  return enums;
}

/** Convert the compiler's symbol report into EnumInfo entries for merging
 * (named imports only; aliases applied; offsets are -1 — positions live in
 * the declaring file via `imported`). */
function toImportedEnumInfos(symbols: rlc.SymbolsFile): analysis.EnumInfo[] {
  const out: analysis.EnumInfo[] = [];
  for (const imp of symbols.imports) {
    if (imp.resolved === null || imp.names.kind !== "named") continue;
    for (const entry of imp.names.entries) {
      const e = imp.enums.find((x) => x.name === entry.name);
      if (!e) continue;
      const casePositions: Record<string, { line: number; col: number }> = {};
      for (const c of e.cases) {
        casePositions[c.tag] = { line: c.line, col: c.col };
      }
      out.push({
        name: entry.alias ?? entry.name,
        nameStart: -1,
        nameEnd: -1,
        generics: e.generics,
        exported: true,
        builtin: false,
        start: -1,
        end: -1,
        cases: e.cases.map((c) => ({
          tag: c.tag,
          tagStart: -1,
          tagEnd: -1,
          fields: (c.fields ?? []).map((f) => ({
            name: f.name,
            optional: f.optional,
            type: f.type,
          })),
          hasParens: c.fields !== null,
        })),
        imported: {
          path: imp.resolved,
          specifier: imp.specifier,
          name: e.name,
          line: e.line,
          col: e.col,
          cases: casePositions,
        },
      });
    }
  }
  return out;
}

// ------------------------------------------------------------- diagnostics

const pendingValidation = new Map<string, NodeJS.Timeout>();
const VALIDATION_DELAY_MS = 300;

/* ------------------------------------------------------------------------
 * The typed rl layer.
 *
 * `rlc --check` answers from the text; what needs a *type* to decide — a
 * mutation through a `val` binding, exhaustiveness over the type a scrutinee
 * actually has — is `rlc --check-types`' answer, and getting it means opening
 * the project and running the TypeScript compiler. That is far too slow to
 * ride the keystroke debounce, so it runs on its own longer one and publishes
 * again when it lands.
 *
 * Both layers are cached per document *version*. A typed answer computed for
 * an older version is not shown: its positions describe text that is no
 * longer there, and a `val` error pointing at the wrong token is worse than
 * one that arrives a moment later.
 * --------------------------------------------------------------------- */
const pendingTypedCheck = new Map<string, NodeJS.Timeout>();
const TYPED_CHECK_DELAY_MS = 1200;

interface VersionedDiagnostics {
  version: number;
  diagnostics: Diagnostic[];
}

/** What `--check` and the TypeScript layer found, as last published. */
const baseDiagnostics = new Map<string, VersionedDiagnostics>();
/** What `--check-types --rl-only` found, for the version it ran on. */
const typedDiagnostics = new Map<string, VersionedDiagnostics>();
let warnedTypedCheckUnavailable = false;

/**
 * Adds the typed diagnostics that say something new.
 *
 * The two passes overlap: enum exhaustiveness is decided from the text by
 * `--check` and from the type by `--check-types`, and both report it at the
 * same place. One squiggle per position, and the pass that already ran wins
 * — its message is the one the user has been reading.
 */
function mergeTyped(into: Diagnostic[], typed: Diagnostic[]): void {
  const taken = new Set(
    into.map((d) => `${d.range.start.line}:${d.range.start.character}`),
  );
  for (const d of typed) {
    const key = `${d.range.start.line}:${d.range.start.character}`;
    if (taken.has(key)) continue;
    taken.add(key);
    into.push(d);
  }
}

/** Publishes the base layer plus the typed layer, when the latter was
 * computed for this very version. */
function publish(uri: string, version: number, base: Diagnostic[]): void {
  const diagnostics = [...base];
  const typed = typedDiagnostics.get(uri);
  if (typed?.version === version) mergeTyped(diagnostics, typed.diagnostics);
  void connection.sendDiagnostics({ uri, diagnostics });
}

function scheduleTypedCheck(doc: TextDocument, compiler: string): void {
  const existing = pendingTypedCheck.get(doc.uri);
  if (existing !== undefined) clearTimeout(existing);
  pendingTypedCheck.set(
    doc.uri,
    setTimeout(() => {
      pendingTypedCheck.delete(doc.uri);
      void typedCheck(doc, compiler);
    }, TYPED_CHECK_DELAY_MS),
  );
}

async function typedCheck(doc: TextDocument, compiler: string): Promise<void> {
  const uri = URI.parse(doc.uri);
  if (uri.scheme !== "file") return;
  const result = await rlc.runTypedCheck(compiler, doc.getText(), uri.fsPath);

  // The buffer may have moved on while the compiler ran.
  const fresh = documents.get(doc.uri);
  if (!fresh || fresh.version !== doc.version) return;

  if (result.kind === "unavailable") {
    // A project with no TypeScript toolchain is a normal state, not an
    // error to put in front of the user: the text-level diagnostics keep
    // working and only the typed layer is missing.
    if (!warnedTypedCheckUnavailable) {
      warnedTypedCheckUnavailable = true;
      connection.console.info(
        `rl: typed checks unavailable (${result.detail}). ` +
          "`val` mutations and typed exhaustiveness are reported by " +
          "`rlc --check-types`, which needs a TypeScript install — set " +
          "rl.typedChecks to false to stop trying.",
      );
    }
    return;
  }

  typedDiagnostics.set(doc.uri, {
    version: doc.version,
    diagnostics: result.diagnostics.map((d) => toDiagnostic(fresh, d)),
  });
  const base = baseDiagnostics.get(doc.uri);
  if (base?.version === doc.version) publish(doc.uri, doc.version, base.diagnostics);
}

function scheduleValidation(doc: TextDocument): void {
  const existing = pendingValidation.get(doc.uri);
  if (existing !== undefined) clearTimeout(existing);
  pendingValidation.set(
    doc.uri,
    setTimeout(() => {
      pendingValidation.delete(doc.uri);
      void validate(doc);
    }, VALIDATION_DELAY_MS),
  );
}

async function validate(doc: TextDocument): Promise<void> {
  const settings = await getSettings(doc.uri);
  const compiler = rlc.findCompiler(settings.compilerPath, workspaceRoots);
  const uri = URI.parse(doc.uri);
  const docName = uri.scheme === "file" ? uri.fsPath : uri.path;

  // The virtual document rides the same debounce; it degrades to raw-text
  // serving on its own when the compiler is missing. Kept as a promise:
  // the type diagnostics below need the emitted text, and awaiting the run
  // already in flight beats starting a second one.
  servedCompiler = compiler;
  const virtualReady = refreshVirtual(doc, compiler);

  const result = await rlc.runCheck(
    compiler,
    doc.getText(),
    docName,
    settings.verify,
  );

  // The buffer may have changed while rlc ran; drop stale results.
  const current = documents.get(doc.uri);
  if (!current || current.version !== doc.version) return;

  if (result.kind === "not-found") {
    if (!warnedCompilerMissing) {
      warnedCompilerMissing = true;
      connection.console.warn(
        `rl: compiler not found (${result.compiler}). ` +
          "Set rl.compilerPath, build target/{debug,release}/rlc, or put rlc on PATH — " +
          "diagnostics are disabled until then.",
      );
      void connection.window.showWarningMessage(
        "rl: rlc compiler not found — diagnostics are disabled. " +
          "Set `rl.compilerPath` or install rlc (cargo install --path .).",
      );
    }
    forget(doc.uri);
    void connection.sendDiagnostics({ uri: doc.uri, diagnostics: [] });
    return;
  }
  if (result.kind === "failed") {
    connection.console.error(`rl: ${result.detail}`);
    forget(doc.uri);
    void connection.sendDiagnostics({ uri: doc.uri, diagnostics: [] });
    return;
  }

  const diagnostics = result.diagnostics.map((d) => toDiagnostic(current, d));

  if (settings.typeDiagnostics) {
    await virtualReady;
    // Awaiting gave the buffer another chance to move on.
    const fresh = documents.get(doc.uri);
    if (!fresh || fresh.version !== doc.version) return;
    diagnostics.push(...(await typeDiagnostics(fresh, docName)));
  }

  baseDiagnostics.set(doc.uri, { version: doc.version, diagnostics });
  publish(doc.uri, doc.version, diagnostics);

  // The typed layer trails on its own debounce; it republishes when it lands.
  if (settings.typedChecks) scheduleTypedCheck(doc, compiler);
}

/** Drops every cached diagnostic layer for a document. */
function forget(uri: string): void {
  baseDiagnostics.delete(uri);
  typedDiagnostics.delete(uri);
  const pending = pendingTypedCheck.get(uri);
  if (pending !== undefined) {
    clearTimeout(pending);
    pendingTypedCheck.delete(uri);
  }
}

/**
 * TypeScript's type errors for a buffer, moved onto the `.rl` source.
 *
 * Only over a virtual document: the service must be seeing the emitted
 * TypeScript, because rl syntax in the raw text is not TypeScript and every
 * error TS recovers from there would be a lie. Spans that map to compiler
 * glue are dropped for the same reason — rlc's emissions are the compiler's
 * responsibility, never something to report at the user (CLAUDE.md, error
 * layers).
 */
async function typeDiagnostics(
  doc: TextDocument,
  fsPath: string,
): Promise<Diagnostic[]> {
  const mapped = activeVirtual(fsPath, doc.version);
  if (!mapped) return [];

  const text = doc.getText();
  const out: Diagnostic[] = [];
  for (const d of await getTsProject().diagnosticsFor(fsPath)) {
    const span = fromServiceSpan(fsPath, mapped.code, d.start, d.length);
    if (!span || span.text !== text) continue;
    // An empty span (an error at a position, not over one) would render as
    // an invisible squiggle; give it the character it points at.
    const end = span.end > span.start ? span.end : span.start + 1;
    out.push({
      severity: d.warning
        ? DiagnosticSeverity.Warning
        : DiagnosticSeverity.Error,
      range: { start: doc.positionAt(span.start), end: doc.positionAt(end) },
      message: d.message,
      code: d.code,
      source: "ts",
    });
  }
  return out;
}

function toDiagnostic(doc: TextDocument, d: rlc.RlcDiagnostic): Diagnostic {
  let range: Range;
  if (d.line > 0) {
    const start = { line: d.line - 1, character: Math.max(0, d.col - 1) };
    const offset = doc.offsetAt(start);
    const word = analysis.wordAt(doc.getText(), offset);
    const end =
      word && word.start === offset
        ? doc.positionAt(word.end)
        : { line: start.line, character: start.character + 1 };
    range = { start, end };
  } else {
    // Positionless (output-verification) errors: flag the first line.
    range = {
      start: { line: 0, character: 0 },
      end: { line: 1, character: 0 },
    };
  }
  return {
    severity: DiagnosticSeverity.Error,
    range,
    message: d.message,
    source: "rlc",
  };
}

documents.onDidOpen((e) => scheduleValidation(e.document));
documents.onDidSave((e) => {
  void rebuildSidecar(e.document);
});

/**
 * Where this file's declarations belong: next to the source when
 * `rl.sidecarDir` is empty, otherwise that directory under the workspace
 * root the file lives in (TypeScript merges the two trees with `rootDirs`).
 */
function resolveSidecarDir(configured: string, filePath: string): string | undefined {
  const dir = configured.trim();
  if (dir === "") return undefined;
  if (path.isAbsolute(dir)) return dir;

  const root = workspaceRoots
    .filter((candidate) => filePath.startsWith(`${candidate}${path.sep}`))
    .sort((a, b) => b.length - a.length)[0];
  return root === undefined ? undefined : path.join(root, dir);
}

/**
 * Keeps a saved `.rl` file's editor sidecar (`x.rl.d.ts` + map) current, so
 * `.ts` files importing it type-check and jump into the original on "go to
 * definition" without a build step.
 */
async function rebuildSidecar(doc: TextDocument): Promise<void> {
  const uri = URI.parse(doc.uri);
  if (uri.scheme !== "file") return;

  const settings = await getSettings(doc.uri);
  if (settings.sidecar === "off") return;

  const compiler = rlc.findCompiler(settings.compilerPath, workspaceRoots);
  const result = await sidecar.refreshSidecar(
    compiler,
    uri.fsPath,
    settings.sidecar,
    resolveSidecarDir(settings.sidecarDir, uri.fsPath),
  );
  if (result.kind === "failed") {
    connection.console.warn(`rl: sidecar refresh failed — ${result.detail}`);
  }
}
documents.onDidChangeContent((e) => {
  // Editing one file can change what its siblings import — drop their
  // cached imported declarations (the edited doc's own entry refreshes by
  // version).
  for (const uri of importedCache.keys()) {
    if (uri !== e.document.uri) importedCache.delete(uri);
  }
  scheduleValidation(e.document);
});
documents.onDidClose((e) => {
  analysisCache.delete(e.document.uri);
  importedCache.delete(e.document.uri);
  forget(e.document.uri);
  const uri = URI.parse(e.document.uri);
  if (uri.scheme === "file") virtualDocs.delete(uri.fsPath);
  const pending = pendingValidation.get(e.document.uri);
  if (pending !== undefined) {
    clearTimeout(pending);
    pendingValidation.delete(e.document.uri);
  }
  void connection.sendDiagnostics({ uri: e.document.uri, diagnostics: [] });
});

// -------------------------------------------------------------- completion

const KEYWORD_SNIPPETS: CompletionItem[] = [
  {
    label: "enum",
    kind: CompletionItemKind.Snippet,
    detail: "rl enum declaration",
    documentation: {
      kind: MarkupKind.Markdown,
      value:
        "rl 태그드 유니언 선언. 케이스에 페이로드 괄호가 있거나 제네릭이 있어야 rl enum입니다.",
    },
    insertTextFormat: InsertTextFormat.Snippet,
    insertText: "enum ${1:Name} {\n\t${2:Case}(${3:field}: ${4:number}),\n\t${5:Unit},\n}",
  },
  {
    label: "match",
    kind: CompletionItemKind.Snippet,
    detail: "rl match expression",
    documentation: {
      kind: MarkupKind.Markdown,
      value:
        "`kind` 태그로 분기하는 match 표현식. `_` 없는 match는 같은 파일·import한 rl enum에 대해 소진성이 검사됩니다.",
    },
    insertTextFormat: InsertTextFormat.Snippet,
    insertText: "match (${1:value}) {\n\t$0\n}",
  },
  {
    label: "try",
    kind: CompletionItemKind.Snippet,
    detail: "rl try statement (Result propagation)",
    documentation: {
      kind: MarkupKind.Markdown,
      value:
        "Rust의 `?`에 해당. `Err`면 둘러싼 함수에서 즉시 return합니다. 세미콜론 필수.",
    },
    insertTextFormat: InsertTextFormat.Snippet,
    insertText: "try ${1:expression};",
  },
  {
    label: "flow",
    kind: CompletionItemKind.Snippet,
    detail: "rl flow composition (point-free pipeline)",
    documentation: {
      kind: MarkupKind.Markdown,
      value:
        "값 대신 함수를 합성해 새 함수를 만듭니다. 첫 스텝이 입력 타입을 정하며, 메서드 스텝이 될 수 없습니다.",
    },
    insertTextFormat: InsertTextFormat.Snippet,
    insertText: "flow |> ${1:first} |> ${0:next}",
  },
  {
    label: "result",
    kind: CompletionItemKind.Snippet,
    detail: "rl result computation block",
    documentation: {
      kind: MarkupKind.Markdown,
      value:
        "`Result` 연산을 평탄하게 잇습니다. `const x <- 식;`은 `Ok` 값을 묶고 `Err`를 블록 밖으로 전파하며, 마지막 값 식(세미콜론 없이)이 `Ok`로 감싸집니다.",
    },
    insertTextFormat: InsertTextFormat.Snippet,
    insertText: "result {\n\tconst ${1:value} <- ${2:expression};\n\t$0\n}",
  },
  {
    label: "let-else",
    kind: CompletionItemKind.Snippet,
    detail: "rl let-else statement",
    documentation: {
      kind: MarkupKind.Markdown,
      value:
        "패턴이 일치하면 필드를 바인딩하고, 아니면 발산하는 else 블록을 실행합니다. 괄호와 세미콜론 필수.",
    },
    insertTextFormat: InsertTextFormat.Snippet,
    insertText:
      "const ${1:Some}(${2:value}) = ${3:expression} else {\n\t${4:return;}\n};",
  },
];

connection.onCompletion(async (params): Promise<CompletionItem[]> => {
  const doc = documents.get(params.textDocument.uri);
  if (!doc) return [];
  await ensureVirtual(doc);
  const { masked, enums, matches } = analyze(doc);
  const offset = doc.offsetAt(params.position);
  const visible = analysis.visibleEnums(enums, await importedEnums(doc));

  // `Enum.` member access → the enum's case constructors, then everything
  // else TypeScript offers on that same object. Both halves are needed:
  // the constructors are rl's (case signature, field snippet, tags an
  // unimported built-in still has), while the rest of a standard-library
  // namespace — `Result.map`, `Option.unwrapOrElse`, the `*P` pipeline
  // variants — is ordinary TypeScript the service already types. Returning
  // only the constructors hid every combinator behind `Result.`/`Option.`
  // (TASK-062). Any other member access (`obj.`) is TypeScript's alone.
  const base = analysis.memberAccessAt(masked, offset);
  if (base !== null) {
    const e = visible.find((x) => x.name === base);
    const members = await tsCompletionsProbed(doc, offset, true);
    if (!e) return members;
    const items = e.cases.map((c) => constructorItem(e, c));
    const tags = new Set(items.map((i) => i.label));
    return items.concat(members.filter((i) => !tags.has(i.label)));
  }

  // A `.` with no identifier in front of it is a member access all the same
  // (`x |> .`, `f().`, `(a + b).`): members belong there, and nothing else —
  // no enum names, no keyword snippets.
  if (probe.atMemberAccess(masked, offset)) {
    return tsCompletionsProbed(doc, offset, true);
  }

  const ctx = analysis.armContextAt(masked, matches, offset);

  // Inside `Tag(` in a pattern → field bindings of that tag.
  if (ctx?.bindingTag) {
    const owners = visible.filter((e) =>
      e.cases.some((c) => c.tag === ctx.bindingTag),
    );
    const items: CompletionItem[] = [];
    for (const e of owners) {
      const c = e.cases.find((x) => x.tag === ctx.bindingTag);
      if (!c) continue;
      for (const f of c.fields) {
        items.push({
          label: f.name,
          kind: CompletionItemKind.Field,
          detail: `${analysis.caseSignature(e, c)} — 필드 바인딩`,
          sortText: `0${f.name}`,
        });
      }
    }
    return items;
  }

  // Arm pattern position → case tags (not yet covered) + `_`.
  if (ctx?.patternPosition) {
    // Structural inference (an `Enum.` mention in the scrutinee, or a unique
    // owner of the written tags) — exact when it fires. Otherwise the whole
    // visible pool: TypeScript 7 has no structural way to name the type of
    // the scrutinee, and offering a superset beats naming the wrong enum.
    const inferred = analysis.inferEnum(masked, ctx.match, visible);
    const pool = inferred ? [inferred] : visible;
    const covered = new Set(analysis.armTags(masked, ctx.match));
    const items: CompletionItem[] = [];
    for (const e of pool) {
      for (const c of e.cases) {
        if (covered.has(c.tag)) continue;
        items.push({
          label: c.tag,
          kind: CompletionItemKind.EnumMember,
          detail: analysis.caseSignature(e, c),
          documentation: e.builtin
            ? { kind: MarkupKind.Markdown, value: `내장 enum \`${e.name}\`의 케이스` }
            : undefined,
          insertText:
            c.fields.length > 0
              ? `${c.tag}(${c.fields.map((f) => f.name).join(", ")})`
              : c.tag,
          sortText: `0${c.tag}`,
        });
      }
    }
    items.push({
      label: "_",
      kind: CompletionItemKind.Keyword,
      detail: "와일드카드 암 — 나머지 모든 케이스 (반드시 마지막)",
      sortText: "1_",
    });
    return items;
  }

  // General position → enum names + rl keyword snippets, then everything
  // TypeScript would offer in a .ts file (sorted after the rl items).
  const items: CompletionItem[] = visible.map((e) => ({
    label: e.name,
    kind: CompletionItemKind.Enum,
    detail: e.builtin
      ? `내장 enum ${e.name}${e.generics}`
      : e.imported
        ? `enum ${e.name}${e.generics} — ${e.imported.specifier}`
        : `enum ${e.name}${e.generics}`,
    sortText: `0${e.name}`,
  }));
  const rlItems = items.concat(KEYWORD_SNIPPETS);
  const seen = new Set(rlItems.map((i) => i.label));
  return rlItems.concat(
    (await tsCompletionsProbed(doc, offset, false)).filter(
      (i) => !seen.has(i.label),
    ),
  );
});

/**
 * The signature and documentation of a delegated completion entry, fetched
 * when the editor asks about the one entry the user is looking at. This is
 * what puts a type on `Result.map` in the completion list; rl's own items
 * carry their signature in `detail` from the start and pass through here
 * untouched.
 */
connection.onCompletionResolve(
  async (item: CompletionItem): Promise<CompletionItem> => {
    const data = item.data as TsCompletionData | undefined;
    if (!data || typeof data.uri !== "string") return item;
    const doc = documents.get(data.uri);
    if (!doc) return item;
    const uri = URI.parse(doc.uri);
    if (uri.scheme !== "file") return item;
    // An entry listed from a probe must be resolved against that probe:
    // the buffer it was computed from is the one with the placeholder.
    const from =
      data.probe && lastProbe?.version === data.probe ? lastProbe : null;
    let detail: tsproject.TsCompletionDetail | null;
    if (from) {
      detail = await withProbe(from, () =>
        getTsProject().completionDetail(from.fsPath, from.offset, data.name),
      );
    } else if (data.probe) {
      return item; // its probe is gone; the buffer has moved on
    } else {
      await ensureVirtual(doc);
      const at = toServiceOffset(uri.fsPath, doc, data.offset);
      if (at === null) return item;
      detail = await getTsProject().completionDetail(uri.fsPath, at, data.name);
    }
    if (!detail) return item;
    if (detail.signature !== "") item.detail = detail.signature;
    if (detail.documentation !== "") {
      item.documentation = {
        kind: MarkupKind.Markdown,
        value: detail.documentation,
      };
    }
    return item;
  },
);

// ---------------------------------------------------------- signature help

/**
 * Parameter hints while a call is being written, delegated wholesale to
 * TypeScript over the virtual document — so the arguments of a standard
 * library combinator (`Result.andThen(r, f)`) are typed as they are typed,
 * including inside a `match` arm or a `|>` pipeline, where the raw text is
 * not TypeScript at all. rl syntax that has no call at the cursor simply
 * yields nothing.
 */
connection.onSignatureHelp(async (params): Promise<SignatureHelp | null> => {
  const doc = documents.get(params.textDocument.uri);
  if (!doc) return null;
  const uri = URI.parse(doc.uri);
  if (uri.scheme !== "file") return null;
  await ensureVirtual(doc);
  const at = toServiceOffset(uri.fsPath, doc, doc.offsetAt(params.position));
  if (at === null) return null;
  const help = await getTsProject().signatureHelpAt(uri.fsPath, at);
  if (!help || help.signatures.length === 0) return null;
  return {
    signatures: help.signatures.map(
      (sig): SignatureInformation => ({
        label: sig.label,
        documentation: sig.documentation
          ? { kind: MarkupKind.Markdown, value: sig.documentation }
          : undefined,
        parameters: sig.parameters.map(
          (p): ParameterInformation => ({
            label: p.label,
            documentation: p.documentation
              ? { kind: MarkupKind.Markdown, value: p.documentation }
              : undefined,
          }),
        ),
      }),
    ),
    activeSignature: help.activeSignature,
    activeParameter: help.activeParameter,
  };
});

function constructorItem(
  e: analysis.EnumInfo,
  c: analysis.CaseInfo,
): CompletionItem {
  const unit = !c.hasParens && c.fields.length === 0;
  const item: CompletionItem = {
    label: c.tag,
    kind: unit ? CompletionItemKind.EnumMember : CompletionItemKind.Constructor,
    detail: analysis.caseSignature(e, c),
    sortText: `0${c.tag}`,
  };
  if (!unit) {
    item.insertTextFormat = InsertTextFormat.Snippet;
    item.insertText =
      c.fields.length > 0
        ? `${c.tag}(${c.fields.map((f, i) => `\${${i + 1}:${f.name}}`).join(", ")})`
        : `${c.tag}()`;
  }
  return item;
}

// ------------------------------------------------------------------- hover

connection.onHover(async (params) => {
  const doc = documents.get(params.textDocument.uri);
  if (!doc) return null;
  await ensureVirtual(doc);
  const { text, masked, enums, matches } = analyze(doc);
  const offset = doc.offsetAt(params.position);

  const w = analysis.wordAt(text, offset);
  if (w && w.word === "match") {
    const m = analysis.matchKeywordAt(matches, offset);
    if (m && m.start === w.start) {
      return {
        contents: {
          kind: MarkupKind.Markdown,
          value:
            "```rl\nmatch (값) { 패턴 => 본문, ... }\n```\n" +
            "rl match 표현식 — 값의 `kind` 필드로 분기합니다. " +
            "`_` 없는 match는 같은 파일·import한 rl enum(내장 `Option`/`Result` 포함)에 대해 소진성이 검사됩니다.",
        },
        range: { start: doc.positionAt(w.start), end: doc.positionAt(w.end) },
      };
    }
  }

  const sym = analysis.symbolAt(
    text,
    masked,
    offset,
    enums,
    matches,
    await importedEnums(doc),
  );
  // Not an rl symbol — an ordinary TypeScript one, perhaps. Delegate.
  if (!sym || !w) return tsHover(doc, offset);

  const range = { start: doc.positionAt(w.start), end: doc.positionAt(w.end) };
  if (sym.kind === "enum") {
    const note = sym.enum.builtin
      ? "내장 enum — 선언 없이도 match 소진성 검사의 대상입니다. 값·타입은 표준 라이브러리 모듈(`@rl/std`)에서 import하세요."
      : sym.enum.imported
        ? `\`${sym.enum.imported.specifier}\`에서 import한 rl enum — match 소진성 검사의 대상입니다.`
        : "rl enum — 같은 이름의 타입 별칭(`kind` 태그드 유니언)과 생성자 객체로 컴파일됩니다.";
    return {
      contents: {
        kind: MarkupKind.Markdown,
        value: `\`\`\`rl\n${analysis.enumSignature(sym.enum)}\n\`\`\`\n${note}`,
      },
      range,
    };
  }

  const unit = !sym.case.hasParens && sym.case.fields.length === 0;
  const what = unit
    ? "유닛 케이스 — 싱글턴 값으로 컴파일됩니다."
    : "페이로드 케이스 — 생성자 함수로 컴파일됩니다.";
  const origin = sym.enum.builtin
    ? `내장 enum \`${sym.enum.name}\``
    : sym.enum.imported
      ? `\`${sym.enum.imported.specifier}\`의 \`enum ${sym.enum.name}\``
      : `\`enum ${sym.enum.name}\``;
  return {
    contents: {
      kind: MarkupKind.Markdown,
      value: `\`\`\`rl\n${analysis.caseSignature(sym.enum, sym.case)}\n\`\`\`\n${origin}의 ${what}`,
    },
    range,
  };
});

// -------------------------------------------------------------- definition

connection.onDefinition(async (params) => {
  const doc = documents.get(params.textDocument.uri);
  if (!doc) return null;
  await ensureVirtual(doc);
  const { text, masked, enums, matches } = analyze(doc);
  const offset = doc.offsetAt(params.position);
  const sym = analysis.symbolAt(
    text,
    masked,
    offset,
    enums,
    matches,
    await importedEnums(doc),
  );
  if (sym && !sym.enum.builtin) {
    // Imported enum: the declaration lives in the imported file, at the
    // 1-based position the compiler reported. The name/tag is an ASCII
    // identifier, so its length is its column width.
    const imported = sym.enum.imported;
    if (imported) {
      const target =
        sym.kind === "enum"
          ? { pos: imported, length: imported.name.length }
          : { pos: imported.cases[sym.case.tag], length: sym.case.tag.length };
      if (target.pos) {
        const start = {
          line: target.pos.line - 1,
          character: target.pos.col - 1,
        };
        return Location.create(URI.file(imported.path).toString(), {
          start,
          end: {
            line: start.line,
            character: start.character + target.length,
          },
        });
      }
    } else {
      const [start, end] =
        sym.kind === "enum"
          ? [sym.enum.nameStart, sym.enum.nameEnd]
          : [sym.case.tagStart, sym.case.tagEnd];
      return Location.create(doc.uri, {
        start: doc.positionAt(start),
        end: doc.positionAt(end),
      });
    }
  }

  // Everything else — ordinary TypeScript symbols, and built-in enum
  // names the user may have imported from the std module — is the
  // TypeScript language service's answer.
  return tsDefinitions(doc, offset);
});

// -------------------------------------------------- references and rename

connection.onReferences(async (params): Promise<Location[] | null> => {
  const doc = documents.get(params.textDocument.uri);
  if (!doc) return null;
  const uri = URI.parse(doc.uri);
  if (uri.scheme !== "file") return null;
  await ensureVirtual(doc);
  const offset = doc.offsetAt(params.position);
  // Delegated wholesale to TypeScript: it resolves the passthrough region
  // exactly, and rl-specific spans degrade to an empty result.
  const at = toServiceOffset(uri.fsPath, doc, offset);
  if (at === null) return null;
  const references = (await getTsProject().referencesAt(uri.fsPath, at)).filter(
    (r) => params.context.includeDeclaration || !r.isDefinition,
  );
  const locations: Location[] = [];
  for (const r of references) {
    const span = fromServiceSpan(r.fileName, r.fileText, r.start, r.length);
    if (!span) continue;
    locations.push(
      Location.create(URI.file(r.fileName).toString(), {
        start: positionOf(span.text, span.start),
        end: positionOf(span.text, span.end),
      }),
    );
  }
  return locations.length > 0 ? locations : null;
});

connection.onRenameRequest(async (params) => {
  const doc = documents.get(params.textDocument.uri);
  if (!doc) return null;
  const uri = URI.parse(doc.uri);
  if (uri.scheme !== "file") return null;
  await ensureVirtual(doc);
  const offset = doc.offsetAt(params.position);

  // rl symbols (enums, case tags) are compiled into the emitted `kind`
  // strings — renaming them needs rl-aware rewriting, so refuse rather
  // than let TypeScript do half the job.
  const { text, masked, enums, matches } = analyze(doc);
  const sym = analysis.symbolAt(
    text,
    masked,
    offset,
    enums,
    matches,
    await importedEnums(doc),
  );
  if (sym) return null;

  const at = toServiceOffset(uri.fsPath, doc, offset);
  if (at === null) return null;
  const locations = await getTsProject().renameAt(uri.fsPath, at);
  if (!locations || locations.length === 0) return null;
  const changes: Record<string, TextEdit[]> = {};
  for (const l of locations) {
    const span = fromServiceSpan(l.fileName, l.fileText, l.start, l.length);
    // A rename edit that cannot be mapped back would silently skip an
    // occurrence and corrupt the rename — refuse the whole operation.
    if (!span) return null;
    const target = URI.file(l.fileName).toString();
    (changes[target] ??= []).push(
      TextEdit.replace(
        {
          start: positionOf(span.text, span.start),
          end: positionOf(span.text, span.end),
        },
        params.newName,
      ),
    );
  }
  return { changes };
});

// ----------------------------------------------------------------- symbols

connection.onDocumentSymbol((params): DocumentSymbol[] => {
  const doc = documents.get(params.textDocument.uri);
  if (!doc) return [];
  const { enums } = analyze(doc);
  return enums.map((e) => {
    const range = {
      start: doc.positionAt(e.start),
      end: doc.positionAt(e.end),
    };
    return {
      name: `${e.name}${e.generics}`,
      kind: SymbolKind.Enum,
      range,
      selectionRange: {
        start: doc.positionAt(e.nameStart),
        end: doc.positionAt(e.nameEnd),
      },
      children: e.cases.map((c) => ({
        name: c.tag,
        detail:
          c.fields.length > 0
            ? `(${c.fields.map((f) => f.name).join(", ")})`
            : undefined,
        kind: SymbolKind.EnumMember,
        range: {
          start: doc.positionAt(c.tagStart),
          end: doc.positionAt(c.tagEnd),
        },
        selectionRange: {
          start: doc.positionAt(c.tagStart),
          end: doc.positionAt(c.tagEnd),
        },
      })),
    };
  });
});

// ------------------------------------------------------------ code actions

const NON_EXHAUSTIVE_RE =
  /match on (?:built-in |imported )?enum ([\w.]+)(?: \(imported from "[^"]+"\))? is not exhaustive: missing (.+?) \(add/;

connection.onCodeAction(async (params): Promise<CodeAction[]> => {
  const doc = documents.get(params.textDocument.uri);
  if (!doc) return [];
  const actions: CodeAction[] = [];

  for (const diag of params.context.diagnostics) {
    if (diag.source !== "rlc") continue;
    const m = NON_EXHAUSTIVE_RE.exec(diag.message);
    if (!m) continue;
    const missing = [...m[2].matchAll(/"([^"]+)"/g)].map((x) => x[1]);
    if (missing.length === 0) continue;

    const { masked, enums, matches } = analyze(doc);
    const offset = doc.offsetAt(diag.range.start);
    const match = analysis.matchKeywordAt(matches, offset);
    if (!match) continue;
    const e = analysis
      .visibleEnums(enums, await importedEnums(doc))
      .find((x) => x.name === m[1]);

    const armFor = (tag: string): string => {
      const c = e?.cases.find((x) => x.tag === tag);
      const bindings =
        c && c.fields.length > 0
          ? `(${c.fields.map((f) => f.name).join(", ")})`
          : "";
      return `${tag}${bindings} => undefined,`;
    };

    actions.push({
      title: `빠진 암 추가: ${missing.join(", ")}`,
      kind: CodeActionKind.QuickFix,
      diagnostics: [diag],
      edit: {
        changes: {
          [doc.uri]: [insertArms(doc, match, missing.map(armFor))],
        },
      },
    });
    actions.push({
      title: "와일드카드 `_` 암 추가",
      kind: CodeActionKind.QuickFix,
      diagnostics: [diag],
      edit: {
        changes: {
          [doc.uri]: [insertArms(doc, match, ["_ => undefined,"])],
        },
      },
    });
    void masked;
  }
  return actions;
});

/** Insert arm lines just before the match body's closing brace. */
function insertArms(
  doc: TextDocument,
  match: analysis.MatchInfo,
  arms: string[],
): TextEdit {
  const text = doc.getText();
  const closePos = doc.positionAt(match.bodyClose);
  const matchPos = doc.positionAt(match.start);
  const matchLineStart = doc.offsetAt({ line: matchPos.line, character: 0 });
  const baseIndent = /^[ \t]*/.exec(
    text.slice(matchLineStart, match.start),
  )![0];
  const armIndent = `${baseIndent}  `;

  const closeLineStart = doc.offsetAt({ line: closePos.line, character: 0 });
  const beforeClose = text.slice(closeLineStart, match.bodyClose);
  if (beforeClose.trim() === "") {
    // `}` alone on its line: insert full arm lines above it.
    const newText = arms.map((a) => `${armIndent}${a}\n`).join("");
    return TextEdit.insert({ line: closePos.line, character: 0 }, newText);
  }
  // Single-line match: splice arms in before `}`.
  const needsComma = /[^,{\s]\s*$/.test(
    text.slice(match.bodyOpen + 1, match.bodyClose),
  );
  const prefix = needsComma ? ", " : " ";
  return TextEdit.insert(closePos, `${prefix}${arms.join(" ")} `);
}

// ------------------------------------------------------------------- start

documents.listen(connection);
connection.listen();
