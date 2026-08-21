# TASK-146: 에디터 잠정 진단의 원인 소유권 정리

- **상태**: 완료
- **시작일**: 2026-08-22
- **완료일**: 2026-08-22
- **커밋**: —

## 목적

직접 RL 원인이 있는 lowering에서 에디터의 잠정 TypeScript 연쇄 진단이 남는
문제를 해결한다. 컴파일러의 원인 소유권 판정이 VSCode 최종 게시 결과에도
그대로 적용되게 한다.

## 범위

- 포함: VSCode 진단 계층 병합과 게시 순서, mixed match 회귀 테스트, 재설치 검증
- 제외: mixed match 의미 규칙 변경, TypeScript checker 규칙 변경

## 의사결정

1. **VSCode에서 진단 코드나 위치를 보고 지우지 않는다.** 편집기 계층에는 생성
   코드의 lowering 소유권이 없으므로 새 문법과 중첩 구문에서 같은 문제가 반복된다.
2. **language-service projection이 직접 RL 원인을 함께 보존한다.** projection을
   만들 때 이미 parser·sema 진단과 lowering anchor를 함께 얻는다. 빠른 checker
   진단을 원문으로 투영하는 같은 경계에서 owner가 일치하는 결과를 제거한다.
3. **batch와 service가 하나의 owner predicate를 공유한다.** 표시 범위 겹침과
   완전한 syntax-owner 일치를 판정하는 함수를 projection 모듈에 두고 두 경로가
   호출한다.

## 작업 내역

- 2026-08-22: 재설치 후 mixed match에서 직접 `404` 오류와 함께 `match` 및
  다른 arm의 잠정 밑줄이 남는 현상을 확인했다.
- 2026-08-22: `ServiceDoc`에 해당 projection이 수집한 RL 진단을 보존하고,
  language-service checker 응답을 원문 진단으로 만들기 전에 공통 owner predicate를
  적용했다.
- 2026-08-22: VSCode engine 회귀 테스트로 direct RL cause가 있는 lowering의
  빠른 TypeScript 진단이 빈 목록인지 고정했다.
- 2026-08-22: 전체 검증 후 release 컴파일러와 VSCode 확장을 재설치하고
  rl-tour 패키지를 갱신했다. 실제 `_errors-demo.rl`의 `tsDiagnostics` 응답에서
  mixed match 주변의 잠정 checker 진단이 빈 목록임을 확인했다.

## 이슈 및 해결

- **증상**: 최종 `typedCheck` 응답에는 mixed-pattern RL 오류만 있었지만 에디터에는
  `match`, `404`, 다른 arm의 밑줄이 함께 보였다.
  **원인**: 빠른 `tsDiagnostics` 경로의 `ServiceDoc`이 projection의 RL 진단을
  버렸다. 최종 typed 경로만 owner 기반 억제를 수행했다.
  **해결**: `ServiceDoc`이 직접 원인을 보존하고 batch와 동일한
  `origin_intersects_rl_error` 판정을 게시 전에 수행하게 했다.

## 검증

- [x] VSCode 직접 원인 소유권 회귀 테스트 통과
- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` — 665개 통과
- [x] VSCode 전체 104개 중 98개 통과, 기존 범위 밖 6개 실패
- [x] release 컴파일러·VSCode 확장·rl-tour 패키지 재설치
- [x] rl-tour 실제 `tsDiagnostics` mixed match 잠정 진단 0개

## 결과

빠른 language-service 검사와 권위 있는 batch typed 검사가 같은 projection의
직접 RL 원인과 syntax owner를 사용한다. 편집기는 어느 응답 시점에도 생성 코드의
연쇄 오류를 받지 않으며, 새 lowering도 공통 anchor·owner 계약으로 같은 판정을
자동 적용받는다.
