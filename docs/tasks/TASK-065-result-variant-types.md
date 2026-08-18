# TASK-065: `Result` 타입 모델 개선 — `Ok<T>` / `Err<E>` 변종 타입

- **상태**: 완료
- **시작일**: 2026-08-18
- **완료일**: 2026-08-18
- **커밋**: `5543e64`

## 목적

표준 라이브러리의 `Result` 생성자가 값에 존재하지 않는 제네릭(팬텀 제네릭)을
받고 있어서, 여러 `try`를 쓴 함수의 반환 타입 추론이 실제보다 나빠진다.
`Result.Ok`/`Result.Err`를 각자 실제로 가진 타입만 받는 변종 타입
(`Ok<T>`/`Err<E>`)으로 바꿔, 에러 타입 합집합을 TypeScript의 union 추론에
그대로 맡긴다.

기존 시그니처는 다음과 같았다.

```ts
Ok: <T, E>(value: T): Result<T, E> => ({ kind: "Ok", value }),
Err: <T, E>(error: E): Result<T, E> => ({ kind: "Err", error }),
```

`Result.Ok(1)`을 호출하는 시점에 `E`를 추론할 정보가 없고(→ `unknown`),
`Result.Err("bad")`에는 `T`를 추론할 정보가 없다. 값에 없는 타입을 생성자
제네릭에 넣은 것이라 실제 피해가 나온다 — 아래 측정 참조.

## 범위

- 포함: `src/stdlib/rl_std.ts`의 `Result` 타입/생성자/타입 가드 형태 변경,
  `tests/stdlib.rs`의 형태 계약 테스트 조정, tsc 통합 테스트 3건 추가,
  `docs/reference/std.md` · `docs/reference/language.md` · `docs/ai/rl.md` 갱신.
- 제외: 컴파일러(`src/` 중 stdlib 외) 변경 — `try` lowering은 그대로이고
  타입 추론은 여전히 전적으로 tsc의 몫이다. `Option`의 대칭 변경
  (`Some<T>`/`None` 타입 추가)도 제외(결정 4). 콤비네이터 시그니처는
  `Result<T, E>` 그대로 유지(결정 3).

## 의사결정

### 결정 0: 먼저 "실제로 무엇이 깨지는가"를 tsc로 측정한다

- **상황**: 제안서의 주장("여러 `try`의 에러 타입이 union으로 추론된다")이
  실제 tsc 동작과 맞는지 확인하지 않으면, 형태만 바꾸고 이득이 없을 수 있다.
- **검토한 대안**: (A) 제안서를 그대로 구현한다. (B) 변경 전/후 최소 재현
  파일을 만들어 tsc로 직접 돌려본다.
- **선택과 근거**: (B). `try` 방출 형태를 손으로 옮긴 파일
  (`const $rl_t0 = (getUser()); if ($rl_t0.kind !== "Ok") return $rl_t0; ...`)
  로 확인했다.
  - 기존 모델: `tsc --noEmit --strict` →
    `TS2322: Type 'Result<Data, unknown>' is not assignable to type
    'Result<Data, UserError | ConfigError>'` — 즉 반환 타입을 적지 않은
    함수의 추론이 마지막 `Result.Ok(...)`의 `E = unknown` 때문에 망가진다.
  - 새 모델: 같은 파일이 통과(exit 0).
  조기 return되는 `Err` 쪽은 원래도 정확했다(방출이 좁혀진 값을 그대로
  return하므로). 문제는 **`Ok` 생성자 하나**였다는 것이 이 측정으로 확정됐다.

### 결정 1: 변종에 이름을 준다 — `Result<T, never>`나 인라인 객체 타입 대신

