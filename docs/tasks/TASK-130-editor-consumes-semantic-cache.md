# TASK-130: 에디터 semantic API가 semantic cache를 소비한다 (Phase 6 2/n)

- **상태**: 완료
- **시작일**: 2026-08-21
- **완료일**: 2026-08-21
- **커밋**: —

## 목적

[TASK-126](./TASK-126-semantic-cache.md)이 만든 cross-snapshot semantic
cache는 typed 패스(`Project::check`)만 소비하고 있었다. 에디터 semantic
API(`language.rs`)의 폴백 경로 — or-패턴 바인딩 hover/definition, 백엔드
부재 시의 선언 테이블 hover, `service_diagnostics`의 glue 번역용 선언
테이블 — 는 요청마다 `pattern_analyses`를 새로 계산했다(파일 lex/parse/
lower/resolve/analyze + import 대상 재파싱). 이 두 소비자를 `Project`의
한 캐시로 통일한다: 같은 (내용, 임포트 선언) 키에 대한 분석은 어느
표면이 먼저 물었든 한 번만 계산된다.

## 범위

- 포함: `Project::cached_semantics`(조회-계산-저장의 단일 지점) 추출,
  `Project::semantic_analyses`(오버레이 우선 + projection 캐시 fast path
  로 externs를 모아 `cached_semantics`에 묻는 에디터용 질의) 신설,
  `language.rs`의 `analyses_of` 삭제와 4개 호출처 전환, 테스트 2건
  (오버레이 우선 + 캐시 적중 회계, typed 패스↔에디터 캐시 공유),
  resolve 모듈 문서의 낡은 문장 정정(TASK-129 정산 반영), 설계 문서 갱신.
- 제외: parse-only 표면(`names`/`hints`/`completions`의 `analyses_for`)
  — "프로젝트 없이도 답한다"가 그 표면의 계약이라 캐시를 소비할 수 없다
  (문서로 명시); query 세분화(pattern_analysis/flow_body 단위)는 Phase 6
  잔여로 유지.

## 의사결정

### 결정 1: 캐시 소비 지점은 `Project` 메서드다 — 캐시를 넘기지 않는다

- **상황**: `language.rs`의 `analyses_of(overlays, path, source)`는 자유
  함수였다. 캐시를 소비시키는 방법은 ① 캐시 참조를 인자로 넘기거나
  ② `Project` 메서드로 올리는 것.
- **선택과 근거**: ②. `semantics_cache`·`semantic_cache_hits`·`overlays`·
  projection `cache`가 전부 `Project`의 소유이고(RefCell/Cell이라 `&self`
  로 충분), 캐시 참조를 돌리면 조회·저장 규칙이 다시 두 곳이 된다.
  조회-계산-저장은 `cached_semantics` 한 곳: typed 패스의
  `file_semantics`와 에디터의 `semantic_analyses`는 externs를 **어디서
  읽는가**(스냅샷의 문서 vs 오버레이/디스크)만 다르고 그 뒤는 같은
  메서드다.

### 결정 2: 에디터 경로의 임포트 대상도 projection 캐시로 답한다

- **상황**: externs 수집이 임포트 대상 파일을 `enum_symbols`로 재파싱해
  왔다. `Project.cache`(projection)는 대상 파일의 `enum_symbols`를
  OnceLock으로 이미 들고 있다.
- **선택과 근거**: 대상의 현재 텍스트(오버레이 우선)가 projection의
  텍스트와 같을 때만 그 심볼을 재사용하고, 다르면(마지막 스냅샷 이후
  편집) 재파싱한다 — snapshot 경로의 `semantics::externs_of`가 스냅샷
  문서에 대해 하는 것과 같은 규칙을 프로젝트의 장수명 캐시에 적용한
  것. 텍스트 동등성 검사가 낡은 심볼 사용을 차단한다.

### 결정 3: parse-only 표면은 캐시를 소비하지 않는다

- **상황**: `names`/`hints`/`completions`도 요청마다 `analyses_for`로
  계산한다.
- **선택과 근거**: 그 표면의 계약이 "toolchain 없이, 프로젝트 없이,
  소스만으로 답한다"(names.rs 모듈 문서)이다. `Project`를 요구하는 순간
  계약이 깨진다. `analyses_for`의 문서에 이 구분(프로젝트가 있는 표면은
  `semantic_analyses`를 묻는다)을 명시하는 것으로 경계를 남겼다.

## 작업 내역

- 2026-08-21: `src/engine/project.rs` — `file_semantics`의 루프 본문을
  `cached_semantics(path, source, externs)`로 추출(해시·externs 동등성
  검사·적중 카운트·저장 동일). `semantic_analyses(path, source)` 신설:
  `rl_imports` → `externs_from`(오버레이 우선 읽기, projection 캐시
  fast path, exported 필터) → `cached_semantics`.
- `src/engine/language.rs` — `analyses_of` 삭제. `match_binding_hover`/
  `declared_hover_unserved`/`match_binding_definitions`/
  `service_diagnostics`(선언 테이블)를 `self.semantic_analyses(...)`로
  전환. `analyses_for`는 디스크 전용 계산으로 단순화하고 문서에 경계
  명시(결정 3).
- `src/resolve/mod.rs` — 모듈 문서의 "동등성 테스트로 고정, Phase 3
  까지" 문장을 현재 상태(단일 구현, analysis는 뷰 소비)로 정정.
- 테스트: `analyses_collect_imported_declarations_like_the_cli`를
  Project 기반으로 재작성(디스크 답 → 같은 질문은 적중 → 임포트
  오버레이가 externs를 바꿔 무효화·재계산) +
  `the_editor_and_the_typed_pass_share_one_semantic_cache`(check가 계산한
  항목을 에디터 질의가 적중) 신설.
- `docs/design/compiler-core.md` — Phase 6 잔여에서 cache 소비 항목 제거
  (query 세분화만 남김).

## 이슈 및 해결

없음.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` (전 스위트 실패 0; 신규 2건 포함)

## 결과

패턴 분석의 계산 지점이 `Project::cached_semantics` 하나가 됐다: typed
패스가 계산한 파일을 에디터 폴백이 다시 계산하지 않고(그 역도 같다),
에디터의 hover/definition 폴백은 편집되지 않은 파일에 대해 요청마다
파싱하던 비용을 잃는다. Phase 6 잔여는 query 세분화만 남았다.
