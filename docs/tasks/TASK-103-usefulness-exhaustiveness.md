# TASK-103: 소진성을 usefulness 알고리즘으로 (P5)

- **상태**: 진행 중
- **시작일**: 2026-08-20
- **완료일**: —
- **커밋**: —

## 목적

[TASK-101](./TASK-101-rust-parity-review.md)의 제안 P5. 소진성 계산을 태그
집합의 곱집합(odometer)에서 **rustc가 쓰는 usefulness 알고리즘**(Maranget)으로
교체한다. 한 알고리즘이 세 가지를 함께 답한다: 소진성, 빠진 것의 **증거**
(witness), 도달 불가 arm.

지금 고쳐지는 실제 오답(실측): 중첩 패턴으로 안쪽을 전부 덮은 **소진된**
match가 "빠졌다"고 거절된다.

```rl
enum Inner { Yes(n: number), No }
enum Outer { Wrap(inner: Inner), Bare }
const a = match (o) {
  Wrap(inner: Yes(n)) => n,
  Wrap(inner: No()) => 0,
  Bare => -1,
};
// rlc: nest.rl:4:11: match on enum Outer is not exhaustive: missing "Wrap"   ← 오답
```

## 범위

- 포함: `analysis/usefulness.rs` 신규(알고리즘), `Coverage` 모델 교체(증거는
  태그가 아니라 **패턴**), 단일·튜플 match 통합, 도달 불가 arm 계산과 모델 노출,
  문서 갱신.
- 제외:
  - 도달 불가 arm을 **에러로 보고**하는 것 — rl에는 경고 계층이 없어 rustc의
    lint를 하드 에러로 바꾸면 지금 동작하는 프로그램이 깨진다(아래 결정 3).
  - 리터럴 match의 소진성 — 타입 질문이라 `--types` 경로의 것이다.
  - 제네릭 인스턴스화 — 여전히 체커의 몫(P4).

## 의사결정

### 결정 1: 행렬을 AST 참조로 두고 사전 lowering을 하지 않는다

- **상황**: usefulness는 고정 arity의 패턴 행렬 위에서 돈다. rl 패턴은 필드를
  **이름으로, 부분집합만** 바인딩하므로(`Circle(radius)`와 `Circle()` 둘 다
  모든 Circle에 매치) 열로 펴려면 생성자의 선언된 필드 목록이 필요하다.
- **검토한 대안**:
  - (a) 먼저 전체를 typed pattern으로 lowering — 중첩 패턴의 arity를 알려면 그
    안쪽 enum을 알아야 하고, 안쪽 enum은 그 열에 쓰인 패턴들로 정해지므로
    순환한다. 열 타입을 먼저 계산하는 별도 top-down 패스를 하나 더 만들어야 한다.
  - (b) 셀을 `Wild | Tag(&TagPattern)`로 두고, **특수화하는 순간** 생성자의 필드
    목록으로 펴기 — 그 시점에는 열 타입이 이미 정해져 있다.
- **선택과 근거**: (b). 순환이 사라지고 패스가 하나 줄며, "패턴이 어떻게
  분해되는지는 열의 생성자가 정한다"는 사실이 코드 모양에 그대로 드러난다.
  비용은 행렬이 AST를 빌린다는 것뿐이다.

### 결정 2: 안쪽 열의 enum은 선언 타입 → 쓰인 패턴 → 미지 순으로 정한다

- **상황**: `Ok(value: Some(v))`에서 `value` 열의 알파벳은 무엇인가. `Ok`의
  필드는 `value: T`로 선언되어 있고 `T`는 enum이 아니다. 여기서 포기하면
  **가장 흔한 중첩 사례(`Result<Option<...>>`)가 하나도 개선되지 않는다** —
  rlc는 타입 인자를 치환하지 않기 때문이다.
