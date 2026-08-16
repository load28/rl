# TASK-002: transform.rs 모듈 분리

- **상태**: 완료
- **시작일**: 2026-08-16
- **완료일**: 2026-08-16
- **커밋**: (해시는 커밋 후 다음 커밋에서 기입)

## 목적

`src/transform.rs`(958줄)가 변환 루프·enum 파싱/방출·match 파싱/방출을 한 파일에
담고 있어 응집도가 낮다. 관심사별 모듈로 분리해 유지보수성과 리뷰 가능성을 높인다.

## 범위

- 포함:
  - `src/transform.rs` → `src/transform/` 디렉터리 모듈로 분리:
    - `mod.rs` — 메인 변환 루프, `Ctx`, 템플릿 재귀, 소진성 검사, 공용 헬퍼.
    - `enums.rs` — rl enum 파싱(`parse_enum` 계열)과 방출(`emit_enum`),
      타입 스캔(`scan_type_end`), 제네릭 파라미터 추출.
    - `matches.rs` — match 파싱(`parse_match` 계열)과 방출(`emit_match`),
      표현식 스캔(`scan_expr_end`).
- 제외:
  - **동작 변경 없음.** 공개 API, 방출 결과, 에러 메시지 모두 바이트 단위 동일
    유지. 테스트 코드 무변경으로 전체 통과가 증명 기준.

## 작업 기록

- 2026-08-16: 분리 경계 결정 — `scan_type_end`는 enum 필드 타입과 제네릭
  파라미터 스캔에만 쓰이므로 `enums.rs`로, `scan_expr_end`는 match 암 본문
  전용이므로 `matches.rs`로. `at`/`is_reserved`/`RESERVED`/
  `REGEX_PRECEDING_WORDS`는 루프와 파서 양쪽에서 쓰여 `mod.rs`에 두고
  `pub(super)`로 공개.

## 검증

- [x] `cargo fmt --check` — 기존 스타일 유지 (TASK-003에서 일괄 정규화 예정)
- [x] `cargo clippy --all-targets -- -D warnings` — 경고 0개
- [x] `cargo test` — 테스트 코드 무변경으로 59개 전체 통과
- [x] 추가 검증: `examples/shapes.rl` 컴파일 출력이 기존 커밋된
  `examples/shapes.ts`와 바이트 단위 동일 (배너 제외 diff 없음)

## 결과

- 삭제: `src/transform.rs` (958줄 단일 파일)
- 신규: `src/transform/mod.rs` (변환 루프 + Ctx + 소진성 검사),
  `src/transform/enums.rs` (enum 파싱·방출),
  `src/transform/matches.rs` (match 파싱·방출)
- 공개 API·방출 결과·에러 메시지 모두 변경 없음. 코드 이동만 수행.
