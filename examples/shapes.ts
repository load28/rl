// @generated from shapes.rl by rlc — do not edit directly.
// examples/shapes.rl — rl is TypeScript plus rl `enum` and `match`.
// Everything here that isn't one of those two constructs is plain TypeScript
// and passes through the compiler untouched.

// `Option`/`Result` come from the standard library. The specifier is bare:
// rlc materializes the module next to this file's output and rewrites the
// import to point at it (see docs/reference/cli.md).
import { Option } from "./rl.js";

export type Shape =
  | { kind: "Circle"; radius: number }
  | { kind: "Rect"; width: number; height: number }
  | { kind: "Point" };
export const Shape = {
  Circle: (radius: number): Shape => ({ kind: "Circle", radius }),
  Rect: (width: number, height: number): Shape => ({ kind: "Rect", width, height }),
  Point: { kind: "Point" } as const,
};

export function area(s: Shape): number {
  return ((() => {
  const $rl_m = (s);
  switch ($rl_m.kind) {
    case "Circle": { const { radius } = $rl_m; return (Math.PI * radius * radius); }
    case "Rect": { const { width, height } = $rl_m; return (width * height); }
    case "Point": { return (0); }
    default: { throw new Error("rl match: unexpected case " + JSON.stringify($rl_m)); }
  }
})());
}

export function describe(s: Shape): string {
  return ((() => {
  const $rl_m = (s);
  switch ($rl_m.kind) {
    case "Rect": { const { width: w, height: h } = $rl_m; const ratio = w / h;
      return `rect (ratio ${ratio.toFixed(2)})`;
      break; }
    default: { return (`some shape with area ${area(s).toFixed(2)}`); }
  }
})());
}

// `Option` is a built-in enum: this match is checked for exhaustiveness even
// though the declaration lives in the standard library.
export function label<T>(o: Option<T>, fallback: string): string {
  return ((() => {
  const $rl_m = (o);
  switch ($rl_m.kind) {
    case "Some": { const { value } = $rl_m; return (`${value}`); }
    case "None": { return (fallback); }
    default: { throw new Error("rl match: unexpected case " + JSON.stringify($rl_m)); }
  }
})());
}

// Plain TypeScript keeps working as-is — including TypeScript's own enum
// (unit-only, so rlc leaves it alone) and `.match` and friends:
enum LogLevel { Debug, Info, Warn }
const level = LogLevel.Info;
const digits = "shape-42".match(/\d+/)?.[0] ?? "none";

const shapes: Shape[] = [Shape.Circle(1), Shape.Rect(3, 4), Shape.Point];
for (const s of shapes) {
  console.log(describe(s), area(s), digits, level);
}

console.log(label(Option.Some(7), "none"), Option.unwrapOr<number>(Option.None, 42));
