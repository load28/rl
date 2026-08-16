# TASK-007: 라이브러리 수준 문서화

- **상태**: 완료
- **시작일**: 2026-08-16
- **완료일**: 2026-08-16
- **커밋**: d9bc03e

## 목적

"문서가 라이브러리 수준이 아님" 이슈 대응. 현재 문서는 README(튜토리얼),
설계 문서, 거버넌스 문서뿐이며, 실제 라이브러리가 갖춰야 할 **규범적 레퍼런스**
(언어 스펙, CLI 스펙, 에러 목록)와 **API 문서**(rustdoc)가 없다. 사용자가
README 예제 너머의 정확한 동작을 알려면 소스를 읽어야 하는 상태를 해소한다.

## 범위

- 포함:
  - `docs/reference/language.md` — 언어 레퍼런스 (문법·판별 규칙·방출 코드·
    소진성 검사 알고리즘·예약어·제한사항의 규범적 명세)
  - `docs/reference/cli.md` — rlc CLI 레퍼런스 (옵션·입출력 경로 규칙·종료 코드)
  - `docs/reference/errors.md` — 에러 레퍼런스 (모든 진단 메시지의 형식·원인·해결)
  - 공개 API rustdoc 확충 (`src/lib.rs`) — doctest 포함, `cargo doc` 산출물이
    docs.rs 수준이 되도록
  - README/CLAUDE.md/CONTRIBUTING.md에 문서 체계 연결, CHANGELOG 기록
- 제외:
  - 언어·CLI 동작 변경 (문서화만; 코드 변경은 rustdoc 주석뿐)
  - 영문 번역본, 문서 사이트(mdBook 등) 구축, docs.rs 배포 설정

## 의사결정

### 결정 1: README 확장 대신 `docs/reference/` 분리

- **상황**: 부족한 내용을 README에 계속 덧붙일지, 규범 문서를 분리할지.
- **검토한 대안**:
  - A. README 확장 — 파일 하나로 유지되지만 이미 200줄이고, 튜토리얼(빠르게
    감 잡기)과 레퍼런스(정확한 동작 규정)는 독자와 서술 방식이 다르다.
  - B. `docs/reference/` 분리 — 표준적인 라이브러리 문서 구조(튜토리얼 →
    레퍼런스 → 설계). 문서별 단일 책임.
- **선택과 근거**: B. 이슈가 요구하는 "라이브러리 수준"은 정확히 이 구조를
  뜻한다. 기존 `docs/design/`(왜 이렇게 설계했나)과 역할이 겹치지 않게
  레퍼런스는 "현재 동작이 무엇인가"만 규정한다.

### 결정 2: 레퍼런스는 한국어, rustdoc은 영어

- **상황**: 새 문서의 언어 선택.
- **검토한 대안**: 전부 한국어 / 전부 영어 / 혼합.
- **선택과 근거**: 기존 관행 유지 — 저장소 문서(README, CLAUDE.md,
  docs/design, docs/tasks)는 전부 한국어, 소스 내 주석·rustdoc은 전부 영어다.
  레퍼런스는 한국어(`docs/reference/`), rustdoc은 영어로 작성해 일관성을 지킨다.

### 결정 3: API 문서는 rustdoc + doctest로 소스에 내장

- **상황**: Rust API 사용법 문서를 어디에 둘지.
- **검토한 대안**:
  - A. `docs/reference/api.md` 별도 마크다운 — 소스와 어긋나도 알 수 없다.
  - B. rustdoc 주석에 doctest 포함 — `cargo test`가 예제 컴파일·실행을
    강제하므로 문서-코드 동기화가 검증 게이트에 편입된다. `cargo doc`으로
    표준 API 문서가 생성된다.
- **선택과 근거**: B. 이미 `Cargo.toml`에 `missing_docs = "warn"` 린트가
  있어 방향도 일치한다. 확인 방법: `cargo test`의 Doc-tests 섹션 통과.

### 결정 4: 레퍼런스 갱신을 컨벤션으로 강제

- **상황**: 레퍼런스는 구현과 어긋나는 순간 가치가 음수가 된다.
- **검토한 대안**: 강제 없음 / CLAUDE.md·CONTRIBUTING에 갱신 규칙 명문화.
- **선택과 근거**: 후자. 언어 표면(구문, 방출 코드, 에러 메시지, CLI)을
  바꾸는 변경은 `docs/reference/` 갱신을 동반해야 한다는 규칙을 CLAUDE.md
  코딩 컨벤션과 CONTRIBUTING 검증 게이트 절에 추가한다.

## 작업 내역

- 2026-08-16: 소스 전체(`lib.rs`, `error.rs`, `main.rs`, `scanner.rs`,
  `transform/{mod,enums,matches}.rs`, `verify.rs`)를 읽고 문서화할 동작 목록
  추출 — 판별 규칙, 필드 타입 스캔 경계, 예약어 목록, async 감지 규칙,
  소진성 후보 선택 알고리즘, CLI 경로 규칙, 전체 에러 메시지 형식.
- 2026-08-16: `docs/reference/language.md` 작성 — 소스 모델(스캔 규칙),
  enum(문법·판별·방출·검사), match(문법·방출·async·소진성), 예약어, 제한사항.
- 2026-08-16: `docs/reference/cli.md` 작성 — 옵션, 입력 수집·출력 경로 규칙,
  배너, 진단 출력 형식, 종료 코드.
- 2026-08-16: `docs/reference/errors.md` 작성 — 컴파일 에러 6종 + CLI 에러
  전부의 형식·원인·해결.
- 2026-08-16: `src/lib.rs` rustdoc 확충 — 크레이트 문서에 빠른 시작 doctest,
  `compile`에 Examples/Errors 섹션(성공·에러 doctest), `Options` 필드 문서
  보강. `src/error.rs`의 `CompileError`에 Display 형식 doctest 추가.
- 2026-08-16: README에 문서 안내 섹션, CLAUDE.md 아키텍처 맵·컨벤션,
  CONTRIBUTING 게이트, CHANGELOG(Unreleased) 갱신.
- 2026-08-16: 검증 게이트 실행 (아래 "검증").

## 이슈 및 해결

없음.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` (doctest 포함)

## 결과

- 신규: `docs/reference/language.md`, `docs/reference/cli.md`,
  `docs/reference/errors.md`
- 수정: `src/lib.rs`, `src/error.rs`(rustdoc만, 동작 변경 없음), `README.md`,
  `CLAUDE.md`, `CONTRIBUTING.md`, `CHANGELOG.md`, `docs/tasks/INDEX.md`
- 후속 작업 없음.
