/**
 * rl — a tiny preprocessor language that compiles to TypeScript.
 *
 * Design goal: every valid TypeScript file is a valid .rl file and compiles
 * to itself, byte for byte. The compiler only rewrites the two constructs
 * rl adds on top of TypeScript:
 *
 *   variant Shape {            →  a discriminated-union type + constructor object
 *     Circle(radius: number),
 *     Point,
 *   }
 *
 *   match (shape) {            →  an immediately-invoked switch over `.kind`,
 *     Circle(radius) => ...,      with a `never` check that makes tsc report
 *     Point => 0,                 non-exhaustive matches at compile time
 *   }
 *
 * Anything that does not fully parse as one of these constructs is passed
 * through untouched, which is what preserves TypeScript compatibility.
 */

export class RlError extends Error {
  constructor(message, filename) {
    super(filename ? `${filename}: ${message}` : message);
    this.name = 'RlError';
  }
}

/**
 * Compile rl source text to TypeScript source text.
 * @param {string} source
 * @param {{ filename?: string }} [options]
 * @returns {{ code: string }}
 */
export function compile(source, options = {}) {
  const ctx = { filename: options.filename };
  return { code: transform(String(source), ctx) };
}

/* ------------------------------------------------------------------ */
/* character helpers                                                   */
/* ------------------------------------------------------------------ */

const PAIR = { '{': '}', '(': ')', '[': ']', '<': '>' };

// Words that can never be a variant tag, match pattern tag, or binding name.
// Meeting one of these while trying to parse an rl construct aborts the
// attempt, so ordinary TypeScript (e.g. a class method named `match`) is
// left untouched.
const RESERVED = new Set([
  'break', 'case', 'catch', 'class', 'const', 'continue', 'debugger',
  'default', 'delete', 'do', 'else', 'enum', 'export', 'extends', 'false',
  'finally', 'for', 'function', 'if', 'import', 'in', 'instanceof', 'let',
  'new', 'null', 'return', 'static', 'super', 'switch', 'this', 'throw',
  'true', 'try', 'typeof', 'var', 'void', 'while', 'with', 'yield',
  'await', 'async', 'of',
]);

// After one of these words, a `/` starts a regex literal, not division.
const REGEX_PRECEDING_WORDS = new Set([
  'return', 'typeof', 'instanceof', 'in', 'of', 'new', 'delete', 'void',
  'throw', 'case', 'do', 'else', 'yield', 'await',
]);

function isIdentStart(c) {
  return c !== undefined && /[A-Za-z_$]/.test(c);
}

function isIdentChar(c) {
  return c !== undefined && /[A-Za-z0-9_$]/.test(c);
}

/** Skip whitespace and comments; returns the index of the next significant char. */
function skipWsComments(src, i) {
  for (;;) {
    while (i < src.length && /\s/.test(src[i])) i++;
    if (src[i] === '/' && src[i + 1] === '/') {
      while (i < src.length && src[i] !== '\n') i++;
      continue;
    }
    if (src[i] === '/' && src[i + 1] === '*') {
      const e = src.indexOf('*/', i + 2);
      i = e === -1 ? src.length : e + 2;
      continue;
    }
    return i;
  }
}

/** src[i] is ' or " — returns the index just past the closing quote. */
function scanString(src, i) {
  const quote = src[i];
  i++;
  while (i < src.length) {
    const c = src[i];
    if (c === '\\') { i += 2; continue; }
    if (c === quote) return i + 1;
    if (c === '\n') return i; // unterminated string: stop at the newline
    i++;
  }
  return i;
}

/** src[i] is ` — returns the index just past the closing backtick. */
function skipTemplate(src, i) {
  i++;
  while (i < src.length) {
    const c = src[i];
    if (c === '\\') { i += 2; continue; }
    if (c === '`') return i + 1;
    if (c === '$' && src[i + 1] === '{') {
      const close = findMatching(src, i + 1);
      i = close === -1 ? src.length : close + 1;
      continue;
    }
    i++;
  }
  return i;
}

