/* --------------------------------------------------------------------------
 * rl language server — a protocol adapter over the rl engine.
 *
 * The server owns the LSP surface: capabilities, document sync with the
 * client, debouncing, configuration, and presentation. Language semantics
 * live elsewhere and are consumed, not implemented, here:
 *
 * - The **engine** (`rlc --server`, engine.ts) is the authoritative project
 *   state. The editor's buffers are forwarded to it (didOpen/didChange/
 *   didClose), and every TypeScript-backed answer — hover, definition,
 *   references, completion with its incomplete-source probe, rename,
 *   signature help, type diagnostics — comes back from it already in `.rl`
 *   coordinates. No projection, source mapping, TypeScript session or
 *   probe logic lives in this process.
 * - The **rl syntax layer** (analysis.ts) answers what needs no types and
 *   must work on a buffer mid-keystroke: enum/case structure, match-arm
 *   completion, document symbols, quick fixes. It is deliberately
 *   diagnostic-free — a near-miss can lose a convenience, never invent an
 *   error.
 * - Diagnostics run the real compiler through rlc.ts (`--check`, and the
 *   typed layer via the engine's `typedCheck`).
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
import * as engine from "./engine";
import * as rlc from "./rlc";
import * as path from "node:path";

import * as sidecar from "./sidecar";

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

// ------------------------------------------------------------- the engine

/** The compiler path last resolved from settings — the engine session is
 * keyed by it, and the synchronous paths (document sync) cannot await
 * settings. */
let servedCompiler: string | null = null;

function currentCompiler(): string {
  return servedCompiler ?? rlc.findCompiler("", workspaceRoots);
}

const logEngine = (message: string) => connection.console.warn(message);

/** A fresh engine session starts with the disk's view of the world; hand it
 * every buffer the editor holds open. Runs on first spawn and on respawn
 * after a crash — recovery the old in-process pipeline never had. */
engine.setOnSessionStart(() => {
  const compiler = currentCompiler();
  for (const doc of documents.all()) {
    const uri = URI.parse(doc.uri);
    if (uri.scheme === "file") {
      engine.openDocument(compiler, uri.fsPath, doc.getText());
    }
  }
});

