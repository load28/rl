// rl standard library — `Option` and `Result` with functional combinators.
//
// Plain TypeScript in exactly the shape rl enums compile to (`kind`-tagged
// unions), so `match` works on these values and rlc checks exhaustiveness
// of `Some`/`None` and `Ok`/`Err` arms. See docs/reference/std.md.

/** An optional value: either `Some` carrying a `value`, or `None`. */
export type Option<T> =
  | { kind: "Some"; value: T }
  | { kind: "None" };

/** A success (`Ok` carrying a `value`) or a failure (`Err` carrying an `error`). */
export type Result<T, E> =
  | { kind: "Ok"; value: T }
  | { kind: "Err"; error: E };

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
};

/** Constructors and combinators for `Result`. */
export const Result = {
  Ok: <T, E>(value: T): Result<T, E> => ({ kind: "Ok", value }),
  Err: <T, E>(error: E): Result<T, E> => ({ kind: "Err", error }),

  /** True if `r` is `Ok` (narrows the type). */
  isOk: <T, E>(r: Result<T, E>): r is { kind: "Ok"; value: T } =>
    r.kind === "Ok",
  /** True if `r` is `Err` (narrows the type). */
  isErr: <T, E>(r: Result<T, E>): r is { kind: "Err"; error: E } =>
    r.kind === "Err",
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
};
