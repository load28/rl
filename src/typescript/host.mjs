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
 * The host is a **session**: one line of JSON in, one line of JSON out, for
 * as long as stdin stays open. The compiler is started once and the project
 * is opened once; a later request only says what changed. That is what makes
 * a watch or an editor viable — reopening a real project per keystroke is
 * not.
 *
 *   open   { tsgoBin (nullable), apiModule, cwd, tsconfig (nullable) }
 *       →  { ok: true }
 *
 *   ask    { modules: [{ path, text }],   // lowered .rl → virtual .ts
 *            literalChecks: [{ module, start, covered: [...] }],
 *            tagChecks: [{ module, start, covered: [...] }],
 *            symbolChecks: [{ module, start }],
 *            emitDeclarations: boolean }
 *       →  { diagnostics: [{ file, start, end, code, message }],
 *            literalMissing: [{ index, missing }],
 *            tagMissing: [{ index, missing }],
 *            symbols: [{ index, id, name, builtin }],
 *            declarations: [{ path, text }] }
 *
 * An `ask` may also answer `{ error: "..." }`, which fails that request
 * without ending the session. EOF on stdin ends it.
 *
 * `start`/`end` are UTF-16 code-unit offsets — TypeScript's own coordinate
 * space. Mapping them back to `.rl` byte positions is rlc's job (`mapper`),
 * not this host's.
 *
 * Inside one `ask` the per-position questions are batched by module through
 * the checker's array overloads (`getTypesAtPositions`,
 * `getSymbolsAtPositions`, `getTypeOfSymbol[]`), falling back to one call
 * per question on a client without them — see `batched`. The protocol above
 * and the meaning of every answer are the same either way.
 *
 * Exit codes: 0 = ran (type errors, if any, are in `diagnostics`),
 * 2 = the TypeScript API could not be loaded, 3 = malformed job,
 * 5 = the resolved TypeScript has no declaration emit API.
 * ----------------------------------------------------------------------- */
import * as fs from "node:fs";
import * as path from "node:path";
import process from "node:process";

/**
 * Writes a whole answer line to stdout **synchronously**.
 *
 * `process.stdout.write` queues anything past the pipe's buffer (64 KB on
 * Linux) for the event loop to flush — and this host then blocks the event
 * loop in `readSync` waiting for the next request, which never comes
 * because the client is still waiting for the rest of the answer. A
 * project with a few hundred diagnostics crosses 64 KB, so the flush has
 * to happen before the loop turns around: partial writes and EAGAIN are
 * retried until every byte is out.
 */
function writeLine(text) {
  const buffer = Buffer.from(text + "\n", "utf8");
  let pos = 0;
  while (pos < buffer.length) {
    try {
      pos += fs.writeSync(1, buffer, pos, buffer.length - pos);
    } catch (e) {
      if (e.code === "EAGAIN") continue;
      throw e;
    }
  }
}

/**
 * Reads stdin one line at a time, blocking. The client waits for each
 * answer before sending the next request, so a single buffer is enough.
 */
function lineReader() {
  const buf = Buffer.alloc(65536);
  let pending = "";
  return function readLine() {
    while (true) {
      const newline = pending.indexOf("\n");
      if (newline >= 0) {
        const line = pending.slice(0, newline);
        pending = pending.slice(newline + 1);
        return line;
      }
      let n;
      try {
        n = fs.readSync(0, buf, 0, buf.length, null);
      } catch (e) {
        if (e.code === "EAGAIN") continue;
        if (e.code === "EOF") n = 0;
        else throw e;
      }
      if (n === 0) return pending.length > 0 ? ((p) => ((pending = ""), p))(pending) : null;
      pending += buf.subarray(0, n).toString("utf8");
    }
  };
}

/**
 * The file system TypeScript sees: rlc's lowered modules layered over the
 * real disk. A `.rl` file is invisible to TypeScript; the `.ts` it lowers to
 * takes its place, including in directory listings, so a `tsconfig.json`
 * that globs a directory picks it up exactly as it would a hand-written file.
 */
