# Changelog

이 프로젝트의 주목할 만한 변경 사항을 기록합니다.
형식은 [Keep a Changelog](https://keepachangelog.com/ko/1.1.0/)를 따르고,
버전은 [Semantic Versioning](https://semver.org/lang/ko/)을 따릅니다.

## [Unreleased]

### Added

- `Option`/`Result` 표준 라이브러리: `rlc --emit-std <file>`이 함수형
  콤비네이터(`map`/`andThen`/`unwrapOr` 등)를 담은 순수 TypeScript 모듈을
  생성 (`docs/reference/std.md`, 라이브러리 API `rlc::STD_SOURCE`).
  `Option`(Some/None)·`Result`(Ok/Err)는 내장 enum으로 인식되어 파일에 선언이
  없어도 match 소진성 검사를 받는다 — 같은 이름의 로컬 rl enum이 있으면
  로컬이 우선. (TASK-011)

- 태스크 관리 체계 (`docs/tasks/`) 및 `CLAUDE.md` 작업 가이드. (TASK-001)
- 린트 게이트: `Cargo.toml [lints]` — `unsafe_code` 금지, `missing_docs` 경고,
  clippy `dbg_macro`/`todo`/`unimplemented`. (TASK-003)
- 거버넌스 문서: `LICENSE`(MIT), `CHANGELOG.md`, `CONTRIBUTING.md`. (TASK-004)
- 패키지 메타데이터: `repository`, `rust-version`, `keywords`, `categories`,
  릴리스 프로파일(lto, strip). (TASK-004)
- CI 파이프라인: fmt/clippy/test 게이트, tsc·node 통합 테스트 포함. (TASK-005)
- 라이브러리 수준 문서화: 규범 레퍼런스 `docs/reference/`(언어·CLI·에러) 신설,
  공개 API rustdoc·doctest 확충, README 문서 안내 섹션. (TASK-007)

### Changed

- `src/transform.rs`를 `src/transform/{mod,enums,matches}.rs` 모듈로 분리 —
  동작 변경 없음. (TASK-002)
- 전체 코드베이스를 rustfmt 기본 스타일로 정규화. (TASK-003)
- 레퍼런스 문서(`docs/reference/`)를 사용자 관점으로 단순화 — 스캔 규칙,
  판별 규칙 안전성 증명, 소진성 검사 알고리즘 등 내부 구현 상세를 제거하고
  사용자가 관찰 가능한 동작만 서술. README의 "동작 원리" 절 제거. (TASK-009)

## [0.3.0] - 2026-08-16 이전

### Added

- Rust 재작성: 바이트 스캔 기반 변환기 + swc 검증 (조각 검증·출력 자가 검사).
- `enum` 키워드 통합: 페이로드/제네릭 규칙으로 rl enum과 TS enum 구분,
  TS enum은 그대로 통과.
- 소진성 검사를 rlc 수준 에러로 이동 (`파일:행:열` 보고, tsc 비위임).
- CLI: 디렉터리 재귀 컴파일, `-o`/`-p`/`--check`/`--no-banner`/`--no-verify`.
- 테스트 3계층: 컴파일 출력 단위 테스트, 통과(passthrough) 계약 테스트,
  tsc/node 통합 테스트.
