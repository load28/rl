# TASK-066: `Result` 에러 타입 합성 — `andThen`/`andThenP`의 유니언 누적

- **상태**: 완료
- **시작일**: 2026-08-18
- **완료일**: 2026-08-18
- **커밋**: `2619616`

## 목적

TASK-065로 `Result`가 `Ok<T> | Err<E>`가 되면서 에러 타입 정보가 정확해졌지만,
그 정보를 **합성 단계에서 잃고 있었다**. `andThen`이 앞 Result와 뒤 함수에 같은
`E`를 요구했기 때문이다.

```ts
andThen: <T, E, U>(r: Result<T, E>, f: (value: T) => Result<U, E>): Result<U, E>
```

`Result<A, E1>`에 `(a: A) => Result<B, E2>`를 이으면 `E1 ≠ E2`라 tsc가 거절한다
(`TS2345`). 의미상 결과는 두 에러를 모두 포함하는 `Result<B, E1 | E2>`여야 한다.
이 태스크는 `andThen`/`andThenP`가 그 의미를 타입으로 표현하게 만든다.

## 범위

- 포함: `src/stdlib/rl_std.ts`의 `Result.andThen`·`Result.andThenP` 시그니처
  변경과 `ErrorOf<R>` 타입 추가, 통합 테스트 8건 추가, `tests/integration.rs`의
  std 실행 스캐폴딩을 `run_with_std()` 헬퍼로 추출, 에디터 테스트 1건의
  시그니처 기대값 갱신, `docs/reference/std.md`·`docs/ai/rl.md`·`CHANGELOG.md`
  갱신.
- 제외:
  - `map`/`mapP` — 새 실패 가능성을 만들지 않으므로 `E`를 그대로 나르는 것이
    맞다 (제안서와 동일한 판단).
  - `Option`의 대응 콤비네이터 — 에러 타입 자체가 없다.
  - 컴파일러(`src/` 중 stdlib 외) 변경 — 에러 타입 수집·분석은 여전히 전부
    tsc의 몫이고 rlc는 아무것도 모으지 않는다.
  - 같은 계열의 다른 콤비네이터(`flatten`, `orElse`의 성공 타입, `collect`)
    정밀화 — 결정 5 참조.

## 의사결정

### 결정 0: 제안 시그니처를 먼저 tsc로 측정한다

- **상황**: 제안서는 "TypeScript `--strict`에서 union 누적이 정상 추론되는 것을
  확인했다"고 적고 있다. 확인 범위가 rlc의 실제 방출 형태(`$rl_ap` 적용,
  `$rl_fl` 합성)까지인지 알 수 없었다.
- **검토한 대안**: (A) 제안서를 그대로 옮긴다. (B) 방출 형태를 포함한 재현
  파일을 만들어 tsc로 직접 돌려본다.
- **선택과 근거**: (B). 스크래치에 `$rl_ap`/`$rl_fl` 헬퍼를 손으로 옮긴 파일을
  만들어 `tsc --strict --noEmit`(TypeScript 6.0.2)로 확인하고, 타입 문자열은
  `ts.createProgram` + `typeToString`으로 직접 찍어 비교했다. 이 측정 덕분에
  제안 그대로로는 부족하다는 것이 드러났다(결정 2).

  - 변경 전 시그니처: `Result<User, CfgE>`에 `(User) => Result<Company, TokE>`를
    잇는 두 형태(데이터-우선, `|>`) 모두 `TS2345`/`TS2322`로 실패 — 문제 재현 확정.
  - 제안 시그니처: 같은 파일이 통과하고, 3단 파이프라인이
    `Result<Profile, E1 | E2 | E3>`로 정확히 나왔다.

### 결정 1: `Ok<T>` 하나만 들어와도 정밀해야 한다

- **상황**: 제안 시그니처 `<T, E, U, F>(r: Result<T, E>, ...)`에서 `E`는 인자
  `r`에서만 추론된다. 그런데 TASK-065 이후 `Result.Ok(1)`이나 "실패하지 않는
  함수"의 값은 타입이 `Ok<number>`라 `Err` 암이 아예 없다 → `E`의 추론 후보가
  없어 `unknown`이 되고, 결과가 `Result<U, unknown>`으로 무너진다(측정으로 확인).
  변경 전에는 `E`를 `f`에서 읽었기 때문에 이 경우가 정확했다 — 즉 그대로 옮기면
  이 케이스는 **퇴보**다.