- **상황**: "생성자가 자기 케이스의 타입만 받는다"를 표현하는 방법이 셋 있다.
- **검토한 대안**:
  - (A) `Ok: <T>(value: T) => Result<T, never>` / `Err: <E>(error: E) =>
    Result<never, E>`. 측정해 보면 위의 다중 `try` 케이스는 **통과한다**
    (`never`는 무엇에나 assignable). 그러나 값의 타입이 `Result<number,
    never>`가 되어, 존재할 수 없는 `Err` 암을 계속 달고 다닌다. 실제로
    에러 메시지가 `Type 'Result<number, never>' is not assignable to type
    'string'. Type '{ kind: "Err"; error: never; }' is not assignable...`
    처럼 있지도 않은 암을 설명한다. 제안서의 "존재하지 않는 타입 정보를
    `never`로 억지로 채우지 않는다"는 목표와 정면으로 어긋난다.
  - (B) 인라인 객체 타입 반환(`(value: T) => { kind: "Ok"; value: T }`).
    타입은 정확하지만 이름이 없어 호버·에러 메시지가 매번 구조를 풀어 쓴다.
  - (C) `export type Ok<T>` / `export type Err<E>`를 선언하고
    `Result<T, E> = Ok<T> | Err<E>`로 둔다.
- **선택과 근거**: (C). 같은 파일에서 (A)와 비교 측정했을 때 에러 메시지가
  `Type 'Ok<number>' is not assignable to type 'string'.` 한 줄로 끝난다.
  ADT 구조(성공 변종/실패 변종/그 합)를 그대로 이름으로 옮긴 것이라
  타입 트릭도 conditional type도 없다.

### 결정 2: `Ok`/`Err`는 타입만 내보낸다 (값 네임스페이스는 건드리지 않는다)

- **상황**: 변종에 이름을 붙이면 `Ok`/`Err`가 모듈의 export 이름이 된다.
  값으로도 내보낼지(`export const Ok = ...`) 정해야 한다.
- **검토한 대안**: (A) 값도 함께 내보내 `Ok(1)`처럼 쓰게 한다 — 생성 경로가
  둘(`Ok(1)` / `Result.Ok(1)`)이 되고, match 암 문법의 `Ok(value)`와 시각적으로
  겹쳐 "암은 식이 아니다"라는 기존 설명이 흐려진다. (B) 타입만 내보낸다.
- **선택과 근거**: (B). 값 생성 경로는 `Result.Ok`/`Result.Err` 하나로 유지된다.
  `import type { Ok, Err } from "@rl/std";`로 타입만 쓰면 되고, 런타임 코드에는
  아무 변화가 없다(방출물 바이트 동일).

### 결정 3: 콤비네이터는 `Result<T, E>` 그대로 둔다

- **상황**: `map`/`mapErr`/`andThen` 등도 변종 타입으로 정밀화할 수 있다
  (예: `map`이 `Ok` 입력에 대해 `Ok<U>`를 돌려주도록 오버로드).
- **검토한 대안**: (A) 오버로드로 정밀화한다 — 24개 콤비네이터 × 오버로드가
  늘고, `Result`를 받아 `Result`를 주는 추상화가 깨진다. 얻는 것은 이미
  `Result`인 값에 대한 추론뿐이라 이득이 작다. (B) 생성자만 변종 타입으로 하고
  나머지는 `Result<T, E>` 유지.
- **선택과 근거**: (B). 변종 타입이 필요한 곳은 "타입 정보가 아직 없는" 생성
  시점뿐이다. `Result<T, E>`가 `Ok<T> | Err<E>`이므로 변종 값은 콤비네이터에
  그대로 들어간다(통합 테스트 `runtime_std_new_combinators`로 확인).
  타입 가드만 예외적으로 `r is { kind: "Ok"; value: T }` → `r is Ok<T>`로
  바꿨다 — 같은 타입의 더 읽기 좋은 표기다.

### 결정 4: `Option`은 이번에 건드리지 않는다

- **상황**: 대칭을 맞춘다면 `Some<T>`/`None`도 같이 내보낼 수 있다.
- **검토한 대안**: (A) 같이 바꾼다 — 대칭은 좋지만, `Option.Some`은 이미
  `<T>` 하나만 받아 **팬텀 제네릭 문제가 없다**. 즉 고칠 버그가 없는 변경이라
  이번 태스크의 근거(측정된 추론 손실)가 적용되지 않는다.
  (B) 범위 밖으로 둔다.
- **선택과 근거**: (B). 필요해지면(예: `isSome` 가드 표기 개선) 별도 태스크로
  등록한다.

