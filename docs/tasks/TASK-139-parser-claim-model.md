# TASK-139: 파서 Claim 커밋 모델

- **상태**: 완료
- **시작일**: 2026-08-21
- **완료일**: 2026-08-21
- **커밋**: —

## 목적

파서가 rl 구문 아님과 rl 구문으로 확정됐지만 잘못됨을 타입으로 구분하고, malformed match·enum을 verify 백스톱이 아닌 위치 있는 rl 진단으로 보고한다.

## 범위

- 포함: `Claim<T>`, Program malformed 진단, match·enum 커밋 경계, 케이스 19·32·33 회귀 테스트와 문서
- 제외: 기존 다른 구문의 stray 필드 통합, 진단 병합

## 의사결정

### 결정 1: 완전 파서는 유지하고 커밋 경계를 래핑한다

- **상황**: 기존 Option 파서 전체를 동시에 바꾸면 passthrough 판별 규칙까지 넓게 흔들린다.
- **검토한 대안**: 모든 하위 파서를 즉시 `Claim`으로 바꾸면 변경 범위가 크다. verify 메시지만 개선하면 원인을 파서 위치에서 잡지 못한다.
- **선택과 근거**: `Claim::{Parsed, NotRl, Malformed}`를 공통 타입으로 도입하고, match·enum의 최상위 래퍼가 완전 파서를 재사용한다. 커밋 근거가 확인된 후보만 malformed로 만든다.

## 작업 내역

- 2026-08-21: TASK-139를 진행 중으로 등록했다.
- 2026-08-21: `Claim<T>`와 `Program::malformed`를 도입하고 sema가 typed 진단으로 변환하도록 연결했다.
- 2026-08-21: match는 괄호 없는 식별자 또는 top-level `=>`가 있는 본문에서, enum은 제네릭 또는 케이스 시작의 payload 괄호에서 커밋하도록 구현했다.
- 2026-08-21: malformed enum 필드, 단일 원소 tuple 패턴, 괄호 없는 scrutinee 회귀 테스트를 추가했다.
- 2026-08-21: passthrough 56건과 plain TS enum 런타임 테스트로 커밋 경계를 검증했다.
- 2026-08-21: errors reference와 AI 가이드의 near-miss 설명을 새 전용 진단에 맞췄다.
- 2026-08-21: 전체 fmt·clippy·test 게이트를 통과했다.

## 이슈 및 해결

### 이슈 1: 함수 호출 `match(x)` 뒤의 블록을 rl match로 커밋했다

- **증상**: passthrough 테스트 3건이 `malformed-match`로 오탐됐다.
- **원인**: 닫는 괄호 다음 `{`만 커밋 근거로 사용했다.
- **해결**: 본문에 top-level arm 화살표 `=>`가 있을 때만 커밋하도록 좁혔다.

### 이슈 2: 앞선 plain TS enum이 뒤의 rl enum 때문에 커밋됐다

- **증상**: `runtime_plain_typescript_enum_coexists`가 첫 enum 위치에서 실패했다.
- **원인**: enum 커밋 스캔이 현재 선언의 닫는 중괄호를 넘어 파일 끝까지 진행했다.
- **해결**: 현재 enum body의 중괄호 깊이가 0으로 돌아오는 지점에서 스캔을 종료했다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`

## 결과

match·enum 후보가 parsed/not-rl/malformed 세 상태로 구분된다. 세 blocking 데모가 verify 백스톱 대신 위치 있는 rl 진단을 내며 전체 게이트가 통과했다.
