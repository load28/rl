# TASK-004: 패키지 메타데이터·라이선스·거버넌스 문서

- **상태**: 대기
- **시작일**: 2026-08-16
- **완료일**: —
- **커밋**: —

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

—

## 검증

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo test`

## 결과

—
