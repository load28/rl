# TASK-044: 튜플 match — 다중 스크루티니와 곱집합 소진성

- **상태**: 완료
- **시작일**: 2026-08-17
- **완료일**: 2026-08-17
- **커밋**: —

## 목적

TASK-042 제안 P1 구현. `match (a, b) { (P, Q) => ... }`로 두 개 이상의 값을
조합으로 매치하고, 태그 곱집합 전체의 소진성을 rlc가 검사한다 — TypeScript로는
얻을 수 없는 검사.

## 범위

- 포함: AST(TupleMatchExpr/TupleArm/TuplePattern), 파서(암 주도 판별 +
  스크루티니 콤마 분리), sema(원소 수·와일드카드 위치·바인딩 충돌·or 바인딩
  집합 + 곱집합 소진성), codegen(다중 임시 + if-체인), 세 계층 테스트,
  레퍼런스 문서.
- 제외: 튜플-패턴 사이의 or(`(A, B) | (C, D)`) — 원소 수준 or로 동일 표현
  가능(`(A, B | D)`), 필요 시 후속 태스크.

## 의사결정

### 결정 1: 판별은 암 주도, 하위호환 유지 (제안서 결정 4 채택)

- **상황**: `match (a, b)`는 기존에도 콤마 식 스크루티니로 파싱된다.
- **선택과 근거**: 모든 암이 튜플-패턴(또는 마지막 bare `_`)이고 스크루티니가
  최상위 콤마로 2개 이상으로 나뉠 때만 튜플 match. 암에 튜플-패턴이 없으면
  기존 의미(콤마 식) 그대로 — compile.rs
  `comma_expression_scrutinee_is_still_a_single_match`로 고정.

### 결정 2: 방출은 중첩 switch가 아니라 if-체인

- **상황**: 제안서 §3.4는 중첩 switch 스케치였다.
- **검토한 대안**: (A) 중첩 switch — 외부 태그별 그룹핑·원소 `_`·or-패턴
  폴스루 조합 시 방출 로직이 크게 복잡해지고, 가드가 있으면 어차피 if-체인
  필요. (B) 항상 if-체인 — 가드 있는 단일 match의 기존 방출 형태와 동일한
  기계(라벨 블록·발산 규칙)를 재사용하고, 조건이 `$rl_m0.kind === "A" &&
  $rl_m1.kind === "B"`라 tsc가 각 임시를 독립적으로 좁혀 구조 분해가 타입
  트릭 없이 통과한다(integration으로 확인).
- **선택과 근거**: (B). 의미는 동일하고 단순함이 우선. 제안서와의 차이를
  제안 문서에 주석으로 남김.

### 결정 3: 소진성 위치 해석은 위치별 독립, 첫 매치 우선

- **상황**: 단일 match의 소진성은 "후보 중 전부 커버된 것이 있으면 통과,
  아니면 빠진 수가 최소인 후보로 보고"다. 곱집합에서 위치별 후보 조합을 전부
  시도하면 후보 수가 곱으로 늘어난다.
- **검토한 대안**: (A) 위치별 후보 조합 전수 — 복잡도 대비 실익 없음(같은
  태그 집합을 담는 enum이 여럿인 경우 자체가 드묾). (B) 위치별로 섀도잉
  순서(로컬 > 임포트 > 내장)에서 태그 집합을 포함하는 첫 enum을 확정.
- **선택과 근거**: (B). 어떤 위치든 해석 실패면 검사 생략(단일 match의 미지
  유니언과 동일한 보수성). 모든 암이 `_`인 위치는 "전칭"으로 제외하고
  메시지에 `_`로 표시.

### 결정 4: 원소 수 불일치는 파서가 아니라 sema 에러

- **상황**: 구조 불일치를 파서가 거부하면 후보 전체가 통과되어 생성물이
  위치 정보 없는 verify 실패가 된다.
