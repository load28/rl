# TASK-003: 포매팅 표준화 및 린트 게이트

- **상태**: 대기
- **시작일**: 2026-08-16
- **완료일**: —
- **커밋**: —

## 목적

코드 스타일을 도구로 강제 가능하게 만든다. 사람/리뷰어 취향이 아니라
`cargo fmt --check`와 `cargo clippy -- -D warnings`가 스타일과 품질의 기준이
되도록 한다.

## 범위

- 포함:
  - 전체 코드베이스에 **rustfmt 기본 스타일** 적용 (커스텀 rustfmt.toml 없이
    표준 기본값 채택 — 엔터프라이즈에서 가장 마찰 없는 선택).
  - `Cargo.toml`에 `[lints.rust]` / `[lints.clippy]` 선언:
    `unsafe_code = "forbid"`, `missing_docs = "warn"` 및 clippy pedantic 중
    가치 있는 항목 선별.
  - 린트 선언으로 새로 드러나는 경고 0개까지 정리.
- 제외: 동작 변경 없음 (포매팅과 린트 대응만).

## 작업 기록

—

## 검증

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo test`

## 결과

—