- **검토한 대안**: (A) 감수한다. (B) `E`에 기본값 `= never`를 준다. (C) 인자
  타입을 바꿔 언제나 추론 후보가 생기게 한다.
- **선택과 근거**: 처음에는 (B)로 갔고(`<T, U, F, E = never>` — 필수 타입
  파라미터가 기본값 뒤에 올 수 없어 순서를 바꿔야 했다), 결정 2에서 인자 타입을
  통째로 받는 형태로 옮기면서 (C)로 흡수됐다. 최종 형태에서
  `Result.andThen(Result.Ok(1), step)`은 `Result<string, FetchError>`다
  (`std_result_and_then_on_a_variant_typed_value_keeps_the_chained_error`).

### 결정 2: 에러를 `ErrorOf<R>`로 뽑는다 — 제안 그대로의 `E`로는 `try`가 안 붙는다

- **상황**: 제안서의 핵심 예제는 "`try`를 여러 번 쓴 함수를 파이프라인에
  잇는다"이다. 그런데 그런 함수의 추론 결과는 `Result<T, E1 | E2>`가 아니라
  **흩어진** `Err<E1> | Ok<T> | Err<E2>`다(TASK-065가 의도한 형태 그대로).
  제안 시그니처의 `r: Result<T, E>` = `Ok<T> | Err<E>`로 이 값을 받으면, tsc는
  두 `Err` 암에서 `E`의 후보를 둘 얻고 **유니언으로 합치지 않고 하나만 고른다**.
  실제 실패:

  ```
  error TS2345: Argument of type '<E = never>(r: Result<User, E>) => ...' is not
  assignable to parameter of type '(v: Err<ConfigError> | Ok<User> | Err<TokenError>) => ...'
      Type 'Err<TokenError>' is not assignable to type 'Err<ConfigError>'.
  ```

  즉 제안 그대로 구현하면 **간판 예제가 컴파일되지 않는다**. (변경 전 시그니처
  로도 실패하므로 회귀는 아니지만, 이 태스크의 목적을 달성하지 못한다.)
- **검토한 대안**: 스크래치에서 다섯 가지를 전부 tsc로 측정했다.
  - (A) 제안 그대로 `<E = never>(r: Result<T, E>)`. 인라인 화살표 콜백의
    매개변수 추론이 유지되는 유일한 형태지만, 흩어진 유니언(= `try`·`result`
    블록의 결과)은 **컴파일 에러**. 목적 미달.
  - (B) 에러 쪽을 통째로 네이키드 타입 파라미터로:
    `<Es extends Err<unknown> = never>(r: Ok<T> | Es): Ok<U> | Err<F> | Es`.
    조건부 타입 없이 흩어진 유니언을 받지만, `Ok<T>`만 들어오면 `Es`가 기본값이
    아니라 **제약(`Err<unknown>`)으로 떨어져** 결과에 `Err<unknown>`이라는
    있지도 않은 암이 붙는다(측정: `Result<Profile, unknown>`). TASK-065가 없앤
    팬텀 타입이 되살아난다.
  - (C) `Extract<R, Err<unknown>>`를 결과에 그대로 얹기. 본문이 타입 검사를
    통과하지 못하고(`TS2322`), 표시 타입도 `Result<...> | Err<A> | Err<B>`로
    지저분하다.
  - (D) 인자를 결과 타입 통째로 받고(`R`), 에러만 조건부 타입으로 뽑는다:
    `ErrorOf<R> = R extends Err<infer E> ? E : never`. 조건부 타입이 유니언에
    분배되므로 흩어진 형태든 `Result<T, E>`든 정확히 같은 답을 준다.
  - (E) (D)에 `Ok<T> | (R & Err<unknown>)` 같은 혼합 인자 타입. (D) 대비 이득
    없음(측정값 동일).
