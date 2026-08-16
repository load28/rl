# TASK-016: let-else 문 (`const Tag(x) = 식 else { ... };`)

- **상태**: 대기
- **시작일**: 2026-08-16
- **완료일**: —
- **커밋**: —

## 목적

"한 케이스만 꺼내고 아니면 이탈"하는 패턴을 Rust의 `let ... else`처럼 한
문장으로 쓸 수 있게 한다. `try`는 `Result`를 그대로 전파할 때만 쓸 수 있어,
`Option`이나 임의 enum에서 사용자 정의 이탈(기본값 return, throw, continue)을
하려면 지금은 match + 중간값 우회가 필요하다.

## 범위

- 포함: `const|let|var Tag(바인딩들) = 식 else { ... };` 문법(세미콜론 필수,
  괄호 필수), else 블록 발산 구문 검사(sema 에러), try 스타일 문장 방출,
  중첩 컨텍스트 금지(try와 동일 규칙), 3계층 테스트, 레퍼런스 갱신.
- 제외: 괄호 없는 유닛 패턴(`const Point = e else ...`), or-패턴·가드·중첩
  패턴, `Option` 전파형 try(별도 논의), 모듈 최상위 사용 감지(try와 동일하게
  출력 검증에 위임).

## 의사결정

### 결정 1: 패턴에 괄호 필수 — `Tag(...)` 형태만 인식

- **상황**: `const Some = ...`은 유효한 TS 선언이므로, 괄호 없는 유닛 패턴을
  허용하면 유효 TS와의 판별이 뒤따르는 `else`에만 의존하게 된다.
- **검토한 대안**: ① 유닛 패턴 허용(`else` 존재로 판별) — 전체 문장은 유효
  TS가 될 수 없어 이론상 안전하지만, 판별 근거가 문장 끝까지 미뤄져 스캐너
  실수 한 번이 통과 계약 위반으로 직결됨. ② `Tag(` 프리픽스 필수 — `const
  식별자(`는 그 시점에 이미 유효 TS가 아니므로 판별이 문장 앞에서 끝난다.
- **선택과 근거**: ②. 통과 계약의 안전 여유가 크고, 검사만 필요한 경우는
  빈 괄호(`const Ok() = r else { ... };`)로 쓸 수 있다. `const enum`은 태그
  자리의 `enum`이 예약어라 자연 배제됨을 테스트로 고정.

### 결정 2: 발산 검사는 "else 블록의 마지막 문장이 return/throw/break/continue"

- **상황**: else 블록이 발산하지 않으면 블록 뒤 구조 분해가 케이스 미보장
  상태로 실행된다(타입·런타임 모두 구멍). Rust는 타입 시스템(`!`)으로
  검사하지만 rlc는 타입 분석을 하지 않는다.
- **검토한 대안**: ① 검사 생략, tsc 구조 분해 에러에 위임 — 에러 계층 계약
  위반(rl 구문이 만든 생성물의 tsc 에러). ② 최상위 문장 경계(`;`, 블록 닫는
  `}`)를 스캔해 마지막 문장의 첫 토큰이 발산 키워드인지 확인하는 구문 검사.
- **선택과 근거**: ②. `if (c) return a; else return b;`처럼 실제로는
  발산하는 형태 일부를 거부하는 보수적 검사지만, 규칙이 한 줄로 설명되고
  거짓 통과(비발산 블록을 통과시켜 tsc 에러 유발)가 없다. 마지막 문장을
  발산 키워드로 끝내도록 재구성하면 되므로 우회 비용도 작다. 판정은
  파서가 계산해 AST에 담고(`diverges`), 에러 보고는 sema가 한다 — 파서
  무오류 원칙 유지.

### 결정 3: 중첩 컨텍스트(match·템플릿 보간·try 식) 금지 — try와 동일 규칙

- **상황**: 표현식 컨텍스트에 문장 방출이 끼어들면 생성물이 깨지고, match
  IIFE 안의 `return`은 의미가 달라진다.
