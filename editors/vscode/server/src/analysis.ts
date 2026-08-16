/* --------------------------------------------------------------------------
 * Lightweight structural analysis of rl source for editor features.
 *
 * This mirrors the compiler's recognition rules (src/parser/{enums,matches}.rs
 * and src/scanner.rs): strings/comments/templates/regexes are masked out
 * first, then rl `enum` declarations and `match` expressions are parsed
 * structurally. Anything that does not fully parse as rl syntax is ignored,
 * matching the compiler's passthrough contract. Diagnostics always come from
 * the real compiler (`rlc --check`); this module only powers completion,
 * hover, definitions and symbols, so a near-miss here can never produce a
 * wrong compile.
 * ----------------------------------------------------------------------- */

/** Reserved words — language.md §7. Not usable as enum names, tags, fields. */
export const RESERVED = new Set(
  (
    "async await break case catch class const continue debugger default " +
    "delete do else enum export extends false finally for function if " +
    "import in instanceof let new null of return static super switch this " +
    "throw true try typeof var void while with yield"
  ).split(" "),
);

const ID_START = /[A-Za-z_$]/;
const ID_CHAR = /[A-Za-z0-9_$]/;

/** ASCII identifier that is not a reserved word (language.md §1, §7). */
export function isIdent(word: string): boolean {
  return /^[A-Za-z_$][A-Za-z0-9_$]*$/.test(word) && !RESERVED.has(word);
}

/** Keywords after which `/` starts a regex literal rather than division. */
const REGEX_PRECEDING_KEYWORDS = new Set([
  "return",
  "case",
  "typeof",
  "do",
  "else",
  "in",
  "of",
  "new",
  "delete",
  "void",
  "instanceof",
  "yield",
  "await",
  "throw",
]);

/**
 * Return a same-length copy of `src` with the contents of strings, comments,
 * template-literal text and regex literals replaced by spaces (newlines are
 * preserved so offsets and line/column mapping stay identical). Code inside
 * template interpolations `${ ... }` is kept.
 */
export function maskNonCode(src: string): string {
  const out = src.split("");
  const n = src.length;
  const blank = (from: number, to: number): void => {
    for (let k = from; k < to && k < n; k++) {
      if (out[k] !== "\n") out[k] = " ";
    }
  };

  let i = 0;
  let lastSig = ""; // last significant code character seen
  let lastWord = ""; // last identifier/keyword seen (for the regex heuristic)

  const regexAllowed = (): boolean => {
    if (lastSig === "") return true;
    if ("([{,;=:!&|?+-*/%<>~^".includes(lastSig)) return true;
    if (ID_CHAR.test(lastSig)) return REGEX_PRECEDING_KEYWORDS.has(lastWord);
    return false;
  };

  const scanString = (quote: string): void => {
    const start = i;
    i++;
    while (i < n) {
      const c = src[i];
      if (c === "\\") {
        i += 2;
        continue;
      }
      if (c === quote || c === "\n") {
        i++;
        break;
      }
      i++;
    }
    blank(start, i);
  };

  const scanLineComment = (): void => {
    const start = i;
    while (i < n && src[i] !== "\n") i++;
    blank(start, i);
  };

  const scanBlockComment = (): void => {
    const start = i;
    i += 2;
    while (i < n && !(src[i] === "*" && src[i + 1] === "/")) i++;
    i = Math.min(n, i + 2);
    blank(start, i);
  };

  const scanRegex = (): void => {
    const start = i;
    i++;
    let inClass = false;
    while (i < n) {
      const c = src[i];
      if (c === "\\") {
        i += 2;
        continue;
      }
      if (c === "\n") break;
      if (c === "[") inClass = true;
      else if (c === "]") inClass = false;
      else if (c === "/" && !inClass) {
        i++;
        while (i < n && ID_CHAR.test(src[i])) i++;
        break;
      }
      i++;
    }
    blank(start, i);
  };

  const scanTemplate = (): void => {
    blank(i, i + 1); // opening backtick
    i++;
    while (i < n) {
      const c = src[i];
      if (c === "\\") {
        blank(i, i + 2);
        i += 2;
        continue;
      }
      if (c === "`") {
        blank(i, i + 1);
        i++;
        return;
      }
      if (c === "$" && src[i + 1] === "{") {
        blank(i, i + 2);
        i += 2;
        scanCode("}"); // interpolation code stays visible
        continue;
      }
      blank(i, i + 1);
      i++;
    }
  };

  // Scan code, blanking non-code regions, until `until` appears at brace
  // depth 0 (the terminator itself is blanked) or the input ends.
  const scanCode = (until: string | null): void => {
    let depth = 0;
    while (i < n) {
      const c = src[i];
      if (until !== null && depth === 0 && c === until) {
        blank(i, i + 1);
        i++;
        return;
      }
      if (c === "'" || c === '"') {
        scanString(c);
        lastSig = '"';
        lastWord = "";
        continue;
      }
      if (c === "`") {
        scanTemplate();
        lastSig = '"';
        lastWord = "";
        continue;
      }
      if (c === "/") {
        if (src[i + 1] === "/") {
          scanLineComment();
          continue;
        }
        if (src[i + 1] === "*") {
          scanBlockComment();
          continue;
        }
        if (regexAllowed()) {
          scanRegex();
          lastSig = '"';
          lastWord = "";
          continue;
        }
        lastSig = "/";
        lastWord = "";
        i++;
        continue;
      }
      if (/\s/.test(c)) {
        i++;
        continue;
      }
      if (ID_START.test(c)) {
        const ws = i;
        while (i < n && ID_CHAR.test(src[i])) i++;
        lastWord = src.slice(ws, i);
        lastSig = lastWord[lastWord.length - 1];
        continue;
      }
      if (c === "{") depth++;
      else if (c === "}" && depth > 0) depth--;
      lastSig = c;
      lastWord = "";
      i++;
    }
  };

  scanCode(null);
  return out.join("");
}