- **선택과 근거**: (D). 측정 결과(모두 `typeToString`으로 확인):

  | 입력 | (A) | (B) | (D) |
  |------|-----|-----|-----|
  | `Result<User, TokE>` | `Result<Profile, TokE \| FetchE>` | 같음 | 같음 |
  | `Err<CfgE> \| Ok<User> \| Err<TokE>` (`try`) | **컴파일 에러** | `Err<CfgE> \| Err<TokE> \| Ok<Profile> \| Err<FetchE>` | `Result<Profile, CfgE \| TokE \| FetchE>` |
  | `Ok<User>` | `Result<Profile, FetchE>` | **`Result<Profile, unknown>`** | `Result<Profile, FetchE>` |
  | `flow` 합성 | 입력 쪽 열림 | — | 입력 쪽 열림, 적용 시 유니언 |

  제안서의 "conditional type 같은 타입 트릭은 필요하지 않다"는 원칙과 어긋나는
  유일한 지점이라, 그 대가를 명시해 둔다: **조건부 타입은 표준 라이브러리
  안의 한 줄(`ErrorOf`)뿐이고, rlc가 방출하는 코드에는 없다.** 불변 원칙 2
  ("생성되는 코드는 타입 트릭 없는 순수 TypeScript")는 그대로 유지된다.
  이 한 줄이 사는 이유는 rl의 세 기능(`try`, `result` 블록, 여러 `Result.Err`
  반환)이 모두 흩어진 유니언을 만들기 때문이다 — 그 형태를 이어 붙이지 못하면
  이번 변경은 반쪽짜리다.

### 결정 3: 데이터-우선은 인자 타입을 교집합으로 적어 화살표 추론을 지킨다

- **상황**: (D)의 인자를 `r: R`로만 적으면 `T`의 추론 근거가 사라져
  `Result.andThen(first, (user) => getCompany(user))`의 `user`가 `unknown`이
  된다(측정). 데이터-우선 형태에서 인라인 화살표는 매우 흔한 표기다.
- **검토한 대안**: (A) `r: R` — 화살표에 주석을 강제한다. (B)
  `r: R & Result<T, unknown>` — `R`(전체 인자 타입)과 `T`(성공값 타입)를 둘 다
  추론 자리에 남긴다.
- **선택과 근거**: (B). 측정에서 `(user) => getCompany(user)`가 그대로
  추론되고(테스트 `std_result_and_then_unions_the_two_error_types`), 흩어진
  유니언·`Ok<T>`·`Result<T, E>` 세 입력 모두 정확한 결과를 준다. 교집합은 값의
  런타임 형태에 아무 영향이 없다.

### 결정 4: 커링 변형(`andThenP`)의 인라인 화살표 주석은 감수하고 문서화한다

- **상황**: 커링 형태에서는 `T`가 **바깥 호출**(`Result.andThenP(f)`) 시점에
  정해져야 한다. 인자 타입을 `Result<T, E>`로 두면 `$rl_ap`의 문맥에서
  역방향으로 `T`가 흘러와 무주석 화살표가 추론되지만((A)의 유일한 장점),
  결정 2의 어떤 대안((B)·(D)·(E))에서도 그 경로가 끊긴다 — 다섯 형태를 모두
  측정해 확인했다.
- **검토한 대안**: (A) 커링만 옛 형태로 남긴다 — 파이프라인에서 `try`·`result`
  블록 결과를 못 잇는다. 이 태스크의 간판 시나리오가 바로 파이프라인이므로
  본말전도. (B) 오버로드로 두 형태를 모두 제공한다 — 오버로드 선택은 인자(=
  넘긴 함수)만 보고 하므로 항상 첫 번째가 뽑힌다. 즉 동작하지 않는다(문서상
  근거: 오버로드 해소는 문맥 반환 타입을 고려하지 않는다). (C) 조건부 형태로
  통일하고, 인라인 화살표에는 매개변수 주석을 요구한다.
- **선택과 근거**: (C). 파이프라인 스텝은 대개 **이름 붙은 함수**이고
  (`|> Result.andThenP(fetchProfile)`), 무주석 화살표가 흔한 `mapP`는 이번
  변경 대상이 아니다. 주석이 빠지면 조용히 틀리는 게 아니라 tsc 에러가 난다.
  `docs/reference/std.md`의 `*P` 절과 `docs/ai/rl.md`에 예시로 명시했고,
  테스트 `std_result_and_then_p_takes_an_annotated_inline_callback`으로 고정했다.

### 결정 5: 같은 계열의 다른 콤비네이터는 건드리지 않는다

- **상황**: `flatten: (r: Result<Result<T, E>, E>) => Result<T, E>`도 안팎의
  에러 타입이 같아야 한다 — `andThen`과 같은 종류의 제약이다.
- **검토한 대안**: (A) 같이 고친다(`Result<Result<T, F>, E>` →
  `Result<T, E | F>`). (B) 범위 밖으로 둔다.
- **선택과 근거**: (B). 요청 범위가 `andThen`/`andThenP`로 명확하고, `flatten`은
  중첩 Result를 손으로 만들었을 때만 등장해 실사용 빈도가 낮다. 필요해지면
  별도 태스크로 등록한다(후속 후보로 아래 결과 절에 기록).