function layeredFileSystem(files, dirs) {
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
  const readLine = lineReader();
  let open;
  try {
    open = JSON.parse(readLine());
  } catch (e) {
    fail(3, "rlc host: malformed open request: " + e.message);
  }

  let API;
  try {
    ({ API } = await import(open.apiModule));
  } catch (e) {
    fail(2, "rlc host: cannot load the TypeScript API from " + open.apiModule + ": " + e.message);
  }

  // The modules rlc serves, mutated in place between requests: the file
  // system the compiler holds is this one, so an updated text is visible as
  // soon as the snapshot is told the file changed.
  const files = new Map();
  const dirs = new Set();
  const api = new API({
    cwd: open.cwd,
    // Omitted for an installed package: the client runs the executable
    // shipped beside it, which is the one it was built against.
    ...(open.tsgoBin ? { tsserverPath: open.tsgoBin } : {}),
    fs: layeredFileSystem(files, dirs),
  });
  writeLine(JSON.stringify({ ok: true }));

  let opened = false;
  try {
    while (true) {
      const line = readLine();
      if (line === null) break;
      let answer;
      try {
        answer = handle(JSON.parse(line));
        opened = true;
      } catch (e) {
        answer = { error: String((e && e.stack) || e) };
      }
      writeLine(JSON.stringify(answer));
    }
  } finally {
    api.close();
  }

  /** One `ask`: refresh the served modules, then answer every question. */
  function handle(job) {
    const out = {
      diagnostics: [],
      literalMissing: [],
      tagMissing: [],
      symbols: [],
      declarations: [],
    };
    const changes = serve(files, dirs, job.modules ?? []);
    // With a `tsconfig.json` the project is the user's own. Without one —
    // a workspace that never configured TypeScript — the modules are opened
    // directly and the compiler infers a project for them, which is what an
    // editor does for a loose file.
    //
    // Opening happens once; from then on the snapshot is only told what
    // changed, which is the whole point of keeping this process alive.
    const paths = (job.modules ?? []).map((m) => m.path);
    // Without a configuration the project is whatever is opened, so the
    // hand-written `.ts` files come along: one nothing imports is still the
    // user's code, and `rlc --types src` is expected to check it.
    const params = opened
      ? { fileChanges: changes }
      : open.tsconfig
        ? { openProjects: [open.tsconfig] }
        : { openFiles: [...paths, ...(job.sources ?? [])] };
    const snapshot = api.updateSnapshot(params);
    const project = open.tsconfig
      ? snapshot.getProject(open.tsconfig)
      : paths.map((p) => snapshot.getDefaultProjectForFile(p)).find(Boolean);
    if (!project) {
      throw new Error("no project for " + (open.tsconfig ?? paths[0] ?? "<nothing>"));
    }

    // The whole program, not just the lowered modules: a hand-written `.ts`
    // and an `.rl` are in one project, so an error in either is this run's to
    // report. Which file it lands in decides how it is positioned, and that
    // is rlc's half.
    for (const d of project.program.getSemanticDiagnostics()) {
      if (!d.fileName) continue;
      out.diagnostics.push({
        file: d.fileName,
        start: d.pos,
        end: d.end,
        code: d.code,
        message: d.text,
      });
    }

    const checker = project.checker;
    /**
     * Whether a declaration lives in one of TypeScript's own lib files.
     * Answered from the program's per-file metadata when the client has it
     * — a small, cached query — rather than by fetching the whole source
     * file just to ask about it.
     */
    const isDefaultLibrary = (declaration) => {
      if (!declaration.path) return false;
      if (typeof project.program.getSourceFileMetadata === "function") {
        const metadata = project.program.getSourceFileMetadata(String(declaration.path));
        return metadata ? metadata.isDefaultLibrary === true : false;
      }
      const file = project.program.getSourceFile(String(declaration.path));
      return file ? project.program.isSourceFileDefaultLibrary(file) : false;
    };

    // The per-position questions, batched by module: the checker's position
    // APIs take an array of positions for one file, so a project's worth of
    // questions costs one round trip per module per kind instead of one per
    // question. The batch is an implementation detail of this host — the
    // job's own indices are what every answer is keyed by, and `perModule`
    // scatters each answer back onto the entry it was asked for.

    // Literal- and tag-match exhaustiveness share one type question: the
    // type TypeScript computes AT the scrutinee — narrowing included —
    // decides what the arms miss.
    const typeChecks = [
      ...(job.literalChecks ?? []).map((check, index) => ({ check, index, tag: false })),
      ...(job.tagChecks ?? []).map((check, index) => ({ check, index, tag: true })),
    ];
    const types = perModule(typeChecks, (module, positions) =>
      batched(
        "typesAtPositions",
        () => checker.getTypeAtPosition(module, positions),
        () => positions.map((p) => checker.getTypeAtPosition(module, p)),
      ));
    // A project's matches share their scrutinee types: one enum matched in
    // three hundred places is one type, and the answers derived from a type
    // — its constituents, each constituent's `kind` — depend on nothing
    // else. Both are asked once per type, not once per match. (Type ids are
    // snapshot-scoped, so the memo lives and dies with this ask.)
    const constituentCache = new Map();
    const constituentsOf = (type) => {
      let constituents = constituentCache.get(type.id);
      if (constituents === undefined) {
        constituents = type.isUnionType?.() ? type.getTypes() : [type];
        constituentCache.set(type.id, constituents);
      }
      return constituents;
    };
    const kindCache = new Map();
    const kindSymbolOf = (constituent) => {
      let kind = kindCache.get(constituent.id);
      if (kind === undefined) {
        kind = checker.getPropertyOfType(constituent, "kind") ?? null;
        kindCache.set(constituent.id, kind);
      }
      return kind;
    };
    // A tag check needs a second round: the `kind` property's type of every
    // constituent. The types of the distinct `kind` symbols — across every
    // tag check — are one batch.
    const tagWork = [];
    typeChecks.forEach((entry, at) => {
      if (!entry.tag) {
        const missing = missingLiterals(types[at], entry.check.covered, constituentsOf);
        if (missing) out.literalMissing.push({ index: entry.index, missing });
        return;
      }
      // An rl enum lowers to a discriminated union, so the question is
      // "which `kind` values does the scrutinee's type still allow?" —
      // again at the match, so a case an earlier guard removed is not
      // demanded back.
      const symbols = tagKindSymbols(types[at], constituentsOf, kindSymbolOf);
      if (symbols) tagWork.push({ index: entry.index, covered: entry.check.covered, symbols });
    });
    if (tagWork.length > 0) {
      // One question per distinct symbol: the same case tag reached from
      // three hundred matches is still one symbol.
      const distinct = new Map();
      for (const work of tagWork) {
        for (const symbol of work.symbols) {
          if (!distinct.has(symbol.id)) distinct.set(symbol.id, symbol);
        }
      }
      const asked = [...distinct.values()];
      const kinds = batched(
        "typesOfSymbols",
        () => checker.getTypeOfSymbol(asked),
        () => asked.map((symbol) => checker.getTypeOfSymbol(symbol)),
      );
      const valueOf = new Map(asked.map((symbol, i) => [symbol.id, literalValue(kinds[i])]));
      for (const work of tagWork) {
        const tags = work.symbols.map((symbol) => valueOf.get(symbol.id));
        // Every constituent must carry a single string-literal `kind`;
        // anything less definite makes the whole question indefinite, and
        // an indefinite question gets no answer.
        if (tags.some((tag) => typeof tag !== "string")) continue;
        const seen = new Set(work.covered);
        const missing = tags.filter((tag) => !seen.has(tag));
        if (missing.length > 0) out.tagMissing.push({ index: work.index, missing });
      }
    }

    // Resolution: the primitive rl's `val` is built from. Which binding an
    // identifier names, and whether a method is a built-in, are both "what
    // symbol is this?" — asked here, interpreted by rl.
    const symbolChecks = (job.symbolChecks ?? []).map((check, index) => ({ check, index }));
    const symbols = perModule(symbolChecks, (module, positions) =>
      batched(
        "symbolsAtPositions",
        () => checker.getSymbolAtPosition(module, positions),
        () => positions.map((p) => checker.getSymbolAtPosition(module, p)),
      ));
    symbolChecks.forEach((entry, at) => {
      const symbol = symbols[at];
      if (!symbol) return; // `any`, unresolved — never a verdict
      const declarations = symbol.declarations ?? [];
      out.symbols.push({
        index: entry.index,
        id: symbol.id,
        name: symbol.name,
        // Whether this is one of TypeScript's own declarations is the
        // compiler's answer, not a guess from the path: a released package
        // reads its libraries from disk while a built checkout serves them
        // from `bundled:///`, and both are the same fact.
        builtin: declarations.length > 0 && declarations.every(isDefaultLibrary),
      });
    });
    // Declaration emit, in memory. The compiler writes the `.d.ts` for a
    // lowered module exactly as it would for a hand-written one, so rlc
    // never generates TypeScript declaration syntax itself.
    if (job.emitDeclarations) {
      // Declaration emit is newer than the checker API: a released 7.0
      // client can check but cannot emit.
      if (typeof project.program.getDeclarationEmit !== "function") {
        process.exitCode = 5;
        fail(5, "rlc host: the resolved TypeScript has no declaration emit API");
      }
      const emitted = project.program.getDeclarationEmit(
        (job.modules ?? []).map((m) => m.path),
      );
      for (const [path, file] of emitted.outputFiles) {
        out.declarations.push({ path, text: file.text });
      }
    }
    return out;
  }
}