export interface FieldInfo {
  name: string;
  optional: boolean;
  type: string;
}

export interface CaseInfo {
  tag: string;
  tagStart: number;
  tagEnd: number;
  /** Empty for unit cases; also empty for `Tag()` (see hasParens). */
  fields: FieldInfo[];
  hasParens: boolean;
}

export interface EnumInfo {
  name: string;
  nameStart: number;
  nameEnd: number;
  /** Full generics text including angle brackets, or "". */
  generics: string;
  exported: boolean;
  builtin: boolean;
  cases: CaseInfo[];
  /** Byte range of the whole declaration (export .. closing brace). */
  start: number;
  end: number;
}

/** Built-in enums (sema.rs BUILTIN_ENUMS): exhaustiveness-checked without a
 * declaration; a same-named local rl enum shadows them. */
export const BUILTIN_ENUMS: EnumInfo[] = [
  {
    name: "Option",
    nameStart: -1,
    nameEnd: -1,
    generics: "<T>",
    exported: false,
    builtin: true,
    start: -1,
    end: -1,
    cases: [
      {
        tag: "Some",
        tagStart: -1,
        tagEnd: -1,
        fields: [{ name: "value", optional: false, type: "T" }],
        hasParens: true,
      },
      { tag: "None", tagStart: -1, tagEnd: -1, fields: [], hasParens: false },
    ],
  },
  {
    name: "Result",
    nameStart: -1,
    nameEnd: -1,
    generics: "<T, E>",
    exported: false,
    builtin: true,
    start: -1,
    end: -1,
    cases: [
      {
        tag: "Ok",
        tagStart: -1,
        tagEnd: -1,
        fields: [{ name: "value", optional: false, type: "T" }],
        hasParens: true,
      },
      {
        tag: "Err",
        tagStart: -1,
        tagEnd: -1,
        fields: [{ name: "error", optional: false, type: "E" }],
        hasParens: true,
      },
    ],
  },
];

/** Declared enums plus non-shadowed built-ins. */
export function visibleEnums(declared: EnumInfo[]): EnumInfo[] {
  return declared.concat(
    BUILTIN_ENUMS.filter((b) => !declared.some((d) => d.name === b.name)),
  );
}

