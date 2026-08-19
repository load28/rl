/* --------------------------------------------------------------------------
 * tsgo_host.mjs — experimental TypeScript 7 native backend for `rlc --types`.
 *
 * This script intentionally accepts the same JSON job shape as `types_host.mjs`
 * and returns the same result shape. It is selected only by
 * `RLC_TS_BACKEND=tsgo`; the legacy JS Compiler API host remains the default
 * until feature parity is proven.
 *
 * It talks to the current `microsoft/typescript-go` source API through the
 * `@typescript/source` condition. Set `RLC_TSGO_ROOT` to the local
 * typescript-go checkout; otherwise it tries `../typescript-go` from cwd.
 * ----------------------------------------------------------------------- */
import * as fs from "node:fs";
import * as path from "node:path";
import process from "node:process";
import { pathToFileURL } from "node:url";

const BUILTIN_MUTATORS = new Map(
  [
    ["Array", ["push", "pop", "shift", "unshift", "splice", "sort", "reverse", "fill", "copyWithin"]],
    ["Map", ["set", "delete", "clear"]],
    ["Set", ["add", "delete", "clear"]],
    ["WeakMap", ["set", "delete"]],
    ["WeakSet", ["add", "delete"]],
    ...[
      "Int8Array", "Uint8Array", "Uint8ClampedArray", "Int16Array", "Uint16Array",
      "Int32Array", "Uint32Array", "Float32Array", "Float64Array",
      "BigInt64Array", "BigUint64Array",
    ].map((name) => [name, ["set", "sort", "reverse", "fill", "copyWithin"]]),
  ].map(([name, methods]) => [name, new Set(methods)]),
);
const TYPE_FLAG_ENUM_LIKE = (1 << 15) | (1 << 16);

const job = JSON.parse(await readStdin());
if (job === null || typeof job !== "object" || !Array.isArray(job.modules)) {
  process.stderr.write("tsgo_host: malformed job\n");
  process.exit(3);
}

const cwd = path.resolve(job.cwd ?? ".");
const tsgoRoot = resolveTsgoRoot();
const fromTsgo = (relative) => pathToFileURL(path.join(tsgoRoot, relative)).href;
let API;
let EmitOnly;
try {
  ({ API, EmitOnly } = await import(fromTsgo("_packages/native-preview/src/api/sync/api.ts")));
} catch (error) {
  process.stderr.write(`tsgo_host: failed to load tsgo source API from ${tsgoRoot}: ${error.message}\n`);
  process.exit(5);
}

const absolute = (p) => path.resolve(cwd, p);
const slash = (p) => p.replaceAll(path.sep, "/");

const overlay = new Map();
const moduleByAbs = new Map();
for (const module of job.modules) {
  const file = slash(absolute(module.path));
  overlay.set(file, module.text);
  moduleByAbs.set(file, module.path);
}

for (const [source, virtualPath] of Object.entries(job.rlModules ?? {})) {
  const sourceFile = slash(absolute(source));
  const virtualFile = slash(absolute(virtualPath));
  const text = overlay.get(virtualFile);
  if (text !== undefined) {
    overlay.set(sourceFile, text);
    overlay.set(arbitraryExtensionDeclarationPath(sourceFile), rlImportShim(sourceFile, virtualFile, text));
  }
}

let stdAbs = null;
if (job.std) {
  stdAbs = slash(absolute(job.std.path));
  overlay.set(stdAbs, job.std.text);
  moduleByAbs.set(stdAbs, job.std.path);
}

const roots = [
  ...job.modules.map((module) => slash(absolute(module.path))),
  ...(job.sources ?? []).map((source) => slash(absolute(source))),
  ...(job.std ? [stdAbs] : []),
];
const compilerOptions = {
  ...(job.compilerOptions ?? {}),
  allowArbitraryExtensions: true,
  baseUrl: job.compilerOptions?.baseUrl ?? cwd,
};
if (job.std) {
  compilerOptions.paths = {
    ...(compilerOptions.paths ?? {}),
    "@rl/std": [path.relative(cwd, stdAbs).replaceAll(path.sep, "/")],
  };
}

