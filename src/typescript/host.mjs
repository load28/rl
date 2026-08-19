/* --------------------------------------------------------------------------
 * host.mjs — the TypeScript 7 native backend's host process.
 *
 * rlc embeds this file (`include_str!`) and runs it with `node`. It is the
 * only place that knows the TypeScript API: it opens ONE real TypeScript
 * project over a layered file system where every `.rl` file appears as the
 * ordinary TypeScript it lowers to, and answers rl's semantic questions
 * against that project's checker.
 *
 * The API comes from the typescript-go tree rlc was pointed at
 * (`RLC_TSGO_ROOT`/`RLC_TSGO_BIN`, see `native.rs`): the JS client and the
 * `tsgo` binary speak an unversioned MessagePack protocol and must come from
 * the same build, so both are resolved from that one tree.
 *
 * Protocol (stdin → stdout, both JSON, one object each):
 *
 *   { tsgoBin, apiModule, cwd, tsconfig,
 *     modules: [{ path, text }],          // lowered .rl → virtual .ts
 *     literalChecks: [{ module, start, end, covered: [...] }],
 *     valChecks: [{ module, start, end, method }] }
 *
 *   { diagnostics: [{ file, start, end, code, message }],
 *     literalMissing: [{ index, missing }],
 *     valMutations: [{ index, receiver, declaredIn }] }
 *
 * `start`/`end` are UTF-16 code-unit offsets — TypeScript's own coordinate
 * space. Mapping them back to `.rl` byte positions is rlc's job (`mapper`),
 * not this host's.
 *
 * Exit codes: 0 = ran (type errors, if any, are in `diagnostics`),
 * 2 = the TypeScript API could not be loaded, 3 = malformed job.
 * ----------------------------------------------------------------------- */
import * as fs from "node:fs";
import * as path from "node:path";
import process from "node:process";

/** Reads the whole of stdin. */
function readStdin() {
  const chunks = [];
  const buf = Buffer.alloc(65536);
  while (true) {
    let n;
    try {
      n = fs.readSync(0, buf, 0, buf.length, null);
    } catch (e) {
      if (e.code === "EAGAIN") continue;
      if (e.code === "EOF") break;
      throw e;
    }
    if (n === 0) break;
    chunks.push(Buffer.from(buf.subarray(0, n)));
  }
  return Buffer.concat(chunks).toString("utf8");
}

/**
 * The file system TypeScript sees: rlc's lowered modules layered over the
 * real disk. A `.rl` file is invisible to TypeScript; the `.ts` it lowers to
 * takes its place, including in directory listings, so a `tsconfig.json`
 * that globs a directory picks it up exactly as it would a hand-written file.
 */
function layeredFileSystem(modules) {
  const files = new Map(modules.map((m) => [m.path, m.text]));
  const dirs = new Set();
  for (const p of files.keys()) {
    for (let d = path.dirname(p); d && d !== path.dirname(d); d = path.dirname(d)) {
      dirs.add(d);
    }
  }
  return {
    fileExists: (f) => (files.has(f) ? true : undefined),
    // `undefined` falls back to the real disk; `null` would mean "absent".
    readFile: (f) => (files.has(f) ? files.get(f) : undefined),
    directoryExists: (d) => (dirs.has(d) ? true : undefined),
    getAccessibleEntries: (d) => {
      let real = { files: [], directories: [] };
      try {
        for (const e of fs.readdirSync(d, { withFileTypes: true })) {
          if (e.isDirectory()) real.directories.push(e.name);
          else real.files.push(e.name);
        }
      } catch {
        if (!dirs.has(d)) return undefined;
      }
      const here = [...files.keys()].filter((f) => path.dirname(f) === d);
      const names = new Set(real.files.map((f) => f));
      for (const f of here) {
        const base = path.basename(f);
        if (!names.has(base)) real.files.push(base);
      }
      // The sources rlc lowered are not TypeScript; hide them so no tool
      // tries to read `.rl` as TypeScript.
      real.files = real.files.filter((f) => !f.endsWith(".rl"));
      return real;
    },
  };
}

function fail(code, message) {
  process.stderr.write(message + "\n");
  process.exit(code);
}

async function main() {
  let job;
  try {
    job = JSON.parse(readStdin());
  } catch (e) {
    fail(3, "rlc host: malformed job: " + e.message);
  }

  let API, createVirtualFileSystem;
  try {
    ({ API } = await import(job.apiModule));
  } catch (e) {
    fail(2, "rlc host: cannot load the TypeScript API from " + job.apiModule + ": " + e.message);
  }

  const api = new API({
    cwd: job.cwd,
    tsserverPath: job.tsgoBin,
    fs: layeredFileSystem(job.modules ?? []),
  });

  const out = { diagnostics: [], literalMissing: [], valMutations: [] };
  try {
    const snapshot = api.updateSnapshot({ openProjects: [job.tsconfig] });
    const project = snapshot.getProject(job.tsconfig);
    if (!project) fail(3, "rlc host: no project for " + job.tsconfig);

    for (const module of job.modules ?? []) {
      for (const d of project.program.getSemanticDiagnostics(module.path)) {
        out.diagnostics.push({
          file: d.fileName ?? module.path,
          start: d.pos,
          end: d.end,
          code: d.code,
          message: d.text,
        });
      }
    }

    const checker = project.checker;

    // Literal-match exhaustiveness: the type TypeScript computes AT the
    // scrutinee — narrowing included — decides what the arms miss.
    (job.literalChecks ?? []).forEach((check, index) => {
      const type = checker.getTypeAtPosition(check.module, check.start);
      const missing = missingLiterals(type, check.covered);
      if (missing) out.literalMissing.push({ index, missing });
    });

    // `val` mutation: what the method resolves to decides, never its name.
    (job.valChecks ?? []).forEach((check, index) => {
      const symbol = checker.getSymbolAtPosition(check.module, check.start);
      if (!symbol) return; // `any`, unresolved — never a verdict
      const declaredIn = (symbol.declarations ?? [])
        .map((d) => String(d.path ?? ""))
        .filter(Boolean);
      if (declaredIn.length === 0) return;
      out.valMutations.push({ index, receiver: symbol.name, declaredIn });
    });
  } finally {
    api.close();
  }

  process.stdout.write(JSON.stringify(out));
}

/**
 * The literals of `type` that `covered` does not, or `null` when the type is
 * not a definite finite union of literals. Anything less definite — `string`,
 * a type parameter, `"a" | string` — is left alone: a missed diagnostic
 * beats a false one.
 */
function missingLiterals(type, covered) {
  if (!type) return null;
  const constituents = type.isUnionType?.() ? type.getTypes() : [type];
  const values = [];
  for (const c of constituents) {
    const value = literalValue(c);
    if (value === undefined) return null;
    values.push(value);
  }
  const seen = new Set(covered.map((c) => JSON.stringify(c)));
  const missing = values.filter((v) => !seen.has(JSON.stringify(v)));
  return missing.length > 0 ? missing : null;
}

/** The value of a literal type, or `undefined` when it is not one. */
function literalValue(type) {
  const v = type.value;
  if (typeof v === "string" || typeof v === "number" || typeof v === "boolean") return v;
  return undefined;
}

await main();