const skipWs = (masked: string, i: number): number => {
  while (i < masked.length && /\s/.test(masked[i])) i++;
  return i;
};

const prevNonWs = (masked: string, i: number): number => {
  let k = i - 1;
  while (k >= 0 && /\s/.test(masked[k])) k--;
  return k;
};

const readIdent = (masked: string, i: number): [string, number] => {
  let j = i;
  if (j < masked.length && ID_START.test(masked[j])) {
    j++;
    while (j < masked.length && ID_CHAR.test(masked[j])) j++;
  }
  return [masked.slice(i, j), j];
};

/** Find the matching closer for the opener at `open`. Returns -1 if none. */
function matchDelim(
  masked: string,
  open: number,
  opener: string,
  closer: string,
): number {
  let depth = 0;
  for (let i = open; i < masked.length; i++) {
    const c = masked[i];
    if (c === opener) depth++;
    else if (c === closer) {
      depth--;
      if (depth === 0) return i;
    }
  }
  return -1;
}

/** Balance `<...>` starting at `open`, skipping `=>` arrows. -1 if none. */
function matchAngle(masked: string, open: number): number {
  let depth = 0;
  for (let i = open; i < masked.length; i++) {
    const c = masked[i];
    if (c === "<") depth++;
    else if (c === ">" && masked[i - 1] !== "=") {
      depth--;
      if (depth === 0) return i;
    }
  }
  return -1;
}

/** Split `masked[from..to)` at top-level commas (over () {} [] <>). */
function splitTopLevel(
  masked: string,
  from: number,
  to: number,
): Array<[number, number]> {
  const parts: Array<[number, number]> = [];
  let depth = 0;
  let start = from;
  for (let i = from; i < to; i++) {
    const c = masked[i];
    if ("({[<".includes(c)) depth++;
    else if (")}]".includes(c)) depth--;
    else if (c === ">" && masked[i - 1] !== "=") depth--;
    else if (c === "," && depth === 0) {
      parts.push([start, i]);
      start = i + 1;
    }
  }
  parts.push([start, to]);
  return parts;
}

/**
 * Parse rl enum declarations. Mirrors parser/enums.rs: a declaration is an
 * rl enum only when at least one case has payload parens or the declaration
 * has generics; `const enum` / `declare enum` and anything that fails to
 * parse completely is skipped (passthrough).
 */
export function parseEnums(src: string, masked: string): EnumInfo[] {
  const enums: EnumInfo[] = [];
  const re = /\benum\b/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(masked)) !== null) {
    const kw = m.index;
    const before = prevNonWs(masked, kw);
    if (before >= 0 && masked[before] === ".") continue;

    // Preceding word: `export` extends the declaration; `const`/`declare`
    // mark a plain TS enum.
    let declStart = kw;
    let exported = false;
    if (before >= 0 && ID_CHAR.test(masked[before])) {
      let ws = before;
      while (ws >= 0 && ID_CHAR.test(masked[ws])) ws--;
      const word = masked.slice(ws + 1, before + 1);
      if (word === "const" || word === "declare") continue;
      if (word === "export") {
        exported = true;
        declStart = ws + 1;
      }
    }

    let j = skipWs(masked, kw + 4);
    const [name, afterName] = readIdent(masked, j);
    if (!isIdent(name)) continue;
    const nameStart = j;
    const nameEnd = afterName;

    j = skipWs(masked, afterName);
    let generics = "";
    if (masked[j] === "<") {
      const close = matchAngle(masked, j);
      if (close === -1) continue;
      generics = src.slice(j, close + 1);
      j = skipWs(masked, close + 1);
    }

    if (masked[j] !== "{") continue;
    const bodyOpen = j;
    const bodyClose = matchDelim(masked, bodyOpen, "{", "}");
    if (bodyClose === -1) continue;

    const cases = parseCases(src, masked, bodyOpen + 1, bodyClose);
    if (cases === null) continue;

    const isRl = generics !== "" || cases.some((c) => c.hasParens);
    if (!isRl) continue;

    enums.push({
      name,
      nameStart,
      nameEnd,
      generics,
      exported,
      builtin: false,
      cases,
      start: declStart,
      end: bodyClose + 1,
    });
    re.lastIndex = bodyClose;
  }
  return enums;
}