### 결정 5: "바이트 단위 형태 계약" 테스트를 **값** 기준으로 다시 쓴다

- **상황**: `tests/stdlib.rs::std_declarations_match_rl_enum_emission`은
  "rl enum을 컴파일한 결과의 모든 줄이 STD_SOURCE에 있어야 한다"였다. 생성자
  시그니처가 달라지면 이 테스트는 반드시 깨진다(실제로 깨졌다).
- **검토한 대안**:
  - (A) 테스트를 지운다 — 형태가 드리프트해도 아무도 못 잡는다. `match`
    코드젠은 `kind`/`value`/`error`에 직접 의존하므로 위험하다.
  - (B) 별칭을 펼친 뒤 비교한다 — 테스트가 타입 별칭 해석기를 흉내내게 되어
    과도하다.
  - (C) 계약을 "선언이 같다"에서 "**값**이 같다"로 좁혀 다시 정의한다.
    Option은 종전대로 전 줄 일치(`std_option_matches_rl_enum_emission`),
    Result는 방출된 **유니언 암**과 **생성자가 만드는 객체 리터럴**이
    STD_SOURCE에 그대로 있는지 검사한다(`std_result_matches_rl_enum_value_shape`).
- **선택과 근거**: (C). `match`·소진성 검사·`JSON.stringify`가 의존하는 것은
  값의 모양뿐이고, 그 부분은 여전히 바이트 단위로 고정된다. 시그니처만
  의도적으로 갈라진다는 사실이 테스트 주석·`src/stdlib.rs` 모듈 문서·
  `docs/reference/std.md`에 같이 적혔다. 테스트는 방출물에서 암/객체를
  뽑아내므로(하드코딩 아님) enum 방출 형태가 바뀌면 여전히 같이 움직인다.

### 결정 6: 추론 이득과 "구멍이 아님"을 둘 다 테스트한다

- **상황**: "여러 `try`가 union으로 추론된다"는 통합 테스트가 없으면 다음
  리팩토링에서 조용히 되돌아갈 수 있다. 반대로 그 테스트만 있으면 "느슨해져서
  통과하는 것 아니냐"는 의심을 못 지운다.
- **검토한 대안**: (A) 긍정 테스트 하나. (B) 긍정 + 부정 테스트.
- **선택과 근거**: (B).
  `try_error_types_infer_as_a_union_without_an_annotation`(반환 타입 없는
  함수의 추론이 `Result<Data, UserError | ConfigError>`에 assignable)과
  `try_error_union_stays_checked_against_the_declared_return_type`
  (반환 타입을 `Result<number, string>`로 적었는데 `{ tag: "user" }`를
  전파하면 **tsc 에러**)를 같이 둔다. 추가로
  `std_result_constructors_type_only_their_own_variant`는
  `Exact<A, B>` 보조 타입으로 `typeof Result.Ok(123)`가 정확히 `Ok<number>`
  임을 고정한다(assignable만 보면 `unknown` 회귀를 못 잡는다).

## 작업 내역

- 2026-08-18: 태스크 문서 생성, INDEX 등록. 이때는 TASK-064로 잡았으나 병렬로
  진행되던 다른 작업과 번호가 겹쳐 나중에 TASK-065로 옮겼다(이슈 4).
- 2026-08-18: 측정 먼저(결정 0). 스크래치에 `old.ts`/`new.ts`/`never.ts`를 만들어
  `tsc --noEmit --strict`로 세 모델을 비교. 기존 모델만 `TS2322`로 실패.
- 2026-08-18: `src/stdlib/rl_std.ts` — `Ok<T>`/`Err<E>` 타입 선언 추가,
  `Result<T, E> = Ok<T> | Err<E>`로 변경, 생성자를
  `Ok: <T>(value: T): Ok<T>` / `Err: <E>(error: E): Err<E>`로,
  가드를 `r is Ok<T>` / `r is Err<E>`로. 파일 머리말 주석과
  `src/stdlib.rs` 모듈 문서에 "값은 바이트 동일, 생성자 시그니처만 의도적으로
  다르다"를 명시.