const configPath = slash(path.join(cwd, "__rl_tsgo_tsconfig.json"));
overlay.set(configPath, JSON.stringify({
  compilerOptions,
  files: roots,
}));

const api = new API({
  cwd,
  fs: overlayFileSystem(overlay),
});

try {
  const snapshot = api.updateSnapshot({ openProject: configPath });
  const project = snapshot.getProject(configPath);
  if (!project) {
    throw new Error(`tsgo did not open ${configPath}`);
  }

  const diagnostics = [
    ...project.program.getConfigFileParsingDiagnostics(),
    ...project.program.getSyntacticDiagnostics(),
    ...project.program.getSemanticDiagnostics(),
  ].map((diagnostic) => plainDiagnostic(diagnostic, overlay));

  const literalMissing = checkLiteralMatches(project, job.literalChecks ?? []);
  const valMutations = checkValMutations(project, job.valChecks ?? []);
  const declarations = declarationEmit(project, job.modules, job.std);

  process.stdout.write(JSON.stringify({
    declarations,
    diagnostics,
    literalMissing,
    valMutations,
  }));
} catch (error) {
  process.stderr.write(`tsgo_host: ${error.stack ?? error.message}\n`);
  process.exit(1);
} finally {
  api.close();
}

function overlayFileSystem(files) {
  return {
    readFile(fileName) {
      const key = slash(path.resolve(fileName));
      if (files.has(key)) return files.get(key);
      return undefined;
    },
    fileExists(fileName) {
      const key = slash(path.resolve(fileName));
      return files.has(key) ? true : undefined;
    },
    directoryExists(directoryName) {
      const key = slash(path.resolve(directoryName));
      if ([...files.keys()].some((file) => file.startsWith(`${key}/`))) return true;
      return undefined;
    },
    getAccessibleEntries(directoryName) {
      const key = slash(path.resolve(directoryName));
      const prefix = key.endsWith("/") ? key : `${key}/`;
      const filesOut = new Set();
      const dirsOut = new Set();
      for (const file of files.keys()) {
        if (!file.startsWith(prefix)) continue;
        const rest = file.slice(prefix.length);
        if (rest.length === 0) continue;
        const slashAt = rest.indexOf("/");
        if (slashAt === -1) filesOut.add(rest);
        else dirsOut.add(rest.slice(0, slashAt));
      }
      if (filesOut.size === 0 && dirsOut.size === 0) return undefined;
      return {
        files: [...filesOut].sort(),
        directories: [...dirsOut].sort(),
      };
    },
    realpath(fileName) {
      return slash(path.resolve(fileName));
    },
  };
}

function checkLiteralMatches(project, checks) {
  const out = [];
  for (const [index, check] of checks.entries()) {
    const file = slash(absolute(check.module));
    const type = project.checker.getTypeAtPosition(file, check.start);
    if (!type) continue;
    const members = finiteLiterals(type);
    if (members === null || members.length === 0) continue;
    const covered = new Set(check.covered.map(literalKey));
    const missing = members.filter((member) => !covered.has(literalKey(member)));
    if (missing.length > 0) {
      out.push({ index, missing: missing.map(displayLiteral).join(", ") });
    }
  }
  return out;
}

function finiteLiterals(type) {
  const parts = type.isUnionType() ? type.getTypes() : [type];
  const out = [];
  for (const part of parts) {
    if (part.flags & TYPE_FLAG_ENUM_LIKE) return null;
    if (part.isStringLiteralType()) out.push({ kind: "string", value: part.value });
    else if (part.isNumberLiteralType()) out.push({ kind: "number", value: part.value });
    else if (part.isBooleanLiteralType()) out.push({ kind: "boolean", value: part.value });
    else return null;
  }
  return out;
}

function checkValMutations(project, checks) {
  const out = [];
  for (const [index, check] of checks.entries()) {
    const file = slash(absolute(check.module));
    const symbol = project.checker.getSymbolAtPosition(file, check.start);
    const receiver = builtinMutator(project, symbol, check.method);
    if (receiver !== null) out.push({ index, receiver });
  }
  return out;
}

