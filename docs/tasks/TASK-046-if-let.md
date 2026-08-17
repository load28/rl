# TASK-046: `if let` 문 — 조건부 값 추출

- **상태**: 완료
- **시작일**: 2026-08-17
- **완료일**: 2026-08-17
- **커밋**: 4f4f2b9

## 목적

TASK-042 제안 P4 구현. let-else의 비발산 짝 —
`if let Some(value: user) = findUser(id) { ... } else if let ... { } else { }`.
바인딩이 `const`로 물질화되어 클로저 경계에서도 좁혀진 타입이 유지된다
(TASK-042 G5 완화의 마지막 조각).

## 범위

- 포함: AST(IfLetStmt/IfLetElse), 파서(parser/iflets.rs, else 체이닝),
  sema(문맥 구분 도입 + 표현식 위치 금지 + 실패 후보 보고), codegen(자기
  완결 블록 방출), 중첩 패턴 지원, 테스트, 문서.
- 제외: `else if (조건)` 이어 붙이기(else 블록 안에 쓰면 동일 — 임의 TS 문
  스캔이 필요해 v1 제외), or-패턴, `if let ... = try 식` 조합.

## 의사결정

### 결정 1: 실패한 `if let` 후보는 통과가 아니라 sema 에러 (`|>`와 동일 원리)

- **상황**: 유효한 TS에서 `if` 뒤에는 반드시 `(`가 온다 — 무점 `if` + `let`
  열은 rl 전용이므로, 파싱 실패 후보를 통과시키면 생성물이 위치 정보 없는
  verify 실패가 된다.
- **선택과 근거**: 파서가 `if`+`let`을 봤는데 클레임에 실패하면 오프셋을
  `Program::stray_if_lets`에 기록하고 sema가 `파일:행:열`로 보고한다 —
  스트레이 파이프와 같은 기계. 통과 계약은 무손상(그 바이트 열이 유효 TS에
  없으므로).

### 결정 2: 배치 규칙을 3-값 문맥(Top/Stmt/Expr)으로 정밀화

- **상황**: 기존 sema는 nested: bool 하나로 try/let-else를 최상위 밖에서
  전부 금지했다. if-let은 자체 `return`이 없는 자기 완결 블록이라 match 암의
  블록 본문 같은 "문장 문맥"에서는 완전히 안전한데, bool로는 이를 표현할 수
  없다.
- **검토한 대안**: (A) if-let도 최상위 전용 — 안전하지만 match 암 블록에서
  못 쓰는 것은 근거 없는 제약. (B) visit_program의 문맥을
  Top(최상위)/Stmt(같은 함수 또는 IIFE 내부의 문장 위치)/Expr(표현식 위치)로
  나눔 — try/let-else는 Top 전용(기존 동작 그대로), if-let은 Expr만 금지.
- **선택과 근거**: (B). 기존 동작 변화 없음(암 블록 본문의 try는 여전히 에러
  — IIFE 안이므로), if-let만 문장 문맥 전반에서 허용된다.

### 결정 3: 중첩 패턴 지원 (제안에서 확장)

- **상황**: 제안서 §6은 let-else와 같은 패턴(별칭만)이었으나, TASK-045의
  경로 조건 기계(pattern_conds_binds)가 있으면 if-let의 방출 형태
  (`if (cond) { binds }`)에 그대로 꽂힌다.
- **선택과 근거**: 지원. `if let Ok(value: Some(value: v)) = r { ... }` —
  Rust의 `if let Ok(Some(v))` 관용구에 대응하며 추가 구현 비용이 사실상 0.
  let-else는 부정 조건(`!==`) 방출이라 중첩 시 De Morgan 변환과 내로잉
  검증이 별도 작업이므로 이번에도 제외(범위에 기록).

### 결정 4: else는 블록 또는 if-let만 — 일반 `else if (조건)` 미지원

