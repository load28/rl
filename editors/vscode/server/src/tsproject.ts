/* --------------------------------------------------------------------------
 * TypeScript language-service delegation.
 *
 * An .rl file is TypeScript plus four constructs, so every symbol the rl
 * analysis does not own (variables, functions, types, imported values, ...)
 * is answered by the real TypeScript language service running over the rl
 * sources themselves. The TS parser's error recovery tolerates the rl
 * constructs; the passthrough region — most of the file — resolves exactly
 * as it would in a .ts file. TS *diagnostics* are never surfaced: the
 * compiler (`rlc --check`) stays the single source of truth for errors.
 *
 * Relative `.rl` specifiers are resolved by a custom module resolver that
 * points them at the .rl file (served as TS content); everything else goes
 * through standard TypeScript resolution.
 * ----------------------------------------------------------------------- */
import * as fs from "fs";
import * as path from "path";
import * as ts from "typescript";

/** A file the editor currently has open, served instead of the disk copy. */
export interface OpenDoc {
  text: string;
  version: number;
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

/** One reference or rename location. */
export interface TsReference extends TsDefinition {
  isDefinition: boolean;
}

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

export class TsProject {
  private readonly service: ts.LanguageService;

  constructor(
    private readonly getOpenDoc: (fileName: string) => OpenDoc | null,
    private readonly getOpenFileNames: () => string[],
    rootDir: string = process.cwd(),
  ) {
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
      getDefaultLibFileName: (options) => ts.getDefaultLibFilePath(options),
      fileExists,
      readFile,
      resolveModuleNameLiterals: (literals, containingFile, _, options) =>
        literals.map((literal) => {
          const spec = literal.text;
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
