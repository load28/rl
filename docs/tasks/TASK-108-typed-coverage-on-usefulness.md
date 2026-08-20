# TASK-108: typed 소진성도 usefulness 위로 (P4 계층 2, 1/2)

- **상태**: 진행 중
- **시작일**: 2026-08-20
- **완료일**: —
- **커밋**: `9fc0b07`

## 목적

tsgo를 소스에서 빌드해 typed 경로를 실제로 돌려 보니, [TASK-103](./TASK-103-usefulness-exhaustiveness.md)이
untyped 경로에 넣은 **중첩 소진성이 typed 경로에는 없다**. 두 경로가 같은 입력에
다른 답을 한다:

```rl
enum Inner { Yes(n: number), No }
enum Outer { Wrap(inner: Inner), Bare }
export const a = match (o) { Wrap(inner: Yes(n)) => n, Bare => -1 };
```

```
rlc --check       → src/nest.rl:4:18: ... missing "Wrap(inner: No)"
rlc --check-types → (침묵)
```

원인은 구조적이다: typed 경로는 `defer_to_checker`로 rl의 coverage를 통째로
건너뛰고, 체커에게는 **최상위 태그 집합**만 묻는다(`TagQuery`). 중첩 열은
아무도 보지 않는다.

## 범위

- 포함: 체커가 스크루티니 타입의 **허용 태그 전체**를 답하게 하고(`tagMembers`),
  typed 경로가 그 집합을 최상위 열로 삼아 **usefulness를 돌리게** 한다.
  한 알고리즘, 더 나은 오라클.
- 제외:
  - 중첩 **열**의 타입 질의 — 중첩 열은 여전히 선언 표로 해석한다. 해석되지
    않으면 typed 경로는 **침묵한다**(untyped처럼 추측하지 않는다).
  - 리터럴 match — 그 경로는 그대로다.

## 의사결정

### 결정 1: 체커를 "한 열의 오라클"로 쓰고 계산은 rl이 한다

- **상황**: typed 경로가 중첩을 못 보는 것을 고치는 방법.
- **검토한 대안**:
  - (a) 체커에게 중첩까지 포함한 소진성을 묻는다 — TypeScript에 그런 질문이
    없다. `switch`의 소진성은 타입 시스템의 개념이 아니다.
  - (b) typed 경로에서도 rl의 선언 표로 최상위 열을 해석한다 — 좁혀진 타입을
    잃는다(가드가 제거한 케이스를 다시 요구하게 된다). 이 경로의 존재 이유를
    버리는 셈이다.
  - (c) 체커가 **스크루티니 타입의 구성원 목록**을 답하고, 그 알파벳 위에서
    rl의 usefulness를 돌린다.
- **선택과 근거**: (c). 각자 아는 것만 답한다 — 타입의 구성원은 체커가, "그
  구성원들을 arm들이 다 덮었는가"는 rl이. 결과적으로 소진성 알고리즘이
  **하나**가 되고(기존에는 typed 경로가 `missing = 구성원 - 커버된 태그`라는
  두 번째 규칙을 갖고 있었다), 좁혀진 타입도 그대로 유지된다.

### 결정 2: 확신하지 못하는 witness는 typed 경로에서 보고하지 않는다

- **상황**: 중첩 열의 알파벳을 rl이 알아내지 못하면(페이로드 타입이 손으로 쓴
  유니언 등) usefulness는 그 자리를 `_`로 두고 witness를 만든다. 기본 경로는
  그것을 보고한다(타입이 없으니 보수적으로 구는 것 외에 선택지가 없다).
- **검토한 대안**:
  - (a) typed 경로도 같이 보고 — 소진된 프로그램을 거절할 수 있다. **더 나은
    경로가 더 나쁜 경로의 오탐을 물려받는** 셈이다.
  - (b) 보고하지 않는다 — 놓치는 것이 생기지만, 그 자리는 체커에게 물으면 알 수
    있고 그 질문은 다음 태스크의 것이다.
- **선택과 근거**: (b). 그래서 `Coverage`의 witness가 `certain` 플래그를 갖게
  됐다(`Uncovered`). 알파벳을 못 알아낸 열에서 나온 `_`와, 아무도 아무것도 쓰지
  않은 튜플 보편 위치의 `_`를 구분해야 해서 `ColTy::Opaque`를
  `Unconstrained`/`Unknown` 둘로 쪼갰다.

### 결정 3: typed 경로의 subject에는 이름이 없다