- **상황**: `else if (c) { ... }`를 문의 일부로 클레임하려면 임의 TS if 문
  전체(조건·본문·재귀 else)를 구조 스캔해야 한다.
- **선택과 근거**: v1 제외. else 블록 안에 일반 if를 쓰면 의미가 같다
  (`else { if (c) { ... } }`). 실패 후보는 결정 1의 에러로 안내된다.

### 결정 5: `= 식` 스캔은 최상위 `{`에서 종료, `=>` 직후 `{`는 중단

- **상황**: 본문 블록의 `{`와 식 내부의 `{`(객체 리터럴, 블록 화살표)를
  구분해야 한다.
- **선택과 근거**: 깊이 0의 `{`가 종료 지점. `match (..) {..}` 형태는 통째로
  건너뛰고(let-else의 expr_until_else와 같은 기법), `=>` 직후의 깊이 0 `{`
  (괄호 없는 블록 화살표)는 중단해 에러로 수렴 — 괄호로 감싸도록 안내.
  객체 리터럴·콜백의 `{`는 괄호 깊이>0이라 자연히 안전.

## 작업 내역

- 2026-08-17: ast.rs에 IfLetStmt/IfLetElse + Program::stray_if_lets.
  parser/iflets.rs 신규 — 패턴(중첩 허용)·`=` 판별(==/=> 배제)·
  expr_until_block·then 블록·else 체이닝(블록 | 재귀 if-let). parser/mod.rs
  에 `if`+`let` 프리체크 클레임과 실패 기록, segment_start 항목.
- 2026-08-17: sema.rs — Ctx(Top/Stmt/Expr) 도입, 전 visit 호출 문맥 재지정
  (암 블록 본문 = Stmt, 나머지 재귀 = Expr, let-else else 블록 = Stmt),
  check_if_let(표현식 위치 금지 + 잎 바인딩 유일성 + 자식 방문),
  stray_if_lets 보고.
- 2026-08-17: codegen/mod.rs emit_if_let — `{ const $rl_tN = (식); if (경로
  조건) { 바인딩; 본문 } else ... }` 한 줄 블록, `$rl_t` 카운터 공유,
  pattern_conds_binds 재사용(pub(super)로 개방).
- 2026-08-17: 테스트 — compile.rs 9건(방출·체이닝·중첩·카운터 공유·문장
  문맥 허용·표현식 위치 에러·실패 후보 에러·중복 바인딩·일반 if 통과),
  passthrough.rs 1건(if 문·`if` 멤버명), integration.rs 2건(런타임 체이닝
  폴백·클로저 내 내로잉 유지 타입체크).
- 2026-08-17: 문서 — language.md §6.5 신설 + 기본 원칙 "여섯 구문" + 제한
  사항, errors.md if let 절, README·CLAUDE.md·lib.rs 문서 갱신, 제안 문서
  P4 상태. 게이트 통과 후 커밋.

## 이슈 및 해결

없음 — 구현·테스트 전 단계가 첫 실행에서 통과했다 (TASK-045의 경로 조건
기계와 let-else의 식 스캐너 선례를 그대로 재사용한 덕).

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` (tsc 6.0.2 + node 22 — 통합 테스트 포함)

## 결과

- 변경: `src/ast.rs`, `src/parser/{mod,iflets}.rs`, `src/sema.rs`,
  `src/codegen/{mod,matches}.rs`, `src/lib.rs`,
  `tests/{compile,passthrough,integration}.rs`,
  `docs/reference/{language,errors}.md`, `docs/design/type-inference-gaps.md`,
  `README.md`, `CLAUDE.md`, 본 문서, `docs/tasks/INDEX.md`.
- rl은 이제 여섯 구문. 사용자 지시분(TASK-013 구현 + 제안 ①②⑤) 전체 완료.
- 후속 후보: let-else 중첩 패턴(De Morgan 방출), `else if (조건)` 체이닝,
  내부 곱 소진성 v2 — 수요 발생 시 별도 제안.
