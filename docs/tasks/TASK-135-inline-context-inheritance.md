# TASK-135: 인라인 문맥의 배치 상속 (Place — try·let-else의 마지막 거짓 거부)

- **상태**: 완료
- **시작일**: 2026-08-21
- **완료일**: 2026-08-21
- **커밋**: —

## 목적

TASK-131/134의 flow 배치는 각 문의 **파스 영역** 안 함수만 봤다. 그런데
`if let` 본문과 let-else `else` 블록은 IIFE가 아니라 **인라인**이다 —
그 문장들은 문이 선 자리에서 실행된다. 함수 안의 `if let` 본문에 쓴
`try`는 그 함수에서 전파되는 건전한 코드(Rust의 `?`가 if-let 안에서
되는 것과 같다)인데도, 하위 프로그램 영역엔 함수가 없어 거부됐다.
인라인 체인이 바닥나는 곳(모듈/함수/IIFE)을 sema가 상속해 판정한다.

또 하나: TASK-131 이후 모듈 최상위 `try`가 rl 진단과 verify 백스톱을
**중복 보고**했다(방출물의 최상위 `return`이 자가 검사에 걸림). 원인이
이미 보고된 파일의 verify 실패는 결과이므로 억제한다.

## 범위

- 포함: sema에 `Place { Module, Function, Iife }` 도입 —
  `visit_program`이 문맥과 함께 전달, `if let` 본문·else와 let-else
  else 블록은 `place.inline(stmt.in_function)`으로 상속, 그 외(match
  암·`result` 문장·모든 표현식 영역)는 `Iife`로 리셋. `check_try`는
  `!in_function && place != Function`일 때만 에러(문구는 `Module`/`Iife`
  로 분기), `check_let_else`는 `place == Iife && !in_function`일 때만.
  `compile_report`의 verify 실패를 rl 에러가 이미 있으면 진단으로 싣지
  않고 방출만 보류(원인-결과). 테스트 4건(+통합 1건), 문서 갱신.
- 제외: `if let`의 Expr 판정(TASK-134 그대로 — Expr 영역은 항상 Iife
  리셋이라 place가 답을 바꾸지 않는다).

## 의사결정

### 결정 1: 상속은 sema의 일 — 파서 사실은 영역-국소로 유지

- **상황**: `in_function`을 영역 경계 너머로 계산하게 만들 수도 있다.
- **선택과 근거**: 파서의 사실은 "이 영역 안에 함수가 있는가"로 두고
  (영역 경계가 IIFE 경계와 일치하는 곳에서는 그대로 정답), 인라인
  경계의 통과는 구성물의 의미를 아는 sema가 `Place` 상속으로 잇는다.
  두 사실의 결합이 규칙 그 자체다: `try`가 유효 ⟺ 자기 영역에 함수가
  있거나(place 무관) 인라인 체인이 함수에서 바닥난다.

### 결정 2: verify 실패는 rl 에러가 있으면 진단이 아니라 방출 보류다

- **상황**: 모듈 최상위 `try`가 두 진단(원인 + "or an rlc bug" 백스톱)을
  냈다.
- **선택과 근거**: coverage 억제와 같은 원인-결과 원칙 — 파일에 이미
  rl 에러가 있으면 자가 검사 실패는 그 에러가 방출물에 남긴 효과다.
  진단은 원인만 싣고 `emit`은 `None`으로(잘못된 산출물은 소비자에게
  주지 않는다); 원인을 고치면 검사는 저절로 되살아난다. 무관한 통과
  영역 문제를 한 라운드 늦게 보게 될 수 있지만, 오도하는 "rlc 버그"
  문구보다 낫다.

## 작업 내역

- 2026-08-21: `src/sema.rs` — `Place` + `Place::inline`, `visit_program`/
  `check_try`/`check_let_else`/`check_if_let` 시그니처와 판정, 모든
  재귀 지점의 명시적 place 전달. `src/lib.rs` — `compile_report`의
  verify 실패 억제(결정 2).
- 테스트: compile.rs — 함수 안 if-let 본문의 try·중첩 let-else 허용,
  match 암 안 체인의 거부 유지, 모듈 최상위 인라인 try의 단일 진단
  (`compile_report`로 개수 고정). integration.rs — if-let 본문 try의
  런타임 전파(tsc --strict + node).
- 문서: language.md §5.4 표에 인라인 행, docs/ai/rl.md.

## 이슈 및 해결

없음.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` (전 스위트 실패 0)

## 결과

배치 규칙이 완결됐다: 세 문 모두 "방출되는 제어 이탈이 어디로 가는가"
하나로 판정되고, 인라인/IIFE/모듈의 구분이 명시적 `Place`로 코드에
남았다. 알려진 거짓 거부와 중복 보고가 없어졌다.
