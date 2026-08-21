# TASK-111: 튜플 match의 typed 소진성

- **상태**: 완료
- **시작일**: 2026-08-20
- **완료일**: 2026-08-20
- **커밋**: `0c23c49`

## 목적

[TASK-101](./TASK-101-rust-parity-review.md)의 GAP-6이 적어 둔 잔여 항목 하나를
없앤다: **튜플 match에는 typed 소진성 프로브가 없었다.** `probe.rs::walk`는
`Segment::Match`에서만 질문을 만들고 `Segment::TupleMatch`에서는 만들지 않았다.
그래서 튜플 match의 소진성은 언제나 **선언 표**로만 판정됐고,

- 좁혀진 타입(`if (d.kind === "South") return 0;` 뒤의 `d`),
- 손으로 쓴 TS 유니언 스크루티니,
- import된 enum의 인스턴스화

같은 "타입만 아는" 사정을 typed 경로(`--check-types`)에서도 전혀 반영하지
못했다. 단일 match는 TASK-108·109에서 이미 체커를 오라클로 쓰고 있었으므로,
같은 프로그램 안에서 단일 match는 정확하고 튜플 match는 부정확한 상태였다.

## 범위

- 포함: 튜플 match의 typed 프로브(위치별 질의), typed 경로의 곱집합 소진성,
  튜플 witness 메시지, 계약 테스트, 문서·CHANGELOG.
- 제외: 배치(untyped) 경로 — 지금처럼 선언 표로 판정한다(타입이 없으므로 다른
  답이 없다). 튜플 원소 자리의 자동완성(GAP-5의 남은 항목)도 제외 — 완성은
  `engine/completions.rs`의 자리 판정 문제이고 소진성과 별개다.

## 의사결정

### 결정 1: 한 match에 질문 하나가 아니라 **위치마다** 하나

- **상황**: 체커에게 물을 수 있는 것은 "이 위치의 타입 구성원"이다. 튜플
  match는 스크루티니가 N개다.
- **검토한 대안**:
  - (a) 스크루티니 span 전체를 한 번 묻는다 — 콤마로 나뉜 여러 식이라 "그
    위치의 타입"이라는 질문이 성립하지 않는다.
  - (b) 방출된 임시(`$rl_m0`, `$rl_m1`, …)를 위치마다 하나씩 묻는다.
- **선택과 근거**: (b). 방출 코드가 이미 위치마다 임시를 하나씩 만든다. 그
  이름 자리는 **그 위치의 타입이 그대로 드러나는 유일한 자리**이고, 단일
  match가 쓰는 것과 똑같은 질의(`TagQuery`)다. 새 질문 종류가 필요 없다.
- **구현**: `codegen/matches.rs::emit_tuple_match`가 임시 이름마다
  `push_mark(expr.keyword_off)`를 남긴다(길이 0 — 방출 바이트 불변). 위치는
  마크가 **방출된 순서**로 구분된다: 셋 다 같은 `match` 키워드를 가리키므로
  출처만으로는 구분되지 않지만, 임시는 좌→우 순서로 방출되기 때문이다.
  `engine/projection.rs`가 `t.src == probe.offset`인 마크를 모아 개수가
  `probe.arity`와 다르면 그 match를 통째로 건너뛴다 — 답을 위치에 잘못
  붙이느니 답하지 않는 편이 낫다.

### 결정 2: 어떤 암도 태그를 쓰지 않은 위치는 `Unconstrained`로 둔다

- **상황**: `match (a, b) { (North, _) => …, (South, _) => … }`에서 두 번째
  위치는 아무 암도 태그를 쓰지 않는다. 체커는 그 위치의 구성원을 성실히
  답해 주므로, 그대로 열로 쓰면 곱집합이 `(South, Slow)`, `(South, Fast)`,
  … 로 폭발한다.
- **검토한 대안**:
  - (a) 답을 그대로 열로 쓴다 — 사실이지만 읽을 수 없고, 사용자가 쓰지도 않은
    구분을 강요한다.
  - (b) 그 위치를 `ColTy::Unconstrained`로 둔다 — witness가 `_`로 렌더되어
    `(South, _)` 하나가 된다.
- **선택과 근거**: (b). 기본(untyped) 경로가 이미 그렇게 판정한다(선언 표에서
  식별되지 않은 열은 열거하지 않는다). 두 경로가 같은 문안을 내는 것이
  소진성 답의 단일 원천이라는 TASK-097의 계약이다. `tuple_position_tags`가
  "어떤 암이든 태그를 쓴 위치"를 세고, 빈 위치만 `Unconstrained`가 된다.

### 결정 3: witness는 튜플이면 따옴표 없이 `(a, b)`