- **검토한 대안**:
  - (a) 선언 타입만 — 위 이유로 사실상 무력.
  - (b) 그 열에 **쓰인 태그**로 정하기(`Some`/`None` → `Option`) — 이것은 이미
    match의 스크루티니를 arm 태그로 정하는 규칙과 **같은 규칙**이다. rl은 어차피
    최상위에서도 스크루티니의 타입을 읽지 않는다.
  - (c) 둘 다: 선언 타입이 enum을 지목하고 그 enum이 그 열의 태그를 전부 가지면
    그것을(섀도잉·정밀도), 아니면 쓰인 태그로.
- **선택과 근거**: (c). 정밀도와 적용 범위를 둘 다 얻는다. 어느 쪽도 실패하면
  `Opaque`로 두고 `_`만 커버로 인정한다 — 모르는 것을 아는 척하지 않는다.
  확인: `nest2.rl`(`Result<Option<number>, string>` 중첩)이 (a)에서는 계속
  거절되고 (c)에서는 통과한다.

### 결정 3: 도달 불가 arm을 계산하되 보고하지 않는다

- **상황**: 같은 재귀가 "이 arm은 앞선 arm들이 이미 다 덮었다"를 답한다. rustc는
  이것을 `unreachable_patterns` **lint**(기본 warn)로 낸다.
- **검토한 대안**:
  - (a) 에러로 보고 — rl에는 경고 계층이 없다. 방어적으로 마지막에
    `Ok(value)` 같은 포괄 arm을 두는 코드가 **지금은 컴파일되는데** 에러가 된다.
    회귀다.
  - (b) 계산하지 않음 — 알고리즘이 공짜로 주는 답을 버린다.
  - (c) 모델(`Coverage::unreachable`)에 담되 아무도 에러로 만들지 않는다.
- **선택과 근거**: (c). 에디터가 힌트로 보여줄 자리(P3)가 이미 예정되어 있고,
  그때 심각도를 고를 수 있다. 기존 중복 arm 에러(무가드 arm이 덮은 태그의 반복)는
  손대지 않았다 — 좁고 확실한 규칙이라 회귀 위험이 없다.

### 결정 4: witness는 rl 패턴으로 렌더하고 와일드카드 필드는 생략한다

- **상황**: 빠진 값을 어떻게 보여줄 것인가. 기존 메시지는 태그를 큰따옴표로
  나열했다(`missing "Rect"`).
- **검토한 대안**: 태그만 유지(중첩 정보 손실) / rustc처럼 백틱 패턴 / 기존
  따옴표를 유지한 채 내용만 패턴으로.
- **선택과 근거**: 마지막. 제약 없는 필드를 생략하므로 평범한 경우의 문안은
  **기존과 바이트 단위로 같고**(`missing "Rect"`), 중첩이 있을 때만 자라난다
  (`missing "Ok(value: None)"`). 그리고 그 문자열은 사용자가 그대로 arm으로
  붙여넣을 수 있는 유효한 rl 패턴이다.

## 작업 내역

- 2026-08-20: `src/analysis.rs` → `src/analysis/mod.rs`로 이동(`git mv`),
  `src/analysis/usefulness.rs` 신규 — `Cell`/`ColTy`/`Witness`,
  `usefulness()`(U(P,q) + witness), `specialize`/`default`/`expand`/`descend`/
  `rebuild`, `missing()`·`is_useful()` 두 진입점.
- 2026-08-20: `Coverage` 교체 — `missing: Vec<Vec<Option<String>>>` →
  `Vec<Vec<String>>`(렌더된 witness), `unreachable: Vec<usize>` 추가.
  `covered`는 "arm이 통째로 덮은 태그"라는 요약으로 남겼다(P3의 arm 완성이
  쓸 자리).
- 2026-08-20: `coverage_of`/`tuple_coverage_of` 재작성 — or-패턴은 행 분리,
  튜플 arm은 원소 대안들의 곱으로 행 생성(`tuple_rows`), 후보 선택은
  "witness가 0인 후보 → 없으면 witness가 가장 적은 후보"로 옮겨
  `Table::resolve_coverage`를 삭제(중복 제거).