- **선택과 근거**: 파서는 구조(모든 암이 튜플 형태)만 판정하고, 원소 수는
  sema가 `파일:행:열`과 함께 보고 — 에러 계층 계약 그대로.

### 결정 5: 한 튜플 패턴 안의 중복 바인딩 이름은 에러

- **상황**: `(Some(value), Some(value))`는 한 스코프에 `const { value }`를 두
  번 방출해 생성물이 tsc 에러가 된다 — "방출 코드가 tsc 에러를 만들면 안
  된다" 계약 위반 경로.
- **선택과 근거**: sema가 원소들을 가로질러 바인딩 이름 유일성을 검사하고
  별칭(`field: alias`) 안내와 함께 에러. Rust의 "identifier bound more than
  once"에 대응.

## 작업 내역

- 2026-08-17: ast.rs에 TupleMatchExpr/TupleArm/TuplePattern 추가.
  parser/matches.rs 재구성 — parse_match가 ParsedMatch(Single|Tuple)를
  반환하고, 튜플 해석을 먼저 시도(split_scrutinees는 `<`/`>`를 괄호로 취급해
  제네릭 인자 콤마를 보호). 암 꼬리(가드·`=>`·본문)를 parse_arm_tail로
  추출해 단일/튜플이 공유.
- 2026-08-17: sema.rs — check_tuple_match(와일드카드 위치·원소 수·or 바인딩
  집합·바인딩 충돌) + TupleMatchCheck 등록, resolve_enum(위치별 해석)과
  check_tuple_exhaustiveness(오도미터 곱집합 순회, 빠진 조합 5개 이상이면
  요약) 추가.
- 2026-08-17: codegen/matches.rs — emit_tuple_match(다중 `$rl_m{i}` 임시,
  조합 조건 if-체인, 라벨 블록·await 규칙은 기존과 동일), bind_str를 임시
  이름 매개변수화.
- 2026-08-17: 테스트 — compile.rs 13건(방출·하위호환·곱집합 에러·arity·
  바인딩 충돌·내장 enum·3-원소·async), passthrough.rs 1건(`match(a, b)` 호출
  형태), integration.rs 3건(런타임 조합 분기·위치별 내로잉 타입체크·평가
  1회/좌→우).
- 2026-08-17: 문서 — language.md §3.7 신설 + 제한사항 2행, errors.md 소진성
  절 갱신 + 튜플 표, CLAUDE.md 소개문, 제안 문서 P1 상태 주석.
- 2026-08-17: 게이트 통과 후 커밋.

## 이슈 및 해결

### 이슈 1: 튜플-패턴 사이 or를 쓴 테스트가 verify 단계에서 실패

- **증상**: `(East, Fast) | (East, Slow) => 2`를 쓴 테스트가
  `generated TypeScript failed to parse` — 후보 전체가 통과되어 위치 없는
  verify 실패.
- **원인**: 튜플-패턴 사이의 or는 v1 문법에 없다. 파서가 첫 튜플-패턴 뒤의
  `|`에서 실패해 전체 match가 verbatim으로 남은 것.
- **해결**: 같은 커버리지를 원소 수준 or(`(East, Fast | Slow)`)로 표현하도록
  테스트를 수정하고, 제한사항 표에 명시. 후보 실패의 verify 수렴은 기존
  malformed match와 동일한 동작이라 별도 처리하지 않음(범위 제외에 기록).

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` (tsc 6.0.2 + node 22 — 통합 테스트 포함)

## 결과

- 변경: `src/ast.rs`, `src/parser/{mod,matches}.rs`, `src/sema.rs`,
  `src/codegen/{mod,matches}.rs`, `tests/{compile,passthrough,integration}.rs`,
  `docs/reference/{language,errors}.md`, `docs/design/type-inference-gaps.md`,
  `CLAUDE.md`, 본 문서, `docs/tasks/INDEX.md`.
- 후속: TASK-045(중첩 패턴), TASK-046(if let). 튜플-패턴 사이 or는 수요가
  생기면 별도 제안.