### 결정 6: std 실행 테스트 스캐폴딩을 헬퍼로 추출한다

- **상황**: 새 런타임 테스트를 추가하려는데, "std를 옆에 쓰고 tsc로 두 파일을
  빌드해 node로 돌린다"는 40줄짜리 스캐폴딩이 이미 **다섯 곳**에 그대로
  복사돼 있었다.
- **검토한 대안**: (A) 여섯 번째 복사본을 만든다. (B) `run_with_std()`로 뽑고
  기존 다섯 곳도 옮긴다.
- **선택과 근거**: (B). 기계적인 추출이고 기대값(출력 라인)은 그대로 두었다 —
  `cargo test --test integration`이 리팩토링 직후 52건 그대로 통과하는 것으로
  확인했다.

## 작업 내역

- 2026-08-18: `INDEX.md`에서 다음 번호(TASK-066)를 확인하고 태스크 문서 생성,
  INDEX에 `진행 중`으로 등록.
- 2026-08-18: 측정 먼저(결정 0). 스크래치
  (`.../scratchpad/exp/{cur,final,alts,b3,b4,b5,df,flow}.ts`)에 `$rl_ap`/`$rl_fl`
  헬퍼와 후보 시그니처들을 옮겨 `tsc --strict --noEmit`으로 비교하고,
  `ts.createProgram`+`typeToString`으로 추론된 타입 문자열을 찍어 표로 정리
  (결정 1·2·3·4의 근거).
- 2026-08-18: `src/stdlib/rl_std.ts` — 제안 시그니처(`<T, E, U, F>`)로 1차 구현
  → 통합 테스트에서 `try` 파이프라인이 실패(이슈 1) → `ErrorOf<R>` 기반으로
  재구현. 최종 형태:

  ```ts
  export type ErrorOf<R> = R extends Err<infer E> ? E : never;

  andThen: <T, U, F, R extends Result<T, unknown>>(
    r: R & Result<T, unknown>,
    f: (value: T) => Result<U, F>,
  ): Result<U, ErrorOf<R> | F> =>
    r.kind === "Ok" ? f(r.value) : (r as Err<ErrorOf<R>>),

  andThenP:
    <T, U, F>(f: (value: T) => Result<U, F>) =>
    <R extends Result<T, unknown>>(r: R): Result<U, ErrorOf<R> | F> =>
      r.kind === "Ok" ? f(r.value) : (r as Err<ErrorOf<R>>),
  ```

- 2026-08-18: `tests/integration.rs` — `run_with_std()` 헬퍼 추출(결정 6)과
  테스트 8건 추가:
  `std_result_and_then_unions_the_two_error_types`,
  `std_result_and_then_on_a_variant_typed_value_keeps_the_chained_error`,
  `std_result_and_then_p_accumulates_error_types_along_a_pipeline`,
  `std_result_map_p_keeps_the_error_type_it_was_given`,
  `std_result_and_then_p_composes_under_flow`,
  `std_result_and_then_p_takes_an_annotated_inline_callback`,
  `std_result_block_output_pipes_into_and_then_p`,
  `std_result_and_then_error_union_stays_checked_against_an_annotation`(부정),
  `runtime_result_and_then_chain_short_circuits_on_the_first_err`(런타임).
  타입 단정은 TASK-065가 도입한 `Exact<A, B>`로 **양방향** 고정 — assignable만
  보면 `unknown` 회귀를 못 잡는다.
- 2026-08-18: 에디터 테스트 1건 갱신(이슈 2), `npx tsc -b` 후
  `node --test "server/out/test/*.test.js"` → 70 pass.
- 2026-08-18: 문서 갱신 — `docs/reference/std.md`(변종 타입 절에 `ErrorOf`,
  새 절 "에러 타입 누적 (`andThen`)", `andThen` 시그니처 행, `*P` 절에 누적
  예시와 인라인 콜백 주석 규칙), `docs/ai/rl.md`(std 절 두 줄 추가, `result`
  블록 줄의 예시를 데이터-우선으로), `CHANGELOG.md`(Unreleased/Changed).
- 2026-08-18: 게이트 실행 — `cargo fmt --check`, `cargo clippy --all-targets
  -- -D warnings`, `cargo test`.

## 이슈 및 해결

### 이슈 1: 제안 시그니처 그대로는 `try` 파이프라인이 컴파일되지 않는다

