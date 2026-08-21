# TASK-129: Table 구축을 resolver 위로 (Phase 3 2/2)

- **상태**: 완료
- **시작일**: 2026-08-21
- **완료일**: 2026-08-21
- **커밋**: —

## 목적

[TASK-123](./TASK-123-resolver-owns-names.md)이 subject 식별·미해결
보고·제안을 resolver로 단일화한 뒤에도, `analysis::Table`의 **구축**
(로컬 선언 수집, extern 변환, builtin 목록, shadowing 순서)은 resolve의
collect와 같은 규칙의 두 번째 구현으로 병존하며 동등성 테스트로만
고정되어 있었다. 이 잔여(설계 문서의 "Phase 3 2/2")를 끝낸다: 선언
가시성 규칙의 구현을 `crate::resolve` 하나로 만들고, analysis에는 그
결과의 **뷰**(coverage/typed model이 쓰는 이름·origin·생성자 목록)만
남긴다.

## 범위

- 포함: `Table::from_resolution`(Resolution → Table 변환) 신설,
  `Table::build`/`build_from_tags`/`assemble`과 모듈 수준
  `collect_local_enums`/`builtin_enums`의 삭제, `pattern_analyses`/
  `coverage_analyses`/`checked_coverage`가 공유하는 `analyses_over`
  (lower → resolve → from_resolution → analyze → attach_resolution)
  도입, `attach_resolution`이 자체 lowering을 중복하지 않도록 시그니처
  변경, 동등성 테스트의 역할 재기술, 설계 문서 정산.
- 제외: usefulness 내부의 태그 문자열 비교를 `VariantRef` 비교로 바꾸는
  identity 표현 정리(의미론적 결함이 아님 — TASK-123 정산; compiler-core
  "남은 후속"에 유지).

## 의사결정

### 결정 1: Table은 남기되 "뷰"로 강등한다

- **상황**: coverage(`usefulness.rs`)와 typed model(바인딩 타입 읽기)은
  이름·태그·선언 필드 텍스트를 소비한다. Table을 통째로 없애고 Resolution을
  직접 소비시킬 수도 있었다.
- **대안**: ① usefulness/typed model이 `Resolution`을 직접 읽는다 —
  usefulness의 alphabet 조립이 resolve의 내부 표현(DefId/VariantRef)에
  결합되고, checker가 이름 없는 타입으로 답하는 typed 경로
  (`entry_of_members` — 선언이 아예 없는 subject)가 Resolution에 억지로
  편입된다. ② Table 구조는 유지하고 구축만 `from_resolution`으로 바꾼다 —
  규칙(가시성·shadowing)은 resolve 한 곳, 소비 형태는 기존 그대로.
- **선택과 근거**: ②. D5의 결함은 "규칙이 두 번 구현된 것"이지 "뷰 자료구조가
  있는 것"이 아니다. `from_resolution`은 규칙 없는 순수 변환(이름 승자
  필터 + 타입 매핑)이라 표류할 규칙 자체가 없다.

### 결정 2: 승자 필터는 `type_ns` 기준

- **상황**: Resolution의 `defs`에는 shadowed 선언(같은 이름의 이전 로컬,
  로컬에 가려진 import, 가려진 builtin)도 들어 있다. Table은 "각 이름당
  하나, 가까운 origin 승"이 계약이다.
- **선택과 근거**: `resolution.type_ns.get(&def.name) == Some(&id)`인
  def만 entry로 만든다 — 이름 경쟁의 승자 판정은 resolve의 스코프 규칙이
  이미 내린 결정이고, 여기서는 그 결정을 **읽기만** 한다. 순서는 `defs`
  arena의 삽입 순서(로컬 소스 순 → import → builtin)라 기존 Table의
  shadowing 순서 계약과 일치한다.

### 결정 3: 동등성 테스트는 삭제하지 않고 재기술한다

- **상황**: `tests/resolve.rs::resolution_matches_the_analysis_answer`는
  "두 구현이 표류하지 않는가"의 감시였다. 구현이 하나가 되면 존재 이유가
  사라진 것처럼 보인다.
- **선택과 근거**: 유지하되 주석을 바꾼다 — 이제 이 테스트는 resolver의
  보고가 analysis 표면(`pattern_analyses().unresolved`)으로 스팬·문안
  그대로 전달되는 **변환 충실도**를 고정한다. 삭제하면 conversion 계층의
  회귀(스팬 어긋남, 제안 누락)를 잡을 테스트가 없다.

## 작업 내역

- 2026-08-21: `src/analysis/mod.rs` —
  - `pattern_analyses`/`coverage_analyses`가 공유하는
    `analyses_over(program, externs, depth)` 신설: `hir::lower_program` →
    `resolve::resolve_file` → `Table::from_resolution` → `analyze` →
    `attach_resolution` 한 경로. `attach_resolution`은 lowering/해석을
    반복하지 않고 `(analyses, hir, resolution)`을 받는 시그니처로 변경.
  - `Table::from_resolution` 추가(결정 1·2), `Table::build`/
    `build_from_tags`/`assemble`·`collect_local_enums`·`builtin_enums`
    삭제. `checked_coverage`도 lower+resolve+from_resolution 경로로.
  - 삭제 과정에서 impl 블록의 `resolve`/`candidates`(usefulness와 typed
    model의 소비 지점)까지 잘려 나간 것을 복구(이슈 1).
  - 순 235줄 변경(+67/−168): 선언 수집·builtin 목록의 두 번째 구현 소멸.
- `tests/resolve.rs` — 동등성 테스트 주석 재기술(결정 3).
- `docs/design/compiler-core.md` — Phase 3 행을 완료로(TASK-123·129),
  "남은 후속"의 Phase 3 2/2 항목을 usefulness identity 표현 정리로 축소.
- 검증: `cargo fmt --check` / `cargo clippy --all-targets -- -D warnings`
  / `cargo test` (13개 스위트 전부 실패 0).

## 이슈 및 해결

### 이슈 1: impl 블록 절제가 사용 중인 메서드까지 잘라 빌드 실패

- **증상**: `error[E0599]: no method named 'candidates'`
  (usefulness.rs:388), `no method named 'resolve'` (mod.rs 두 곳),
  그로 인한 `E0282` 타입 주석 요구 하나.
- **원인**: 구축 메서드(`build`~`assemble`) 삭제를 impl 블록 텍스트
  범위로 잘랐는데, 그 사이에 있던 소비 메서드 `resolve`/`candidates`가
  함께 삭제됐다.
- **해결**: 두 메서드를 원문 그대로 새 impl 블록(`from_resolution` 뒤)에
  복원. 둘 다 private — usefulness는 analysis의 자식 모듈이라 접근에
  문제 없다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`

## 결과

선언 가시성·shadowing·builtin 규칙의 구현이 `crate::resolve` 하나가
됐다. analysis에 남은 것은 규칙 없는 변환(`from_resolution`)과 소비
전용 뷰뿐이다 — D5 종결, Phase 3 완료.
