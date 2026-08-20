# TASK-098: 폴백 타입의 한계 실측 — nested·tuple 패턴과 제네릭 인스턴스화

- **상태**: 완료
- **시작일**: 2026-08-20
- **완료일**: 2026-08-20
- **커밋**: 2df3534

## 목적

TASK-096의 hover는 3단이다: ① TS 서비스 → ② or-pattern 대안 격리 프로브
→ ③ 선언 테이블 폴백. ③은 **선언 텍스트**를 그대로 보여주므로 제네릭
enum에서 `T`가 노출되고, ②는 `alternatives > 1`인 자리에만 적용된다.
어떤 위치가 실제로 부정확하거나 침묵하는지 **목록으로 확정**하고, 프로브
적용 범위를 넓힐지(그리고 넓힌다면 어디까지인지) 판단한다.

이것은 구현 태스크가 아니라 **조사·판단 태스크**다. 결론이 "넓힌다"이면
구현 범위를 이 문서에 확정하고 별도 태스크로 등록한다.

## 범위

- 포함:
  - 소스 수준으로 hover 답이 결정되는 경로를 위치 종류별로 분해하고
    (단일 대안 / or-pattern / nested leaf / 튜플 위치 / 내장 제네릭),
    각 위치가 ①·②·③ 중 어디서 답을 받는지 표로 확정.
  - 확정된 격차마다 증상과 비용을 적고, 프로브 확대의 이득/위험 평가.
  - 판단: 확대할 자리 / 유지할 자리 / 애초에 답하지 않을 자리.
- 제외:
  - 프로브 확대 구현 자체 (판단 결과에 따라 별도 태스크).
  - RL 자체 타입 추론(제네릭 치환)을 만드는 방향 — 계약상 비목표.

## 조사 결과 — 위치별로 누가 답하는가

hover는 `src/engine/language.rs:243`에서 ① `service_hover`(TS) → ②
`match_binding_hover` 안의 대안 격리 프로브 → ③ 선언 테이블 순으로 답한다.
②가 걸리는 조건은 `binding.alternatives > 1`(`language.rs:329`) 하나다.
어느 위치가 매핑되는지는 codegen이 결정하므로 `src/codegen/matches.rs`를
기준으로 표를 확정했다.

| 위치 | 방출 형태 | 매핑 | 답하는 단계 | 정확도 |
|------|-----------|------|-------------|--------|
| 단일 대안 패턴 binding `A(x)` | `const { x } = $rl_m;` (`binding_list`) | O | ① TS | 인스턴스화·narrowing 포함 |
| 중첩 패턴 leaf `Ok(value: Some(v))` | `const { v } = $rl_m.value;` + `.kind` 조건 체인 (`collect_conds_binds`) | O | ① TS | 인스턴스화 포함 (조건 체인이 narrowing까지 준다) |
| 튜플 단일 대안 원소 `(North(deg), …)` | `const { deg } = $rl_m0;` | O | ① TS | 인스턴스화 포함 |
| or-pattern binding `A(x) \| B(x)` | `const { x } = $rl_m;` (`binding_list_lit` — 의도적 무매핑) | X | ② 프로브 → ③ 폴백 | 프로브가 답하면 인스턴스화 포함 |
| 튜플 or-원소 `(_, Fast(kmh) \| Slow(kmh))` | 같음 (`bind_rope_from(.., false)`) | X | ② 프로브 → ③ 폴백 | 같음 |
| arm body의 binding 참조 | verbatim | O | ① TS (없으면 ③ 병합 타입) | 병합 union |

확인 방법: 매핑 여부는 `tests/emit_map.rs`의
`pattern_bindings_are_mapped_to_their_destructurings`(중첩 포함),
`or_pattern_bindings_are_left_unmapped`, 이번에 추가한
`tuple_element_bindings_follow_the_same_rule`이 고정한다.

### 격차 목록 (실제로 남은 것)

1. **폴백 ③의 제네릭**: 선언 텍스트를 그대로 보여주므로 `Option<number>`의
   `Some(value)`는 `T`로 보인다. 사용자 제네릭 enum도 같다.
2. **폴백 ③의 narrowing 부재**: scrutinee가 이미 좁혀진 경우를 모른다.
3. **subject 미해석**: 손으로 쓴 유니언 등은 아무 답도 하지 않는다(의도).

세 가지 모두 **③에서만** 발생한다. ③에 도달하는 조건은 (a) 툴체인 부재,
(b) 서비스 세션 사망, (c) 프로브 방출 실패다.

## 의사결정

### 결정 1: 프로브 적용 범위를 넓히지 않는다 (넓힐 자리가 없다)

- **상황**: 착수 시 가설은 "프로브가 or-pattern에만 걸려 있으니 중첩/튜플
  위치가 폴백으로 새고 있을 것"이었다.
