# TASK-003: 포매팅 표준화 및 린트 게이트

- **상태**: 완료
- **시작일**: 2026-08-16
- **완료일**: 2026-08-16
- **커밋**: `ca7d7df`

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

- 2026-08-16: rustfmt 기본값 채택 결정 — 커스텀 `rustfmt.toml`을 두지 않는 것이
  도구 업그레이드·에디터 연동 마찰이 가장 적음. `cargo fmt` 일괄 적용
  (src 6개 파일 + tests 3개 파일 재포맷, 동작 무변경).
- 2026-08-16: 린트 선언은 `Cargo.toml [lints]`로 중앙화.
  `unsafe_code = "forbid"`, `missing_docs = "warn"`(문서 누락 방지),
  clippy `dbg_macro`/`todo`/`unimplemented`(디버그 잔재 유입 방지).
  clippy restriction 계열(`unwrap_used` 등)은 스캐너 내부 불변식과 충돌해
  채택하지 않음.
- 2026-08-16: `missing_docs`로 드러난 공개 API 문서 누락 보완 —
  `Options` 구조체, `CompileError`의 `message`/`filename`/`col` 필드.

## 검증

- [x] `cargo fmt --check` — 통과
- [x] `cargo clippy --all-targets -- -D warnings` — 경고 0개
- [x] `cargo test` — 59개 전체 통과

## 결과

- 전체 코드베이스 rustfmt 기본 스타일로 정규화.
- `Cargo.toml`에 `[lints.rust]` / `[lints.clippy]` 게이트 선언.
- 공개 API 문서 커버리지 100% (`missing_docs` 경고 0개).
