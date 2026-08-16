// @generated from shapes.rl by rlc — do not edit directly.
// examples/shapes.rl — rl is TypeScript plus `variant` and `match`.
// Everything here that isn't one of those two constructs is plain TypeScript
// and passes through the compiler untouched.

export type Shape =
  | { kind: "Circle"; radius: number }
  | { kind: "Rect"; width: number; height: number }
  | { kind: "Point" };
export const Shape = {
  Circle: (radius: number): Shape => ({ kind: "Circle", radius }),
  Rect: (width: number, height: number): Shape => ({ kind: "Rect", width, height }),
  Point: { kind: "Point" } as const,
};

export type Option<T> =
  | { kind: "Some"; value: T }
  | { kind: "None" };
export const Option = {
  Some: <T>(value: T): Option<T> => ({ kind: "Some", value }),
  None: { kind: "None" } as const,
};

export function area(s: Shape): number {
  return ((() => {
  const $rl_m = (s);
  switch ($rl_m.kind) {
    case "Circle": { const { radius } = $rl_m; return (Math.PI * radius * radius); }
    case "Rect": { const { width, height } = $rl_m; return (width * height); }
    case "Point": { return (0); }
    default: {
      const $rl_never: never = $rl_m;
      throw new Error("rl match: unhandled variant " + JSON.stringify($rl_never));
    }
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

export function unwrapOr<T>(o: Option<T>, fallback: T): T {
  return ((() => {
  const $rl_m = (o);
  switch ($rl_m.kind) {
    case "Some": { const { value } = $rl_m; return (value); }
    case "None": { return (fallback); }
    default: {
      const $rl_never: never = $rl_m;
      throw new Error("rl match: unhandled variant " + JSON.stringify($rl_never));
    }
  }
})());
}

// Plain TypeScript keeps working as-is — including `.match` and friends:
const digits = "shape-42".match(/\d+/)?.[0] ?? "none";

const shapes: Shape[] = [Shape.Circle(1), Shape.Rect(3, 4), Shape.Point];
for (const s of shapes) {
  console.log(describe(s), area(s), digits);
}

console.log(unwrapOr(Option.Some(7), 0), unwrapOr<number>(Option.None, 42));