/** Parse an enum body. Returns null when it is not a valid rl case list. */
function parseCases(
  src: string,
  masked: string,
  from: number,
  to: number,
): CaseInfo[] | null {
  const cases: CaseInfo[] = [];
  let i = skipWs(masked, from);
  while (i < to) {
    const [tag, afterTag] = readIdent(masked, i);
    if (!isIdent(tag)) return null;
    const tagStart = i;
    const tagEnd = afterTag;
    i = skipWs(masked, afterTag);

    let hasParens = false;
    const fields: FieldInfo[] = [];
    if (i < to && masked[i] === "(") {
      hasParens = true;
      const close = matchDelim(masked, i, "(", ")");
      if (close === -1 || close >= to) return null;
      if (masked.slice(i + 1, close).trim() !== "") {
        for (const [fs, fe] of splitTopLevel(masked, i + 1, close)) {
          if (masked.slice(fs, fe).trim() === "" && fe === close) continue; // trailing comma
          const field = parseField(src, masked, fs, fe);
          if (field === null) return null;
          fields.push(field);
        }
      }
      i = skipWs(masked, close + 1);
    }

    cases.push({ tag, tagStart, tagEnd, fields, hasParens });

    if (i >= to) break;
    if (masked[i] !== ",") return null;
    i = skipWs(masked, i + 1);
  }
  return cases;
}

function parseField(
  src: string,
  masked: string,
  from: number,
  to: number,
): FieldInfo | null {
  let i = skipWs(masked, from);
  const [name, afterName] = readIdent(masked, i);
  if (!isIdent(name)) return null;
  i = skipWs(masked, afterName);
  let optional = false;
  if (masked[i] === "?") {
    optional = true;
    i = skipWs(masked, i + 1);
  }
  if (masked[i] !== ":") return null;
  const type = src.slice(i + 1, to).trim();
  if (type === "") return null;
  return { name, optional, type };
}

export interface MatchInfo {
  /** Offset of the `match` keyword. */
  start: number;
  /** Offsets of the scrutinee's `(` and `)`. */
  scrutOpen: number;
  scrutClose: number;
  /** Offsets of the body's `{` and `}`. */
  bodyOpen: number;
  bodyClose: number;
}

/** Find all structurally complete `match (expr) { ... }` expressions. */
export function parseMatches(masked: string): MatchInfo[] {
  const matches: MatchInfo[] = [];
  const re = /\bmatch\b/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(masked)) !== null) {
    const kw = m.index;
    const before = prevNonWs(masked, kw);
    if (before >= 0 && masked[before] === ".") continue; // str.match(...)

    const scrutOpen = skipWs(masked, kw + 5);
    if (masked[scrutOpen] !== "(") continue;
    const scrutClose = matchDelim(masked, scrutOpen, "(", ")");
    if (scrutClose === -1) continue;

    const bodyOpen = skipWs(masked, scrutClose + 1);
    if (masked[bodyOpen] !== "{") continue;
    const bodyClose = matchDelim(masked, bodyOpen, "{", "}");
    if (bodyClose === -1) continue;

    matches.push({ start: kw, scrutOpen, scrutClose, bodyOpen, bodyClose });
  }
  return matches;
}

/** Innermost match whose body contains `offset`. */
export function matchBodyAt(
  matches: MatchInfo[],
  offset: number,
): MatchInfo | null {
  let best: MatchInfo | null = null;
  for (const m of matches) {
    if (offset > m.bodyOpen && offset <= m.bodyClose) {
      if (best === null || m.bodyOpen > best.bodyOpen) best = m;
    }
  }
  return best;
}

