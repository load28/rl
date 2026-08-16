# TASK-014: match or-패턴 (`A | B => ...`)

- **상태**: 완료
- **시작일**: 2026-08-16
- **완료일**: 2026-08-16
- **커밋**: (이 문서를 포함하는 TASK-014 커밋)

## 목적

match에서 여러 케이스가 같은 본문을 공유할 때 본문을 복붙하거나 `_`로
소진성 검사를 포기해야 하는 문제를 해결한다. Rust의 or-패턴처럼
`Escape | Tab => "cancel"` 형태로 한 암이 여러 태그를 커버하게 한다.

## 범위

- 포함: match 암 패턴의 `|` 구분 태그 대안 (`태그-패턴 ("|" 태그-패턴)*`),
  대안별 바인딩 동일성 검사(sema), switch 폴스루 방출(codegen),
  소진성 검사에 모든 대안 태그 반영, 3계층 테스트, 레퍼런스 갱신.
- 제외: 와일드카드 `_`와의 조합(`A | _`), 리터럴/중첩 패턴, let-else의
  or-패턴. 가드와의 조합은 TASK-015에서 다룬다.

## 의사결정

### 결정 1: AST 표현 — `Pattern::Tag`를 `Pattern::Tags(Vec<TagPattern>)`로 일반화

- **상황**: 기존 `Pattern::Tag { tag, bindings }`는 대안 하나만 표현 가능.
- **검토한 대안**: ① `Pattern::Or(Vec<Pattern>)` 별도 배리언트 추가 —
  Wildcard가 중첩될 수 있는 무의미한 상태 공간이 생기고 모든 소비처가
  2중 match를 하게 됨. ② `Pattern::Tags(Vec<TagPattern>)`로 교체 — 단일
  태그는 길이 1 리스트. 상태 공간이 정확히 문법과 일치.
- **선택과 근거**: ②. 소비처(sema/codegen)가 "대안 목록 순회" 하나로
  일반화되고, 잘못된 상태(빈 목록, 중첩 or)가 타입상 표현 불가능하거나
  파서 계약으로 배제된다. 대안별 `tag_off`를 함께 저장해 에러 위치를
  대안 단위로 보고한다.

### 결정 2: 바인딩 규칙 — 모든 대안의 (필드, 바인딩 이름) 집합이 동일해야 함

- **상황**: 방출은 폴스루(`case "A": case "B": { const {...} = $rl_m; ... }`)
  하나의 구조 분해를 공유하므로, 대안마다 다른 바인딩은 표현 불가.
- **검토한 대안**: ① Rust처럼 "같은 이름 바인딩" 요구 — 그러나 rl 바인딩은
  이름 기준 구조 분해라 `A(x) | B(y: x)`처럼 같은 이름을 다른 필드에서
  가져오는 경우가 생기고, 이는 한 구조 분해로 방출 불가. ② (필드명, 바인딩
  이름) 쌍의 집합이 대안 간 완전히 동일할 것을 요구 (순서는 무관).
- **선택과 근거**: ②. 방출 형태가 곧 규칙이 되어 구현·설명 모두 단순하고,
  집합 비교라 `A(x, y) | B(y, x)` 같은 순서 차이는 허용된다. 괄호 없음과
  빈 괄호 `()`는 둘 다 "바인딩 없음"으로 동등하게 취급.

### 결정 3: `||`는 or-패턴으로 해석하지 않음

- **상황**: `A || B => ...`를 만났을 때의 처리.
- **검토한 대안**: ① 빈 대안으로 에러 보고 — 그러나 이 텍스트는 rl 구문으로
  확정된 적이 없으므로 에러 계층 원칙(구문 확정 후에만 에러)에 어긋남.
  ② 파싱 실패로 원문 통과.
- **선택과 근거**: ②. `|` 다음이 다시 `|`면 대안 파싱이 자연히 실패해 match
  전체가 원문 통과한다. 기존 "완전 파싱될 때만 변환" 계약 그대로.

## 작업 내역

- 2026-08-16: `src/ast.rs` — `Pattern::Tag` → `Pattern::Tags(Vec<TagPattern>)`,
  `TagPattern { tag, tag_off, bindings }` 신설.
- 2026-08-16: `src/parser/matches.rs` — 태그 패턴 파싱을 `parse_tag_pattern`으로
  분리하고, `|`(단일)로 이어지는 대안 루프 추가. `||`·비식별자 대안은 파싱
  실패 → 원문 통과.
- 2026-08-16: `src/sema.rs` — 중복 검사를 대안 단위로(암 내부 `A | A` 포함),
  바인딩 집합 동일성 검사 추가(`or-pattern alternatives must bind ...`),
  소진성 태그 수집을 모든 대안으로 확장.
- 2026-08-16: `src/codegen/matches.rs` — 대안들을 `case "A": case "B"` 폴스루
  라벨로 방출, 구조 분해는 첫 대안 기준(검사로 전 대안 동일 보장).
- 2026-08-16: `tests/compile.rs`(방출 형태·에러 5건), `tests/passthrough.rs`
  (비트 OR 인자 통과), `tests/integration.rs`(런타임 or-패턴 1건) 추가.
- 2026-08-16: `docs/reference/language.md` §3 문법·의미·소진성, §7 제한 갱신,
  `docs/reference/errors.md`에 바인딩 불일치 에러 추가.
- 검증: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`.

## 이슈 및 해결

### 이슈 1: 기존 중복 암 테스트의 위치 기대값

- **증상**: 중복 암 에러 위치가 기존에는 `pattern_off`(암 시작)였는데, 대안
  단위 검사로 바꾸면 위치 의미가 바뀔 수 있음.
- **원인**: 단일 태그 암에서는 첫 대안의 `tag_off == pattern_off`이므로
  실질 변화 없음을 확인.
- **해결**: 중복 에러는 해당 대안의 `tag_off`로 보고. 기존 테스트
  (`match_duplicate_arm_is_error`)의 기대 위치 (1,31) 그대로 통과.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`

## 결과

변경 파일: `src/ast.rs`, `src/parser/matches.rs`, `src/sema.rs`,
`src/codegen/matches.rs`, `tests/compile.rs`, `tests/passthrough.rs`,
`tests/integration.rs`, `docs/reference/language.md`,
`docs/reference/errors.md`. 가드와의 조합은 TASK-015로 이어진다.
