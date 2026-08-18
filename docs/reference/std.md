# rl 표준 라이브러리 레퍼런스

`@rl/std`가 제공하는 `Option<T>`/`Result<T, E>`와 콤비네이터입니다. 언어에서의
위치(내장 enum, 소진성 검사)는
[`language.md` §4](./language.md#4-표준-라이브러리와-내장-enum), 실체화 방식은
[`cli.md`](./cli.md)를 보세요.

```rl
import { Option, Result } from "@rl/std";
```

지정자가 bare인 이유는 모듈의 위치가 소비 층마다 다르기 때문입니다.

| 소비자 | 해석 |
|--------|------|
| `rlc` | 출력 트리에 모듈을 자동으로 쓰고 지정자를 그 상대 경로로 바꿉니다 |
| 번들러 | 플러그인이 가상 모듈로 제공합니다 — 파일이 생기지 않습니다 |
| tsc·에디터 | `tsconfig.json`의 `paths`로 매핑합니다 (`rlc --types`가 `rl.d.ts`를 함께 만듭니다) |

파일이 직접 필요하면 `rlc --emit-std`로 stdout에 받을 수 있습니다.

## 값의 형태 계약

모듈이 만드는 **값**은 아래 rl enum을 컴파일한 결과와 **바이트 단위로 같은
형태**입니다 (컴파일러 테스트로 보장).

```rl
export enum Option<T> { Some(value: T), None }
export enum Result<T, E> { Ok(value: T), Err(error: E) }
```

즉 값은 순수 데이터(`kind` 태그드 객체)라 `match`·소진성 검사·`JSON.stringify`가
그대로 동작합니다. 페이로드 필드명은 `Some`/`Ok`가 `value`, `Err`가 `error`이고,
match 바인딩은 이름 기준이므로 `Some(value)`·`Err(error)` 또는 별칭
`Some(value: v)`로 씁니다. 콤비네이터는 **데이터-우선 정적 함수**가 기본이고
(메서드 체이닝 없음), 파이프라인 연산자 `|>`용으로 **data-last 커링 변형**이
`P` 접미사로 함께 제공됩니다 ([§파이프라인 변형](#파이프라인-변형-p)).

`Result`는 두 케이스에 각각 이름이 붙어 있고, 생성자는 자기 케이스가 실제로
담는 타입만 받습니다.

```ts
export type Ok<T> = { kind: "Ok"; value: T };
export type Err<E> = { kind: "Err"; error: E };
export type Result<T, E> = Ok<T> | Err<E>;

Result.Ok(123)     // Ok<number>
Result.Err("bad")  // Err<string>
```

`Result<T, E>`가 곧 `Ok<T> | Err<E>`이므로 개별 변종은 `Result`가 필요한 자리에
그대로 들어갑니다.

```rl
const r: Result<number, string> = Result.Ok(1);     // OK
const e: Result<number, string> = Result.Err("bad"); // OK

function parse(value: string): Result<number, string> {
  if (value.length === 0) {
    return Result.Err("empty");
  }
  return Result.Ok(Number(value));
}
```

`Ok`/`Err`는 타입만 내보냅니다 (`import type { Ok, Err } from "@rl/std";`).
값 생성은 `Result.Ok`/`Result.Err`로만 합니다.

### 여러 `try`의 에러 타입

생성자가 없는 타입을 지어내지 않으므로, 반환 타입을 적지 않은 함수의 추론이
정확해집니다. `try`는 `Err`를 그대로 둘러싼 함수에서 return하므로
([`language.md` §5.3](./language.md#53-컴파일-결과)) 반환 경로가 곧 변종의
합집합입니다.

```rl
function load() {
  const user = try getUser();     // Result<User, UserError>
  const config = try getConfig(); // Result<Config, ConfigError>
  return Result.Ok({ user, config });
}
// 추론: Ok<Data> | Err<UserError> | Err<ConfigError>
//     = Result<Data, UserError | ConfigError>
```

Rust처럼 하나의 에러 타입으로 `From` 변환을 강요하지 않고, 실제 에러 집합을
유니언으로 유지합니다. rlc는 에러 타입을 모으거나 유니언을 만들지 않습니다 —
lowering은 평범한 TypeScript 반환문이고, 추론은 tsc가 합니다.

## `Option<T>`

| 함수 | 시그니처 | 설명 |
|------|----------|------|
| `Some` | `<T>(value: T) => Option<T>` | 값이 있는 케이스 생성 |
| `None` | `Option<T>` (싱글턴) | 값이 없는 케이스 |
| `isSome` | `(o: Option<T>) => boolean` | `Some`이면 true (타입 내로잉 가드) |
| `isNone` | `(o: Option<T>) => boolean` | `None`이면 true (타입 내로잉 가드) |
| `map` | `(o, f: (T) => U) => Option<U>` | `Some` 안의 값에 `f` 적용 |
| `andThen` | `(o, f: (T) => Option<U>) => Option<U>` | `Option`을 반환하는 계산 연결 (flatMap) |
| `orElse` | `(o, f: () => Option<T>) => Option<T>` | `None`이면 `f()`로 대체 |
| `filter` | `(o, pred: (T) => boolean) => Option<T>` | 술어가 거짓이면 `None` |
| `unwrapOr` | `(o, fallback: T) => T` | 값 또는 기본값 |
| `unwrapOrElse` | `(o, f: () => T) => T` | 값 또는 `f()` |
| `expect` | `(o, message: string) => T` | 값 또는 `Error(message)` throw |
| `okOr` | `(o, error: E) => Result<T, E>` | `Some`→`Ok`, `None`→`Err(error)` |
| `fromNullable` | `(value: T \| null \| undefined) => Option<T>` | nullable 값 감싸기 |
| `toNullable` | `(o) => T \| null` | `None`→`null`로 풀기 |
| `zip` | `(a: Option<T>, b: Option<U>) => Option<[T, U]>` | 둘 다 `Some`일 때만 튜플로 묶기 |
| `flatten` | `(o: Option<Option<T>>) => Option<T>` | 중첩 한 겹 풀기 |
| `transpose` | `(o: Option<Result<T, E>>) => Result<Option<T>, E>` | 층 교환: `None`→`Ok(None)`, `Some(Err(e))`→`Err(e)` |
| `collect` | `(items: readonly Option<T>[]) => Option<T[]>` | 전부 `Some`이면 값 배열, 하나라도 `None`이면 `None` |

## `Result<T, E>`

| 함수 | 시그니처 | 설명 |
|------|----------|------|
| `Ok` | `<T>(value: T) => Ok<T>` | 성공 케이스 생성 |
| `Err` | `<E>(error: E) => Err<E>` | 실패 케이스 생성 |
| `isOk` | `(r: Result<T, E>) => r is Ok<T>` | `Ok`이면 true (타입 내로잉 가드) |
| `isErr` | `(r: Result<T, E>) => r is Err<E>` | `Err`이면 true (타입 내로잉 가드) |
| `map` | `(r, f: (T) => U) => Result<U, E>` | `Ok` 값에 `f` 적용 |
| `mapErr` | `(r, f: (E) => F) => Result<T, F>` | `Err` 에러에 `f` 적용 |
| `andThen` | `(r, f: (T) => Result<U, E>) => Result<U, E>` | `Result`를 반환하는 계산 연결 |
| `orElse` | `(r, f: (E) => Result<T, F>) => Result<T, F>` | `Err`에서 복구 |
| `unwrapOr` | `(r, fallback: T) => T` | `Ok` 값 또는 기본값 |
| `unwrapOrElse` | `(r, f: (E) => T) => T` | `Ok` 값 또는 `f(error)` |
| `expect` | `(r, message: string) => T` | `Ok` 값 또는 `Error(message)` throw |
| `ok` | `(r) => Option<T>` | `Ok` 값을 `Option`으로 (에러 버림) |
| `err` | `(r) => Option<E>` | `Err` 에러를 `Option`으로 |
| `fromThrowable` | `<T>(f: () => T) => Result<T, unknown>` | `f` 실행, throw를 `Err`로 포획 |
| `fromPromise` | `<T>(p: Promise<T>) => Promise<Result<T, unknown>>` | rejection을 `Err`로 포획 (`fromThrowable`의 비동기 짝) |
| `flatten` | `(r: Result<Result<T, E>, E>) => Result<T, E>` | 중첩 한 겹 풀기 |
| `transpose` | `(r: Result<Option<T>, E>) => Option<Result<T, E>>` | 층 교환: `Ok(None)`→`None`, `Err(e)`→`Some(Err(e))` |
| `collect` | `(items: readonly Result<T, E>[]) => Result<T[], E>` | 전부 `Ok`이면 값 배열, 아니면 첫 `Err` |

생성자 외의 콤비네이터는 전부 `Result<T, E>`를 그대로 쓰므로 변종 타입을
의식할 일이 없습니다. 전체 `Result` 타입이 필요하면 주변 문맥(변수 주석,
함수 반환 타입)에서 지정하세요 — `Result.Ok<number, string>(1)`처럼 두 개의
타입 인자를 넘기는 형태는 더 이상 없습니다 (`Result.Ok<number>(1)`).

`Result`를 반환하는 연산을 여러 단계 잇는 코드는 `andThen`을 중첩하는 대신
`result` 계산 블록으로 평탄하게 씁니다
([`language.md` §8](./language.md#8-result-계산-블록)) — 단계마다 에러 타입이
달라도 블록의 타입에서 그대로 합쳐집니다.

```rl
const view = result {
  const user <- getUser(id);
  const company <- getCompany(user.companyId);
  { user, company }
};
```

## 파이프라인 변형 (`*P`)

`|>`([`language.md` §7](./language.md#7-파이프라인-연산자-))에 바로 끼울 수
있는 data-last 커링 변형입니다. `Option.mapP(f)`는 `Option<T>`를 받는 단항
함수를 돌려주므로 `o |> Option.mapP(f)`가 `Option.map(o, f)`와 같습니다.
동작은 data-first 원본과 동일합니다.

| 원본 | 커링 변형 |
|------|-----------|
| `Option.map/andThen/orElse/filter/unwrapOr/unwrapOrElse/expect/okOr` | `mapP/andThenP/orElseP/filterP/unwrapOrP/unwrapOrElseP/expectP/okOrP` |
| `Result.map/mapErr/andThen/orElse/unwrapOr/unwrapOrElse/expect` | `mapP/mapErrP/andThenP/orElseP/unwrapOrP/unwrapOrElseP/expectP` |

이미 단항인 멤버는 변형 없이 그대로 파이프에 들어갑니다:
`r |> Result.ok |> Option.toNullable`.

```rl
const label = half(4)
  |> Option.mapP(x => x + 1)
  |> Option.unwrapOrP(0)
  |> .toFixed(1);
```

## 사용 예

```rl
import { Option, Result } from "@rl/std";

function parseNum(raw: string): Result<number, string> {
  const n = Number(raw);
  return Number.isNaN(n) ? Result.Err("not a number: " + raw) : Result.Ok(n);
}

const half = (n: number): Option<number> =>
  n % 2 === 0 ? Option.Some(n / 2) : Option.None;

// 콤비네이터로 파이프라인을 만들고, 마지막 분기는 match로:
const msg = match (Result.map(parseNum("42"), half)) {
  Ok(value) => match (value) {
    Some(value: h) => `half=${h}`,
    None => "odd",
  },
  Err(error) => `error: ${error}`,
};
```

`Some`/`None`, `Ok`/`Err`에 대한 `_` 없는 match는 내장 enum 소진성 검사를 받고
([§4.2](./language.md#42-내장-enum과-소진성-검사)), `Result`를 반환하는 함수
안에서는 `try`로 Rust의 `?`처럼 전파할 수 있습니다
([§5](./language.md#5-에러-전파-try-문)).