- 2026-08-18: `tests/stdlib.rs`를 결정 5대로 3개 테스트로 재구성
  (`cargo test --test stdlib` → 3 passed).
- 2026-08-18: `tests/integration.rs` — 두 개의 타입 인자를 넘기던 호출 3곳을
  갱신(`Result.Ok<number, string>(3)` → `Result.Ok<number>(3)`,
  `Result.collect([...])`·`Result.flatten(...)`은 호출 쪽에 타입 인자를 옮김).
  `typecheck_with_std()` 헬퍼(스니펫 + `rl.ts`를 tsc `--noEmit`) 추가하고
  테스트 3건 신설(결정 6). `cargo test --test integration` → 46 passed.
- 2026-08-18: 문서 갱신 — `docs/reference/std.md`(값의 형태 계약에 변종 타입
  절과 "여러 `try`의 에러 타입" 절 추가, `Ok`/`Err`/`isOk`/`isErr` 시그니처
  행 수정, 마이그레이션 한 줄), `docs/reference/language.md`(§4.1 형태 계약
  문구, §5.3에 무주석 함수의 union 추론 항목), `docs/ai/rl.md`(try 절과
  std 절).
- 2026-08-18: 게이트 실행 — `cargo fmt --check`, `cargo clippy --all-targets
  -- -D warnings`, `cargo test`(285 passed). CI가 함께 도는 에디터 테스트도
  확인: `npm ci && npx tsc -b && PATH=target/debug node --test
  "server/out/test/*.test.js"` → 70 pass.
- 2026-08-18: PR #26을 연 뒤 `main`에 PR #25(`result` 계산 블록)가 머지되어
  충돌. `origin/main` 위로 리베이스하고 문서 충돌 2건을 양쪽 다 살리는 방향으로
  해소, 태스크 번호를 TASK-065로 재배정(이슈 4). 병합 후 게이트 재실행.

## 이슈 및 해결

### 이슈 1: 형태 계약 테스트가 깨졌다 (예상된 실패)

- **증상**: `cargo test --test stdlib` →
  `std module drifted from rl enum emission; missing line:
    | { kind: "Ok"; value: T }` (tests/stdlib.rs:25).
- **원인**: 테스트가 방출된 **모든 줄**이 STD_SOURCE에 문자열로 들어 있기를
  요구한다. 새 모듈은 같은 암을 `export type Ok<T> = { kind: "Ok"; value: T };`
  라는 별칭으로 쓰므로 `  | ` 접두사가 붙은 줄은 없다.
- **해결**: 계약을 값 기준으로 다시 정의했다(결정 5). Option은 전 줄 일치를
  그대로 유지하고, Result는 유니언 암과 생성자 객체 리터럴만 비교한다.

### 이슈 2: 통합 테스트의 명시적 타입 인자가 `TS2558`로 깨졌다

- **증상**: `runtime_std_new_combinators`에서 tsc가
  `error TS2558: Expected 1 type arguments, but got 2.` 4건.
- **원인**: 테스트가 `Result.Ok<number, string>(1)`처럼 옛 두-제네릭 형태를
  쓰고 있었다. 이 형태가 사라지는 것이 이번 변경의 의도된 부분이다.
- **해결**: 타입 인자를 필요한 곳으로 옮겼다 —
  `Result.Ok<number>(3)`, `Result.collect<number, string>([Result.Ok(1),
  Result.Err("x")])`, `Result.flatten<number, string>(Result.Ok(Result.Ok(4)))`.
  런타임 출력 기대값은 그대로이며 실제로 그대로 통과한다.

### 이슈 3: 문서 편집에서 문단이 잘못 이어붙었다

- **증상**: `docs/reference/std.md`의 "값의 형태 계약" 절에서 원래 문단 뒤쪽
  ("페이로드 필드명은 …")이 새로 추가한 마지막 문단 끝에 붙어버렸다.
- **원인**: 문자열 치환의 앵커를 문단 중간까지만 잡아 뒤쪽 문장이 삽입 지점
  뒤로 밀렸다.
