/* --------------------------------------------------------------------------
 * Editor-side `val` typed mutation probes.
 *
 * The compiler owns the normative `val` analysis. This module only collects
 * the type-dependent method-call questions the editor can answer with the
 * TypeScript language service it already runs over the emitted virtual doc.
 *
 * Collection is deliberately conservative: when a later ordinary binding
 * might shadow a `val` binding, the probe is skipped. The TypeChecker makes
 * the final built-in verdict, so user-defined `set`/`push` methods are not
 * reported from their names alone.
 * ----------------------------------------------------------------------- */
import { maskNonCode } from "./analysis";

export interface ValMethodProbe {
  /** Source UTF-16 span of the access-path root; used for the diagnostic. */
  rootStart: number;
  rootEnd: number;
  /** The `val` binding the access path is rooted at. */
  binding: string;
  /** Candidate built-in mutator method. */
  method: string;
  /** Source UTF-16 span of the method name; mapped into the virtual doc. */
  nameStart: number;
  nameEnd: number;
}

interface BindingDecl {
  name: string;
  start: number;
  isVal: boolean;
}

const IDENT = "[A-Za-z_$][A-Za-z0-9_$]*";
const MUTATOR_NAMES = [
  "push",
  "pop",
  "shift",
  "unshift",
  "splice",
  "sort",
  "reverse",
  "fill",
  "copyWithin",
  "set",
  "delete",
  "clear",
  "add",
];

const METHOD_CALL = new RegExp(
  `\\b(${IDENT})\\b` +
    `(?:\\s*(?:\\?\\.|\\.)\\s*${IDENT}|\\s*!|\\s*\\[[^\\]\\n]*\\])*` +
    `\\s*(?:\\?\\.|\\.)\\s*(${MUTATOR_NAMES.join("|")})\\s*\\(`,
  "g",
);

/** Candidate built-in mutator calls through source bindings declared `val`. */
export function valMethodProbes(source: string): ValMethodProbe[] {
  const masked = maskNonCode(source);
  const bindings = collectBindings(masked);
  if (bindings.every((b) => !b.isVal)) return [];

  const out: ValMethodProbe[] = [];
  for (const match of masked.matchAll(METHOD_CALL)) {
    const text = match[0];
    const binding = match[1];
    const method = match[2];
    const rootStart = match.index ?? 0;
    const rootEnd = rootStart + binding.length;
    const decl = lastBindingBefore(bindings, binding, rootStart);
    if (!decl?.isVal) continue;
    const nameStart = rootStart + text.lastIndexOf(method);
    out.push({
      rootStart,
      rootEnd,
      binding,
      method,
      nameStart,
      nameEnd: nameStart + method.length,
    });
  }
  return out;
}

function collectBindings(masked: string): BindingDecl[] {
  const byPosition = new Map<string, BindingDecl>();
  const add = (decl: BindingDecl): void => {
    const key = `${decl.start}:${decl.name}`;
    const existing = byPosition.get(key);
    if (!existing || decl.isVal) byPosition.set(key, decl);
  };

  for (const match of masked.matchAll(
    new RegExp(`\\bval[ \\t]+(?:const|let|var)[ \\t]+(${IDENT})`, "g"),
  )) {
    const name = match[1];
    add({
      name,
      start: (match.index ?? 0) + match[0].lastIndexOf(name),
      isVal: true,
    });
  }

  for (const match of masked.matchAll(
    new RegExp(
      `[(,][ \\t]*(?:(?:public|private|protected|readonly|override)[ \\t]+)*val[ \\t]+(${IDENT})`,
      "g",
    ),
  )) {
    const name = match[1];
    add({
      name,
      start: (match.index ?? 0) + match[0].lastIndexOf(name),
      isVal: true,
    });
  }

  for (const match of masked.matchAll(
    new RegExp(`\\b(?:const|let|var|function)[ \\t]+(${IDENT})`, "g"),
  )) {
    const name = match[1];
    add({
      name,
      start: (match.index ?? 0) + match[0].lastIndexOf(name),
      isVal: false,
    });
  }

  for (const match of masked.matchAll(
    new RegExp(`[(,][ \\t]*(?!val\\b)(${IDENT})[ \\t]*(?::|[=,)])`, "g"),
  )) {
    const name = match[1];
    add({
      name,
      start: (match.index ?? 0) + match[0].lastIndexOf(name),
      isVal: false,
    });
  }

  return [...byPosition.values()].sort((a, b) => a.start - b.start);
}

function lastBindingBefore(
  bindings: BindingDecl[],
  name: string,
  offset: number,
): BindingDecl | null {
  let found: BindingDecl | null = null;
  for (const binding of bindings) {
    if (binding.start >= offset) break;
    if (binding.name === name) found = binding;
  }
  return found;
}