/** The open document's path when the engine can answer about it. */
function enginePath(doc: TextDocument): string | null {
  const uri = URI.parse(doc.uri);
  return uri.scheme === "file" ? uri.fsPath : null;
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
 * actually has — is the engine's typed pass. The engine keeps the compiler
 * session alive and answers incrementally (TASK-076), so the pass runs on a
 * debounce close to the base layer's and publishes again when it lands.
 *
 * Both layers are cached per document *version*. A typed answer computed for
 * an older version is not shown: its positions describe text that is no
 * longer there, and a `val` error pointing at the wrong token is worse than
 * one that arrives a moment later.
 * --------------------------------------------------------------------- */
const pendingTypedCheck = new Map<string, NodeJS.Timeout>();
const TYPED_CHECK_DELAY_MS = 250;

interface VersionedDiagnostics {
  version: number;
  diagnostics: Diagnostic[];
}

/** What `--check` and the TypeScript layer found, as last published. */
const baseDiagnostics = new Map<string, VersionedDiagnostics>();
/** What the typed rl layer found, for the version it ran on. */
const typedDiagnostics = new Map<string, VersionedDiagnostics>();
let warnedTypedCheckUnavailable = false;

/**
 * Adds the typed diagnostics that say something new.
 *
 * The two passes overlap: enum exhaustiveness is decided from the text by
 * `--check` and from the type by the typed pass, and both report it at the
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
          "the typed pass, which needs a TypeScript install — set " +
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
  servedCompiler = compiler;

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

  // The typed layer trails on its own debounce; schedule it before awaiting
  // the TypeScript diagnostics so that wait does not push it further out.
  // Publication order is safe either way: publish() merges the typed layer
  // only when both layers were computed for this very version.
  if (settings.typedChecks) scheduleTypedCheck(doc, compiler);

  if (settings.typeDiagnostics) {
    diagnostics.push(...(await typeDiagnostics(doc, compiler)));
    // Awaiting gave the buffer another chance to move on.
    const fresh = documents.get(doc.uri);
    if (!fresh || fresh.version !== doc.version) return;
  }

  baseDiagnostics.set(doc.uri, { version: doc.version, diagnostics });
  publish(doc.uri, doc.version, diagnostics);
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
 * TypeScript's type errors for a buffer, already on the `.rl` source.
 *
 * The engine computes them over the buffer's projection and drops anything
 * landing in compiler glue — rlc's emissions are the compiler's
 * responsibility, never something to report at the user (CLAUDE.md, error
 * layers). This side only converts and version-gates.
 */
async function typeDiagnostics(
  doc: TextDocument,
  compiler: string,
): Promise<Diagnostic[]> {
  const fsPath = enginePath(doc);
  if (fsPath === null) return [];
  const items = await engine.tsDiagnostics(compiler, fsPath, logEngine);
  return items.map((d) => ({
    severity: d.warning
      ? DiagnosticSeverity.Warning
      : DiagnosticSeverity.Error,
    range: d.range,
    message: d.message,
    code: d.code,
    source: "ts",
  }));
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

documents.onDidOpen((e) => {
  const fsPath = enginePath(e.document);
  if (fsPath !== null) {
    engine.openDocument(currentCompiler(), fsPath, e.document.getText());
  }
  scheduleValidation(e.document);
});
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
  const fsPath = enginePath(e.document);
  if (fsPath !== null) {
    engine.updateDocument(currentCompiler(), fsPath, e.document.getText());
  }
  // Editing one file can change what its siblings import — drop their
  // cached imported declarations (the edited doc's own entry refreshes by
  // version).
  for (const uri of importedCache.keys()) {
    if (uri !== e.document.uri) importedCache.delete(uri);
  }
  scheduleValidation(e.document);
});
documents.onDidClose((e) => {
  const fsPath = enginePath(e.document);
  if (fsPath !== null) {
    engine.closeDocument(currentCompiler(), fsPath);
  }
  analysisCache.delete(e.document.uri);
  importedCache.delete(e.document.uri);
  forget(e.document.uri);
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

/** TypeScript element-kind strings → LSP completion kinds. */
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
 * (`completionItem/resolve`). Positions are in source coordinates and the
 * engine re-maps at resolve time — the buffer may have moved on since. */
interface TsCompletionData {
  uri: string;
  offset: number;
  name: string;
  /** The engine's probe the entry was listed from, when it came from one —
   * the detail must be fetched against that same text. */
  probe?: number;
}

/**
 * True when `offset` sits right after a member-access dot. `masked` is the
 * masked source (analysis.ts), so a `.` inside a string, comment or regex
 * is not one.
 */
function atMemberAccess(masked: string, offset: number): boolean {
  return offset > 0 && masked[offset - 1] === ".";
}

/**
 * TypeScript completions from the engine, sorted after the rl-specific
 * items (`2` prefix; rl items use `0`/`1`). The engine applies the whole
 * member/probe policy: at a member access only a member answer comes back,
 * probed from the mended source when the buffer's own text cannot answer.
 */
async function tsCompletions(
  doc: TextDocument,
  offset: number,
  atMember: boolean,
): Promise<CompletionItem[]> {
  const fsPath = enginePath(doc);
  if (fsPath === null) return [];
  const list = await engine.completion(
    currentCompiler(),
    fsPath,
    doc.positionAt(offset),
    atMember,
    logEngine,
  );
  if (!list) return [];
  return list.items.map((entry) => ({
    label: entry.label,
    kind: TS_COMPLETION_KINDS[entry.kind] ?? CompletionItemKind.Text,
    sortText: `2${entry.sortText}`,
    data: {
      uri: doc.uri,
      offset,
      name: entry.label,
      probe: list.probe ?? undefined,
    } satisfies TsCompletionData,
  }));
}

connection.onCompletion(async (params): Promise<CompletionItem[]> => {
  const doc = documents.get(params.textDocument.uri);
  if (!doc) return [];
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
    const members = await tsCompletions(doc, offset, true);
    if (!e) return members;
    const items = e.cases.map((c) => constructorItem(e, c));
    const tags = new Set(items.map((i) => i.label));
    return items.concat(members.filter((i) => !tags.has(i.label)));
  }

  // A `.` with no identifier in front of it is a member access all the same
  // (`x |> .`, `f().`, `(a + b).`): members belong there, and nothing else —
  // no enum names, no keyword snippets.
  if (atMemberAccess(masked, offset)) {
    return tsCompletions(doc, offset, true);
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
    (await tsCompletions(doc, offset, false)).filter((i) => !seen.has(i.label)),
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
    const fsPath = enginePath(doc);
    if (fsPath === null) return item;
    const detail = await engine.completionResolve(
      currentCompiler(),
      fsPath,
      doc.positionAt(data.offset),
      data.name,
      data.probe,
      logEngine,
    );
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
 * the engine — so the arguments of a standard library combinator
 * (`Result.andThen(r, f)`) are typed as they are typed, including inside a
 * `match` arm or a `|>` pipeline, where the raw text is not TypeScript at
 * all. rl syntax that has no call at the cursor simply yields nothing.
 */
connection.onSignatureHelp(async (params): Promise<SignatureHelp | null> => {
  const doc = documents.get(params.textDocument.uri);
  if (!doc) return null;
  const fsPath = enginePath(doc);
  if (fsPath === null) return null;
  const help = await engine.signatureHelp(
    currentCompiler(),
    fsPath,
    params.position,
    logEngine,
  );
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
  if (!sym || !w) return tsHover(doc, params.position);

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

/** Hover for everything the rl layer does not own, from the engine. */
async function tsHover(
  doc: TextDocument,
  position: { line: number; character: number },
) {
  const fsPath = enginePath(doc);
  if (fsPath === null) return null;
  const info = await engine.hover(currentCompiler(), fsPath, position, logEngine);
  if (!info || info.signature === "") return null;
  const value =
    "```ts\n" +
    info.signature +
    "\n```" +
    (info.documentation ? `\n${info.documentation}` : "");
  return {
    contents: { kind: MarkupKind.Markdown, value },
    range: info.range,
  };
}

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
  // engine's answer, already in `.rl` coordinates.
  const fsPath = enginePath(doc);
  if (fsPath === null) return null;
  const locations = await engine.definition(
    currentCompiler(),
    fsPath,
    params.position,
    logEngine,
  );
  if (locations.length === 0) return null;
  return locations.map((l) =>
    Location.create(URI.file(l.path).toString(), l.range),
  );
});

// -------------------------------------------------- references and rename

connection.onReferences(async (params): Promise<Location[] | null> => {
  const doc = documents.get(params.textDocument.uri);
  if (!doc) return null;
  const fsPath = enginePath(doc);
  if (fsPath === null) return null;
  // Delegated wholesale to the engine: TypeScript resolves the passthrough
  // region exactly, and rl-specific spans degrade to an empty result.
  const references = (
    await engine.references(currentCompiler(), fsPath, params.position, logEngine)
  ).filter((r) => params.context.includeDeclaration || !r.isDefinition);
  if (references.length === 0) return null;
  return references.map((r) =>
    Location.create(URI.file(r.path).toString(), r.range),
  );
});

connection.onRenameRequest(async (params) => {
  const doc = documents.get(params.textDocument.uri);
  if (!doc) return null;
  const fsPath = enginePath(doc);
  if (fsPath === null) return null;

  // rl symbols (enums, case tags) are compiled into the emitted `kind`
  // strings — renaming them needs rl-aware rewriting, so refuse rather
  // than let TypeScript do half the job.
  const { text, masked, enums, matches } = analyze(doc);
  const sym = analysis.symbolAt(
    text,
    masked,
    doc.offsetAt(params.position),
    enums,
    matches,
    await importedEnums(doc),
  );
  if (sym) return null;

  // The engine owns the safety rule: an edit that cannot be mapped back to
  // source would corrupt the rename, so such a rename comes back null —
  // whole or not at all.
  const edits = await engine.rename(
    currentCompiler(),
    fsPath,
    params.position,
    logEngine,
  );
  if (!edits || edits.length === 0) return null;
  const changes: Record<string, TextEdit[]> = {};
  for (const edit of edits) {
    // What the engine wants written, which is not always the bare name: a
    // destructuring shorthand — what an rl pattern binding `Some(value)`
    // compiles to — expands to `value: <new>`, and dropping the expansion
    // would rebind a *different* field under the new name.
    const newText =
      edit.newText === null
        ? params.newName
        : edit.newText.split(engine.RENAME_PLACEHOLDER).join(params.newName);
    const target = URI.file(edit.path).toString();
    (changes[target] ??= []).push(TextEdit.replace(edit.range, newText));
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

    const { enums, matches } = analyze(doc);
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
