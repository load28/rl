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
  ProposedFeatures,
  Range,
  SymbolKind,
  TextDocuments,
  TextDocumentSyncKind,
  TextEdit,
} from "vscode-languageserver/node";
import { TextDocument } from "vscode-languageserver-textdocument";
import { URI } from "vscode-uri";

import * as analysis from "./analysis";
import * as rlc from "./rlc";
import * as path from "node:path";

import * as sidecar from "./sidecar";
import * as tsproject from "./tsproject";
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
      completionProvider: { triggerCharacters: [".", "(", "|"] },
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
  sidecar: sidecar.SidecarMode;
  sidecarDir: string;
}

const DEFAULT_SETTINGS: RlSettings = {
  compilerPath: "",
  verify: true,
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

// ----------------------------------------- virtual documents (TASK-048)

/* The compiler's emitted TypeScript for each open .rl buffer, refreshed on
 * the validation cadence (`rlc --emit-map`). When current, it is what the
 * TypeScript language service sees instead of the raw .rl text — the
 * emitted code is plain TypeScript, so inference works inside match arms,
 * scrutinees and the other rl constructs. Offsets are translated through
 * the emit mappings in both directions; while the entry lags the buffer
 * (or the compiler is missing) the raw text is served and offsets pass
 * through unchanged — the pre-TASK-048 error-recovery behavior. */

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
  if (!doc) return { text: fileText, offset }; // disk file, served raw
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
 * the fallback for definitions and hover; TS diagnostics are never
 * surfaced. */

let tsProjectInstance: tsproject.TsProject | null = null;

function getTsProject(): tsproject.TsProject {
  tsProjectInstance ??= new tsproject.TsProject(
    (fileName) => {
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
      return null;
    },
    () =>
      documents
        .all()
        .map((d) => URI.parse(d.uri))
        .filter((u) => u.scheme === "file")
        .map((u) => u.fsPath),
    workspaceRoots[0] ?? process.cwd(),
  );
  return tsProjectInstance;
}

function tsDefinitions(doc: TextDocument, offset: number): Location[] | null {
  const uri = URI.parse(doc.uri);
  if (uri.scheme !== "file") return null;
  const at = toServiceOffset(uri.fsPath, doc, offset);
  if (at === null) return null;
  const definitions = getTsProject().definitionsAt(uri.fsPath, at);
  const locations: Location[] = [];
  for (const d of definitions) {
    const span = fromServiceSpan(d.fileName, d.fileText, d.start, d.length);
    if (!span) continue;
    locations.push(
      Location.create(URI.file(d.fileName).toString(), {
        start: tsproject.positionAt(span.text, span.start),
        end: tsproject.positionAt(span.text, span.end),
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

/** TS completions, sorted after the rl-specific items (`2` prefix; rl
 * items use `0`/`1`). */
function tsCompletions(doc: TextDocument, offset: number): CompletionItem[] {
  const uri = URI.parse(doc.uri);
  if (uri.scheme !== "file") return [];
  const at = toServiceOffset(uri.fsPath, doc, offset);
  if (at === null) return [];
  return getTsProject()
    .completionsAt(uri.fsPath, at)
    .map((entry) => ({
      label: entry.name,
      kind: TS_COMPLETION_KINDS[entry.kind] ?? CompletionItemKind.Text,
      sortText: `2${entry.sortText}`,
    }));
}

function tsHover(doc: TextDocument, offset: number) {
  const uri = URI.parse(doc.uri);
  if (uri.scheme !== "file") return null;
  const at = toServiceOffset(uri.fsPath, doc, offset);
  if (at === null) return null;
  const info = getTsProject().quickInfoAt(uri.fsPath, at);
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
  // serving on its own when the compiler is missing.
  void refreshVirtual(doc, compiler);

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
    void connection.sendDiagnostics({ uri: doc.uri, diagnostics: [] });
    return;
  }
  if (result.kind === "failed") {
    connection.console.error(`rl: ${result.detail}`);
    void connection.sendDiagnostics({ uri: doc.uri, diagnostics: [] });
    return;
  }

  const diagnostics = result.diagnostics.map((d) =>
    toDiagnostic(current, d),
  );
  void connection.sendDiagnostics({ uri: doc.uri, diagnostics });
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
  const { masked, enums, matches } = analyze(doc);
  const offset = doc.offsetAt(params.position);
  const visible = analysis.visibleEnums(enums, await importedEnums(doc));

  // `Enum.` member access → constructors of that enum; any other member
  // access (`obj.`) is TypeScript's to complete.
  const base = analysis.memberAccessAt(masked, offset);
  if (base !== null) {
    const e = visible.find((x) => x.name === base);
    if (!e) return tsCompletions(doc, offset);
    return e.cases.map((c) => constructorItem(e, c));
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
    // Structural inference first (an `Enum.` mention in the scrutinee, or a
    // unique owner of the written tags) — it is exact when it fires. Then
    // the TypeScript-inferred type of the scrutinee expression, matched
    // against the visible rl enums (TASK-048). Only then the full pool.
    const inferred =
      analysis.inferEnum(masked, ctx.match, visible) ??
      tsScrutineeEnum(doc, ctx.match, visible);
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
    tsCompletions(doc, offset).filter((i) => !seen.has(i.label)),
  );
});

/**
 * The visible rl enum a match scrutinee has, according to TypeScript's
 * inferred type of the scrutinee expression (TASK-048). The type name must
 * match a visible enum (pre-alias import names included), and when
 * TypeScript reports the declaring file it must be the file that enum
 * actually lives in — a same-named unrelated TS type is not a hit. Null
 * on any doubt: the caller falls back to the full visible pool.
 */
function tsScrutineeEnum(
  doc: TextDocument,
  m: analysis.MatchInfo,
  visible: analysis.EnumInfo[],
): analysis.EnumInfo | null {
  const uri = URI.parse(doc.uri);
  if (uri.scheme !== "file") return null;

  // The scrutinee expression, whitespace-trimmed, in source coordinates.
  const text = doc.getText();
  let start = m.scrutOpen + 1;
  let end = m.scrutClose;
  while (start < end && /\s/.test(text[start])) start++;
  while (end > start && /\s/.test(text[end - 1])) end--;
  if (start >= end) return null;

  let queryStart = start;
  let queryEnd = end;
  const mapped = activeVirtual(uri.fsPath, doc.version);
  if (mapped) {
    const s = mapped.srcToOut(start);
    const e = mapped.srcToOut(end - 1);
    if (s === null || e === null || e < s) return null;
    queryStart = s;
    queryEnd = e + 1;
  }
  const info = getTsProject().typeAt(uri.fsPath, queryStart, {
    start: queryStart,
    end: queryEnd,
  });
  if (!info) return null;

  return (
    visible.find((e) => {
      if (e.name !== info.name && e.imported?.name !== info.name) return false;
      // Built-ins (Option/Result) have no on-disk declaration to compare.
      if (e.builtin) return true;
      if (info.declFile === null) return true;
      const declared = e.imported ? e.imported.path : uri.fsPath;
      return path.resolve(info.declFile) === path.resolve(declared);
    }) ?? null
  );
}

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

connection.onReferences((params): Location[] | null => {
  const doc = documents.get(params.textDocument.uri);
  if (!doc) return null;
  const uri = URI.parse(doc.uri);
  if (uri.scheme !== "file") return null;
  const offset = doc.offsetAt(params.position);
  // Delegated wholesale to TypeScript: it resolves the passthrough region
  // exactly, and rl-specific spans degrade to an empty result.
  const at = toServiceOffset(uri.fsPath, doc, offset);
  if (at === null) return null;
  const references = getTsProject()
    .referencesAt(uri.fsPath, at)
    .filter((r) => params.context.includeDeclaration || !r.isDefinition);
  const locations: Location[] = [];
  for (const r of references) {
    const span = fromServiceSpan(r.fileName, r.fileText, r.start, r.length);
    if (!span) continue;
    locations.push(
      Location.create(URI.file(r.fileName).toString(), {
        start: tsproject.positionAt(span.text, span.start),
        end: tsproject.positionAt(span.text, span.end),
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
  const locations = getTsProject().renameAt(uri.fsPath, at);
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
          start: tsproject.positionAt(span.text, span.start),
          end: tsproject.positionAt(span.text, span.end),
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
