# TASK-064: `result` 계산 블록 — Result 바인딩 `<-`

- **상태**: 완료
- **시작일**: 2026-08-18
- **완료일**: 2026-08-18
- **커밋**: `b70dfbc`

## 목적

여러 `Result` 연산을 순차적으로 이어붙일 때 생기는 콜백 중첩
(`Result.andThenP(user => ... Result.andThenP(company => ...))`)을 없애고,
이전 단계의 값을 그대로 들고 다음 단계로 넘어가는 코드를 평탄하게 쓰게 한다.

```rl
const data = result {
  const user <- getUser(id);
  const company <- getCompany(user.companyId);
  const permission <- getPermission(user, company);
  { user, company, permission }
};
```

## 범위

- 포함: `result { ... }` 계산 블록 표현식, 그 안의 Result 바인딩
  `const|let|var <바인딩> <- <식>;`, 블록의 마지막 식을 `Ok`로 감싸는 방출,
  파서/세마/코드젠/테스트/레퍼런스 문서/AI 문서/에디터 지원.
- 제외: 범용 monad, `Promise`/`Option` do-notation, 사용자 정의 flatMap 프로토콜,
  `result` 블록 밖의 임의 `<-` 식, rl 자체 타입 추론(타입은 전부 tsc에 맡긴다).

## 의사결정

### 결정 1: 방출 형태는 콤비네이터 중첩이 아니라 **이른 return의 IIFE**

- **상황**: 제안서의 개념적 lowering은 `Result.flatMap(A(), x => ...)`의 중첩이었고,
  "정확한 방출 형태는 기존 Result API와 tsc 추론 결과를 보고 정한다"가 열려 있었다.
  결정해야 할 것은 (a) 콜백 중첩, (b) 전용 헬퍼 함수, (c) IIFE + 이른 `return`.
- **검토한 대안**:
  - **(a) `Result.andThen` 중첩** — 개념과 1:1이지만 세 가지 문제가 있다.
    ① 표준 라이브러리 import를 강제한다(`result` 블록을 쓰려면 `@rl/std`를
    import해야 하고, `Result`를 직접 정의해 쓰는 코드에서는 아예 못 쓴다) —
    "방출 코드는 런타임을 주입하지 않는다"는 계약과 충돌한다.
    ② 현재 `andThen`의 시그니처가 `<T, E, U>(r: Result<T, E>, f: (T) => Result<U, E>)`라
    **에러 타입이 하나로 고정**돼 있어 단계마다 에러 타입이 다르면 타입 에러가 난다.
    제안서가 지적한 대로 std 시그니처를 `E | F`로 넓히는 후속 변경이 필요해진다.
    ③ 바인딩 사이의 평범한 문장(`const name = ...;`)을 콜백 안으로 재배치해야 해
    파서/코드젠이 복잡해지고 원본 줄 구조가 무너진다.
  - **(b) 파일당 헬퍼(`$rl_rb`)** — `$rl_ap`처럼 헬퍼를 방출하면 import는 피하지만,
    성공 케이스만 반환하는 콜백에서 에러 타입 인자를 추론할 근거가 없어
    (`F`가 후보 없음 → `unknown` 또는 기본값 `never` 트릭 필요) 타입이 불안정하다.
    (a)의 ③ 문제도 그대로다.
  - **(c) IIFE + 이른 `return`** — `try` 문이 이미 쓰는 모양(`const $rl_t = (식);
    if ($rl_t.kind !== "Ok") return $rl_t;`)을 함수 대신 블록 범위로 옮긴 것.
- **선택과 근거**: **(c)**. 근거는 확인 가능하다.
  - **에러 타입이 저절로 합쳐진다.** IIFE의 반환 타입은 반환된 `Err`들과 마지막
    `Ok`의 유니언이므로, `Result<User, UserError>`와 `Result<Company, CompanyError>`를
    잇는 블록이 `Result<_, UserError | CompanyError>`에 그대로 대입된다.
    std 시그니처를 건드릴 필요가 없다 (확인:
    `cargo test --test integration result_block_unions_the_error_types_of_its_bindings`,
    빠뜨린 주석은 tsc가 잡는다 —
    `result_block_missing_an_error_type_is_a_type_error`).
  - **헬퍼도 import도 없다.** `kind` 문자열만 보므로 `@rl/std` 없이 손으로 쓴
    `Result` enum에도 그대로 동작한다 (통합 테스트의 `Res<T, E>` 프렐류드).
  - **바인딩 사이의 문장이 그대로 있는다.** 원본 문장 순서와 줄 구조가 유지되고,
    파서는 문장을 나누기만 하면 된다.
  - 대가는 "블록 안의 `return`이 블록에서 빠져나간다"는 점인데, 이는 (a)/(b)에서도
    콜백 경계 때문에 똑같이 발생한다. 문서화하고 `try`·let-else를 블록 안에서
    금지하는 것으로 정리했다(결정 5).

