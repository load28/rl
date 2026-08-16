import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';
import { compile, RlError } from '../src/compile.js';

const TSC_FLAGS = [
  '--strict', '--target', 'es2022', '--module', 'esnext',
  '--moduleResolution', 'bundler', '--skipLibCheck',
];

const hasTsc = spawnSync('tsc', ['--version'], { encoding: 'utf8' }).status === 0;

function tmpdir() {
  return fs.mkdtempSync(path.join(os.tmpdir(), 'rl-test-'));
}

/** Compile rl → ts, type-check + emit JS with tsc, import the module. */
async function runRl(source) {
  assert.ok(hasTsc, 'tsc is required for runtime tests');
  const dir = tmpdir();
  const tsPath = path.join(dir, 'main.ts');
  fs.writeFileSync(tsPath, compile(source).code);
  const res = spawnSync('tsc', [tsPath, '--outDir', dir, ...TSC_FLAGS], { encoding: 'utf8' });
  assert.equal(res.status, 0, `tsc failed:\n${res.stdout}\n---compiled---\n${compile(source).code}`);
  return import(pathToFileURL(path.join(dir, 'main.js')));
}

/** Type-check compiled output; returns { ok, output }. */
function typecheckRl(source) {
  const dir = tmpdir();
  const tsPath = path.join(dir, 'main.ts');
  fs.writeFileSync(tsPath, compile(source).code);
  const res = spawnSync('tsc', [tsPath, '--noEmit', ...TSC_FLAGS], { encoding: 'utf8' });
  return { ok: res.status === 0, output: res.stdout };
}

/* ------------------------------------------------------------------ */
/* TypeScript passthrough: valid TS must come out byte-for-byte        */
/* ------------------------------------------------------------------ */

const PASSTHROUGH_SAMPLES = {
  'String.prototype.match': `const m = "abc".match(/b/);\n`,
  'optional chaining .match': `const m = s?.match(re) ?? [];\n`,
  'class method named match': `
class Router {
  match(pathname: string): boolean {
    return this.routes.some((r) => r.test(pathname));
  }
}
`,
  'object method named match': `
const matcher = {
  match(s: string) { return s.length > 0; },
};
`,
  'interface member named match': `
interface Matcher {
  match(s: string): boolean;
}
`,
  'function named match': `
function match(a: number, b: number) {
  return a === b;
}
const ok = match(1, 1);
`,
  'variable named variant': `
const variant = { kind: "a" };
console.log(variant.kind, variant);
`,
  'match inside a string': `const s = "match (x) { A => 1 }";\n`,
  'match inside a comment': `// match (x) { A => 1 }\n/* match (y) { B => 2 } */\nconst z = 1;\n`,
  'match inside a template chunk': 'const s = `match (x) { A => 1 } and ${1 + 2}`;\n',
  'regex containing braces': `const re = /match \\(x\\) \\{.*\\}/g;\n`,
  'generics and arrows': `
const pick = <T,>(xs: T[], i: number): T | undefined => xs[i];
type Fn = (a: string, b: number) => Map<string, Array<number>>;
`,
  'match property key': `const cfg = { match: true, mode: "all" };\n`,
  'labeled misc code': `
export async function main(): Promise<void> {
  const data = await fetch("/api").then((r) => r.json());
  switch (data.kind) {
    case "a": break;
    default: break;
  }
}
`,
};

for (const [name, src] of Object.entries(PASSTHROUGH_SAMPLES)) {
  test(`passthrough: ${name}`, () => {
    assert.equal(compile(src).code, src);
  });
}

/* ------------------------------------------------------------------ */
/* variant compilation                                                 */
/* ------------------------------------------------------------------ */

