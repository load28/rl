# TASK-131: try·let-else 배치를 flow 사실로 (Phase 5 2/n)

- **상태**: 완료
- **시작일**: 2026-08-21
- **완료일**: 2026-08-21
- **커밋**: —

## 목적

`try`의 배치 규칙은 구문 중첩 근사(`Ctx::Top`에서만 허용)였고, 두 방향
모두에서 틀렸다:

- **거짓 거부**: rl 구성물 안이라도 **사용자가 그 자리에 쓴 함수 본문**
  안의 `try`는 건전하다 — 방출되는 `return`이 그 함수를 벗어난다(Rust의
  클로저 안 `?`). 가드·스크루티니·템플릿 보간·파이프라인 스텝의 화살표
  함수 안 `try`가 전부 에러였다.
- **누락**: 모듈 최상위(함수 밖)의 `try`는 `Ctx::Top`이라 통과됐고,
  방출된 최상위 `return`이 verify 백스톱("invalid TypeScript ... or an
  rlc bug")으로 죽었다 — rlc가 방출한 코드가 tsc/파서 에러를 내는 계약
  위반이 진단 없이 새고 있었다.

설계(compiler-core.md §9)의 "‘try’가 반환 가능한 body 안인지"를 구현한다:
배치는 중첩 규칙이 아니라 **flow 사실** — "방출되는 `return`이 나갈
사용자 함수가 파스 영역 안에 있는가".

## 범위

- 포함: `flow::in_function_body`(+`function_body_brace`/
  `paren_heads_function`), `TryStmt`/`LetElseStmt`의 `in_function`
  (파서가 기록), sema의 flow 기반 배치 판정과 새 문구(모듈 최상위 전용
  문구 포함), 테스트(단위 flow 12건 상당, compile.rs 전환·신설,
  integration 클로저 전파 1건), 레퍼런스(§5.4·§6.4·§7 표·errors.md)와
  docs/ai/rl.md 갱신.
- 제외: `result` early-return 범위·분기별 초기화(Phase 5 잔여로 유지),
  let-else else 블록 발산 키워드의 문맥 유효성(모듈 최상위 let-else의
  `return` 발산은 사용자 코드의 TS 에러 — tsc/백스톱 소관).

## 의사결정

### 결정 1: 판정 단위는 "파스 영역 안의 함수 본문 중괄호"

- **상황**: "함수 안인가"를 어디서 어떻게 판정하나.
- **대안**: ① sema에서 전체 파일을 다시 스캔 — sema는 토큰이 없다.
  ② 파서가 구성물 인식 시점에 현재 영역 토큰으로 판정해 AST에 기록
  (let-else `diverges`의 선례) — 재귀 파스의 영역 경계가 곧 의미 경계다:
  하위 프로그램(암 본문·스크루티니 등)은 자기 슬라이스만 보므로, 구성물
  IIFE 밖의 함수는 세지 않고 안에 쓴 함수만 센다.
- **선택과 근거**: ②. 영역 경계가 공짜로 맞아떨어지고, sema는 기록된
  사실을 소비만 한다(무오류 파서 계약도 유지 — bool 기록일 뿐).

### 결정 2: 중괄호 분류는 왼쪽 문맥으로, 타입 어노테이션은 역방향 보행으로

- **상황**: 어떤 `{`가 함수 본문인가. `=> {`와 `) {`가 기본이지만 반환
  타입(`): Promise<T> {`, `): { a: number } {`)이 사이에 끼고, `) {`
  중에도 제어 헤드(`if (…) {`)와 상속절(`class A extends mixin(B) {`)은
  함수가 아니다.
- **선택과 근거**: `=> {`는 화살표 본문; `) {`는 매개변수 목록의 본문
  (제어 헤드·`for await`·상속절 제외, `f<T>(…) {`·`function* (…) {`
  포함); 그 외에는 `{`에서 역방향으로 타입에 나올 수 있는 토큰만 균형
  보행해 `) :`를 찾으면 그 괄호로 판정한다. 보행은 문장 전용 키워드나
  타입에 없는 토큰을 만나면 중단(보수적으로 "함수 아님") — 제네릭 클래스
  본문(`class A<T> {`)이 이 중단으로 정확히 걸러진다.

### 결정 3: let-else도 같은 사실로 — 단 모듈 최상위는 허용

- **상황**: let-else의 배치도 같은 `Ctx` 근사였고 같은 거짓 거부가 있다.
- **선택과 근거**: 같은 `in_function`을 기록하되 판정은
  `ctx != Top && !in_function`만 에러. let-else 자체는 `return`을
  방출하지 않고(`throw` 발산 else는 어디서나 유효한 TS), 모듈 최상위는
  이미 허용돼 왔다 — 규칙을 try와 통일하되 이 차이는 의미가 있어 남긴다
  (§6.4에 명시).

### 결정 4: 코드 유지, 문구 분리

- 진단 코드는 `try-placement`/`let-else-placement` 유지(안정 코드 계약).
  문구는 두 상황을 구분한다: 구성물 안(IIFE로 새는 `return` — 함수 추출
  또는 `<-` 안내)과 모듈 최상위(`try` 전용 — 나갈 함수가 없음).

## 작업 내역

- 2026-08-21: `src/flow/mod.rs` — `in_function_body`(영역 접두의 중괄호
  스택에서 함수 본문 존재 여부)와 분류기(결정 2), 단위 테스트
  (함수·메서드·getter·생성자·제네릭·반환 타입 5형 = 허용 / 모듈·제어
  블록·namespace·상속절·static 블록·제네릭 클래스 본문·닫힌 함수 = 불허).
- `src/ast.rs` — `TryStmt.in_function`/`LetElseStmt.in_function`.
  `src/parser/mod.rs` — 세 파스 지점에서 `flow::in_function_body`로 기록.
  `src/parser/tries.rs`/`lets.rs` — 필드 초기화.
- `src/sema.rs` — `check_try`: `!in_function`이면 에러(ctx로 문구 분기);
  `check_let_else`: `ctx != Top && !in_function`이면 에러. 모듈 문서 갱신.
- 테스트: compile.rs — 가드·스크루티니·암 본문·템플릿 보간·파이프라인
  스텝의 **함수 안** try 5건을 긍정 테스트로 전환/신설, 모듈 최상위·
  namespace try 에러 신설, 암 본문 직접 try·result 블록 직접 try/let-else
  에러의 문구 갱신, 함수 안 let-else 긍정 신설. integration.rs —
  `runtime_try_inside_a_closure_propagates_from_the_closure`(스크루티니
  화살표 안 try가 화살표에서 전파, tsc+node 실행 검증).
- 문서: language.md §5.4(제어 흐름 판정 표)·§6.4(let-else 차이)·§7
  파이프라인 표, errors.md 두 행 갱신+최상위 행 신설, docs/ai/rl.md
  3곳, compiler-core.md Phase 5 잔여 정산.

## 이슈 및 해결

### 이슈 1: 반환 타입 어노테이션 누락으로 기존 긍정 테스트 8건 파손

- **증상**: `function f(): X { const n = try g(); … }` 픽스처들이 배치
  에러로 실패.
- **원인**: 최초 분류가 `=> {`·`) {`만 다뤄 `): X {`의 본문 `{`(직전
  토큰이 타입의 마지막 토큰)를 함수 본문으로 보지 못했다.
- **해결**: 결정 2의 역방향 타입 보행 추가. `): Promise<void>`·객체
  타입·배열/유니언 반환 타입 단위 테스트로 고정.

### 이슈 2: 확장 테스트 54건 skip — `rlc`가 PATH에 없음

- **증상**: `npm test`가 pass 20 / skip 54.
- **원인**: 확장 테스트는 서버와 같은 방식으로 PATH의 `rlc`를 찾는다
  (`toolchain.ts`); 이 세션의 빌드는 `target/debug/rlc`에만 있다.
- **해결**: `PATH=target/debug:$PATH RLC_TSGO_ROOT=../typescript-go
  npm test` → **74/74, skip 0**.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` (625 통과, 실패 0 — integration 포함)
- [x] `editors/vscode`: `npx tsc -b` + `npm test` 74/74 (skip 0)

## 결과

`try`/let-else의 배치가 rustc처럼 제어 흐름 사실로 판정된다: 클로저 안
`?`에 해당하는 코드가 컴파일되고, 모듈 최상위 `try`는 verify 백스톱
대신 위치 있는 rl 진단을 받는다(계약 2의 구멍 봉합). Phase 5 잔여는
`result` early-return 범위·분기별 초기화·HIR body 연동이 남는다.
