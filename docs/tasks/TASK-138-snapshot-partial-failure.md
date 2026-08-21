# TASK-138: Snapshot 부분 실패

- **상태**: 완료
- **시작일**: 2026-08-21
- **완료일**: 2026-08-21
- **커밋**: —

## 목적

한 `.rl` 파일이 lower되지 않아도 Snapshot과 나머지 파일의 typed 검사를 유지하고, 실패 파일의 rl 진단도 같은 보고서에 포함한다.

## 범위

- 포함: Snapshot의 projected/blocked 파일 상태, projection 실패 누적, typed 보고와 enum extern 수집
- 제외: 파일 읽기 실패와 깨진 심볼릭 링크 복구, `--no-verify` typed 옵션, 진단 병합

## 의사결정

### 결정 1: lower 실패를 Snapshot 데이터로 보존한다

- **상황**: `Project::update`의 `Err`가 첫 실패에서 프로젝트 전체 검사를 중단한다.
- **검토한 대안**: 실패 파일을 완전히 버리면 자체 진단과 overlay의 enum 선언을 잃는다. 최소 TS 스텁은 모듈 의미를 추측해야 한다.
- **선택과 근거**: Snapshot이 성공 projection과 blocked source를 별도로 보존한다. TypeScript 프로그램에는 성공 파일만 넣고, rl 진단과 source 기반 enum 선언에는 blocked 파일도 사용한다.

## 작업 내역

- 2026-08-21: TASK-138을 진행 중으로 등록했다.
- 2026-08-21: 실패 재현 테스트에서 정렬상 첫 파일의 stray pipeline이 `Project::update` 전체를 중단하는 것을 확인했다.
- 2026-08-21: `Snapshot`에 `BlockedFile` 컬렉션을 추가하고 source·전체 rl 진단·지연 enum symbol을 보존했다.
- 2026-08-21: `Project::update`가 lower 실패를 누적하고 성공 projection만 cache와 TypeScript 질의에 넣도록 변경했다.
- 2026-08-21: typed report가 blocked 진단을 포함하고, 정상 파일의 extern 수집이 blocked overlay의 enum 선언도 읽도록 변경했다.
- 2026-08-21: `docs/design/compiler-core.md`, `docs/reference/errors.md`, `docs/ai/rl.md`에 프로젝트 부분 실패 계약을 반영했다.
- 2026-08-21: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`를 실행했다.

## 이슈 및 해결

없음.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`

## 결과

한 파일의 lower 실패는 Snapshot의 데이터가 된다. 해당 파일의 rl 진단과 다른 파일의 분석은 함께 보고되며, 전체 검증 게이트가 통과했다.