- **증상**: `std_result_and_then_p_accumulates_error_types_along_a_pipeline`가
  실패.

  ```
  error TS2345: Argument of type '<E = never>(r: Result<User, E>) => Result<Profile, FetchError | E>'
  is not assignable to parameter of type '(v: Err<ConfigError> | Ok<User> | Err<TokenError>) => ...'
        Type 'Err<TokenError>' is not assignable to type 'Err<ConfigError>'.
  ```

- **원인**: 반환 타입을 적지 않은 `try` 함수의 추론 결과는 `Ok<T> | Err<E1> |
  Err<E2>`로 **흩어져** 있다. 이 값을 `Result<T, E>`(= `Ok<T> | Err<E>`)로 받으면
  tsc는 두 `Err` 암에서 `E`의 후보를 둘 얻고, 공변 위치의 다중 후보는 유니언이
  아니라 그중 하나로 결정된다(여기서는 `ConfigError`). 나머지 후보는 버려지고
  대입이 깨진다. 다른 시그니처 후보 다섯 개를 모두 측정해 원인과 해법을 확정했다
  (결정 2의 표).
- **해결**: 인자를 결과 타입 통째로 받고 에러만 `ErrorOf<R>`로 분배해서 뽑는
  형태로 바꿨다. 이 값이 `try`뿐 아니라 `result` 블록에서도 나오므로
  `std_result_block_output_pipes_into_and_then_p`로 함께 고정했다.

### 이슈 2: 에디터 자동완성 테스트가 옛 시그니처 문자열을 기대했다

- **증상**: `node --test "server/out/test/*.test.js"` →
  `not ok 27 - a completion entry resolves to its type`,
  `signature was: (property) andThen: <T, U, F, R extends Result<T, unknown>>(r: R & Result<T, unknown>, f: (value: T) => Result<U, F>) => Result<U, ErrorOf<R> | F>`.
- **원인**: `editors/vscode/server/src/test/completion.test.ts`가 완성 항목의
  상세 시그니처에 `"Result<U, E>"`가 들어 있는지로 "타입이 해석됐다"를 확인한다.
  의도된 시그니처 변경이라 기대 문자열만 낡았다.
- **해결**: 기대값을 `"Result<U, ErrorOf<R> | F>"`로 갱신했다. 테스트의 취지
  (완성 항목이 `any`가 아니라 실제 타입으로 해석된다)는 그대로다.

### 이슈 3: 새 테스트가 rustfmt 스타일과 어긋났다

- **증상**: `cargo fmt --check`가 `assert!(ok, "...:\n{out}");` 세 줄을 여러 줄
  형태로 바꾸라고 보고.
- **원인**: 100열을 넘는 `assert!` 한 줄 표기.
- **해결**: `cargo fmt` 실행 후 재확인(통과).

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` — 317 passed, 0 failed (integration 61건 포함, skip 없음)
- [x] `editors/vscode`: `npx tsc -b` + `node --test "server/out/test/*.test.js"`
      → 70 pass (CI가 도는 게이트라 함께 확인)

## 결과

- `src/stdlib/rl_std.ts`: `ErrorOf<R>` export 추가, `Result.andThen`·
  `Result.andThenP`가 에러 타입을 유니언으로 누적하도록 시그니처 변경(방출되는
  런타임 코드는 바이트 그대로 — 본문은 `r.kind === "Ok" ? f(r.value) : r`이고
  `as`는 타입 표기뿐).
- `tests/integration.rs`: `run_with_std()` 헬퍼 추출(기존 5곳 이관), 타입 테스트
  7건 + 런타임 테스트 1건 추가.
- `editors/vscode/server/src/test/completion.test.ts`: 기대 시그니처 갱신.
- `docs/reference/std.md`, `docs/ai/rl.md`, `CHANGELOG.md`: 누적 규칙·`ErrorOf`·
  인라인 콜백 주석 규칙 반영.

사용자 영향: 이전에 `TS2345`로 거절되던 합성이 통과하게 되는 **완화** 방향이라
기존에 컴파일되던 코드는 그대로 컴파일된다. 두 가지만 다르다 — (1) 커링
`Result.andThenP`에 무주석 인라인 화살표를 넘기면 매개변수가 `unknown`이 되므로
주석이 필요하다, (2) `Result.andThen`에 명시적 타입 인자를 넘기던 코드는 인자
순서가 `<T, U, F, R>`로 바뀌었다(타입 인자를 넘기지 않는 코드는 영향 없음).

후속 후보(등록하지 않음): `Result.flatten`을
`(r: Result<Result<T, F>, E>) => Result<T, E | F>`로 같은 규칙에 맞추는 것
(결정 5).
