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
      textDocumentSync: TextDocumentSyncKind.Incremental,
      completionProvider: { triggerCharacters: [".", "(", "|"] },
      hoverProvider: true,
      definitionProvider: true,
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
}

const DEFAULT_SETTINGS: RlSettings = { compilerPath: "", verify: true };

async function getSettings(uri: string): Promise<RlSettings> {
  if (!hasConfigurationCapability) return DEFAULT_SETTINGS;
  const conf = (await connection.workspace.getConfiguration({
    scopeUri: uri,
    section: "rl",
  })) as Partial<RlSettings> | null;
  return {
    compilerPath: conf?.compilerPath ?? DEFAULT_SETTINGS.compilerPath,
    verify: conf?.verify ?? DEFAULT_SETTINGS.verify,
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
documents.onDidChangeContent((e) => scheduleValidation(e.document));
documents.onDidClose((e) => {
  analysisCache.delete(e.document.uri);
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
        "`kind` 태그로 분기하는 match 표현식. `_` 없는 match는 같은 파일의 rl enum에 대해 소진성이 검사됩니다.",
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

connection.onCompletion((params): CompletionItem[] => {
  const doc = documents.get(params.textDocument.uri);
  if (!doc) return [];
  const { masked, enums, matches } = analyze(doc);
  const offset = doc.offsetAt(params.position);
  const visible = analysis.visibleEnums(enums);

  // `Enum.` member access → constructors of that enum.
  const base = analysis.memberAccessAt(masked, offset);
  if (base !== null) {
    const e = visible.find((x) => x.name === base);
    if (!e) return [];
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

  // General position → enum names + rl keyword snippets.
  const items: CompletionItem[] = visible.map((e) => ({
    label: e.name,
    kind: CompletionItemKind.Enum,
    detail: e.builtin
      ? `내장 enum ${e.name}${e.generics}`
      : `enum ${e.name}${e.generics}`,
    sortText: `0${e.name}`,
  }));
  return items.concat(KEYWORD_SNIPPETS);
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

connection.onHover((params) => {
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
            "`_` 없는 match는 같은 파일의 rl enum(내장 `Option`/`Result` 포함)에 대해 소진성이 검사됩니다.",
        },
        range: { start: doc.positionAt(w.start), end: doc.positionAt(w.end) },
      };
    }
  }

  const sym = analysis.symbolAt(text, masked, offset, enums, matches);
  if (!sym || !w) return null;

  const range = { start: doc.positionAt(w.start), end: doc.positionAt(w.end) };
  if (sym.kind === "enum") {
    const note = sym.enum.builtin
      ? "내장 enum — 선언 없이도 match 소진성 검사의 대상입니다. 값·타입은 표준 라이브러리 모듈(`rlc --emit-std`)에서 import하세요."
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
  const origin = sym.enum.builtin ? `내장 enum \`${sym.enum.name}\`` : `\`enum ${sym.enum.name}\``;
  return {
    contents: {
      kind: MarkupKind.Markdown,
      value: `\`\`\`rl\n${analysis.caseSignature(sym.enum, sym.case)}\n\`\`\`\n${origin}의 ${what}`,
    },
    range,
  };
});

// -------------------------------------------------------------- definition

connection.onDefinition((params) => {
  const doc = documents.get(params.textDocument.uri);
  if (!doc) return null;
  const { text, masked, enums, matches } = analyze(doc);
  const offset = doc.offsetAt(params.position);
  const sym = analysis.symbolAt(text, masked, offset, enums, matches);
  if (!sym || sym.enum.builtin) return null;

  const [start, end] =
    sym.kind === "enum"
      ? [sym.enum.nameStart, sym.enum.nameEnd]
      : [sym.case.tagStart, sym.case.tagEnd];
  return Location.create(doc.uri, {
    start: doc.positionAt(start),
    end: doc.positionAt(end),
  });
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
  /match on (?:built-in )?enum (\w+) is not exhaustive: missing (.+?) \(add/;

connection.onCodeAction((params): CodeAction[] => {
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
      .visibleEnums(enums)
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