function builtinMutator(project, symbol, method) {
  if (!symbol || symbol.name !== method || symbol.declarations.length === 0) {
    return null;
  }
  const receivers = new Set();
  for (const handle of symbol.declarations) {
    const declaration = handle.resolve(project);
    if (!declaration) return null;
    const sourceMeta = project.program.getSourceFileMetadata(handle.path);
    if (!sourceMeta?.isDefaultLibrary) return null;
    const owner = declaration.parent;
    const ownerName = owner?.name?.text;
    if (typeof ownerName !== "string") return null;
    const mutators = BUILTIN_MUTATORS.get(ownerName);
    if (!mutators?.has(method)) return null;
    receivers.add(ownerName);
  }
  return [...receivers].join(" | ");
}

function declarationEmit(project, modules, std) {
  const emit = project.program.emitToString(EmitOnly.OnlyDts);
  const out = [];
  const wanted = new Map();
  for (const module of [...modules, ...(std ? [std] : [])]) {
    const abs = slash(absolute(module.path));
    wanted.set(abs.replace(/\.ts$/, ".d.ts"), module.path);
  }
  for (const [fileName, file] of emit.outputFiles) {
    const sourcePath = wanted.get(slash(fileName));
    if (sourcePath) out.push({ path: sourcePath, text: file.text });
  }
  return out;
}

function arbitraryExtensionDeclarationPath(file) {
  return file.replace(/\.rl$/, ".d.rl.ts");
}

function rlImportShim(sourceFile, virtualFile, text) {
  let specifier = path.relative(path.dirname(sourceFile), virtualFile).replaceAll(path.sep, "/");
  if (!specifier.startsWith(".")) specifier = `./${specifier}`;
  const target = JSON.stringify(specifier.replace(/\.ts$/, ""));
  const defaultExport = hasDefaultExport(text) ? `export { default } from ${target};\n` : "";
  return `${defaultExport}export * from ${target};\n`;
}

function hasDefaultExport(text) {
  return /\bexport\s+default\b/.test(text) || /\bexport\s*{[^}]*\bdefault\b[^}]*}/.test(text);
}

function plainDiagnostic(diagnostic, files) {
  if (diagnostic.fileName === undefined || diagnostic.pos === undefined) {
    return { file: null, line: 0, col: 0, code: diagnostic.code, message: diagnostic.text };
  }
  const file = slash(diagnostic.fileName);
  const text = files.get(file) ?? fs.readFileSync(file, "utf8");
  const { line, col } = lineColUtf16(text, diagnostic.pos);
  return {
    file: path.relative(cwd, file),
    line,
    col,
    code: diagnostic.code,
    message: diagnostic.text,
  };
}

function lineColUtf16(text, offset) {
  let line = 1;
  let lineStart = 0;
  for (let i = 0; i < text.length && i < offset; i++) {
    if (text.charCodeAt(i) === 10) {
      line++;
      lineStart = i + 1;
    }
  }
  return {
    line,
    col: text.slice(lineStart, offset).length + 1,
  };
}

function literalKey(literal) {
  if (literal !== null && typeof literal === "object") {
    return `${literal.kind}:${String(literal.value)}`;
  }
  return `${typeof literal}:${String(literal)}`;
}

function displayLiteral(literal) {
  return literal.kind === "string" ? JSON.stringify(literal.value) : String(literal.value);
}

async function readStdin() {
  const chunks = [];
  for await (const chunk of process.stdin) chunks.push(chunk);
  return Buffer.concat(chunks).toString("utf8");
}

function resolveTsgoRoot() {
  const explicit = process.env.RLC_TSGO_ROOT;
  const candidates = [
    explicit,
    path.resolve(cwd, "../typescript-go"),
    "/Users/seominyeong/orca/typescript-go",
  ].filter(Boolean);
  for (const candidate of candidates) {
    const api = path.join(candidate, "_packages/native-preview/src/api/sync/api.ts");
    const exe = path.join(candidate, "built/local/tsgo");
    if (fs.existsSync(api) && fs.existsSync(exe)) return candidate;
  }
  process.stderr.write(
    "tsgo_host: typescript-go checkout not found; set RLC_TSGO_ROOT and build it with `npm run build`\n",
  );
  process.exit(2);
}