/**
 * src[i] is one of ( { [ < — returns the index of the matching closer,
 * or -1 if unbalanced. Skips strings, templates and comments. When
 * matching < >, `=>` is skipped so arrow/function types don't miscount.
 */
function findMatching(src, i) {
  const open = src[i];
  const close = PAIR[open];
  let depth = 0;
  while (i < src.length) {
    const c = src[i];
    if (c === '/' && src[i + 1] === '/') {
      const nl = src.indexOf('\n', i);
      i = nl === -1 ? src.length : nl;
      continue;
    }
    if (c === '/' && src[i + 1] === '*') {
      const e = src.indexOf('*/', i + 2);
      i = e === -1 ? src.length : e + 2;
      continue;
    }
    if (c === '"' || c === "'") { i = scanString(src, i); continue; }
    if (c === '`') { i = skipTemplate(src, i); continue; }
    if (open === '<' && c === '=' && src[i + 1] === '>') { i += 2; continue; }
    if (c === open) depth++;
    else if (c === close) {
      depth--;
      if (depth === 0) return i;
    }
    i++;
  }
  return -1;
}

/** src[i] is / and a regex literal is allowed here — returns index past it, or -1. */
function scanRegex(src, i) {
  i++;
  let inClass = false;
  while (i < src.length) {
    const c = src[i];
    if (c === '\\') { i += 2; continue; }
    if (c === '\n') return -1;
    if (c === '[') inClass = true;
    else if (c === ']') inClass = false;
    else if (c === '/' && !inClass) {
      i++;
      while (i < src.length && isIdentChar(src[i])) i++; // flags
      return i;
    }
    i++;
  }
  return -1;
}

/** True if `text` contains an `await` token in code position (including inside
 *  template interpolations, excluding strings and comments). */
function containsAwait(text) {
  let i = 0;
  while (i < text.length) {
    const c = text[i];
    if (c === '/' && text[i + 1] === '/') {
      const nl = text.indexOf('\n', i);
      i = nl === -1 ? text.length : nl;
      continue;
    }
    if (c === '/' && text[i + 1] === '*') {
      const e = text.indexOf('*/', i + 2);
      i = e === -1 ? text.length : e + 2;
      continue;
    }
    if (c === '"' || c === "'") { i = scanString(text, i); continue; }
    if (c === '`') {
      i++;
      while (i < text.length) {
        if (text[i] === '\\') { i += 2; continue; }
        if (text[i] === '`') { i++; break; }
        if (text[i] === '$' && text[i + 1] === '{') {
          const close = findMatching(text, i + 1);
          const end = close === -1 ? text.length : close;
          if (containsAwait(text.slice(i + 2, end))) return true;
          i = end + 1;
          continue;
        }
        i++;
      }
      continue;
    }
    if (isIdentStart(c)) {
      let j = i + 1;
      while (j < text.length && isIdentChar(text[j])) j++;
      if (text.slice(i, j) === 'await') return true;
      i = j;
      continue;
    }
    i++;
  }
  return false;
}

/* ------------------------------------------------------------------ */
/* main transform                                                      */
/* ------------------------------------------------------------------ */