/**
 * Replaces the served modules with `modules`, reporting what changed so the
 * snapshot can be updated rather than rebuilt.
 */
function serve(files, dirs, modules) {
  const changed = [];
  const created = [];
  const deleted = [];
  const seen = new Set();
  for (const module of modules) {
    seen.add(module.path);
    if (!files.has(module.path)) created.push(module.path);
    else if (files.get(module.path) !== module.text) changed.push(module.path);
    files.set(module.path, module.text);
    for (let d = path.dirname(module.path); d && d !== path.dirname(d); d = path.dirname(d)) {
      dirs.add(d);
    }
  }
  for (const known of [...files.keys()]) {
    if (!seen.has(known)) {
      deleted.push(known);
      files.delete(known);
    }
  }
  return { changed, created, deleted };
}

/**
 * The literals of `type` that `covered` does not, or `null` when the type is
 * not a definite finite union of literals. Anything less definite — `string`,
 * a type parameter, `"a" | string` — is left alone: a missed diagnostic
 * beats a false one.
 */
function missingLiterals(type, covered, constituentsOf) {
  if (!type) return null;
  const constituents = constituentsOf(type);
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

/**
 * The `kind` property symbols of `type`'s constituents, or `null` when the
 * type is not a union of tagged object types — a bare object, a type
 * parameter or `any` makes the whole question indefinite, and an indefinite
 * question gets no answer. Whether each `kind` is a single string literal is
 * the caller's half, batched over every tag check at once.
 */
function tagKindSymbols(type, constituentsOf, kindSymbolOf) {
  if (!type) return null;
  const constituents = constituentsOf(type);
  const symbols = [];
  for (const c of constituents) {
    const kind = kindSymbolOf(c);
    if (!kind) return null;
    symbols.push(kind);
  }
  return symbols;
}

/**
 * Runs `ask(module, positions)` once per module over `entries` — each
 * `{ check: { module, start } }` — and returns the answers aligned with
 * `entries`' own order. The grouping is invisible to the caller: entry `i`'s
 * answer is at `i`, whatever module it was grouped under.
 */
function perModule(entries, ask) {
  const byModule = new Map();
  entries.forEach((entry, at) => {
    let group = byModule.get(entry.check.module);
    if (!group) byModule.set(entry.check.module, (group = []));
    group.push(at);
  });
  const answers = new Array(entries.length);
  for (const [module, group] of byModule) {
    const batch = ask(module, group.map((at) => entries[at].check.start));
    group.forEach((at, i) => (answers[at] = batch[i]));
  }
  return answers;
}

/**
 * Whether each batched checker endpoint is served by the resolved client,
 * discovered on first use. A released client older than the batch overloads
 * still answers — one position at a time.
 */
const batchable = {};

/**
 * `batch()` when the client supports it, `single()` otherwise. The two
 * compute the same answers; only the number of round trips differs, so a
 * client without the batch endpoint changes no verdict.
 */
function batched(name, batch, single) {
  if (batchable[name] !== false) {
    try {
      const result = batch();
      if (Array.isArray(result)) {
        batchable[name] = true;
        return result;
      }
    } catch {
      // fall through — an endpoint the server does not serve
    }
    batchable[name] = false;
  }
  return single();
}

/** The value of a literal type, or `undefined` when it is not one. */
function literalValue(type) {
  const v = type.value;
  if (typeof v === "string" || typeof v === "number" || typeof v === "boolean") return v;
  return undefined;
}

await main();
