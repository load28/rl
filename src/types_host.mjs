/* --------------------------------------------------------------------------
 * types_host.mjs — declaration emit for `rlc --types`, in memory.
 *
 * rlc embeds this file (`include_str!`) and runs it with `node`, handing it a
 * job on stdin: the compiled text of every `.rl` module, keyed by the path
 * that module would occupy if it had been written out. Hand-written `.ts`
 * files are NOT part of the job — the compiler host reads those from disk
 * where they already are, which is what keeps the build from copying a
 * single source file.
 *
 * `.rl` specifiers and `@rl/std` never hit the file system either: the host
 * resolves them to the in-memory modules directly, so no declaration shims
 * and no synthesized tsconfig are needed.
 *
 * Protocol (stdin → stdout, both JSON):
 *
 *   { cwd, compilerOptions, modules: [{ path, text }], std: { path, text } | null,
 *     sources: ["<hand-written>.ts"],
 *     rlModules: { "<source>.rl": "<virtual>.ts" },
 *     literalChecks: [{ module, start, end, covered: [...] }] }
 *
 *   { declarations: [{ path, text }],
 *     diagnostics: [{ file, line, col, code, message }],
 *     literalMissing: [{ index, missing }] }
 *
 * `literalChecks` is the typed half of rl's literal `match`: rlc names the
 * scrutinee of every wildcard-free literal match (as a UTF-16 range in the
 * emitted module) and the literals its arms cover; the checker here resolves
 * the scrutinee's type and, *only* when that type is a finite union of
 * string/number/boolean literals, reports what the arms miss. Anything less
 * definite (`string`, a type parameter, `"a" | string`) is left alone — a
 * missed diagnostic beats a false one.
 *
 * Exit codes: 0 = ran (type errors, if any, are in `diagnostics`),
 * 2 = TypeScript not installed, 3 = malformed job, 4 = the only resolvable
 * TypeScript has no JS compiler API (the TypeScript 7 native compiler).
 * ----------------------------------------------------------------------- */
import { createRequire } from "node:module";
import * as fs from "node:fs";
import * as path from "node:path";
import process from "node:process";

const job = JSON.parse(await readStdin());
if (job === null || typeof job !== "object" || !Array.isArray(job.modules)) {
  process.stderr.write("types_host: malformed job\n");
  process.exit(3);
}

const ts = resolveTypescript();
if (ts === null) {
  process.stderr.write("types_host: typescript not resolvable\n");
  process.exit(2);
}

const absolute = (p) => path.resolve(job.cwd, p);
const key = (p) => path.resolve(p);

/** Virtual modules: absolute path → text. */
const virtual = new Map();
for (const module of job.modules) {
  virtual.set(key(absolute(module.path)), module.text);
}
const stdPath = job.std ? key(absolute(job.std.path)) : null;
if (job.std) {
  virtual.set(stdPath, job.std.text);
}

/** `.rl` source path → the virtual module compiled from it. */
const rlModules = new Map(
  Object.entries(job.rlModules ?? {}).map(([source, virtualPath]) => [
    key(absolute(source)),
    key(absolute(virtualPath)),
  ]),
);

const { options, errors } = ts.convertCompilerOptionsFromJson(job.compilerOptions ?? {}, job.cwd);
if (errors.length > 0) {
  process.stdout.write(JSON.stringify({ declarations: [], diagnostics: errors.map(plain) }));
  process.exit(0);
}

const host = ts.createCompilerHost(options, true);
const readFile = host.readFile.bind(host);
const fileExists = host.fileExists.bind(host);
const getSourceFile = host.getSourceFile.bind(host);

host.fileExists = (file) => virtual.has(key(file)) || fileExists(file);
host.readFile = (file) => virtual.get(key(file)) ?? readFile(file);
host.getSourceFile = (file, languageVersion, onError, shouldCreate) => {
  const text = virtual.get(key(file));
  return text === undefined
    ? getSourceFile(file, languageVersion, onError, shouldCreate)
    : ts.createSourceFile(file, text, languageVersion, true, ts.ScriptKind.TS);
};

// The two specifiers TypeScript cannot resolve on its own. Everything else
// goes through the normal resolver, which reads the project from disk.
host.resolveModuleNames = (names, containingFile) =>
  names.map((name) => {
    if (stdPath !== null && name === "@rl/std") {
      return { resolvedFileName: stdPath, extension: ts.Extension.Ts };
    }
    if (name.endsWith(".rl")) {
      const target = key(path.resolve(path.dirname(containingFile), name));
      const mapped = rlModules.get(target);
      return mapped === undefined
        ? undefined
        : { resolvedFileName: mapped, extension: ts.Extension.Ts };
    }
    return ts.resolveModuleName(name, containingFile, options, host).resolvedModule;
  });

// Root names: the in-memory modules plus the project's own `.ts` files by
// path. Including the latter is what keeps type errors in hand-written code
// visible — they are read from disk, never copied.
const roots = [...virtual.keys(), ...(job.sources ?? []).map((p) => key(absolute(p)))];
const program = ts.createProgram(roots, options, host);

const declarations = [];
for (const module of [...job.modules, ...(job.std ? [job.std] : [])]) {
  const file = absolute(module.path);
  const sourceFile = program.getSourceFile(file);
  if (sourceFile === undefined) continue;
  program.emit(
    sourceFile,
    (name, text) => {
      if (name.endsWith(".d.ts")) declarations.push({ path: module.path, text });
    },
    undefined,
    /* emitOnlyDtsFiles */ true,
  );
}