function transform(src, ctx) {
  let out = '';
  let i = 0;
  let prevSig = '';  // last significant char emitted
  let prevWord = ''; // last identifier/keyword emitted

  while (i < src.length) {
    const c = src[i];

    // comments — copied verbatim
    if (c === '/' && src[i + 1] === '/') {
      let e = src.indexOf('\n', i);
      if (e === -1) e = src.length;
      out += src.slice(i, e);
      i = e;
      continue;
    }
    if (c === '/' && src[i + 1] === '*') {
      let e = src.indexOf('*/', i + 2);
      e = e === -1 ? src.length : e + 2;
      out += src.slice(i, e);
      i = e;
      continue;
    }

    // string literals — copied verbatim
    if (c === '"' || c === "'") {
      const e = scanString(src, i);
      out += src.slice(i, e);
      i = e;
      prevSig = c;
      prevWord = '';
      continue;
    }

    // template literals — interpolations are transformed recursively
    if (c === '`') {
      const t = transformTemplate(src, i, ctx);
      out += t.text;
      i = t.end;
      prevSig = '`';
      prevWord = '';
      continue;
    }

    // regex literals — copied verbatim (heuristic: by preceding token)
    if (c === '/' && regexAllowed(prevSig, prevWord)) {
      const e = scanRegex(src, i);
      if (e !== -1) {
        out += src.slice(i, e);
        i = e;
        prevSig = '/';
        prevWord = '';
        continue;
      }
    }

    if (isIdentStart(c)) {
      let j = i + 1;
      while (j < src.length && isIdentChar(src[j])) j++;
      const word = src.slice(i, j);
      const dotted = prevSig === '.'; // property access like `str.match(...)`

      if (!dotted && (word === 'variant' || word === 'export')) {
        let exported = false;
        let kwEnd = j;
        if (word === 'export') {
          const k = skipWsComments(src, j);
          if (isIdentStart(src[k])) {
            let m = k + 1;
            while (m < src.length && isIdentChar(src[m])) m++;
            if (src.slice(k, m) === 'variant') {
              exported = true;
              kwEnd = m;
            }
          }
        }
        if (word === 'variant' || exported) {
          const parsed = parseVariant(src, kwEnd, exported, ctx);
          if (parsed) {
            out += parsed.text;
            i = parsed.end;
            prevSig = ';';
            prevWord = '';
            continue;
          }
        }
      }

      if (!dotted && word === 'match') {
        const parsed = parseMatch(src, j, ctx);
        if (parsed) {
          out += parsed.text;
          i = parsed.end;
          prevSig = ')';
          prevWord = '';
          continue;
        }
      }

      out += word;
      i = j;
      prevWord = word;
      prevSig = word[word.length - 1];
      continue;
    }

    out += c;
    if (!/\s/.test(c)) {
      prevSig = c;
      prevWord = '';
    }
    i++;
  }
  return out;
}

function regexAllowed(prevSig, prevWord) {
  if (prevWord) return REGEX_PRECEDING_WORDS.has(prevWord);
  if (prevSig === '') return true;
  return '(,=:[!&|?{};~+-*%^<>'.includes(prevSig);
}

/** src[i] is ` — copies the template, transforming code inside ${ }. */
function transformTemplate(src, i, ctx) {
  let text = '`';
  i++;
  while (i < src.length) {
    const c = src[i];
    if (c === '\\') {
      text += src.slice(i, i + 2);
      i += 2;
      continue;
    }
    if (c === '`') {
      text += '`';
      i++;
      return { end: i, text };
    }
    if (c === '$' && src[i + 1] === '{') {
      const close = findMatching(src, i + 1);
      const end = close === -1 ? src.length : close;
      text += '${' + transform(src.slice(i + 2, end), ctx) + '}';
      i = end + 1;
      continue;
    }
    text += c;
    i++;
  }
  return { end: i, text };
}

/* ------------------------------------------------------------------ */
/* variant                                                             */
/* ------------------------------------------------------------------ */

function parseVariant(src, j, exported, ctx) {
  let i = skipWsComments(src, j);
  if (!isIdentStart(src[i])) return null;
  let k = i + 1;
  while (k < src.length && isIdentChar(src[k])) k++;
  const name = src.slice(i, k);
  if (RESERVED.has(name)) return null;

  i = skipWsComments(src, k);
  let generics = '';
  if (src[i] === '<') {
    const close = findMatching(src, i);
    if (close === -1) return null;
    generics = src.slice(i, close + 1);
    i = skipWsComments(src, close + 1);
  }
  if (src[i] !== '{') return null;
  const closeBrace = findMatching(src, i);
  if (closeBrace === -1) return null;

  const cases = parseVariantCases(src.slice(i + 1, closeBrace));
  if (!cases || cases.length === 0) return null;

  const seen = new Set();
  for (const c of cases) {
    if (seen.has(c.tag)) {
      throw new RlError(`variant ${name}: duplicate case "${c.tag}"`, ctx.filename);
    }
    seen.add(c.tag);
  }

  return { end: closeBrace + 1, text: emitVariant(name, generics, cases, exported) };
}

