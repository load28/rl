// rl standard library — `Option` and `Result` with functional combinators.
//
// Plain TypeScript whose values have exactly the shape rl enums compile to
// (`kind`-tagged objects), so `match` works on these values and rlc checks
// exhaustiveness of `Some`/`None` and `Ok`/`Err` arms. `Result`'s variants
// additionally get names of their own (`Ok<T>`/`Err<E>`) so each constructor
// can be typed by what its value really carries. See docs/reference/std.md.

/** An optional value: either `Some` carrying a `value`, or `None`. */
export type Option<T> =
  | { kind: "Some"; value: T }
  | { kind: "None" };

/** The success variant of a `Result`, carrying a `value`. */
export type Ok<T> = { kind: "Ok"; value: T };

/** The failure variant of a `Result`, carrying an `error`. */
export type Err<E> = { kind: "Err"; error: E };

/** A success (`Ok` carrying a `value`) or a failure (`Err` carrying an `error`). */
export type Result<T, E> =
  | Ok<T>
  | Err<E>;

/** Constructors and combinators for `Option`. */
export const Option = {
  Some: <T>(value: T): Option<T> => ({ kind: "Some", value }),
  None: { kind: "None" } as const,

  /** True if `o` is `Some` (narrows the type). */
  isSome: <T>(o: Option<T>): o is { kind: "Some"; value: T } =>
    o.kind === "Some",
  /** True if `o` is `None` (narrows the type). */
  isNone: <T>(o: Option<T>): o is { kind: "None" } => o.kind === "None",
  /** Applies `f` to the value inside `Some`; leaves `None` untouched. */
  map: <T, U>(o: Option<T>, f: (value: T) => U): Option<U> =>
    o.kind === "Some" ? { kind: "Some", value: f(o.value) } : { kind: "None" },
  /** Chains a computation that itself returns an `Option`. */
  andThen: <T, U>(o: Option<T>, f: (value: T) => Option<U>): Option<U> =>
    o.kind === "Some" ? f(o.value) : { kind: "None" },
  /** Returns `o` if it is `Some`, otherwise the fallback produced by `f`. */
  orElse: <T>(o: Option<T>, f: () => Option<T>): Option<T> =>
    o.kind === "Some" ? o : f(),
  /** Keeps `Some` only when the predicate holds. */
  filter: <T>(o: Option<T>, pred: (value: T) => boolean): Option<T> =>
    o.kind === "Some" && pred(o.value) ? o : { kind: "None" },
  /** The value inside `Some`, or `fallback` for `None`. */
  unwrapOr: <T>(o: Option<T>, fallback: T): T =>
    o.kind === "Some" ? o.value : fallback,
  /** The value inside `Some`, or the result of `f` for `None`. */
  unwrapOrElse: <T>(o: Option<T>, f: () => T): T =>
    o.kind === "Some" ? o.value : f(),
  /** The value inside `Some`; throws `Error(message)` for `None`. */
  expect: <T>(o: Option<T>, message: string): T => {
    if (o.kind === "Some") return o.value;
    throw new Error(message);
  },
  /** Converts to a `Result`: `Some` becomes `Ok`, `None` becomes `Err(error)`. */
  okOr: <T, E>(o: Option<T>, error: E): Result<T, E> =>
    o.kind === "Some" ? { kind: "Ok", value: o.value } : { kind: "Err", error },
  /** Wraps a nullable value: `null`/`undefined` become `None`. */
  fromNullable: <T>(value: T | null | undefined): Option<T> =>
    value == null ? { kind: "None" } : { kind: "Some", value },
  /** Unwraps to a nullable value: `None` becomes `null`. */
  toNullable: <T>(o: Option<T>): T | null =>
    o.kind === "Some" ? o.value : null,
  /** Pairs two options: `Some` of the tuple only when both are `Some`. */
  zip: <T, U>(a: Option<T>, b: Option<U>): Option<[T, U]> =>
    a.kind === "Some" && b.kind === "Some"
      ? { kind: "Some", value: [a.value, b.value] }
      : { kind: "None" },
  /** Flattens one level of nesting: `Some(Some(x))` becomes `Some(x)`. */
  flatten: <T>(o: Option<Option<T>>): Option<T> =>
    o.kind === "Some" ? o.value : { kind: "None" },
  /**
   * Swaps the layers: `Some(Ok(x))` → `Ok(Some(x))`, `Some(Err(e))` →
   * `Err(e)`, `None` → `Ok(None)`.
   */
  transpose: <T, E>(o: Option<Result<T, E>>): Result<Option<T>, E> =>
    o.kind === "None"
      ? { kind: "Ok", value: { kind: "None" } }
      : o.value.kind === "Ok"
        ? { kind: "Ok", value: { kind: "Some", value: o.value.value } }
        : o.value,
  /** Collects an array: all `Some` → `Some` of the values, any `None` → `None`. */
  collect: <T>(items: readonly Option<T>[]): Option<T[]> => {
    const values: T[] = [];
    for (const o of items) {
      if (o.kind === "None") return { kind: "None" };
      values.push(o.value);
    }
    return { kind: "Some", value: values };
  },

  // Data-last curried variants (`P` suffix) for the `|>` pipeline operator:
  // `o |> Option.mapP(f)` instead of `Option.map(o, f)`.

  /** Curried `map` for pipelines. */
  mapP:
    <T, U>(f: (value: T) => U) =>
    (o: Option<T>): Option<U> =>
      o.kind === "Some" ? { kind: "Some", value: f(o.value) } : { kind: "None" },
  /** Curried `andThen` for pipelines. */
  andThenP:
    <T, U>(f: (value: T) => Option<U>) =>
    (o: Option<T>): Option<U> =>
      o.kind === "Some" ? f(o.value) : { kind: "None" },
  /** Curried `orElse` for pipelines. */
  orElseP:
    <T>(f: () => Option<T>) =>
    (o: Option<T>): Option<T> =>
      o.kind === "Some" ? o : f(),
  /** Curried `filter` for pipelines. */
  filterP:
    <T>(pred: (value: T) => boolean) =>
    (o: Option<T>): Option<T> =>
      o.kind === "Some" && pred(o.value) ? o : { kind: "None" },
  /** Curried `unwrapOr` for pipelines. */
  unwrapOrP:
    <T>(fallback: T) =>
    (o: Option<T>): T =>
      o.kind === "Some" ? o.value : fallback,
  /** Curried `unwrapOrElse` for pipelines. */
  unwrapOrElseP:
    <T>(f: () => T) =>
    (o: Option<T>): T =>
      o.kind === "Some" ? o.value : f(),
  /** Curried `expect` for pipelines. */
  expectP:
    <T>(message: string) =>
    (o: Option<T>): T => {
      if (o.kind === "Some") return o.value;
      throw new Error(message);
    },
  /** Curried `okOr` for pipelines. */
  okOrP:
    <T, E>(error: E) =>
    (o: Option<T>): Result<T, E> =>
      o.kind === "Some" ? { kind: "Ok", value: o.value } : { kind: "Err", error },
};

