# TASK-134: `if let` 배치도 flow 사실로 (TASK-131 잔여)

- **상태**: 완료
- **시작일**: 2026-08-21
- **완료일**: 2026-08-21
- **커밋**: —

## 목적

[TASK-131](./TASK-131-try-placement-on-flow.md)이 try·let-else의 배치를
flow 사실로 옮길 때 `if let`은 빠뜨렸다: 표현식 영역(스크루티니·템플릿
보간·가드·파이프라인 스텝)에 **쓴 함수의 본문 안**의 `if let`은 문장
위치라 건전한데도 `Ctx::Expr` 하나로 거부됐다(자체 점검에서 발견).
try와 같은 사실을 반대 방향으로 적용한다: `if let`은 IIFE를 탈출할
`return`이 없고 문장 스트림만 필요하므로, 표현식 영역이라도 함수가
있으면 허용한다.

## 범위

- 포함: `IfLetStmt.in_function`(파서 기록), sema 판정
  `ctx == Expr && !in_function`으로 완화 + 문구에 함수 탈출구 명시,
  테스트(스크루티니·보간 안 화살표 속 `if let` 긍정 2형, 직접 보간
  에러 유지), 레퍼런스·AI 문서 갱신.
- 제외: 체인된 `else if let`의 별도 기록 — 체인은 항상 문장 문맥
  (`Ctx::Stmt`)으로 재귀 검사되므로 바깥 문만 판정 대상이다(코드 주석).

## 의사결정

### 결정 1: 판정은 `Expr && !in_function`

- **상황**: try는 `!in_function`이면 어디서든 에러지만 `if let`은 자체
  `return`이 없어 모든 문장 문맥(`Top`/`Stmt`)이 이미 유효하다.
- **선택과 근거**: 표현식 영역만 문제고, 그 영역 안의 함수 본문은 문장
  위치를 제공한다 — try의 `in_function`과 같은 사실의 소비 방향만
  다르다. 같은 `flow::in_function_body`를 그대로 쓴다.

## 작업 내역

- 2026-08-21: `src/ast.rs`·`src/parser/iflets.rs`·`src/parser/mod.rs` —
  `in_function` 기록(파스 지점, try·let-else와 동일). `src/sema.rs` —
  `check_if_let` 판정 완화와 문서화. 테스트: compile.rs
  `if_let_inside_a_function_inside_an_expression_region_is_allowed`
  (스크루티니·템플릿 보간), 기존 직접-보간 에러 테스트 유지.
- 문서: language.md §6.5, errors.md 행, docs/ai/rl.md.

## 이슈 및 해결

없음.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` (전 스위트 실패 0)

## 결과

세 문(try·let-else·`if let`)의 배치가 모두 하나의 flow 사실
(`in_function_body`) 위에서 판정된다 — TASK-131의 규칙이 언어 표면
전체에 일관된다.