- **해결**: 해당 문장을 잘라 원래 자리(값 형태 설명 직후)로 되돌리고 렌더링을
  다시 확인했다.

### 이슈 4: 태스크 번호와 문서가 다른 작업과 충돌했다

- **증상**: PR #26을 연 뒤 `git fetch` 하니 `main`에 PR #25가 머지돼 있었고,
  그 작업도 **TASK-064**(`result` 계산 블록)를 쓰고 있었다. `git rebase
  origin/main` → `docs/tasks/INDEX.md`와 `docs/reference/std.md`에서
  `CONFLICT (content)`.
- **원인**: 두 작업이 같은 시점에 INDEX의 "다음 태스크 번호"(TASK-064)를 읽고
  각자 진행했다. INDEX 등록은 번호를 예약해 주지 못한다 — 상대 브랜치가
  머지되기 전까지 서로의 등록이 보이지 않는다.
- **해결**: 먼저 머지된 PR #25가 TASK-064를 유지하고, 이 작업을 **TASK-065**로
  옮겼다 — 태스크 문서 파일명·제목·INDEX 행·`tests/stdlib.rs`의 주석 참조를
  모두 갱신하고 "다음 태스크 번호"를 TASK-066으로 올렸다. 커밋 메시지도
  태스크 ID로 시작해야 하므로(CLAUDE.md 워크플로 4) 아직 머지되지 않은 이
  브랜치의 커밋 두 개를 리베이스로 다시 써서 `TASK-065:`로 맞췄다.
- **문서 충돌 두 건은 둘 다 덧붙이기였다**: INDEX는 두 행을 모두 남기고
  번호만 조정, `std.md`는 생성자 문단(이 작업)과 `result` 계산 블록 문단
  (TASK-064)을 순서대로 둘 다 유지했다.
- **기능 충돌은 없다**: `result` 블록은 마지막 값을
  `{ kind: "Ok" as const, value: (...) }` 객체 리터럴로 직접 방출하고 표준
  라이브러리 생성자를 거치지 않으므로 이번 시그니처 변경의 영향을 받지 않는다.
  블록의 에러 타입 union 테스트(`result_block_unions_the_error_types_of_its_bindings`)도
  병합 후 그대로 통과한다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` — 285 passed, 0 failed (integration 46건 포함, skip 없음)
- [x] `editors/vscode`: `npx tsc -b` + `node --test "server/out/test/*.test.js"`
      → 70 pass (CI가 도는 게이트라 함께 확인)

## 결과

- `src/stdlib/rl_std.ts`: `Ok<T>`/`Err<E>` 타입 추가, `Result<T, E>`를 그 합으로
  정의, 생성자를 변종 타입 반환으로, `isOk`/`isErr` 가드 표기를 변종 타입으로.
- `src/stdlib.rs`: 모듈 문서에 "값은 바이트 동일 / `Result` 생성자만 의도적
  일탈" 명시.
- `tests/stdlib.rs`: 형태 계약 테스트를 Option(전 줄 일치) / Result(값 모양
  일치) 두 개로 분리.
- `tests/integration.rs`: `typecheck_with_std()` 헬퍼, 테스트 3건 추가,
  옛 두-제네릭 호출 3곳 갱신.
- `docs/reference/std.md`, `docs/reference/language.md`, `docs/ai/rl.md`: 변종
  타입·다중 `try` 추론·마이그레이션 반영.

사용자 영향(파괴적 변경): `Result.Ok<T, E>(...)` / `Result.Err<T, E>(...)`처럼
타입 인자를 두 개 넘기던 호출은 `Result.Ok<T>(...)` / `Result.Err<E>(...)`로
바꾸거나, 그냥 주변 문맥(변수 주석·함수 반환 타입)에 맡기면 된다. 타입 인자를
넘기지 않던 코드는 그대로 동작한다. 방출되는 런타임 값은 바이트 단위로 동일하다.

후속 후보(등록하지 않음): `Option`에도 같은 방식으로 `Some<T>`/`None` 타입을
내보낼지 — 팬텀 제네릭 문제가 없어 이득이 표기 개선뿐이다(결정 4).
