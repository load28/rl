// Tree-shakeable Result constructors and combinators.

import type { TOption } from "./option.js";

/** The success variant of a `Result`, carrying a `value`. */
export type TOk<T> = { kind: "Ok"; value: T };

/** The failure variant of a `Result`, carrying an `error`. */
export type TErr<E> = { kind: "Err"; error: E };

/** A success or failure value. */
export type TResult<T, E> = TOk<T> | TErr<E>;

/** The union of error types carried by a result type. */
export type TErrorOf<R> = R extends TErr<infer E> ? E : never;

/** Constructs `Ok(value)`. */
export const Ok = <T>(value: T): TOk<T> => ({ kind: "Ok", value });

/** Constructs `Err(error)`. */
export const Err = <E>(error: E): TErr<E> => ({ kind: "Err", error });

/** True if `r` is `Ok` (narrows the type). */
export const isOk = <T, E>(r: TResult<T, E>): r is TOk<T> => r.kind === "Ok";

/** True if `r` is `Err` (narrows the type). */
export const isErr = <T, E>(r: TResult<T, E>): r is TErr<E> => r.kind === "Err";

/** Applies `f` to the `Ok` value; leaves `Err` untouched. */
export const map = <T, E, U>(r: TResult<T, E>, f: (value: T) => U): TResult<U, E> =>
  r.kind === "Ok" ? Ok(f(r.value)) : r;

/** Applies `f` to the `Err` error; leaves `Ok` untouched. */
export const mapErr = <T, E, F>(
  r: TResult<T, E>,
  f: (error: E) => F,
): TResult<T, F> => (r.kind === "Err" ? Err(f(r.error)) : r);

/** Chains a computation and unions its error with the incoming errors. */
export const andThen = <T, U, F, R extends TResult<T, unknown>>(
  r: R & TResult<T, unknown>,
  f: (value: T) => TResult<U, F>,
): TResult<U, TErrorOf<R> | F> =>
  r.kind === "Ok" ? f(r.value) : (r as TErr<TErrorOf<R>>);

/** Recovers from `Err` with a computation returning a `Result`. */
export const orElse = <T, E, F>(
  r: TResult<T, E>,
  f: (error: E) => TResult<T, F>,
): TResult<T, F> => (r.kind === "Ok" ? r : f(r.error));

/** The `Ok` value, or `fallback` for `Err`. */
export const unwrapOr = <T, E>(r: TResult<T, E>, fallback: T): T =>
  r.kind === "Ok" ? r.value : fallback;

/** The `Ok` value, or the result of `f(error)` for `Err`. */
export const unwrapOrElse = <T, E>(r: TResult<T, E>, f: (error: E) => T): T =>
  r.kind === "Ok" ? r.value : f(r.error);

/** The `Ok` value; throws `Error(message)` for `Err`. */
export const expect = <T, E>(r: TResult<T, E>, message: string): T => {
  if (r.kind === "Ok") return r.value;
  throw new Error(message);
};

/** The `Ok` value as an `Option` (drops the error). */
export const ok = <T, E>(r: TResult<T, E>): TOption<T> =>
  r.kind === "Ok" ? { kind: "Some", value: r.value } : { kind: "None" };

/** The `Err` error as an `Option`. */
export const err = <T, E>(r: TResult<T, E>): TOption<E> =>
  r.kind === "Err" ? { kind: "Some", value: r.error } : { kind: "None" };

/** Runs `f`, capturing a thrown exception as `Err`. */
export const fromThrowable = <T>(f: () => T): TResult<T, unknown> => {
  try {
    return Ok(f());
  } catch (error) {
    return Err(error);
  }
};

/** Awaits `p`, capturing a rejection as `Err`. */
export const fromPromise = <T>(p: Promise<T>): Promise<TResult<T, unknown>> =>
  p.then(
    (value): TResult<T, unknown> => Ok(value),
    (error): TResult<T, unknown> => Err(error),
  );

/** Flattens one level of nesting. */
export const flatten = <T, E>(r: TResult<TResult<T, E>, E>): TResult<T, E> =>
  r.kind === "Ok" ? r.value : r;

/** Swaps `Result<Option<T>, E>` into `Option<Result<T, E>>`. */
export const transpose = <T, E>(r: TResult<TOption<T>, E>): TOption<TResult<T, E>> =>
  r.kind === "Err"
    ? { kind: "Some", value: r }
    : r.value.kind === "Some"
      ? { kind: "Some", value: Ok(r.value.value) }
      : { kind: "None" };

/** Collects all `Ok` values, or returns the first `Err`. */
export const collect = <T, E>(items: readonly TResult<T, E>[]): TResult<T[], E> => {
  const values: T[] = [];
  for (const r of items) {
    if (r.kind === "Err") return r;
    values.push(r.value);
  }
  return Ok(values);
};

/** Curried `map` for pipelines. */
export const mapP =
  <T, E, U>(f: (value: T) => U) =>
  (r: TResult<T, E>): TResult<U, E> =>
    r.kind === "Ok" ? Ok(f(r.value)) : r;

/** Curried `mapErr` for pipelines. */
export const mapErrP =
  <T, E, F>(f: (error: E) => F) =>
  (r: TResult<T, E>): TResult<T, F> =>
    r.kind === "Err" ? Err(f(r.error)) : r;

/** Curried `andThen` for pipelines. */
export const andThenP =
  <T, U, F>(f: (value: T) => TResult<U, F>) =>
  <R extends TResult<T, unknown>>(r: R): TResult<U, TErrorOf<R> | F> =>
    r.kind === "Ok" ? f(r.value) : (r as TErr<TErrorOf<R>>);

/** Curried `orElse` for pipelines. */
export const orElseP =
  <T, E, F>(f: (error: E) => TResult<T, F>) =>
  (r: TResult<T, E>): TResult<T, F> =>
    r.kind === "Ok" ? r : f(r.error);

/** Curried `unwrapOr` for pipelines. */
export const unwrapOrP =
  <T, E>(fallback: T) =>
  (r: TResult<T, E>): T =>
    r.kind === "Ok" ? r.value : fallback;

/** Curried `unwrapOrElse` for pipelines. */
export const unwrapOrElseP =
  <T, E>(f: (error: E) => T) =>
  (r: TResult<T, E>): T =>
    r.kind === "Ok" ? r.value : f(r.error);

/** Curried `expect` for pipelines. */
export const expectP =
  <T, E>(message: string) =>
  (r: TResult<T, E>): T => {
    if (r.kind === "Ok") return r.value;
    throw new Error(message);
  };