- **상황**: typed 경로의 기존 문안은 `missing "North"`처럼 태그를 따옴표로
  감쌌다.
- **검토한 대안**: 조합도 따옴표로(`"(North, Slow)"`) / 조합만 따옴표 없이.
- **선택과 근거**: 후자. TASK-110에서 확인한 대로 이 문자열은 **그대로 암으로
  붙여 넣을 수 있어야** 한다. 튜플 암은 `(North, Slow) => …`이고 따옴표는
  패턴의 일부처럼 읽힌다. 기본 경로의 튜플 문안도 따옴표를 쓰지 않는다.
  네 개를 넘으면 기본 경로와 같은 방식·같은 문구로 줄인다
  (`…, … (N combinations in total)`).

### 결정 4: 소진성 계산은 `usefulness`를 **그대로** 쓴다

- **상황**: 튜플 소진성만 따로 계산할 수도 있다.
- **선택과 근거**: 쓰지 않는다. TASK-103의 `usefulness::missing`은 처음부터
  다열(多列) 알고리즘이다 — 단일 match는 열이 하나인 특수 경우다. 튜플은
  `types`를 N개로 만들어 넘기기만 하면 된다. 도달 불가 암(`unreachable_arms`)도
  같은 함수로 함께 얻는다.

## 작업 내역

1. `src/probe.rs` — `TagMatch::arity` 필드 추가(단일은 `1`),
   `collect_tuple(expr, out)` 신설. 와일드카드 암이 하나라도 있으면(모든 조합을
   커버하므로) 건너뛰고, 어느 원소에도 태그 패턴이 없으면(열거할 것이 없으므로)
   건너뛴다. `covered`는 빈 채로 둔다 — 무엇이 커버됐는지는 **조합**에 대한
   질문이고 암에서 계산되므로 체커에게 물을 것이 아니다.
   `walk`의 `Segment::TupleMatch` 갈래에서 호출.
2. `src/codegen/matches.rs::emit_tuple_match` — 임시 이름마다 마크.
3. `src/engine/projection.rs` — `TagQuery`를 위치마다 하나씩, 임시의 방출
   순서대로. 개수가 `arity`와 다르면 그 match는 건너뛴다.
4. `src/engine/semantics.rs` — 답을 match별로 묶는다
   (`type MatchAlphabets = (usize, Vec<Vec<String>>)`). 튜플 witness 렌더링과
   4개 초과 절단.
5. `src/analysis/mod.rs` — `collect_matches`가 튜플 match도 모은다(별도
   `Vec`으로: 타입이 다르다). `checked_coverage`에 튜플 루프 추가,
   `tuple_position_tags` 신설.
6. `tests/native.rs` — `typed_exhaustiveness_covers_tuple_matches_too`,
   `a_tuple_position_the_checker_narrowed_is_not_demanded_back`.

확인:

```sh
RLC_TSGO_ROOT=/home/user/typescript-go cargo test --test native   # 30 passed
RLC_TSGO_ROOT=/home/user/typescript-go cargo test                 # 11개 바이너리 전부
```

손으로도 확인했다: 두 enum의 튜플 match에서 한 조합을 빼면
`missing (North, Slow)`가 나오고, 앞에 `if (d.kind === "South") return 0;`을
두어 첫 위치를 좁히면 조용해진다(그 조합이 더는 존재하지 않으므로).

## 이슈 및 해결

- **증상**: 처음에는 마크를 `match` 키워드 하나에만 남겼더니 위치를 구분할 수
  없었다 — `scrutinee_position`이 첫 임시만 찾아 주고 나머지 위치는 사라졌다.
  **원인**: 마크의 출처가 전부 같은 `match` 키워드다. **해결**: 마크를 임시마다
  남기고, 소비 쪽에서 `src`가 같은 마크들을 **방출 순서**로 위치에 대응시킨다.
  개수가 `arity`와 다르면 대응이 깨진 것이므로 답하지 않는다(결정 1).
- **증상**: 두 번째 위치가 폭발해 `(South, Slow)`, `(South, Fast)`가 함께
  보고됐다. **원인**: 체커는 아무도 묻지 않은 열의 구성원도 성실히 답한다.
  **해결**: 결정 2.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` (tsgo 있음 — native 30 포함)

## 결과

- `--check-types`에서 튜플 match의 소진성이 **타입 기준**으로 판정된다.
- 단일·튜플이 같은 알고리즘, 같은 오라클, 같은 문안을 쓴다.
- GAP-6의 남은 항목: 중첩 패턴 내부 소진성 v2, let-else·`if let`의 or-패턴,
  미청구 `result` 바인딩 진단.