function parseVariantCases(text) {
  const cases = [];
  let i = 0;
  for (;;) {
    i = skipWsComments(text, i);
    if (i >= text.length) break;
    if (!isIdentStart(text[i])) return null;
    let j = i + 1;
    while (j < text.length && isIdentChar(text[j])) j++;
    const tag = text.slice(i, j);
    if (RESERVED.has(tag)) return null;
    i = skipWsComments(text, j);

    let fields = null;
    if (text[i] === '(') {
      const close = findMatching(text, i);
      if (close === -1) return null;
      fields = parseFields(text.slice(i + 1, close));
      if (fields === null) return null;
      i = close + 1;
    }
    cases.push({ tag, fields });

    i = skipWsComments(text, i);
    if (i >= text.length) break;
    if (text[i] === ',') { i++; continue; }
    return null;
  }
  return cases;
}

/** Parses `name: Type, name?: Type, ...`. Returns null on failure. */
function parseFields(text) {
  const fields = [];
  let i = 0;
  for (;;) {
    i = skipWsComments(text, i);
    if (i >= text.length) break;
    if (!isIdentStart(text[i])) return null;
    let j = i + 1;
    while (j < text.length && isIdentChar(text[j])) j++;
    const name = text.slice(i, j);
    if (RESERVED.has(name)) return null;
    i = skipWsComments(text, j);

    let optional = false;
    if (text[i] === '?') {
      optional = true;
      i = skipWsComments(text, i + 1);
    }
    if (text[i] !== ':') return null;
    i++;
    const start = i;
    i = scanTypeEnd(text, i);
    const type = text.slice(start, i).trim();
    if (!type) return null;
    fields.push({ name, optional, type });

    if (i >= text.length) break;
    if (text[i] === ',') { i++; continue; }
    return null;
  }
  return fields;
}

/** Scans a type annotation until a top-level `,` or closing bracket. */
function scanTypeEnd(text, i) {
  let depth = 0;
  while (i < text.length) {
    const c = text[i];
    if (c === '/' && text[i + 1] === '/') {
      const nl = text.indexOf('\n', i);
      i = nl === -1 ? text.length : nl;
      continue;
    }
    if (c === '/' && text[i + 1] === '*') {
      const e = text.indexOf('*/', i + 2);
      i = e === -1 ? text.length : e + 2;
      continue;
    }
    if (c === '"' || c === "'") { i = scanString(text, i); continue; }
    if (c === '`') { i = skipTemplate(text, i); continue; }
    if (c === '=' && text[i + 1] === '>') { i += 2; continue; }
    if (c === '(' || c === '[' || c === '{' || c === '<') depth++;
    else if (c === ')' || c === ']' || c === '}') {
      if (depth === 0) return i;
      depth--;
    } else if (c === '>') {
      if (depth > 0) depth--;
    } else if (c === ',' && depth === 0) return i;
    i++;
  }
  return i;
}

function emitVariant(name, generics, cases, exported) {
  const exp = exported ? 'export ' : '';

  const typeArms = cases.map((c) => {
    if (!c.fields || c.fields.length === 0) return `{ kind: "${c.tag}" }`;
    const fields = c.fields
      .map((f) => `${f.name}${f.optional ? '?' : ''}: ${f.type}`)
      .join('; ');
    return `{ kind: "${c.tag}"; ${fields} }`;
  });
  const typeDecl = `${exp}type ${name}${generics} =\n  | ${typeArms.join('\n  | ')};`;

  const typeArgs = generics ? `<${genericParamNames(generics).join(', ')}>` : '';
  const ctors = cases.map((c) => {
    if (c.fields === null) {
      // unit case: a singleton value, kept narrow via `as const`
      return `  ${c.tag}: { kind: "${c.tag}" } as const,`;
    }
    const params = c.fields
      .map((f) => `${f.name}${f.optional ? '?' : ''}: ${f.type}`)
      .join(', ');
    const props = c.fields.map((f) => f.name);
    const obj = ['kind: ' + JSON.stringify(c.tag), ...props].join(', ');
    return `  ${c.tag}: ${generics}(${params}): ${name}${typeArgs} => ({ ${obj} }),`;
  });
  const constDecl = `${exp}const ${name} = {\n${ctors.join('\n')}\n};`;

  return `${typeDecl}\n${constDecl}`;
}

