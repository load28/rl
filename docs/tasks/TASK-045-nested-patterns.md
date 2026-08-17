# TASK-045: 중첩 패턴 — `Ok(value: Some(v))`

- **상태**: 완료
- **시작일**: 2026-08-17
- **완료일**: 2026-08-17
- **커밋**: —

## 목적

TASK-042 제안 P2 구현. match 패턴의 바인딩 자리에 내부 태그 패턴을 허용해
`Ok(value: Some(value: v))`처럼 중첩 판별을 한 패턴으로 쓴다. 내부 불일치는
가드 실패처럼 다음 암으로 폴스루한다.

## 범위

- 포함: AST(Binding.nested), 파서(별칭/중첩 판별), sema(or-조합 금지·중복
  바인딩·보수적 소진성), codegen(경로 조건 + 경로별 구조 분해), 단일·튜플
  match 양쪽, 테스트, 문서.
- 제외: let-else의 중첩 패턴(별칭 유지 — 발산 조건과 결합하면 방출·내로잉
  검증이 별도 작업), 내부 곱 소진성 v2(제안서 §4.3), or-패턴과의 조합.

## 의사결정

### 결정 1: 중첩은 괄호 필수 — `field: None`은 지금처럼 별칭

- **상황**: 제안서 예시의 `Ok(value: None)`은 현재 문법에서 "필드 value를
  변수 None으로 바인딩"인 유효한 rl이다. 괄호 없는 태그를 중첩으로
  재해석하면 기존 프로그램의 의미가 바뀐다.
- **검토한 대안**: (A) 대문자 시작이면 태그로 추정 — rl에 대소문자 규칙이
  없고 암묵 규칙 신설은 계약 위반 소지. (B) 괄호 필수 — `field: Tag(...)`
  형태(식별자+`(`)는 현재 문법에 존재하지 않아 무손상. 유닛 케이스는
  `field: None()`로 쓴다(패턴의 빈 괄호 = 바인딩 없음은 기존 의미 그대로).
- **선택과 근거**: (B). 하위호환 무손상 + 판별이 토큰 하나 lookahead로 끝남.
  compile.rs `plain_alias_is_still_an_alias_not_a_nested_pattern`으로 고정.

### 결정 2: 방출은 경로 조건 + 경로별 구조 분해 (if-체인)

- **상황**: 중첩 불일치의 "다음 암으로 폴스루"는 switch로 표현 불가 — 가드와
  같은 문제.
- **선택과 근거**: 중첩 패턴이 있는 match는 가드 match와 동일하게 if-체인
  방출로 전환하고, 조건은 경로 체인(`$rl_m.kind === "Ok" &&
  $rl_m.value.kind === "Some"`), 바인딩은 각 단계 경로에서의 구조 분해
  (`const { value: v } = $rl_m.value;`)로 방출한다. TASK-042 실측(G4)대로
  tsc가 조건 체인의 프로퍼티 경로를 완전하게 좁히므로 타입 트릭이 없다
  (integration `nested_pattern_bindings_typecheck_through_the_paths`로 확인).
  단일 대안·무중첩 암은 기존과 바이트 동일한 방출을 유지하도록
  pattern_conds_binds가 기존 형태로 수렴한다.

### 결정 3: or-패턴과 조합 금지 (sema 에러)

- **상황**: or-대안들은 하나의 구조 분해를 공유하는데(스위치 폴스루 설계),
  중첩 패턴은 대안별로 다른 경로 조건·구조 분해가 필요하다.
- **검토한 대안**: (A) 대안별 분리 방출로 지원 — or-패턴의 "공유 본문" 방출
  구조 전면 개편. (B) v1 금지, 암을 나누도록 안내.
- **선택과 근거**: (B). 같은 커버리지를 암 분리로 항상 표현 가능하고, 기존
  "대안은 같은 바인딩 집합" 규칙과 한 몸인 제약이라 사용자 모델이 일관된다.

### 결정 4: 소진성은 보수적 v1 — 중첩 암은 커버 불인정 (제안서 결정 6 채택)

