# TASK-004: 패키지 메타데이터·라이선스·거버넌스 문서

- **상태**: 완료
- **시작일**: 2026-08-16
- **완료일**: 2026-08-16
- **커밋**: (해시는 커밋 후 다음 커밋에서 기입)

## 목적

엔터프라이즈 채택의 전제 조건인 라이선스 파일, 변경 이력, 기여 가이드,
완전한 패키지 메타데이터를 갖춘다.

## 범위

- 포함:
  - `LICENSE` — Cargo.toml에 선언된 MIT의 실제 라이선스 본문.
  - `CHANGELOG.md` — Keep a Changelog 형식, 기존 릴리스 이력 소급 기록.
  - `CONTRIBUTING.md` — 개발 환경, 검증 게이트, 태스크 문서 프로세스 안내.
  - `Cargo.toml` 메타데이터 보강: `repository`, `readme`, `keywords`,
    `categories`, `rust-version`.
  - `[profile.release]` 최적화 설정 (lto, strip).
- 제외: 코드 로직 변경 없음.

## 작업 기록

- 2026-08-16: LICENSE 저작권자는 특정 개인명 대신 "The rl Authors"로 표기
  (기여자가 늘어나도 갱신 불필요한 관례적 표기).
- 2026-08-16: MSRV는 1.88로 결정 — 코드가 사용하는 let-chains(1.88 안정화)와
  edition 2024가 근거.
- 2026-08-16: CHANGELOG는 Keep a Changelog 형식. 0.3.0 이전 이력은 git
  히스토리에서 소급 정리, 이번 리팩토링은 Unreleased 섹션에 태스크 ID와 함께 기록.

## 검증

- [x] `cargo fmt --check` — 통과
- [x] `cargo clippy --all-targets -- -D warnings` — 경고 0개
- [x] `cargo test` — 59개 전체 통과 (메타데이터 변경 후 재확인)

## 결과

- 신규: `LICENSE`(MIT), `CHANGELOG.md`, `CONTRIBUTING.md`.
- `Cargo.toml`: `repository`/`readme`/`keywords`/`categories`/`rust-version`
  보강, `[profile.release]`(thin LTO + symbol strip) 추가.
