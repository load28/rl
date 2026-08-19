# TASK-088: CI native 테스트 guard 개수 갱신

- **상태**: 완료
- **시작일**: 2026-08-20
- **완료일**: 2026-08-20
- **커밋**: 98bf21c

## 목적

PR #36의 CI가 native 테스트 자체는 모두 통과한 뒤, 오래된 테스트 개수 guard에서
실패하는 문제를 수정한다.

## 범위

- 포함: `.github/workflows/ci.yml`의 native 테스트 통과 개수 guard 갱신.
- 제외: TypeScript-Go 핀 변경, native 테스트 추가·삭제, 언어 동작 변경.

## 의사결정

### 결정 1: CI guard의 기대 통과 개수를 현재 native suite 개수로 맞춘다

- **상황**: GitHub Actions 로그에서 `tests/native.rs`는 `23 passed`로 통과했지만,
  workflow가 `19 passed`를 grep해서 실패했다.
- **검토한 대안**: guard 제거 / `23 passed`로 갱신. guard 제거는 toolchain 누락으로
  테스트가 조용히 return되는 경우를 잡지 못한다. 개수 갱신은 기존 CI 의도를 유지한다.
- **선택과 근거**: `23 passed`로 갱신한다. `gh api /repos/load28/rl/actions/jobs/.../logs`
  확인 결과 두 실패 job 모두 `23 passed` 뒤 `grep -q "19 passed"`에서 종료 코드 1로 실패했다.

## 작업 내역

- 2026-08-20: PR #36 CI 로그를 `gh pr checks`, `gh run view`, `gh api .../logs`로 확인했다.
- 2026-08-20: `.github/workflows/ci.yml`의 두 native test guard를 `19 passed`에서
  `23 passed`로 갱신했다.
- 2026-08-20: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test`로 변경을 검증했다.

## 이슈 및 해결

### 이슈 1: CI가 테스트 성공 뒤 실패함

- **증상**: `fmt / clippy / test`와 `type checking (typescript-go)` job이 실패했다.
- **원인**: `tests/native.rs`가 23개 테스트를 실행했지만 workflow guard가 이전 개수인
  `19 passed`를 기대했다.
- **해결**: 두 job의 guard 문자열을 현재 native suite 결과인 `23 passed`로 갱신했다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`

## 결과

`.github/workflows/ci.yml`의 native suite skip guard가 현재 테스트 개수와 일치한다.