- **선택과 근거**: 가드 암과 동일 취급("내부 태그가 다를 수 있으므로"). 오류
  방향이 "빠졌다고 잘못 보고" 쪽이라 안전하고, 기존 가드 규칙과 같은 문장
  으로 문서화된다. 내부 곱 소진성은 v2로 명시 이연.

### 결정 5: 패턴 내 중복 바인딩 이름 검사를 전 패턴으로 확대

- **상황**: 중첩으로 잎 바인딩이 흩어지면 `Ok(value: Some(value), error:
  value)` 같은 충돌이 쉬워진다. 기존 단일 패턴의 `Ok(value: v, error: v)`도
  검사 없이 방출돼 tsc 에러가 되는 기존 격차가 있었다.
- **선택과 근거**: 잎 바인딩 이름 유일성 검사를 단일·튜플 패턴 공통으로
  추가(`leaf_bindings` 수집). 기존 격차도 함께 닫힌다 — 에러 계층 계약
  ("방출 코드가 tsc 에러를 만들면 안 된다")의 회복이므로 파괴적 변경이
  아니라 버그 수정으로 분류.

## 작업 내역

- 2026-08-17: ast.rs Binding에 `nested: Option<TagPattern>` 추가(Vec 간접으로
  재귀 무한 크기 없음). parser/matches.rs parse_bindings에 allow_nested
  플래그 — `:` 우측 식별자 뒤 `(`면 중첩 파싱, lets.rs는 false로 별칭 유지.
- 2026-08-17: sema.rs — has_nested/leaf_bindings 헬퍼, or+중첩 금지, 잎
  바인딩 유일성(check_leaf_bindings, 튜플은 원소 가로질러), 커버 집합에서
  중첩 암 제외(단일 MatchCheck·튜플 covered 행 모두), 중첩 암의 태그 반복
  허용(가드 암과 동일 규칙이 기존 duplicate 검사에서 자연 성립함을 확인).
- 2026-08-17: codegen/matches.rs — pattern_conds_binds/collect_conds_binds
  (경로 조건·경로별 구조 분해 재귀 생성), emit_match의 if-체인 전환 조건에
  중첩 추가, emit_if_chain·emit_tuple_match의 단일 대안 경로를 새 헬퍼로
  통일(무중첩 방출은 기존과 동일 바이트).
- 2026-08-17: 테스트 — compile.rs 12건(방출·2단 중첩·혼합 바인딩·별칭 유지·
  소진성 불인정·태그 반복·중복 암·or 금지·중복 바인딩·튜플 원소 중첩·가드
  조합·let-else 미지원), integration.rs 2건(런타임 폴스루·경로 내로잉
  타입체크).
- 2026-08-17: 문서 — language.md(§3.1 문법·§3.2 의미 표·§3.4 방출 표·§3.6
  소진성·§9 제한사항), errors.md(신규 에러 2종 + 소진성 문구), 제안 문서
  P2 상태. 게이트 통과 후 커밋.

## 이슈 및 해결

### 이슈 1: 런타임 통합 테스트가 v1 소진성 규칙에 걸림

- **증상**: `Ok(value: Some(..))`/`Ok(value: None())`/`Err(..)` 세 암으로 쓴
  통합 테스트가 `match on enum Res is not exhaustive: missing "Ok"`로 컴파일
  실패.
- **원인**: 의도된 동작 — 결정 4의 보수적 규칙(중첩 암은 Ok를 커버하지
  않음)이 그대로 작동한 것.
- **해결**: 테스트에 `_` 암을 추가하고 주석으로 규칙을 명시. 문서(§3.6)에도
  같은 예시로 안내를 남김.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` (tsc 6.0.2 + node 22 — 통합 테스트 포함)

## 결과

- 변경: `src/ast.rs`, `src/parser/{matches,lets}.rs`, `src/sema.rs`,
  `src/codegen/matches.rs`, `tests/{compile,integration}.rs`,
  `docs/reference/{language,errors}.md`, `docs/design/type-inference-gaps.md`,
  본 문서, `docs/tasks/INDEX.md`.
- 후속: TASK-046(if let). 내부 곱 소진성 v2와 let-else 중첩은 수요 발생 시
  별도 제안.
