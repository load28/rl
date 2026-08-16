# TASK-005: CI 파이프라인 구축

- **상태**: 완료
- **시작일**: 2026-08-16
- **완료일**: 2026-08-16
- **커밋**: `b3d04f2`

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

- 2026-08-16: 잡 2개 구성 — `check`(fmt/clippy/test + Node 22 + typescript 전역
  설치로 통합 테스트 실제 수행)와 `msrv`(1.88 빌드). MSRV 1.88은
  `cargo metadata`로 의존성 최고 rust-version(1.88)과 일치함을 확인.
- 2026-08-16: 범위 추가 — README 개발 섹션의 구조 표기가 TASK-002 이전
  레이아웃(`src/transform.rs`)을 가리키고 있어 새 모듈 레이아웃과 거버넌스
  문서 목록으로 갱신.

## 검증

- [x] `cargo fmt --check` — 통과
- [x] `cargo clippy --all-targets -- -D warnings` — 경고 0개
- [x] `cargo test` — 59개 전체 통과
- [x] CI 워크플로 YAML 문법 점검 (`yaml.safe_load` 통과)

## 결과

- 신규: `.github/workflows/ci.yml` — push(main)/PR 트리거, fmt → clippy(-D
  warnings) → test 게이트, Rust stable + 캐시, Node.js 22 + typescript 설치로
  tsc/node 통합 테스트가 skip되지 않고 실제 수행됨. MSRV(1.88) 빌드 잡 별도.
- README 개발 섹션 갱신.