### 결정 2: `result` 판별은 키워드가 아니라 **바인딩 존재**로

- **상황**: `result`는 TypeScript에서 아주 흔한 식별자다. 게다가 ASI 때문에
  `result` 다음 줄의 블록 문(`result\n{ ... }`)은 **유효한 TypeScript**다.
  `match`처럼 "키워드 + 뒤따르는 모양"만으로 판별하면 통과 계약이 깨질 수 있다.
- **검토한 대안**:
  - **A. `result` + `{`이면 무조건 rl 블록** — 위 ASI 코드(죽은 코드지만 유효)와
    `function f(): result { ... }`(반환 타입이 `result`인 함수)를 잘못 변환한다.
  - **B. 줄바꿈 금지 규칙** — `result`와 `{` 사이에 줄바꿈이 없을 때만 claim.
    렉서가 트리비아를 버리므로 파서가 다시 바이트를 봐야 하고, 무엇보다
    `function f(): result { ... }`를 못 막는다.
  - **C. 블록 안에 Result 바인딩(`const x <- 식;`)이 하나 이상일 때만 claim.**
- **선택과 근거**: **C**. 선언 키워드 뒤의 `<-`는 유효한 TypeScript일 수 없으므로
  (선언자는 초기화 `=`가 필요하다) claim된 블록을 담은 파일은 애초에 유효한 TS가
  아니다 — 통과 계약이 정의상 깨지지 않는다. rl enum을 "페이로드 괄호가 있을 때만"
  판별하는 기존 규칙과 같은 성격이다. 바인딩 없는 `result { ... }`는 `Ok(식)`과
  같아서 잃는 표현력도 없다.
  확인: `tests/passthrough.rs`의 `result_is_an_ordinary_identifier_in_typescript`,
  `identifier_statement_followed_by_a_block_passes_through`.

### 결정 3: `<-`는 전역 연산자가 아니라 **선언 런의 첫 최상위 연산자**

- **상황**: 제안서가 지적한 대로 `a < -b`는 유효한 TypeScript다. `<-`를 토큰으로
  만들면 통과 계약이 깨진다.
- **검토한 대안**: ① 렉서에서 `<-`를 융합 토큰으로(→ `a <-b`가 있는 유효 TS가
  깨진다), ② 블록 안 어디서든 `<-`를 바인딩으로(→ `const x = a <-b;`가 깨진다),
  ③ 선언 키워드로 시작하는 최상위 런에서, **초기화 `=`보다 먼저** 나오는
  **바이트가 붙은** `<` + `-`만 바인딩.
- **선택과 근거**: **③**. 렉서는 그대로 두고(`<`와 `-`는 여전히 별개 Punct)
  파서의 문맥 규칙으로만 인식한다. `=`가 먼저 오면 평범한 선언이므로
  `const x = a < -b;`는 그대로 통과한다. 붙여 쓰기 요구는 문서화된 표기 규칙이다.
  확인: `tests/passthrough.rs::less_than_negation_passes_through`.

### 결정 4: 제네릭 음수 리터럴 타입 인자(`let x: Foo<-1>;`) 예외

- **상황**: 결정 3의 규칙에도 구멍이 하나 있다. `let x: Foo<-1>;`는 **유효한
  TypeScript**이고 선언 키워드 뒤에 `=` 없이 붙은 `<-`가 나온다. `result` 블록으로
  오인될 수 있는 자리(예: `function f(): result { let x: Foo<-1>; ... }`)에 있으면
  잘못 변환된다 — 통과 계약 위반.
- **검토한 대안**: ① `<`/`>` 깊이를 추적해 제네릭을 구분(→ `<-`가 제네릭 여는
  괄호이기도 해서 구분이 안 된다), ② 바인딩 텍스트에 `:` 주석이 있으면 claim
  금지(→ `const n: number <- parse(raw);`라는 정당한 형태를 잃는다),
  ③ `<-` **뒤**를 본다 — 최상위에 짝 없는 닫는 `>`가 남으면 그 `<`는 제네릭을
  연 것이므로 바인딩이 아니다.