const diagnostics = ts.getPreEmitDiagnostics(program).map(plain);
const literalMissing = checkLiteralMatches(program, job.literalChecks ?? []);
process.stdout.write(JSON.stringify({ declarations, diagnostics, literalMissing }));

/**
 * Typed exhaustiveness for literal matches — see the protocol note above.
 * A check the checker cannot answer definitely is silently skipped.
 */
function checkLiteralMatches(program, checks) {
  if (checks.length === 0) return [];
  const checker = program.getTypeChecker();
  const out = [];
  for (const [index, check] of checks.entries()) {
    const file = program.getSourceFile(absolute(check.module));
    if (file === undefined) continue;
    const node = widestNodeIn(file, check.start, check.end);
    if (node === null) continue;
    const members = finiteLiterals(checker.getTypeAtLocation(node));
    if (members === null || members.length === 0) continue;
    const covered = new Set(check.covered.map(literalKey));
    const missing = members.filter((m) => !covered.has(literalKey(m)));
    if (missing.length > 0) {
      out.push({ index, missing: missing.map(display).join(", ") });
    }
  }
  return out;
}

/**
 * The widest node fully inside `[start, end)` — the scrutinee expression.
 * Asking for the node *at* `start` would answer about `obj` in `obj.field`;
 * the emitted scrutinee occupies exactly this range, so the widest node in
 * it is the whole expression.
 */
function widestNodeIn(file, start, end) {
  let best = null;
  const visit = (node) => {
    const from = node.getStart(file);
    const to = node.getEnd();
    if (to <= start || from >= end) return;
    if (from >= start && to <= end) {
      if (best === null || to - from > best.getEnd() - best.getStart(file)) best = node;
    }
    ts.forEachChild(node, visit);
  };
  ts.forEachChild(file, visit);
  return best;
}

/**
 * The type as a finite set of string/number/boolean literals, or null when
 * it is anything else. `boolean` qualifies: TypeScript models it as the
 * union `true | false`, which is finite. Enum member types do not — their
 * members are written `E.A`, not as literal patterns, so reporting one
 * missing would be a false positive.
 */
function finiteLiterals(type) {
  const parts = type.isUnion() ? type.types : [type];
  const out = [];
  for (const part of parts) {
    if (part.flags & ts.TypeFlags.EnumLike) return null;
    if (part.flags & ts.TypeFlags.StringLiteral) out.push({ kind: "string", value: part.value });
    else if (part.flags & ts.TypeFlags.NumberLiteral) out.push({ kind: "number", value: part.value });
    else if (part.flags & ts.TypeFlags.BooleanLiteral) {
      out.push({ kind: "boolean", value: part.intrinsicName === "true" });
    } else return null;
  }
  return out;
}

/** A literal's comparison key — kind included, so `1` is not `"1"`. */
function literalKey(literal) {
  if (literal !== null && typeof literal === "object") {
    return `${literal.kind}:${String(literal.value)}`;
  }
  return `${typeof literal}:${String(literal)}`;
}

/** A missing literal as it appears in the rl diagnostic. */
function display(literal) {
  return literal.kind === "string" ? JSON.stringify(literal.value) : String(literal.value);
}

/** A diagnostic reduced to what rlc reports. */
function plain(diagnostic) {
  const message = ts.flattenDiagnosticMessageText(diagnostic.messageText, " ");
  if (diagnostic.file === undefined || diagnostic.start === undefined) {
    return { file: null, line: 0, col: 0, code: diagnostic.code, message };
  }
  const { line, character } = diagnostic.file.getLineAndCharacterOfPosition(diagnostic.start);
  return {
    file: path.relative(job.cwd, diagnostic.file.fileName),
    line: line + 1,
    col: character + 1,
    code: diagnostic.code,
    message,
  };
}

async function readStdin() {
  const chunks = [];
  for await (const chunk of process.stdin) chunks.push(chunk);
  return Buffer.concat(chunks).toString("utf8");
}

/**
 * TypeScript, preferably the version the project pinned.
 *
 * Falls back to the package that owns a `tsc` on PATH, which is how a
 * globally installed TypeScript is found — the same setups the previous
 * tsc-binary lookup supported.
 *
 * TypeScript 7 is the native (Go) compiler: its npm package exposes no JS
 * compiler API (`require("typescript")` exports only `version`), so this
 * host cannot drive it. Such a package is skipped in favor of a usable one
 * further down the list; if nothing usable exists, exit 4 tells rlc to
 * explain the situation instead of crashing on a missing function.
 */
function resolveTypescript() {
  const bases = [path.join(job.cwd, "index.js"), ...globalTypescriptBases()];
  let apiless = null;
  for (const base of bases) {
    try {
      const mod = createRequire(base)("typescript");
      if (typeof mod.convertCompilerOptionsFromJson === "function") {
        return mod;
      }
      apiless ??= mod?.version ?? "unknown version";
    } catch {
      // try the next base
    }
  }
  if (apiless !== null) {
    process.stderr.write(
      `types_host: typescript ${apiless} has no JS compiler API\n`,
    );
    process.exit(4);
  }
  return null;
}

/** Real paths of every `tsc` on PATH — each one sits inside its package. */
function globalTypescriptBases() {
  const entries = (process.env.PATH ?? "").split(path.delimiter).filter(Boolean);
  const bases = [];
  for (const dir of entries) {
    for (const name of ["tsc", "tsc.cmd"]) {
      const candidate = path.join(dir, name);
      try {
        if (fs.existsSync(candidate)) bases.push(fs.realpathSync(candidate));
      } catch {
        // unreadable PATH entry
      }
    }
  }
  return bases;
}