/** The match whose `match` keyword sits at (or right before) `offset`. */
export function matchKeywordAt(
  matches: MatchInfo[],
  offset: number,
): MatchInfo | null {
  return (
    matches.find((m) => offset >= m.start && offset <= m.start + 5) ?? null
  );
}

export interface ArmContext {
  match: MatchInfo;
  /** Cursor is where an arm pattern tag may be typed. */
  patternPosition: boolean;
  /** Cursor is inside the binding parens of this tag, if any. */
  bindingTag: string | null;
}

/** Classify the cursor position inside the innermost match body. */
export function armContextAt(
  masked: string,
  matches: MatchInfo[],
  offset: number,
): ArmContext | null {
  const m = matchBodyAt(matches, offset);
  if (m === null) return null;

  let depth = 0;
  let segStart = m.bodyOpen + 1;
  let parenOpen = -1;
  for (let i = m.bodyOpen + 1; i < offset; i++) {
    const c = masked[i];
    if (c === "(" || c === "{" || c === "[") {
      if (depth === 0 && c === "(") parenOpen = i;
      depth++;
    } else if (c === ")" || c === "}" || c === "]") {
      depth--;
      if (depth <= 0) {
        parenOpen = -1;
        if (depth < 0) depth = 0;
      }
    } else if (c === "," && depth === 0) {
      segStart = i + 1;
    }
  }

  const seg = masked.slice(segStart, offset);
  const hasArrow = seg.includes("=>");
  if (depth === 0) {
    return { match: m, patternPosition: !hasArrow, bindingTag: null };
  }
  if (depth === 1 && parenOpen !== -1 && !hasArrow) {
    let k = parenOpen;
    while (k > segStart && /\s/.test(masked[k - 1])) k--;
    const idEnd = k;
    while (k > segStart && ID_CHAR.test(masked[k - 1])) k--;
    const tag = masked.slice(k, idEnd);
    if (isIdent(tag)) {
      return { match: m, patternPosition: false, bindingTag: tag };
    }
  }
  return { match: m, patternPosition: false, bindingTag: null };
}

/** Leading pattern tags of each arm (or-pattern alternatives included). */
export function armTags(masked: string, m: MatchInfo): string[] {
  const tags: string[] = [];
  for (const [from, to] of splitArms(masked, m)) {
    const seg = masked.slice(from, to);
    const arrow = seg.indexOf("=>");
    const head = arrow === -1 ? seg : seg.slice(0, arrow);
    for (const alt of head.split("|")) {
      const lead = /^\s*([A-Za-z_$][A-Za-z0-9_$]*)/.exec(alt);
      const t = lead ? lead[1] : "";
      if (t !== "_" && isIdent(t)) tags.push(t);
    }
  }
  return tags;
}

/** Top-level arm segments of a match body. */
function splitArms(masked: string, m: MatchInfo): Array<[number, number]> {
  const parts: Array<[number, number]> = [];
  let depth = 0;
  let start = m.bodyOpen + 1;
  for (let i = m.bodyOpen + 1; i < m.bodyClose; i++) {
    const c = masked[i];
    if ("({[".includes(c)) depth++;
    else if (")}]".includes(c)) depth--;
    else if (c === "," && depth === 0) {
      parts.push([start, i]);
      start = i + 1;
    }
  }
  parts.push([start, m.bodyClose]);
  return parts;
}

/**
 * Best-effort enum for a match: an `Enum.` mention in the scrutinee wins,
 * then a unique owner of the tags already written in the arms.
 */
export function inferEnum(
  masked: string,
  m: MatchInfo,
  enums: EnumInfo[],
): EnumInfo | null {
  const scrut = masked.slice(m.scrutOpen + 1, m.scrutClose);
  for (const e of enums) {
    const name = e.name.replace(/\$/g, "\\$");
    if (new RegExp(`(?<![.\\w$])${name}\\s*\\.`).test(scrut)) return e;
  }
  const tags = armTags(masked, m);
  if (tags.length > 0) {
    const owners = enums.filter((e) =>
      tags.some((t) => e.cases.some((c) => c.tag === t)),
    );
    if (owners.length === 1) return owners[0];
  }
  return null;
}

