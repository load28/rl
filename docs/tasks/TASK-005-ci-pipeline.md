# TASK-005: CI 파이프라인 구축

- **상태**: 대기
- **시작일**: 2026-08-16
- **완료일**: —
- **커밋**: —

## 목적

검증 게이트(fmt/clippy/test)를 사람 손이 아니라 CI가 강제하게 한다.
tsc/node 기반 통합 테스트까지 CI에서 실제로 수행되도록 한다.

## 범위

- 포함:
  - `.github/workflows/ci.yml` — push/PR 트리거:
    1. `cargo fmt --check`
    2. `cargo clippy --all-targets -- -D warnings`
    3. `cargo test` (Node.js + typescript 설치 후 — 통합 테스트가 skip되지
       않고 실제 수행되도록)
  - Rust 툴체인 stable 고정, 의존성 캐시.
- 제외: 릴리스/배포 자동화 (추후 태스크).

## 작업 기록

—

## 검증

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo test`
- [ ] CI 워크플로 YAML 문법 자체 점검

## 결과

—