- **선택과 근거**: **③**. `let x: Foo<-1>;`의 꼬리는 `1>`로 짝 없는 `>`가 남고,
  식은 그런 `>`를 남길 수 없다(비교식은 양쪽이 식이라 여는 `<`가 먼저 온다).
  대가는 `const x <- a > b;`처럼 최상위 `>`가 있는 바인딩이 통과로 빠진다는 것인데,
  `Result`가 아닌 비교식을 바인딩하는 형태라 실질 손실이 없고 괄호로 해결된다
  (제한사항 표에 기록). 확인:
  `tests/passthrough.rs::negative_literal_type_arguments_pass_through`.

### 결정 5: 블록 본문은 `Ctx::Stmt` — `try`·let-else 금지, `if let` 허용

- **상황**: 방출이 IIFE이므로 블록 안의 `return`은 블록에서 나간다. `try`와
  let-else는 "둘러싼 함수에서 나가는 `return`"으로 컴파일된다.
- **검토한 대안**: ① 허용 — `const x = try f();`가 사실상 `const x <- f();`와
  같은 의미가 되므로 동작은 한다. 하지만 "함수에서 나간다"고 읽는 사용자를
  배신한다. ② 금지 — 기존 `Ctx` 규칙(`try`/let-else는 `Ctx::Top`에서만)에
  그대로 얹힌다.
- **선택과 근거**: **②**. 블록 본문을 `Ctx::Stmt`로 방문하면 기존 검사가 그대로
  동작하고, 에러 메시지에 "a `result` block"을 추가하는 것으로 끝난다.
  `if let`은 자체 완결 블록으로 컴파일돼 `return`이 없으므로 `Ctx::Stmt`에서
  이미 허용된다 — 별도 처리가 필요 없었다.

### 결정 6: 문장 경계와 "마지막 값 식"

- **상황**: 블록 본문을 바인딩/일반 문장/마지막 값 식으로 나눠야 한다.
- **검토한 대안**: ① `;`만 경계로 — 그러면 `if (c) { ... }`처럼 `;` 없이 끝나는
  문장 뒤의 값 식을 값으로 잡지 못한다. ② `;` + "블록 문을 닫은 `}`"를 경계로.
- **선택과 근거**: **②**. "`}` 뒤에 오는 토큰이 식을 이어가는가"를 판정하는
  로직이 파이프라인 head 추적기(`Parser::brace_ends_expression`)에 이미 있어
  그대로 재사용했다(가시성만 `pub(super)`로). 마지막 값 식은 **세미콜론 없이**
  쓰게 했다(Rust와 같고, 제안서 예시와도 같다). 값 식이 없거나 바인딩에 `;`가
  빠지면 통과가 아니라 **위치를 담은 에러**다(결정 2에 따라 통과시킬 수 없으므로).

### 결정 7: 바인딩 자리는 `try` 선언 형태와 동일

- **상황**: 제안서의 1차 범위는 `const <binding> <- 식;`이고 "복잡한 구조 분해"는
  제외였다.
- **검토한 대안**: ① 식별자만 허용, ② `try` 선언 형태와 동일하게 키워드와 `<-`
  사이의 원문을 그대로 바인딩 텍스트로 사용.
- **선택과 근거**: **②**. 코드가 오히려 단순하고(`const {kw} {binding} =
  $rl_rN.value;`로 그대로 방출), 타입 주석·구조 분해·`let`/`var`가 공짜로 따라온다.
  기존 `try` 문과 표기가 일치하는 것도 이득이다.

### 결정 8: 표준 라이브러리는 건드리지 않는다

- **상황**: 제안서는 "필요하다면 std의 `flatMap` 타입을 개선한다"고 열어 뒀다.
- **선택과 근거**: 결정 1의 방출이 에러 타입을 스스로 합치므로 `Result.andThen`
  (rl std의 이름은 `flatMap`이 아니라 `andThen`) 시그니처를 넓힐 이유가 없어졌다.
  시그니처를 넓히는 변경은 기존 코드의 추론 결과를 바꿀 수 있으므로 하지 않았다.
  std 문서에는 "`andThen` 중첩 대신 `result` 블록" 안내만 추가했다.

## 작업 내역

- 2026-08-18: 태스크 등록(`INDEX.md`, 다음 번호 TASK-065).
- 2026-08-18: 기존 구현 조사 — `src/parser/{tries,lets,iflets,pipes}.rs`의 구조
  파싱 관례, `src/codegen/mod.rs`의 `try`/let-else 방출, `sema.rs`의 `Ctx` 규칙,
  `stdlib/rl_std.ts`의 `Result` 시그니처(제안서의 `flatMap`은 실제로 `andThen`).