/** `Base.` member-access base identifier right before `offset`, if any. */
export function memberAccessAt(masked: string, offset: number): string | null {
  let k = offset;
  while (k > 0 && ID_CHAR.test(masked[k - 1])) k--;
  let j = k;
  while (j > 0 && /[ \t]/.test(masked[j - 1])) j--;
  if (j === 0 || masked[j - 1] !== ".") return null;
  j--;
  while (j > 0 && /[ \t]/.test(masked[j - 1])) j--;
  const end = j;
  while (j > 0 && ID_CHAR.test(masked[j - 1])) j--;
  const base = masked.slice(j, end);
  return isIdent(base) ? base : null;
}

export interface WordAt {
  word: string;
  start: number;
  end: number;
}

/** Identifier covering `offset` (offset may sit at its end). */
export function wordAt(src: string, offset: number): WordAt | null {
  let start = offset;
  while (start > 0 && ID_CHAR.test(src[start - 1])) start--;
  let end = offset;
  while (end < src.length && ID_CHAR.test(src[end])) end++;
  if (start === end) return null;
  const word = src.slice(start, end);
  if (!ID_START.test(word[0])) return null;
  return { word, start, end };
}

export type SymbolAt =
  | { kind: "enum"; enum: EnumInfo }
  | { kind: "case"; enum: EnumInfo; case: CaseInfo };

/** Resolve the enum or case the identifier at `offset` refers to. */
export function symbolAt(
  src: string,
  masked: string,
  offset: number,
  declared: EnumInfo[],
  matches: MatchInfo[],
): SymbolAt | null {
  const w = wordAt(src, offset);
  if (w === null || !isIdent(w.word)) return null;
  const enums = visibleEnums(declared);

  // Enum.Tag member access.
  const base = memberAccessAt(masked, w.start);
  if (base !== null) {
    const e = enums.find((x) => x.name === base);
    const c = e?.cases.find((x) => x.tag === w.word);
    if (e && c) return { kind: "case", enum: e, case: c };
  }

  // Inside an enum declaration: its own name or one of its tags.
  const decl = declared.find((e) => w.start >= e.start && w.end <= e.end);
  if (decl) {
    if (decl.name === w.word && w.start === decl.nameStart) {
      return { kind: "enum", enum: decl };
    }
    const c = decl.cases.find((x) => x.tag === w.word && x.tagStart === w.start);
    if (c) return { kind: "case", enum: decl, case: c };
  }

  // Inside a match body: a pattern tag of the inferred enum, else of any.
  const m = matchBodyAt(matches, offset);
  if (m !== null) {
    const inferred = inferEnum(masked, m, enums);
    const pool = inferred ? [inferred, ...enums] : enums;
    for (const e of pool) {
      const c = e.cases.find((x) => x.tag === w.word);
      if (c) return { kind: "case", enum: e, case: c };
    }
  }

  const byName = enums.find((e) => e.name === w.word);
  if (byName) return { kind: "enum", enum: byName };
  return null;
}

const fieldSig = (f: FieldInfo): string =>
  `${f.name}${f.optional ? "?" : ""}: ${f.type}`;

/** Render an enum declaration the way it would appear in source. */
export function enumSignature(e: EnumInfo): string {
  const cases = e.cases.map((c) =>
    c.hasParens || c.fields.length > 0
      ? `${c.tag}(${c.fields.map(fieldSig).join(", ")})`
      : c.tag,
  );
  const body = cases.length > 0 ? `\n  ${cases.join(",\n  ")},\n` : "";
  return `${e.exported ? "export " : ""}enum ${e.name}${e.generics} {${body}}`;
}

/** Render a case as `Enum.Tag(field: type, ...)`. */
export function caseSignature(e: EnumInfo, c: CaseInfo): string {
  if (!c.hasParens && c.fields.length === 0) return `${e.name}.${c.tag}`;
  return `${e.name}.${c.tag}(${c.fields.map(fieldSig).join(", ")})`;
}
