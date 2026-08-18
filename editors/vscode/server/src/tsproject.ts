/* --------------------------------------------------------------------------
 * TypeScript language-service delegation.
 *
 * An .rl file is TypeScript plus four constructs, so every symbol the rl
 * analysis does not own (variables, functions, types, imported values, ...)
 * is answered by the real TypeScript language service running over the rl
 * sources themselves. The TS parser's error recovery tolerates the rl
 * constructs; the passthrough region — most of the file — resolves exactly
 * as it would in a .ts file.
 *
 * TS *type* diagnostics are surfaced (TASK-057) — but only over a virtual
 * document, where the service sees the emitted TypeScript rather than rl
 * syntax. rl-level errors stay the compiler's (`rlc --check`): the two
 * layers never overlap, which is the error-layer contract in CLAUDE.md.
 *
 * Relative `.rl` specifiers are resolved by a custom module resolver that
 * points them at the .rl file (served as TS content); `"@rl/std"` falls back
 * to the compiler's own standard library module when the project does not
 * map the specifier itself (TASK-055); everything else goes through standard
 * TypeScript resolution.
 * ----------------------------------------------------------------------- */
import * as fs from "fs";
import * as path from "path";
import * as ts from "typescript";

/** A file the editor currently has open, served instead of the disk copy.
 * `version` may carry a suffix distinguishing raw from virtual (emitted)
 * serving of the same buffer version, so the service re-reads on switch. */
export interface OpenDoc {
  text: string;
  version: number | string;
}

/** One definition target, with the target file's text for offset→position
 * conversion (offsets are UTF-16 code units, the LSP default). */
export interface TsDefinition {
  fileName: string;
  start: number;
  length: number;
  fileText: string;
}

/** Quick-info for hover: a code-ish signature plus optional documentation,
 * and the source span it applies to. */
export interface TsQuickInfo {
  signature: string;
  documentation: string;
  start: number;
  length: number;
}

/** One completion entry. `kind` is TypeScript's ScriptElementKind string
 * (e.g. "function", "const", "property") — mapped to an LSP kind by the
 * server so this module stays the only place importing `typescript`. */
export interface TsCompletion {
  name: string;
  kind: string;
  sortText: string;
}

/** One type error, in the coordinates of the text the service was given
 * (the emitted TypeScript for an `.rl` module — the caller maps it back). */
export interface TsDiagnostic {
  start: number;
  length: number;
  message: string;
  /** TypeScript's error number, e.g. 2345. */
  code: number;
  /** True for `ts.DiagnosticCategory.Warning`; everything reported here is
   * otherwise an error. */
  warning: boolean;
}

/** One reference or rename location. */
export interface TsReference extends TsDefinition {
  isDefinition: boolean;
}

/** The TypeScript-inferred type at a position (see [`TsProject.typeAt`]):
 * display name without generic arguments, plus the declaring file of its
 * symbol when TypeScript knows one. */
export interface TsTypeInfo {
  name: string;
  declFile: string | null;
}

/** The standard library's bare specifier (docs/reference/std.md). */
const STD_SPECIFIER = "@rl/std";

const COMPILER_OPTIONS: ts.CompilerOptions = {
  // Internal but load-bearing (the standard trick of embedded-TS tooling —
  // VS Code itself, Volar, Svelte): without it the program silently drops
  // root files whose extension is not .ts/.tsx/.js, i.e. every .rl file.
  ...({ allowNonTsExtensions: true } as ts.CompilerOptions),
  allowJs: true,
  checkJs: false,
  target: ts.ScriptTarget.ESNext,
  module: ts.ModuleKind.ESNext,
  moduleResolution: ts.ModuleResolutionKind.Bundler,
};

/** The `lib.*.d.ts` these options are checked against (`lib.esnext.full.d.ts`). */
const DEFAULT_LIB_NAME = ts.getDefaultLibFileName(COMPILER_OPTIONS);

/**
 * TypeScript's default library file, or null when it cannot be found.
 *
 * `ts.getDefaultLibFilePath` only computes a path — it assumes the
 * `lib.*.d.ts` files sit next to the `typescript` module that is executing,
 * which a packaging or install accident can break. Losing them is silent
 * and does not look like a missing file: the program still type-checks,
 * only with no global types at all, so a `[number, number]` has no
 * `Array.prototype[Symbol.iterator]` and TypeScript reports `TS2488` on
 * ordinary tuple destructuring the user wrote. Worse, the errors that would
 * give the cause away (`Cannot find name 'Error'`, `'JSON'`) land in rlc's
 * generated glue and are dropped by the span mapping, so a single
 * unexplainable error on correct code is all that reaches the editor.
 *
 * So every candidate is verified on disk, and when none is, the caller
 * disables type diagnostics rather than reporting invented ones.
 */