- 2026-08-18: AST 확장 — `src/ast.rs`에 `Segment::ResultBlock`, `ResultBlock`,
  `ResultItem`(`Stmts`/`Bind`), `ResultBind`와 `Program::stray_results` 추가.
- 2026-08-18: 파서 — `src/parser/results.rs` 신규(문장 경계 분할, `scan_bind`,
  `Attempt::{Claimed, Malformed, Pass}`), `src/parser/mod.rs`에 훅과
  `segment_start`/`stray_results` 배선, `brace_ends_expression`을 `pub(super)`로.
- 2026-08-18: 코드젠 — `Emitter::emit_result_block`(IIFE, `$rl_rN` 임시 변수,
  `await` 감지 시 async IIFE). 임시 변수 카운터는 `try`/let-else/`if let`과 공유해
  중첩에도 이름이 겹치지 않게 했다.
- 2026-08-18: 세마 — `check_result_block`(항목은 `Ctx::Stmt`, 바인딩 식과 값은
  `Ctx::Expr`), `stray_results` 리포트, `try`/let-else 위치 에러 메시지에
  "a `result` block" 추가.
- 2026-08-18: 중첩 지원 — `try`/let-else/`if let`의 식 스캐너가 `result { ... }`의
  자체 중괄호에서 멈추지 않도록 `cursor::skip_match_shape`를
  `skip_braced_construct(tokens, word, k)`로 일반화하고 세 호출부를 교체.
- 2026-08-18: 테스트 — `tests/compile.rs` 12개(방출 형태, 바인딩 종류, async,
  중첩, 파이프라인 head, 위치 제약, 4가지 에러), `tests/passthrough.rs` 4개
  (식별자/ASI 블록/`a < -b`/`Foo<-1>`), `tests/integration.rs` 6개(에러 타입 유니언,
  누락 주석은 tsc 에러, 바인딩 내로잉, 단락 평가 런타임, async 런타임, std를 쓴
  실제 3단계 예제).
- 2026-08-18: 문서 — `language.md` §8 신설(§8 모듈 → §9, §9 제한사항 → §10으로
  번호 이동, 상호 참조 5곳 갱신), `errors.md`에 `result` 블록 절, `std.md`,
  `README.md`, `CHANGELOG.md`, `CLAUDE.md`(일곱 구문·아키텍처 맵), `docs/ai/rl.md`와
  `docs/ai/README.md`, `rlc help result` 주제 추가(`src/main.rs`).
- 2026-08-18: 에디터 — `rl.tmLanguage.json`에 `result` 키워드와 `<-` 바인딩 규칙,
  언어 서버에 `result` 스니펫, 확장 README 갱신. `npx tsc -b` + `node --test`로
  70개 서버 테스트 통과 확인.
- 2026-08-18: 검증 게이트 3종 통과.

## 이슈 및 해결

### 이슈 1: `try result { ... };`가 통과로 빠져 출력이 깨졌다

- **증상**: `const w = try result { const q <- m(); q };`가
  `generated TypeScript failed to parse: Expression expected`로 실패.
- **원인**: `try` 문의 식 스캐너(`tries::stmt_expr_end`)는 최상위의 맨 `{`를 만나면
  파싱을 포기한다(멤버 선언 모양 배제). `match ( ... ) { ... }`만 통째로 건너뛰는
  예외(`skip_match_shape`)가 있었고 `result { ... }`는 없었다. let-else와 `if let`의
  스캐너도 같은 구조였다.
- **해결**: `skip_match_shape`를 `skip_braced_construct(tokens, word, k)`로 일반화해
  `match`와 `result`를 모두 건너뛰게 하고 세 스캐너의 호출부를 교체했다.
  회귀 방지: `tests/compile.rs::result_block_is_an_expression_in_every_nested_position`.

### 이슈 2: `;`를 빠뜨린 바인딩이 위치 없는 출력 검증 에러로 나왔다

- **증상**: `result { const x <- f()\n  x }`가
  `generated TypeScript failed to parse: ... (line 1, col 18 of the generated output)`.
  rl 구문임이 확정된 코드인데 위치가 원본이 아니라 생성물 기준이었다.