test('variant: emits a discriminated union type and constructors', () => {
  const out = compile(`
variant Shape {
  Circle(radius: number),
  Rect(width: number, height: number),
  Point,
}
`).code;
  assert.match(out, /type Shape =/);
  assert.match(out, /\{ kind: "Circle"; radius: number \}/);
  assert.match(out, /\{ kind: "Rect"; width: number; height: number \}/);
  assert.match(out, /\{ kind: "Point" \}/);
  assert.match(out, /const Shape = \{/);
  assert.match(out, /Circle: \(radius: number\): Shape => \(\{ kind: "Circle", radius \}\)/);
  assert.match(out, /Point: \{ kind: "Point" \} as const/);
});

test('variant: export prefix is preserved on both declarations', () => {
  const out = compile(`export variant Color { Red, Green, Blue }`).code;
  assert.match(out, /export type Color =/);
  assert.match(out, /export const Color = \{/);
});

test('variant: generics flow into constructors', () => {
  const out = compile(`
variant Option<T> {
  Some(value: T),
  None,
}
`).code;
  assert.match(out, /type Option<T> =/);
  assert.match(out, /Some: <T>\(value: T\): Option<T> => \(\{ kind: "Some", value \}\)/);
});

test('variant: duplicate case is a compile error', () => {
  assert.throws(
    () => compile(`variant X { A, A }`),
    (e) => e instanceof RlError && /duplicate case "A"/.test(e.message)
  );
});

test('variant: complex field types with generics and commas', () => {
  const out = compile(`
variant Node {
  Leaf(entries: Map<string, number[]>),
  Branch(children: Array<Node>, meta: { tag: string, depth: number }),
}
`).code;
  assert.match(out, /entries: Map<string, number\[\]>/);
  assert.match(out, /meta: \{ tag: string, depth: number \}/);
});

/* ------------------------------------------------------------------ */
/* match compilation                                                   */
/* ------------------------------------------------------------------ */

test('match: compiles to a switch with a never-typed default', () => {
  const out = compile(`
const area = match (shape) {
  Circle(radius) => 3.14 * radius * radius,
  Point => 0,
};
`).code;
  assert.match(out, /switch \(\$rl_m\.kind\)/);
  assert.match(out, /case "Circle": \{ const \{ radius \} = \$rl_m; return \(3\.14 \* radius \* radius\); \}/);
  assert.match(out, /case "Point": \{ return \(0\); \}/);
  assert.match(out, /const \$rl_never: never = \$rl_m;/);
});

test('match: wildcard arm becomes default and drops the never check', () => {
  const out = compile(`const r = match (x) { A => 1, _ => 0 };`).code;
  assert.match(out, /default: \{ return \(0\); \}/);
  assert.doesNotMatch(out, /never/);
});

test('match: wildcard must be last', () => {
  assert.throws(
    () => compile(`const r = match (x) { _ => 0, A => 1 };`),
    (e) => e instanceof RlError && /must be the last arm/.test(e.message)
  );
});

test('match: duplicate arm is a compile error', () => {
  assert.throws(
    () => compile(`const r = match (x) { A => 1, A => 2 };`),
    (e) => e instanceof RlError && /duplicate arm "A"/.test(e.message)
  );
});

test('match: await in an arm produces an awaited async IIFE', () => {
  const out = compile(`const r = match (x) { A(url) => await fetch(url), _ => null };`).code;
  assert.match(out, /\(await \(async \(\) => \{/);
});

test('match: nested match expressions compile recursively', () => {
  const out = compile(`
const r = match (a) {
  X(inner) => match (inner) { Y => 1, _ => 2 },
  _ => 0,
};
`).code;
  const count = (out.match(/switch \(\$rl_m\.kind\)/g) || []).length;
  assert.equal(count, 2);
});

test('match: works inside template interpolation', () => {
  const out = compile('const s = `v=${match (x) { A => 1, _ => 0 }}`;').code;
  assert.match(out, /switch \(\$rl_m\.kind\)/);
});

/* ------------------------------------------------------------------ */
/* runtime behavior (needs tsc)                                        */
/* ------------------------------------------------------------------ */

test('runtime: variant construction and match evaluation', { skip: !hasTsc }, async () => {
  const mod = await runRl(`
export variant Shape {
  Circle(radius: number),
  Rect(width: number, height: number),
  Point,
}

export function area(s: Shape): number {
  return match (s) {
    Circle(radius) => Math.PI * radius * radius,
    Rect(width, height) => width * height,
    Point => 0,
  };
}

export const results = [
  area(Shape.Circle(1)),
  area(Shape.Rect(3, 4)),
  area(Shape.Point),
];
`);
  assert.deepEqual(mod.results, [Math.PI, 12, 0]);
  assert.deepEqual(mod.Shape.Circle(2), { kind: 'Circle', radius: 2 });
  assert.deepEqual(mod.Shape.Point, { kind: 'Point' });
});

test('runtime: binding aliases and block bodies', { skip: !hasTsc }, async () => {
  const mod = await runRl(`
export variant Msg {
  Quit,
  Move(x: number, y: number),
  Write(text: string),
}

export function describe(m: Msg): string {
  return match (m) {
    Move(x: px, y: py) => {
      const sum = px + py;
      return "move:" + sum;
    },
    Write(text) => "write:" + text,
    Quit => "quit",
  };
}

export const out = [
  describe(Msg.Move(2, 3)),
  describe(Msg.Write("hi")),
  describe(Msg.Quit),
];
`);
  assert.deepEqual(mod.out, ['move:5', 'write:hi', 'quit']);
});

test('runtime: generic variant with match', { skip: !hasTsc }, async () => {
  const mod = await runRl(`
export variant Option<T> {
  Some(value: T),
  None,
}

export function unwrapOr<T>(o: Option<T>, fallback: T): T {
  return match (o) {
    Some(value) => value,
    None => fallback,
  };
}

export const a = unwrapOr(Option.Some(7), 0);
export const b = unwrapOr<number>(Option.None, 42);
`);
  assert.equal(mod.a, 7);
  assert.equal(mod.b, 42);
});

test('runtime: async match with await arms', { skip: !hasTsc }, async () => {
  const mod = await runRl(`
export variant Job {
  Fetch(n: number),
  Idle,
}

async function double(n: number): Promise<number> {
  return n * 2;
}

export async function run(j: Job): Promise<number> {
  return match (j) {
    Fetch(n) => await double(n),
    Idle => 0,
  };
}
`);
  assert.equal(await mod.run({ kind: 'Fetch', n: 21 }), 42);
  assert.equal(await mod.run({ kind: 'Idle' }), 0);
});

test('runtime: unmatched variant throws at runtime through wildcard-free default', { skip: !hasTsc }, async () => {
  const mod = await runRl(`
export variant AB { A, B }
export function f(x: AB): number {
  return match (x) {
    A => 1,
    B => 2,
  };
}
export const g = f as (x: unknown) => number;
`);
  assert.throws(() => mod.g({ kind: 'C' }), /unhandled variant/);
});

/* ------------------------------------------------------------------ */
/* exhaustiveness checking via tsc                                     */
/* ------------------------------------------------------------------ */

test('typecheck: exhaustive match passes', { skip: !hasTsc }, () => {
  const { ok, output } = typecheckRl(`
variant Shape { Circle(radius: number), Point }
const f = (s: Shape) => match (s) {
  Circle(radius) => radius,
  Point => 0,
};
`);
  assert.ok(ok, output);
});

test('typecheck: non-exhaustive match is a type error', { skip: !hasTsc }, () => {
  const { ok, output } = typecheckRl(`
variant Shape { Circle(radius: number), Rect(w: number, h: number), Point }
const f = (s: Shape) => match (s) {
  Circle(radius) => radius,
  Point => 0,
};
`);
  assert.ok(!ok, 'expected tsc to reject the non-exhaustive match');
  assert.match(output, /never/);
});

test('typecheck: wildcard makes a partial match exhaustive', { skip: !hasTsc }, () => {
  const { ok, output } = typecheckRl(`
variant Shape { Circle(radius: number), Rect(w: number, h: number), Point }
const f = (s: Shape) => match (s) {
  Circle(radius) => radius,
  _ => 0,
};
`);
  assert.ok(ok, output);
});

test('typecheck: match works on any hand-written discriminated union', { skip: !hasTsc }, () => {
  const { ok, output } = typecheckRl(`
type AppEvent =
  | { kind: "click"; x: number; y: number }
  | { kind: "key"; code: string };
const f = (e: AppEvent) => match (e) {
  click(x, y) => x + y,
  key(code) => code.length,
};
`);
  assert.ok(ok, output);
});