/** `<T extends string, U = number>` → ["T", "U"] */
function genericParamNames(generics) {
  const inner = generics.slice(1, -1);
  const names = [];
  let i = 0;
  for (;;) {
    i = skipWsComments(inner, i);
    if (i >= inner.length) break;
    if (!isIdentStart(inner[i])) break;
    let j = i + 1;
    while (j < inner.length && isIdentChar(inner[j])) j++;
    let word = inner.slice(i, j);
    if (word === 'const' || word === 'in' || word === 'out') {
      // modifier — the actual name follows
      i = j;
      continue;
    }
    names.push(word);
    i = scanTypeEnd(inner, j); // skip constraint/default up to the next comma
    if (inner[i] === ',') i++;
  }
  return names;
}

/* ------------------------------------------------------------------ */
/* match                                                               */
/* ------------------------------------------------------------------ */

function parseMatch(src, j, ctx) {
  let k = skipWsComments(src, j);
  if (src[k] !== '(') return null;
  const closeParen = findMatching(src, k);
  if (closeParen === -1) return null;
  const scrutRaw = src.slice(k + 1, closeParen);
  if (!scrutRaw.trim()) return null;

  let b = skipWsComments(src, closeParen + 1);
  if (src[b] !== '{') return null;
  const closeBrace = findMatching(src, b);
  if (closeBrace === -1) return null;

  const arms = parseArms(src.slice(b + 1, closeBrace));
  if (!arms || arms.length === 0) return null;

  // sanity checks — by now this is clearly meant to be an rl match
  const seen = new Set();
  arms.forEach((arm, idx) => {
    if (arm.wildcard) {
      if (idx !== arms.length - 1) {
        throw new RlError('match: the wildcard arm `_` must be the last arm', ctx.filename);
      }
    } else {
      if (seen.has(arm.tag)) {
        throw new RlError(`match: duplicate arm "${arm.tag}"`, ctx.filename);
      }
      seen.add(arm.tag);
    }
  });

  return { end: closeBrace + 1, text: emitMatch(scrutRaw, arms, ctx) };
}

function parseArms(text) {
  const arms = [];
  let i = 0;
  for (;;) {
    i = skipWsComments(text, i);
    if (i >= text.length) break;

    // pattern
    let wildcard = false;
    let tag = null;
    let bindings = null;
    if (text[i] === '_' && !isIdentChar(text[i + 1])) {
      wildcard = true;
      i++;
    } else if (isIdentStart(text[i])) {
      let j = i + 1;
      while (j < text.length && isIdentChar(text[j])) j++;
      tag = text.slice(i, j);
      if (RESERVED.has(tag)) return null;
      i = skipWsComments(text, j);
      if (text[i] === '(') {
        const close = findMatching(text, i);
        if (close === -1) return null;
        bindings = parseBindings(text.slice(i + 1, close));
        if (bindings === null) return null;
        i = close + 1;
      }
    } else {
      return null;
    }

    i = skipWsComments(text, i);
    if (!(text[i] === '=' && text[i + 1] === '>')) return null;
    i = skipWsComments(text, i + 2);

    // body: `{ ... }` block or a single expression
    let bodyRaw;
    let block = false;
    if (text[i] === '{') {
      const close = findMatching(text, i);
      if (close === -1) return null;
      bodyRaw = text.slice(i + 1, close);
      block = true;
      i = close + 1;
    } else {
      const start = i;
      i = scanExprEnd(text, i);
      bodyRaw = text.slice(start, i);
      if (!bodyRaw.trim()) return null;
    }

    arms.push({ wildcard, tag, bindings, bodyRaw, block });

    i = skipWsComments(text, i);
    if (i >= text.length) break;
    if (text[i] === ',') { i++; continue; }
    return null;
  }
  return arms;
}