- **원인**: `;`가 없으면 문장 경계가 생기지 않아 그 런이 "마지막 값 식"이 되고,
  바인딩 판정 자체가 돌지 않아 `Pass`(통과)로 끝났다.
- **해결**: ① 마지막 런에도 `scan_bind`를 돌려 바인딩 모양이면 `Malformed`로,
  ② 바인딩 식의 최상위에 문장 전용 키워드(`const` 등)가 나오면 다음 문장까지
  삼킨 것이므로 `Malformed`로 처리했다. 두 경우 모두 `result` 위치를 담은 rl
  에러가 된다. 회귀 방지: `result_binding_without_a_semicolon_is_an_error`.

### 이슈 3: `let x: Foo<-1>;`가 바인딩으로 오인돼 통과 계약이 깨졌다

- **증상**(리뷰 중 발견): `function f(): result { let x: Foo<-1>; ... }`는 유효한
  TypeScript인데, `result` 뒤의 `{`와 선언 키워드 뒤의 붙은 `<-` 때문에 블록이
  claim되고 `const $rl_r0 = (1>);`가 방출된다.
- **원인**: 결정 3의 "선언 키워드 + `=`보다 먼저 오는 붙은 `<-`" 규칙이
  제네릭 타입 인자의 음수 리터럴을 배제하지 못했다.
- **해결**: 결정 4 — `<-` 뒤 최상위에 짝 없는 `>`가 남으면 `NotBind`(통과).
  회귀 방지: `tests/passthrough.rs::negative_literal_type_arguments_pass_through`.

### 이슈 4: 통합 테스트 프렐류드의 `enum UserError { NoUser }`가 TS enum이었다

- **증상**: `Property 'kind' does not exist on type '... | UserError'`.
- **원인**: 페이로드 괄호도 제네릭도 없는 enum은 (설계대로) TypeScript enum으로
  통과한다. 테스트 프렐류드의 실수였다.
- **해결**: `enum UserError { NoUser() }`(빈 괄호)로 rl enum임을 명시.

### 이슈 5: `try (result { ... });`는 여전히 안 된다

- **증상**: 괄호로 감싼 형태는 통과 후 출력 검증 에러.
- **원인**: `try` 문의 식은 `(`로 시작할 수 없다는 기존 제약
  (`language.md` §10 제한사항, 인터페이스의 `try(x): T` 멤버 시그니처와 구분 불가).
- **해결**: 이 태스크 범위 밖의 기존 제약이므로 그대로 뒀다. 괄호 없이
  `try result { ... };`로 쓰면 동작한다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` (compile 167, passthrough 44, integration 49 — tsc/node 설치
      환경에서 실제 실행됨, 그 외 포함 전부 통과)
- [x] VSCode 확장: `npm ci && npx tsc -b && node --test "server/out/test/*.test.js"`
      (70개 통과)

## 결과

| 파일 | 변경 |
|------|------|
| `src/ast.rs` | `Segment::ResultBlock`, `ResultBlock`/`ResultItem`/`ResultBind`, `Program::stray_results` |
| `src/parser/results.rs` | 신규 — 블록 구조 파싱과 바인딩 판정 |
| `src/parser/mod.rs` | `result` 훅, `segment_start`, stray 배선, `brace_ends_expression` 공개 |
| `src/parser/cursor.rs` | `skip_match_shape` → `skip_braced_construct`(`match`/`result`) |
| `src/parser/{tries,lets,iflets}.rs` | 식 스캐너가 `result` 블록을 통째로 건너뛰게 |
| `src/codegen/mod.rs` | `emit_result_block`(IIFE + 이른 return, async 감지) |
| `src/sema.rs` | `check_result_block`, stray 리포트, `try`/let-else 메시지 |
| `src/main.rs` | `rlc help result` 주제 |
| `tests/{compile,passthrough,integration}.rs` | 22개 테스트 추가 |
| `docs/reference/{language,errors,std,cli}.md` | §8 신설과 번호 이동, 에러 절, 상호 참조 |
| `docs/ai/{rl,README}.md`, `README.md`, `CHANGELOG.md`, `CLAUDE.md` | 일곱 구문 반영 |
| `docs/design/{compiler-architecture,module-graph,project-front-end}.md` | 절 번호 참조 갱신 |
| `editors/vscode/**` | 문법 하이라이팅, `result` 스니펫, README |

후속으로 검토할 만한 것(별도 태스크 필요, 이번 범위 아님): `Option` 블록이나
`Promise` do-표기법으로의 일반화, 값 식이 이미 `Result`일 때의 자동 평탄화.