function findDefaultLib(rootDir: string): string | null {
  const candidates: string[] = [];
  const add = (dir: string): void => {
    candidates.push(path.join(dir, DEFAULT_LIB_NAME));
  };
  // Where the TypeScript this server loaded keeps its libraries.
  try {
    candidates.push(ts.getDefaultLibFilePath(COMPILER_OPTIONS));
  } catch {
    // Not consumed as a node module — the other candidates still apply.
  }
  try {
    add(path.dirname(require.resolve("typescript")));
  } catch {
    // No resolvable `typescript` package from here.
  }
  // The project's own install, walking up from the workspace root.
  for (let dir = path.resolve(rootDir); ; ) {
    add(path.join(dir, "node_modules", "typescript", "lib"));
    const parent = path.dirname(dir);
    if (parent === dir) break;
    dir = parent;
  }
  return candidates.find((file) => fs.existsSync(file)) ?? null;
}

export class TsProject {
  private readonly service: ts.LanguageService;

  /** The default library the program checks against — null when none was
   * found, which is what [`typeEnvironmentError`] reports. */
  private readonly defaultLib: string | null;

  constructor(
    /** The text the server serves for a file, if it serves one — an open
     * buffer or the emitted TypeScript of an `.rl` module on disk. */
    private readonly getOpenDoc: (fileName: string) => OpenDoc | null,
    private readonly getOpenFileNames: () => string[],
    rootDir: string = process.cwd(),
    /** Path of the standard library module to fall back to when the project
     * does not resolve `"@rl/std"` itself. Null disables the fallback. */
    private readonly getStdModule: () => string | null = () => null,
    /** Overrides the default-library search: a path to use, or null to run
     * without one (tests of the broken-environment guard). */
    defaultLib?: string | null,
  ) {
    this.defaultLib =
      defaultLib === undefined ? findDefaultLib(rootDir) : defaultLib;
    const readFile = (fileName: string): string | undefined => {
      const open = this.getOpenDoc(fileName);
      if (open) return open.text;
      try {
        return fs.readFileSync(fileName, "utf8");
      } catch {
        return undefined;
      }
    };
    const fileExists = (fileName: string): boolean =>
      this.getOpenDoc(fileName) !== null || fs.existsSync(fileName);

    const resolutionHost: ts.ModuleResolutionHost = { fileExists, readFile };

    const host: ts.LanguageServiceHost = {
      getCompilationSettings: () => COMPILER_OPTIONS,
      getScriptFileNames: () => this.getOpenFileNames(),
      getScriptVersion: (fileName) => {
        const open = this.getOpenDoc(fileName);
        if (open) return `open-${open.version}`;
        try {
          return `disk-${fs.statSync(fileName).mtimeMs}`;
        } catch {
          return "missing";
        }
      },
      getScriptSnapshot: (fileName) => {
        const text = readFile(fileName);
        return text === undefined
          ? undefined
          : ts.ScriptSnapshot.fromString(text);
      },
      // .rl files are TypeScript as far as the service is concerned.
      getScriptKind: (fileName) => {
        if (fileName.endsWith(".tsx")) return ts.ScriptKind.TSX;
        if (fileName.endsWith(".jsx")) return ts.ScriptKind.JSX;
        if (/\.(m|c)?js$/.test(fileName)) return ts.ScriptKind.JS;
        if (fileName.endsWith(".json")) return ts.ScriptKind.JSON;
        return ts.ScriptKind.TS; // .ts, .rl
      },
      getCurrentDirectory: () => rootDir,
      // A verified path, or a bare name no host can resolve — in which case
      // `diagnosticsFor` stays silent instead of checking against nothing.
      getDefaultLibFileName: () => this.defaultLib ?? DEFAULT_LIB_NAME,
      fileExists,
      readFile,
      resolveModuleNameLiterals: (literals, containingFile, _, options) =>
        literals.map((literal) => {
          const spec = literal.text;
          if (spec === STD_SPECIFIER) {
            // A project that maps the specifier itself (tsconfig `paths` to
            // `rlc --types` output, or a package on disk) keeps its own
            // module; otherwise the compiler's copy stands in, so a buffer
            // that imports the std types is never inferred as `any` just
            // because the project has no mapping yet.
            const own = ts.resolveModuleName(
              spec,
              containingFile,
              options,
              resolutionHost,
            );
            if (own.resolvedModule) return own;
            const builtin = this.getStdModule();
            if (!builtin) return { resolvedModule: undefined };
            return {
              resolvedModule: {
                resolvedFileName: builtin,
                extension: ts.Extension.Ts,
                isExternalLibraryImport: false,
              },
            };
          }
          const relative = spec.startsWith("./") || spec.startsWith("../");
          if (relative && spec.endsWith(".rl")) {
            const resolved = path.resolve(path.dirname(containingFile), spec);
            if (!fileExists(resolved)) return { resolvedModule: undefined };
            return {
              resolvedModule: {
                resolvedFileName: resolved,
                extension: ts.Extension.Ts,
                isExternalLibraryImport: false,
              },
            };
          }
          return ts.resolveModuleName(
            spec,
            containingFile,
            options,
            resolutionHost,
          );
        }),
    };

    this.service = ts.createLanguageService(host, ts.createDocumentRegistry());
  }