/** Parses `a, b: alias, ...` between the parens of a pattern. Returns null on failure. */
function parseBindings(text) {
  const bindings = [];
  let i = 0;
  for (;;) {
    i = skipWsComments(text, i);
    if (i >= text.length) break;
    if (!isIdentStart(text[i])) return null;
    let j = i + 1;
    while (j < text.length && isIdentChar(text[j])) j++;
    const name = text.slice(i, j);
    if (RESERVED.has(name)) return null;
    i = skipWsComments(text, j);

    let alias = null;
    if (text[i] === ':') {
      i = skipWsComments(text, i + 1);
      if (!isIdentStart(text[i])) return null;
      let m = i + 1;
      while (m < text.length && isIdentChar(text[m])) m++;
      alias = text.slice(i, m);
      if (RESERVED.has(alias)) return null;
      i = skipWsComments(text, m);
    }
    bindings.push({ name, alias });

    if (i >= text.length) break;
    if (text[i] === ',') { i++; continue; }
    return null;
  }
  return bindings;
}

/** Scans an arm's expression body until a top-level `,` or closing bracket. */
function scanExprEnd(text, i) {
  let depth = 0;
  while (i < text.length) {
    const c = text[i];
    if (c === '/' && text[i + 1] === '/') {
      const nl = text.indexOf('\n', i);
      i = nl === -1 ? text.length : nl;
      continue;
    }
    if (c === '/' && text[i + 1] === '*') {
      const e = text.indexOf('*/', i + 2);
      i = e === -1 ? text.length : e + 2;
      continue;
    }
    if (c === '"' || c === "'") { i = scanString(text, i); continue; }
    if (c === '`') { i = skipTemplate(text, i); continue; }
    if (c === '(' || c === '[' || c === '{') depth++;
    else if (c === ')' || c === ']' || c === '}') {
      if (depth === 0) return i;
      depth--;
    } else if (c === ',' && depth === 0) return i;
    i++;
  }
  return i;
}

function emitMatch(scrutRaw, arms, ctx) {
  const scrutinee = transform(scrutRaw, ctx);
  const isAsync =
    containsAwait(scrutRaw) || arms.some((a) => containsAwait(a.bodyRaw));

  let cases = '';
  let hasWildcard = false;
  for (const arm of arms) {
    const label = arm.wildcard ? 'default' : `case ${JSON.stringify(arm.tag)}`;
    if (arm.wildcard) hasWildcard = true;

    let bind = '';
    if (arm.bindings && arm.bindings.length > 0) {
      const parts = arm.bindings
        .map((b) => (b.alias ? `${b.name}: ${b.alias}` : b.name))
        .join(', ');
      bind = `const { ${parts} } = $rl_m; `;
    }

    if (arm.block) {
      const body = transform(arm.bodyRaw, ctx).trim();
      // `break` (not `return`) so an arm whose block always returns doesn't
      // widen the match's type with `undefined`; if the block doesn't return,
      // the arm evaluates to undefined, which the inferred type then reflects.
      cases += `    ${label}: { ${bind}${body}\n      break; }\n`;
    } else {
      const expr = transform(arm.bodyRaw, ctx).trim();
      // a trailing line comment would swallow the closing paren
      const nl = expr.split('\n').pop().includes('//') ? '\n    ' : '';
      cases += `    ${label}: { ${bind}return (${expr}${nl}); }\n`;
    }
  }

  if (!hasWildcard) {
    cases +=
      '    default: {\n' +
      '      const $rl_never: never = $rl_m;\n' +
      '      throw new Error("rl match: unhandled variant " + JSON.stringify($rl_never));\n' +
      '    }\n';
  }

  const fn = isAsync ? 'async () => {' : '() => {';
  const body =
    `(${fn}\n` +
    `  const $rl_m = (${scrutinee});\n` +
    `  switch ($rl_m.kind) {\n` +
    cases +
    `  }\n` +
    `})()`;

  return isAsync ? `(await ${body})` : `(${body})`;
}

export default compile;
