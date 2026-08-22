// Tree-shakeable Option constructors and combinators.

import type { TResult } from "./result.js";

/** An optional value: either `Some` carrying a `value`, or `None`. */
export type TOption<T> =
  | { kind: "Some"; value: T }
  | { kind: "None" };

/** Constructs `Some(value)`. */
export const Some = <T>(value: T): TOption<T> => ({ kind: "Some", value });

/** The `None` value. */
export const None = { kind: "None" } as const;

/** True if `o` is `Some` (narrows the type). */
export const isSome = <T>(o: TOption<T>): o is { kind: "Some"; value: T } =>
  o.kind === "Some";

/** True if `o` is `None` (narrows the type). */
export const isNone = <T>(o: TOption<T>): o is { kind: "None" } => o.kind === "None";

/** Applies `f` to the value inside `Some`; leaves `None` untouched. */
export const map = <T, U>(o: TOption<T>, f: (value: T) => U): TOption<U> =>
  o.kind === "Some" ? Some(f(o.value)) : None;

/** Chains a computation that itself returns an `Option`. */
export const andThen = <T, U>(
  o: TOption<T>,
  f: (value: T) => TOption<U>,
): TOption<U> => (o.kind === "Some" ? f(o.value) : None);

/** Returns `o` if it is `Some`, otherwise the fallback produced by `f`. */
export const orElse = <T>(o: TOption<T>, f: () => TOption<T>): TOption<T> =>
  o.kind === "Some" ? o : f();

/** Keeps `Some` only when the predicate holds. */
export const filter = <T>(
  o: TOption<T>,
  pred: (value: T) => boolean,
): TOption<T> => (o.kind === "Some" && pred(o.value) ? o : None);

/** The value inside `Some`, or `fallback` for `None`. */
export const unwrapOr = <T>(o: TOption<T>, fallback: T): T =>
  o.kind === "Some" ? o.value : fallback;

/** The value inside `Some`, or the result of `f` for `None`. */
export const unwrapOrElse = <T>(o: TOption<T>, f: () => T): T =>
  o.kind === "Some" ? o.value : f();

/** The value inside `Some`; throws `Error(message)` for `None`. */
export const expect = <T>(o: TOption<T>, message: string): T => {
  if (o.kind === "Some") return o.value;
  throw new Error(message);
};

/** Converts to a `Result`: `Some` becomes `Ok`, `None` becomes `Err(error)`. */
export const okOr = <T, E>(o: TOption<T>, error: E): TResult<T, E> =>
  o.kind === "Some" ? { kind: "Ok", value: o.value } : { kind: "Err", error };

/** Wraps a nullable value: `null`/`undefined` become `None`. */
export const fromNullable = <T>(value: T | null | undefined): TOption<T> =>
  value == null ? None : Some(value);

/** Unwraps to a nullable value: `None` becomes `null`. */
export const toNullable = <T>(o: TOption<T>): T | null =>
  o.kind === "Some" ? o.value : null;

/** Pairs two options: `Some` of the tuple only when both are `Some`. */
export const zip = <T, U>(a: TOption<T>, b: TOption<U>): TOption<[T, U]> =>
  a.kind === "Some" && b.kind === "Some" ? Some([a.value, b.value]) : None;

/** Flattens one level of nesting: `Some(Some(x))` becomes `Some(x)`. */
export const flatten = <T>(o: TOption<TOption<T>>): TOption<T> =>
  o.kind === "Some" ? o.value : None;

/** Swaps `Option<Result<T, E>>` into `Result<Option<T>, E>`. */
export const transpose = <T, E>(o: TOption<TResult<T, E>>): TResult<TOption<T>, E> =>
  o.kind === "None"
    ? { kind: "Ok", value: None }
    : o.value.kind === "Ok"
      ? { kind: "Ok", value: Some(o.value.value) }
      : o.value;

/** Collects all `Some` values, or returns `None`. */
export const collect = <T>(items: readonly TOption<T>[]): TOption<T[]> => {
  const values: T[] = [];
  for (const o of items) {
    if (o.kind === "None") return None;
    values.push(o.value);
  }
  return Some(values);
};

/** Curried `map` for pipelines. */
export const mapP =
  <T, U>(f: (value: T) => U) =>
  (o: TOption<T>): TOption<U> =>
    o.kind === "Some" ? Some(f(o.value)) : None;

/** Curried `andThen` for pipelines. */
export const andThenP =
  <T, U>(f: (value: T) => TOption<U>) =>
  (o: TOption<T>): TOption<U> =>
    o.kind === "Some" ? f(o.value) : None;

/** Curried `orElse` for pipelines. */
export const orElseP =
  <T>(f: () => TOption<T>) =>
  (o: TOption<T>): TOption<T> =>
    o.kind === "Some" ? o : f();

/** Curried `filter` for pipelines. */
export const filterP =
  <T>(pred: (value: T) => boolean) =>
  (o: TOption<T>): TOption<T> =>
    o.kind === "Some" && pred(o.value) ? o : None;

/** Curried `unwrapOr` for pipelines. */
export const unwrapOrP =
  <T>(fallback: T) =>
  (o: TOption<T>): T =>
    o.kind === "Some" ? o.value : fallback;

/** Curried `unwrapOrElse` for pipelines. */
export const unwrapOrElseP =
  <T>(f: () => T) =>
  (o: TOption<T>): T =>
    o.kind === "Some" ? o.value : f();

/** Curried `expect` for pipelines. */
export const expectP =
  <T>(message: string) =>
  (o: TOption<T>): T => {
    if (o.kind === "Some") return o.value;
    throw new Error(message);
  };

/** Curried `okOr` for pipelines. */
export const okOrP =
  <T, E>(error: E) =>
  (o: TOption<T>): TResult<T, E> =>
    o.kind === "Some" ? { kind: "Ok", value: o.value } : { kind: "Err", error };
