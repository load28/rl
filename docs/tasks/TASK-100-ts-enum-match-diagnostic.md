# TASK-100: TS enum을 scrutinee로 쓴 `match`를 rl 진단으로

- **상태**: 대기
- **시작일**: —
- **완료일**: —
- **커밋**: —

## 목적

TASK-073 이슈 3이 남긴 근본 해결이다. 괄호 없는 케이스만 가진
`enum Plain { A, B }`는 판별 규칙상 **TypeScript enum**이므로 그대로
통과되지만(계약 1), 거기에 `match`를 쓰면 방출 코드가 `$rl_m.kind` switch라
`Property 'kind' does not exist on type 'Plain'`이 나온다. 지금은 그 진단이
**rlc가 만든 글루 코드**에 떨어져 있어(TASK-089가 위치만 가장 가까운 앞선
verbatim 바이트로 보정하고 `(in code rlc generated for this construct)`를
붙인다) 사용자는 "왜 내 match가 TS 에러를 내는가"를 스스로 번역해야 한다.

에러 계층 계약(계약 2)상 이것은 **rl 수준 에러**다 — "이 scrutinee는 rl
enum이 아니다"는 rl의 판단이고, rlc가 위치와 함께 직접 보고해야 한다.
TASK-073~077로 타입을 물을 수 있게 됐으므로 이제 가능하다.

## 범위 (착수 시 확정)

- 포함 후보:
  - typed 경로(`--check-types`/`--types`/`--server`)에서 match scrutinee의
    타입을 물어, 판별자(`kind`) 필드가 없는 타입이면 rl 진단으로 보고.
  - 진단 문안과 `docs/reference/errors.md` 항목, `docs/ai/rl.md` 반영.
- 제외 후보:
  - untyped 배치 빌드에서의 판정 — 타입 없이는 알 수 없으므로 현행 유지.
  - TS enum에 대한 `match` 지원(방출 형태 변경) — 별도 사안.

## 의사결정

*착수 시 기록.*

## 작업 내역

*착수 시 기록.*

## 이슈 및 해결

*착수 시 기록.*

## 검증

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo test`

## 결과

*작업 완료 시 기록.*
