/* --------------------------------------------------------------------------
 * Text-shape utilities for the language server.
 *
 * What lives here is cursor-context detection over raw text: masking
 * strings/comments/templates/regexes so offsets survive, finding the word
 * or the `Base.` member access at the cursor. What deliberately does NOT
 * live here any more is rl *semantics* — which enums are visible, what a
 * match is over, what a case's fields are. That used to be a second,
 * regex-based implementation of the compiler's rules and could disagree
 * with it (docs/design/rust-parity-analysis.md GAP-3); the compiler now
 * answers those itself through the server's `declarations` method
 * (server.ts `declarationsOf`).
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