- **상황**: 체커는 *타입*을 답하지 선언을 답하지 않는다. 메시지에 enum 이름을
  넣을 수 없다.
- **선택과 근거**: 기존 문안 유지(`match is not exhaustive: missing ...`).
  이미 `cli.md`가 규범으로 적고 있던 차이이고, 이름을 지어내는 것보다 낫다.

## 작업 내역

- 2026-08-20: tsgo를 소스에서 빌드해 typed 경로를 실측할 수 있게 했다
  (`go build ./cmd/tsgo` + `npx tsc -b _packages/native-preview`,
  `RLC_TSGO_ROOT`로 주입). 이 환경에서 처음으로 `tests/native.rs` 23개와
  에디터 78개가 전부 실행됐다.
- 2026-08-20: `host.mjs` — `tagMembers: [{ index, tags }]` 추가(구성원 전체).
  `backend.rs`에 `TagMembers`, `native.rs`에 파싱.
- 2026-08-20: `analysis/usefulness.rs` — `ColTy::Opaque` → `Unconstrained` /
  `Unknown`, `Witness::Unknown`과 `Witness::certain()`.
- 2026-08-20: `analysis/mod.rs` — `Uncovered { pattern, certain }`,
  `match_rows`(행 조립을 분리), `checked_coverage(source, members)`,
  `Table::entry_of_members`, `collect_matches`.
- 2026-08-20: `engine/semantics.rs` — typed 경로가 `tag_members`로
  `checked_coverage`를 돌리고 `certain`한 witness만 보고.
- 2026-08-20: 테스트 — `tests/native.rs` +3(중첩 구멍, 좁혀진 타입 유지,
  불확실한 witness는 침묵). 기존 `native.rs` 1개는 TASK-104가 바꾼 계약을
  고정하고 있어 새 계약으로 갱신(아래 이슈 1).
- 2026-08-20: 문서 — `language.md` §3.9, `cli.md`, `rust-parity-analysis.md`
  §10.3 상태, `CHANGELOG.md`.

## 이슈 및 해결

### 이슈 1: tsgo를 켜자 TASK-104의 계약을 고정한 테스트가 실패

- **증상**: `a_diagnostic_on_generated_code_still_names_the_construct_it_came_from`이
  실패. 기대는 `(in code rlc generated for this construct)`인데 실제 출력은
  `match on a tag pattern needs a value with a `kind` discriminant ...`였다.
- **원인**: TASK-104가 바꾼 동작 그대로다. 그 태스크는 tsgo가 없어 e2e를 돌릴
  수 없었고, 이 테스트는 `require_tsgo!()`로 조용히 skip되고 있었다.
- **해결**: 테스트를 새 계약으로 다시 썼다(`a_diagnostic_on_generated_code_is_restated_in_rls_words`)
  — 위치는 `match` 키워드, 문안은 rl의 것, 원문이 괄호 안에 동봉. **TASK-104가
  실제 체커에서 동작한다는 것이 이로써 확인됐다.**

### 이슈 2: 에디터 테스트 4개가 실패하고 있었음 (TASK-107에서 발견·수정)

- 같은 원인 계열이다: `rlc`만 확인하고 tsgo는 확인하지 않는 skip 가드. 이제
  tsgo가 있으므로 **78개 전부 실행되고 전부 통과한다**(skip 0).

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` (RLC_TSGO_ROOT) — 11개 바이너리 전부 통과, native 26개 포함
- [x] `npm test` (editors/vscode, RLC_TSGO_ROOT + PATH) — **78/78, skip 0**
- [x] 수동 실측: 중첩 구멍(typed/untyped 같은 답), 좁혀진 타입(typed만 침묵),
      손으로 쓴 유니언(둘 다 침묵)

## 결과

- 두 경로가 같은 알고리즘을 쓴다. typed 경로는 그 위에 **체커의 구성원 목록**을
  얹는다.
- `Coverage`의 witness가 `certain`을 갖는다 — 계산이 확신한 답과 추측을
  구분하는 것은 이 프로젝트가 계속 지켜 온 "모르면 답하지 않는다"의 자료 구조
  버전이다.
- 후속: 중첩 **열**에 대한 같은 질문(`$rl_m.value` 자리의 구성원 목록) — 그것이
  들어가면 `certain` 필터가 걸러내는 자리도 답할 수 있다. 그 밖에
  `Discriminant`/`Property`/`Display`(TASK-101 §10.3)와 튜플 match의 typed
  프로브(§GAP-6)가 남는다.
