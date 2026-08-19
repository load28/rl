# TASK-075: `types_host.mjs` 제거 — 타입 경로 단일화

- **상태**: 대기
- **시작일**: —
- **완료일**: —
- **커밋**: —

## 목적

TypeScript 5/6의 JS 컴파일러 API에 의존하는 `src/types_host.mjs`를 없애고,
타입 계층을 네이티브 백엔드 하나로 만든다. TS 7이 그 API를 더 제공하지
않으므로 이 경로는 유통기한이 있다 (TASK-051 참조).

## 범위

- 포함: `--types`가 하던 일을 `--native-check`가 전부 할 수 있는지 최종
  확인, CLI 정리(`rlc check` / `rlc build` 모드 정립과 마이그레이션 안내),
  `types_host.mjs`·관련 코드·CI의 `typescript@6` 고정 제거,
  `docs/reference/cli.md` 갱신.
- 제외: 에디터 규약(TASK-074) — 그것이 먼저 끝나야 한다.

## 선행 조건

1. TASK-074 완료 (사이드카 규약과 에디터).
2. 기능 parity 확인: 진단·소진성·`val`·선언 emit·`@rl/std`는 확인됨
   (TASK-073). 남은 것은 `-w` 감시 모드와 `--jobs`.

## 의사결정

*작업 시작 시 기록.*

## 작업 내역

*작업 시작 시 기록.*

## 이슈 및 해결

*작업 시작 시 기록.*

## 검증

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo test`

## 결과

*작업 완료 시 기록.*
