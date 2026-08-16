// examples/shapes.rl — rl is TypeScript plus rl `enum` and `match`.
// Everything here that isn't one of those two constructs is plain TypeScript
// and passes through the compiler untouched.

export enum Shape {
  Circle(radius: number),
  Rect(width: number, height: number),
  Point,
}

export enum Option<T> {
  Some(value: T),
  None,
}

export function area(s: Shape): number {
  return match (s) {
    Circle(radius) => Math.PI * radius * radius,
    Rect(width, height) => width * height,
    Point => 0,
  };
}

export function describe(s: Shape): string {
  return match (s) {
    // bindings can be renamed with `name: alias`
    Rect(width: w, height: h) => {
      const ratio = w / h;
      return `rect (ratio ${ratio.toFixed(2)})`;
    },
    _ => `some shape with area ${area(s).toFixed(2)}`,
  };
}

export function unwrapOr<T>(o: Option<T>, fallback: T): T {
  return match (o) {
    Some(value) => value,
    None => fallback,
  };
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

console.log(unwrapOr(Option.Some(7), 0), unwrapOr<number>(Option.None, 42));
