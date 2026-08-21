# TASK-126: cross-snapshot semantic cache와 의존성 무효화 (Phase 6 1/n)

- **상태**: 완료
- **시작일**: 2026-08-21
- **완료일**: 2026-08-21
- **커밋**: —

## 목적

컴파일러 중심부([TASK-119](./TASK-119-compiler-core-design.md), §11)의
query/증분 계층 착수. 기존 증분은 "내용이 같은 파일의 projection 재사용"
한 단계뿐이었고(D9), semantic 계산(pattern analyses, extern 수집)은 check
마다 파일 전체를 — import 대상의 재파싱 포함 — 다시 돌렸다. 파일 단위
semantic 캐시를 (내용, import된 선언) 키로 두어, **의존 파일의 body만
바뀌면 importer가 재분석되지 않고, exported 선언이 바뀌면 정확히
importer들만 무효화**되게 한다. 측정 가능한 cache hit 계측과 테스트가
계약이다.

## 범위

- 포함: `ProjectedDocument`의 파일-로컬 lazy 캐시(`enum_symbols`/
  `rl_imports` — 내용 버전에 고정), `FileSemantics`(externs + analyses)와
  `Project`의 cross-snapshot 캐시(`semantics_cache`, 키 = 내용 해시 +
  externs 동등성), `semantic_cache_hits()` 계측, `semantics::report`가
  재파싱 대신 캐시를 소비(진단 번역 선언 표·`checked_coverage` externs),
  `language::externs_from`(파싱 없이 심볼 제공자 기반의 extern 수집 층),
  invalidation 계약 테스트.
- 제외: query 시스템의 일반화(파일 밖 세분화 — pattern_analysis/flow_body
  단위 query), 디스크 캐시·red-green(설계상 비목표), 에디터 semantic
  API(`language.rs`)의 캐시 소비(별도 태스크 — 지금은 자체 externs_of
  경로 유지).

## 의사결정

### 결정 1: 캐시 키는 (내용 해시, externs 동등성) — 의존성 그래프 기록이 아니라 서명 비교

- **상황**: "exported 선언 변경 시 importer만 무효화"를 어떻게 판정할지.
- **검토한 대안**: (a) 역방향 의존 그래프를 유지하고 변경 전파 — 그래프
  유지 비용과 이동/삭제 경계 케이스. (b) importer의 키에 **의존의 서명**
  (그 파일 스코프의 imported 선언 목록)을 포함 — 서명이 같으면 그래프를
  몰라도 재사용이 정당하다.
- **선택과 근거**: (b). externs는 어차피 report가 계산해야 하는 값이고
  (파일-로컬 lazy 심볼 캐시 덕에 재파싱 없이 싸다), 값 동등성이 무효화
  판정 그 자체가 된다 — false hit이 구조적으로 불가능하다.

### 결정 2: 백엔드 없이도 도는 계약 테스트

- **상황**: `check()` 경유 테스트는 tsgo가 필요해 보였고, env 조작은
  Rust 2024에서 `unsafe`(저장소는 `forbid(unsafe_code)`).
- **선택과 근거**: TASK-124의 강등 덕에 `check()`는 툴체인 유무와 무관하게
  돌고 캐시 계측도 동일하다 — env 조작 없이 어느 환경에서든 같은 단언이
  성립한다(툴체인이 있으면 실제 백엔드까지 도는 더 강한 실행).

## 작업 내역

- 2026-08-21: `engine/projection.rs` — `enum_symbols`/`rl_imports`
  OnceLock 캐시. `engine/language.rs` — `externs_of`를
  `externs_from`(심볼 제공자 기반) 위의 wrapper로 분리.
  `engine/semantics.rs` — `FileSemantics` 정의, `report`가 캐시를 소비
  (선언 표 재파싱·externs 재수집 제거), `externs_of`가 snapshot의 캐시된
  심볼을 읽음. `engine/project.rs` — `semantics_cache`/`CachedSemantics`/
  `semantic_cache_hits`/`file_semantics`, `check()` 연결.
- `tests/engine_cache.rs` — 무변경 hit(2), 의존 body 변경 시 importer
  hit(+1), exported 선언 변경 시 importer miss(카운터 동결) 및 projection
  Arc 재사용.

## 이슈 및 해결

### 이슈 1: env 조작이 `forbid(unsafe_code)`에 걸림

- **증상**: `std::env::set_var`가 Rust 2024에서 unsafe — 컴파일 거부.
- **원인**: 백엔드를 죽여 테스트를 빠르게 하려던 접근.
- **해결**: 결정 2 — 강등 계약 위에서 env 조작 자체를 제거.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` (615건 전부 통과 — engine_cache 2건 추가)

## 결과

check 경로의 파일 semantic이 (내용, import 서명) 키의 캐시에서 재사용되고,
무효화 경계가 테스트로 고정됐다. 후속: 에디터 semantic API의 캐시 소비,
query 세분화.