- **검토한 대안**: ① 문장 컨텍스트(암 블록 본문 등)는 선별 허용 — 유용하지만
  컨텍스트 종류별 규칙이 늘어나고 try와 비대칭. ② try와 동일하게 재귀 파싱
  컨텍스트 전면 금지.
- **선택과 근거**: ②. 규칙이 try와 하나로 설명되고("중첩 컨텍스트에서는 try·
  let-else 불가 → 헬퍼 함수로 추출"), 선별 허용은 필요해지면 후속 태스크로
  완화할 수 있는 단방향 문이다.

### 결정 4: 방출은 try와 같은 한 줄 문장 + `$rl_t` 임시 변수 공유

- **상황**: 임시 변수 네임스페이스를 새로 만들지 결정 필요.
- **검토한 대안**: ① 별도 `$rl_l` 카운터 — 이름 공간이 늘고 이득 없음.
  ② try와 같은 `$rl_tN` 파일 단위 카운터 공유.
- **선택과 근거**: ②. 유일성 보장 메커니즘이 하나로 유지된다. 방출:
  `const $rl_tN = (식); if ($rl_tN.kind !== "Tag") { else본문 } kw { 바인딩 } = $rl_tN;`
  — else 블록이 발산하므로 tsc의 제어 흐름 내로잉이 구조 분해를 해당
  케이스로 좁혀 타입 트릭 없이 타입이 맞는다(통합 테스트의 `--strict`로 확인).

## 작업 내역

- 2026-08-16: `src/ast.rs` — `Segment::LetElse(LetElseStmt)` 추가
  (`kw/tag/bindings/expr/else_body/diverges/else_off/keyword_off`).
- 2026-08-16: `src/parser/lets.rs` 신설 — `const|let|var Tag(바인딩) = 식
  else { ... };` 구조 파싱. 식 스캔은 try의 문장 식 스캐너를 최상위 `else`
  종결로 변형(`scan_expr_until_else`); match 식 통짜 건너뛰기 포함. 발산
  판정(`block_diverges`) 계산. `src/parser/matches.rs`의 `parse_bindings`를
  `pub(super)`로 열어 재사용. `src/parser/mod.rs`의 `const|let|var` 분기에서
  try 선언 파싱 실패 시 let-else 시도.
- 2026-08-16: `src/sema.rs` — 중첩 금지 + 비발산 else 에러 보고, 자식
  프로그램(식·else 본문) 방문.
- 2026-08-16: `src/codegen/mod.rs` — `emit_let_else` (try와 동일한 한 줄
  문장 스타일, 마지막 줄 행 주석 시 개행 보정).
- 2026-08-16: `tests/compile.rs`(방출·에러 7건), `tests/passthrough.rs`
  (`else` 블록·`Some` 이름 함수 등 통과 3건), `tests/integration.rs`
  (`--strict` 타입체크 + 런타임 1건) 추가.
- 2026-08-16: `docs/reference/language.md` §6 신설(let-else), 예약어·제한사항
  §7/§8로 재번호 및 링크 갱신. `docs/reference/errors.md` let-else 에러 2종
  추가.
- 검증: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`.

## 이슈 및 해결

### 이슈 1: `= try 식 else { ... };` 조합이 인식되지 않음

- **증상**: try 선언 파싱은 식에서 `else`(문장 전용 키워드)를 만나 실패하고,
  let-else 파싱은 식에서 `try`를 만나 실패 — 조합 문장은 원문 통과 후 출력
  검증 에러가 된다.
- **원인**: 두 구문의 식 스캐너가 서로의 키워드를 문장 전용 키워드로 배제.
- **해결**: 의미상으로도 두 이탈 경로(Err 전파 + else 이탈)가 겹치는 조합은
  지원하지 않기로 하고 레퍼런스 제한사항에 명시. 필요하면 후속 태스크.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`

## 결과

변경 파일: `src/ast.rs`, `src/parser/mod.rs`, `src/parser/lets.rs`(신설),
`src/parser/matches.rs`, `src/sema.rs`, `src/codegen/mod.rs`,
`tests/compile.rs`, `tests/passthrough.rs`, `tests/integration.rs`,
`docs/reference/language.md`, `docs/reference/errors.md`.