/** Constructors and combinators for `Result`. */
export const Result = {
  // Each constructor takes only the type its own variant actually carries:
  // `Ok(v)` is an `Ok<T>`, `Err(e)` is an `Err<E>`. A `Result<T, E>` is the
  // union of the two, so both still fit wherever one is expected — and a
  // function with several `try`s gets `Ok<T> | Err<E1> | Err<E2>` inferred
  // instead of an `unknown` error. See docs/reference/std.md.
  Ok: <T>(value: T): Ok<T> => ({ kind: "Ok", value }),
  Err: <E>(error: E): Err<E> => ({ kind: "Err", error }),

  /** True if `r` is `Ok` (narrows the type). */
  isOk: <T, E>(r: Result<T, E>): r is Ok<T> => r.kind === "Ok",
  /** True if `r` is `Err` (narrows the type). */
  isErr: <T, E>(r: Result<T, E>): r is Err<E> => r.kind === "Err",
  /** Applies `f` to the `Ok` value; leaves `Err` untouched. */
  map: <T, E, U>(r: Result<T, E>, f: (value: T) => U): Result<U, E> =>
    r.kind === "Ok" ? { kind: "Ok", value: f(r.value) } : r,
  /** Applies `f` to the `Err` error; leaves `Ok` untouched. */
  mapErr: <T, E, F>(r: Result<T, E>, f: (error: E) => F): Result<T, F> =>
    r.kind === "Err" ? { kind: "Err", error: f(r.error) } : r,
  /** Chains a computation that itself returns a `Result`. */
  andThen: <T, E, U>(
    r: Result<T, E>,
    f: (value: T) => Result<U, E>,
  ): Result<U, E> => (r.kind === "Ok" ? f(r.value) : r),
  /** Recovers from `Err` with a computation returning a `Result`. */
  orElse: <T, E, F>(
    r: Result<T, E>,
    f: (error: E) => Result<T, F>,
  ): Result<T, F> => (r.kind === "Ok" ? r : f(r.error)),
  /** The `Ok` value, or `fallback` for `Err`. */
  unwrapOr: <T, E>(r: Result<T, E>, fallback: T): T =>
    r.kind === "Ok" ? r.value : fallback,
  /** The `Ok` value, or the result of `f(error)` for `Err`. */
  unwrapOrElse: <T, E>(r: Result<T, E>, f: (error: E) => T): T =>
    r.kind === "Ok" ? r.value : f(r.error),
  /** The `Ok` value; throws `Error(message)` for `Err`. */
  expect: <T, E>(r: Result<T, E>, message: string): T => {
    if (r.kind === "Ok") return r.value;
    throw new Error(message);
  },
  /** The `Ok` value as an `Option` (drops the error). */
  ok: <T, E>(r: Result<T, E>): Option<T> =>
    r.kind === "Ok" ? { kind: "Some", value: r.value } : { kind: "None" },
  /** The `Err` error as an `Option`. */
  err: <T, E>(r: Result<T, E>): Option<E> =>
    r.kind === "Err" ? { kind: "Some", value: r.error } : { kind: "None" },
  /** Runs `f`, capturing a thrown exception as `Err`. */
  fromThrowable: <T>(f: () => T): Result<T, unknown> => {
    try {
      return { kind: "Ok", value: f() };
    } catch (error) {
      return { kind: "Err", error };
    }
  },
  /** Awaits `p`, capturing a rejection as `Err` — `fromThrowable`'s async twin. */
  fromPromise: <T>(p: Promise<T>): Promise<Result<T, unknown>> =>
    p.then(
      (value): Result<T, unknown> => ({ kind: "Ok", value }),
      (error): Result<T, unknown> => ({ kind: "Err", error }),
    ),
  /** Flattens one level of nesting: `Ok(Ok(x))` becomes `Ok(x)`. */
  flatten: <T, E>(r: Result<Result<T, E>, E>): Result<T, E> =>
    r.kind === "Ok" ? r.value : r,
  /**
   * Swaps the layers: `Ok(Some(x))` → `Some(Ok(x))`, `Ok(None)` → `None`,
   * `Err(e)` → `Some(Err(e))`.
   */
  transpose: <T, E>(r: Result<Option<T>, E>): Option<Result<T, E>> =>
    r.kind === "Err"
      ? { kind: "Some", value: r }
      : r.value.kind === "Some"
        ? { kind: "Some", value: { kind: "Ok", value: r.value.value } }
        : { kind: "None" },
  /** Collects an array: all `Ok` → `Ok` of the values, otherwise the first `Err`. */
  collect: <T, E>(items: readonly Result<T, E>[]): Result<T[], E> => {
    const values: T[] = [];
    for (const r of items) {
      if (r.kind === "Err") return r;
      values.push(r.value);
    }
    return { kind: "Ok", value: values };
  },

  // Data-last curried variants (`P` suffix) for the `|>` pipeline operator:
  // `r |> Result.mapP(f)` instead of `Result.map(r, f)`.

  /** Curried `map` for pipelines. */
  mapP:
    <T, E, U>(f: (value: T) => U) =>
    (r: Result<T, E>): Result<U, E> =>
      r.kind === "Ok" ? { kind: "Ok", value: f(r.value) } : r,
  /** Curried `mapErr` for pipelines. */
  mapErrP:
    <T, E, F>(f: (error: E) => F) =>
    (r: Result<T, E>): Result<T, F> =>
      r.kind === "Err" ? { kind: "Err", error: f(r.error) } : r,
  /** Curried `andThen` for pipelines. */
  andThenP:
    <T, E, U>(f: (value: T) => Result<U, E>) =>
    (r: Result<T, E>): Result<U, E> =>
      r.kind === "Ok" ? f(r.value) : r,
  /** Curried `orElse` for pipelines. */
  orElseP:
    <T, E, F>(f: (error: E) => Result<T, F>) =>
    (r: Result<T, E>): Result<T, F> =>
      r.kind === "Ok" ? r : f(r.error),
  /** Curried `unwrapOr` for pipelines. */
  unwrapOrP:
    <T, E>(fallback: T) =>
    (r: Result<T, E>): T =>
      r.kind === "Ok" ? r.value : fallback,
  /** Curried `unwrapOrElse` for pipelines. */
  unwrapOrElseP:
    <T, E>(f: (error: E) => T) =>
    (r: Result<T, E>): T =>
      r.kind === "Ok" ? r.value : f(r.error),
  /** Curried `expect` for pipelines. */
  expectP:
    <T, E>(message: string) =>
    (r: Result<T, E>): T => {
      if (r.kind === "Ok") return r.value;
      throw new Error(message);
    },
};