  /**
   * Null when the type environment is sound; otherwise why it is not — a
   * message the server logs once before falling silent.
   *
   * Only a complete environment may produce user-facing diagnostics. A
   * program without global types reports `TS2488` on a plain tuple
   * destructuring and `TS2304` on `Error`/`JSON`, none of which is a real
   * error in the user's code — reporting them would break the error-layer
   * contract just as surely as reporting an error from generated code.
   */
  typeEnvironmentError(): string | null {
    if (this.defaultLib === null) {
      return (
        `TypeScript's default library (${DEFAULT_LIB_NAME}) was not found — ` +
        "type diagnostics are disabled. Reinstall the extension's " +
        "dependencies, or install `typescript` in the workspace."
      );
    }
    let loaded: ts.SourceFile | undefined;
    try {
      loaded = this.service.getProgram()?.getSourceFile(this.defaultLib);
    } catch {
      loaded = undefined;
    }
    return loaded
      ? null
      : `TypeScript's default library (${this.defaultLib}) could not be ` +
          "loaded — type diagnostics are disabled.";
  }

  /** The text the service currently sees for a file (open doc or disk). */
  fileText(fileName: string): string | null {
    const open = this.getOpenDoc(fileName);
    if (open) return open.text;
    try {
      return fs.readFileSync(fileName, "utf8");
    } catch {
      return null;
    }
  }

  /** TypeScript's definitions for the symbol at a UTF-16 offset. Never
   * throws — a service failure just means no delegated answer. */
  definitionsAt(fileName: string, offset: number): TsDefinition[] {
    let definitions: readonly ts.DefinitionInfo[] | undefined;
    try {
      definitions = this.service.getDefinitionAtPosition(fileName, offset);
    } catch {
      return [];
    }
    const out: TsDefinition[] = [];
    for (const d of definitions ?? []) {
      const fileText = this.fileText(d.fileName);
      if (fileText === null) continue;
      out.push({
        fileName: d.fileName,
        start: d.textSpan.start,
        length: d.textSpan.length,
        fileText,
      });
    }
    return out;
  }

  /** TypeScript's completions at a UTF-16 offset. */
  completionsAt(fileName: string, offset: number): TsCompletion[] {
    let completions: ts.CompletionInfo | undefined;
    try {
      completions = this.service.getCompletionsAtPosition(
        fileName,
        offset,
        undefined,
      );
    } catch {
      return [];
    }
    return (completions?.entries ?? []).map((e) => ({
      name: e.name,
      kind: e.kind,
      sortText: e.sortText,
    }));
  }

  /**
   * TypeScript's type errors for a file, in that file's own coordinates.
   *
   * Semantic diagnostics only: a syntax error means the text handed to the
   * service is not the pure TypeScript this is meant to check (a buffer
   * whose rl syntax did not compile, so the emitted text still carries it),
   * and every type error it would report from there is noise — the whole
   * file is dropped instead. A program missing its default library is
   * dropped for the same reason (see [`typeEnvironmentError`]).
   */
  diagnosticsFor(fileName: string): TsDiagnostic[] {
    if (this.typeEnvironmentError() !== null) return [];
    let diagnostics: ts.Diagnostic[];
    try {
      if (this.service.getSyntacticDiagnostics(fileName).length > 0) return [];
      diagnostics = this.service.getSemanticDiagnostics(fileName);
    } catch {
      return [];
    }
    const out: TsDiagnostic[] = [];
    for (const d of diagnostics) {
      if (d.start === undefined || d.length === undefined) continue;
      out.push({
        start: d.start,
        length: d.length,
        message: ts.flattenDiagnosticMessageText(d.messageText, " "),
        code: d.code,
        warning: d.category === ts.DiagnosticCategory.Warning,
      });
    }
    return out;
  }