- **검토한 대안**: (A) 중첩 leaf·튜플 위치까지 프로브 확대 / (B) 유지.
- **선택과 근거**: (B). 표가 보여주듯 **매핑이 없는 위치는 or-pattern
  binding(단일 match·튜플 원소) 둘뿐**이고, 그 둘은 이미 프로브가 덮는다.
  중첩 leaf는 `collect_conds_binds`가 매핑된 구조 분해를 조건 체인 안쪽에
  방출하므로 ①이 인스턴스화·narrowing까지 정확히 답한다 — 프로브를 붙일
  이유가 없다(같은 답을 더 비싸게 얻는다). 게다가 중첩 패턴은 sema가
  or-pattern과의 결합을 거부하므로("nested patterns cannot be combined
  with or-patterns") 무매핑 중첩 leaf라는 조합 자체가 존재하지 않는다.

### 결정 2: 폴백의 제네릭 인스턴스화는 구현하지 않는다

- **상황**: 격차 1·2를 없애려면 폴백이 타입 인자를 치환하고 narrowing을
  흉내 내야 한다.
- **검토한 대안**: (A) 선언의 타입 파라미터를 scrutinee의 선언 타입 인자로
  치환하는 최소 구현 / (B) 유지 + 폴백임을 계속 명시.
- **선택과 근거**: (B). (A)는 rl 안에 작은 타입 치환기를 만드는 일이고,
  그것이 곧 "rlc는 TypeScript 타입 시스템을 흉내 내지 않는다"는 계약이
  금지하는 방향이다. 게다가 ③은 정의상 **체커에 물을 수 없는 상황**이라
  치환의 입력(실제 인스턴스화된 scrutinee 타입)도 신뢰할 수 없다. 지금
  폴백은 `Pattern binding of \`Option.Some\` (declared type).`라고 출처를
  밝히므로(`language.rs:1062`), 사용자는 이것이 선언 타입임을 안다 —
  틀린 답을 확신에 차서 말하는 것보다 낫다.

### 결정 3: 후속 태스크를 만들지 않는다

- **상황**: TASK-096이 "nested/tuple 패턴 폴백 타입의 제네릭 인스턴스화"를
  후속 후보로 남겼으므로, 이 조사의 산출물은 태스크 등록 여부다.
- **선택과 근거**: 등록하지 않는다. 결정 1·2에 따라 남은 격차는 전부
  "체커가 없을 때의 정직한 근사"이고, 이는 설계가 선택한 한계다
  (`match-analysis.md` §6). 대신 그 한계를 문서에 명시적으로 옮겨 적어
  다음 사람이 같은 조사를 반복하지 않게 한다.

## 작업 내역

- 2026-08-20: 착수. hover 3단 경로와 분기 조건 확인
  (`src/engine/language.rs`의 `hover`/`match_binding_hover`/
  `isolated_alternative_hover`/`declared_binding_hover`).
- 2026-08-20: 매핑 여부를 codegen에서 확정 — `binding_list`(매핑) vs
  `binding_list_lit`(무매핑), `collect_conds_binds`(중첩: 매핑),
  튜플 경로의 `pattern_conds_binds` / `bind_rope_from(.., false)`.
- 2026-08-20: 표의 주장 중 테스트로 고정돼 있지 않던 칸(튜플 원소)을
  `tests/emit_map.rs::tuple_element_bindings_follow_the_same_rule`로 고정.
  `cargo test --test emit_map` → 12 passed.
- 2026-08-20: 결론(확대 없음)과 근거를 이 문서에 기록하고,
  `docs/design/match-analysis.md` §6에 한계를 옮겨 적음.

## 이슈 및 해결

### 이슈 1: tsgo가 없는 환경이라 e2e 실측은 불가

- **증상**: 이 세션에는 `tsgo`도 `RLC_TSGO_ROOT`도 없어(`which tsgo` →
  없음) ①·②가 실제로 어떤 문자열을 답하는지 실행으로 확인할 수 없다.
- **원인**: 환경 제약. CI의 `native` 잡은 typescript-go를 빌드해 이 경로를
  돌린다(`.github/workflows/ci.yml`).
- **해결**: 조사의 **판정 근거를 매핑 여부**로 옮겼다 — 어느 단계가 답하는지는
  emit-map이 결정하고, 그것은 이 환경에서 테스트로 확인할 수 있다. 실제
  문자열까지 확인하는 e2e는 TASK-096이 남긴 별개의 후속(engine.test.ts
  계층)이며 이 태스크의 결론을 바꾸지 않는다 — 결론이 "확대 없음"이므로
  검증 대상이 늘지 않는다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`

## 결과

변경 파일: `tests/emit_map.rs`(튜플 원소 매핑 규칙 고정),
`docs/design/match-analysis.md`(§6 한계), 본 태스크 문서.

판단: **프로브 확대 없음, 폴백 인스턴스화 없음, 후속 태스크 없음.** 근거는
매핑 표다 — checker가 도달하지 못하는 자리는 or-pattern binding 둘뿐이고
이미 프로브가 덮는다. 남은 부정확함은 "체커에 물을 수 없을 때의 정직한
선언 타입"이며, 설계가 선택한 한계로 문서에 남겼다.

남은 별개 후속(이 태스크가 만든 것 아님): TASK-096이 적어 둔 tsgo 통합
환경에서의 hover 프로브 e2e 테스트 — 이번 판단으로 범위가 늘지는 않는다.