- 2026-08-20: `sema.rs` 튜플 렌더링을 `Vec<String>` 모양에 맞춤.
- 2026-08-20: 테스트 — `tests/compile.rs` +5(중첩 소진 통과, 제네릭 페이로드,
  witness 문안, 전부 가드, 3단 중첩), `src/analysis/mod.rs` +3(도달 불가,
  가드, 중첩 열). 기존 테스트 2개 갱신(아래 이슈 2·3).
- 2026-08-20: 문서 — `language.md` §3.6·제한사항 2행, `errors.md` 소진성 절,
  `docs/ai/rl.md`, `CHANGELOG.md`, `match-analysis.md` §5,
  `type-inference-gaps.md` §4.3(해제 표시).

## 이슈 및 해결

### 이슈 1: 모두 가드인 match의 문안이 `missing "_"`로 퇴화

- **증상**: `fully_guarded_match_is_not_exhaustive`가 실패. 기대는
  `missing "Some", "None"`인데 `missing "_"`가 나왔다.
- **원인**: 커버하는 행이 하나도 없으면 그 열에 **쓰인 생성자도 없다**. witness
  머리를 고를 때 "쓰인 것이 없으면 `_`"로 두었기 때문에, 열이 알려진 enum인데도
  이름을 대지 않았다.
- **해결**: `missing_heads`에서 `ColTy::Enum`은 **항상** 빠진 생성자를 나열하도록
  했다(쓰인 것이 없으면 전부가 빠진 것이다). `_`는 알파벳을 모르는 `Opaque`
  열에만 남는다 — 튜플의 보편 위치가 그 경우다.

### 이슈 2: 기존 테스트가 v1 한계를 계약으로 고정하고 있었음

- **증상**: `nested_pattern_arm_covers_nothing_for_exhaustiveness`가 실패.
  기대 문안이 `missing "Ok"`인데 `missing "Ok(value: None)"`이 나왔다.
- **원인**: 그 테스트는 이번 태스크가 **없애려는 동작**을 고정한 것이다.
- **해결**: `a_nested_pattern_covers_exactly_what_it_matches`로 이름과 기대를
  바꿔 새 계약(안쪽까지 검사, witness는 패턴)을 고정했다.

### 이슈 3: 유닛 케이스만 있는 enum으로 쓴 테스트가 조용히 빗나감

- **증상**: 3단 중첩 테스트가 `missing "A1(b: B1(c: C2))"` 대신
  `missing "A1(b: B1)"`을 냈다.
- **원인**: `enum C { C1, C2 }`는 괄호 있는 케이스도 제네릭도 없으므로 **TS
  enum**이고 선언 표에 없다(`parser/enums.rs`). 그래서 `c` 열이 `Opaque`가 되고
  witness가 `_`(렌더 시 생략)로 나온 것 — 알고리즘은 정확히 옳게 답했다.
- **해결**: 테스트의 `C`를 페이로드가 있는 rl enum으로 고쳤다. (TASK-102에서도
  같은 함정을 밟았다. TS enum을 스크루티니로 쓴 match를 rl 진단으로 바꾸는
  TASK-100이 이 혼동의 근본 대응이다.)

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` — 11개 테스트 바이너리 전부 통과 (compile 237, lib 45)

## 결과

- 신규: `src/analysis/usefulness.rs`(알고리즘). 이동: `analysis.rs` →
  `analysis/mod.rs`. 삭제: `Table::resolve_coverage`(규칙이 알고리즘으로 흡수).
- 실측 효과:
  - 중첩으로 안쪽을 다 덮은 match가 통과한다(이전에는 `missing "Ok"`로 거절).
  - 제네릭 페이로드(`Result<Option<T>>`)도 통과한다 — 열 타입을 쓰인 패턴으로
    정하는 폴백 덕분.
  - 빠진 값은 `missing "Wrap(inner: No)"`처럼 **붙여넣을 수 있는 패턴**이다.
  - 튜플·or-패턴·가드·보편 위치의 기존 답은 그대로다(기존 테스트 무수정 통과).
- 후속: 도달 불가 arm의 보고 채널(P3의 에디터 힌트), 타입이 필요한 나머지
  (제네릭 인스턴스화·손으로 쓴 유니언)는 P4.