  /** TypeScript's references for the symbol at a UTF-16 offset.
   * `isDefinition` is computed against the definition spans — the service's
   * own flag is unreliable in current TypeScript. */
  referencesAt(fileName: string, offset: number): TsReference[] {
    let references: ts.ReferenceEntry[] | undefined;
    try {
      references = this.service.getReferencesAtPosition(fileName, offset);
    } catch {
      return [];
    }
    const definitions = this.definitionsAt(fileName, offset);
    return this.toReferences(
      (references ?? []).map((r) => ({
        ...r,
        isDefinition: definitions.some(
          (d) => d.fileName === r.fileName && d.start === r.textSpan.start,
        ),
      })),
    );
  }

  /** TypeScript's rename locations for the symbol at a UTF-16 offset, or
   * null when the symbol cannot be renamed. The caller applies the new
   * name at every location. */
  renameAt(fileName: string, offset: number): TsReference[] | null {
    try {
      if (!this.service.getRenameInfo(fileName, offset, {}).canRename) {
        return null;
      }
      const locations = this.service.findRenameLocations(
        fileName,
        offset,
        false,
        false,
        {},
      );
      if (!locations) return null;
      return this.toReferences(
        locations.map((l) => ({ ...l, isDefinition: false })),
      );
    } catch {
      return null;
    }
  }

  private toReferences(
    entries: readonly (ts.DocumentSpan & { isDefinition?: boolean })[],
  ): TsReference[] {
    const out: TsReference[] = [];
    for (const entry of entries) {
      const fileText = this.fileText(entry.fileName);
      if (fileText === null) continue;
      out.push({
        fileName: entry.fileName,
        start: entry.textSpan.start,
        length: entry.textSpan.length,
        fileText,
        isDefinition: entry.isDefinition ?? false,
      });
    }
    return out;
  }

  /** The TypeScript-inferred type at a UTF-16 offset: its display name
   * (generic arguments stripped: `Option<string>` → `Option`) and the file
   * its symbol is declared in. When `within` is given, the type is taken
   * from the widest expression node contained in that span — the whole
   * scrutinee expression rather than just the token under the offset.
   * Never throws — null means no delegated answer. */
  typeAt(
    fileName: string,
    offset: number,
    within?: { start: number; end: number },
  ): TsTypeInfo | null {
    try {
      const program = this.service.getProgram();
      if (!program) return null;
      const sourceFile = program.getSourceFile(fileName);
      if (!sourceFile) return null;
      const checker = program.getTypeChecker();

      const deepest = (node: ts.Node): ts.Node | null => {
        if (offset < node.getStart(sourceFile) || offset >= node.getEnd()) {
          return null;
        }
        for (const child of node.getChildren(sourceFile)) {
          const hit = deepest(child);
          if (hit) return hit;
        }
        return node;
      };
      let node = deepest(sourceFile);
      if (!node || node === sourceFile) return null;
      if (within) {
        let widest = node;
        for (let p = node.parent; p && p !== sourceFile; p = p.parent) {
          if (p.getStart(sourceFile) >= within.start && p.getEnd() <= within.end) {
            widest = p;
          }
        }
        node = widest;
      }

      const type = checker.getTypeAtLocation(node);
      let name = checker.typeToString(type);
      const lt = name.indexOf("<");
      if (lt > 0) name = name.slice(0, lt);
      const symbol = type.aliasSymbol ?? type.getSymbol();
      const decl = symbol?.declarations?.[0];
      return { name, declFile: decl ? decl.getSourceFile().fileName : null };
    } catch {
      return null;
    }
  }

  /** TypeScript's quick-info (hover signature) at a UTF-16 offset. */
  quickInfoAt(fileName: string, offset: number): TsQuickInfo | null {
    let info: ts.QuickInfo | undefined;
    try {
      info = this.service.getQuickInfoAtPosition(fileName, offset);
    } catch {
      return null;
    }
    if (!info) return null;
    return {
      signature: ts.displayPartsToString(info.displayParts ?? []),
      documentation: ts.displayPartsToString(info.documentation ?? []),
      start: info.textSpan.start,
      length: info.textSpan.length,
    };
  }
}

/** Convert a UTF-16 offset in `text` to an LSP position. */
export function positionAt(
  text: string,
  offset: number,
): { line: number; character: number } {
  const clamped = Math.max(0, Math.min(offset, text.length));
  let line = 0;
  let lineStart = 0;
  for (let i = 0; i < clamped; i++) {
    if (text[i] === "\n") {
      line++;
      lineStart = i + 1;
    }
  }
  return { line, character: clamped - lineStart };
}
