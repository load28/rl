# TASK-085: call-capability 검사의 callee를 symbol identity로 — typed 경로의 마지막 이름 근사 제거

- **상태**: 완료
- **시작일**: 2026-08-19
- **완료일**: 2026-08-19
- **커밋**: 241a6d3

## 목적

TASK-082 계획의 P3을 구현한다. typed 경로(`--check-types`/`--types`)에 남은
마지막 name-resolution 근사 — "어느 함수를 불렀는가"를 파일 전역 이름 표로
대응시키던 것 — 를 callee 심볼과 선언 심볼의 pairing으로 교체한다. 작은
코드에서는 이름 충돌이 드물지만 큰 프로젝트에서는 섀도잉·재선언·import와의
이름 겹침이 실제로 발생하고, 그때 이름 표는 검사를 포기하거나(다른 시그니처)
엉뚱한 선언의 시그니처를 적용한다(같은 이름의 import를 로컬 선언으로 오인).

TASK-083에서 이 작업을 보류한 사유였던 규범(`language.md` §10.5의 "같은 이름
다른 시그니처는 제외")은 이번에 함께 갱신한다 — 사용자가 큰 프로젝트에서의
개선 가치를 확인하고 진행을 승인했다.

## 범위

- 포함: probes의 callee·선언 수집 구조 변경, project/check의 pairing 배선,
  §10.5 규범 갱신, 섀도잉 회귀 테스트.
- 제외: cross-module callee(°import된 함수는 alias 심볼이 선언 심볼과
  일치하지 않아 검사하지 않음 — 종전과 동일한 same-file 범위), untyped
  경로(이름 표 유지 — 문서화된 근사), P4(`val_method_calls` 제거).

## 의사결정

### 결정 1: pairing 단위는 (callee 심볼 id, 선언 심볼 id)의 일치다

- **상황**: 종전에는 파일의 함수 선언을 이름으로 표에 넣고 호출의 callee
  이름으로 찾았다. probes 모드도 같은 표를 썼다.
- **검토한 대안**: ① 이름 표 유지, ② 선언 identifier와 호출의 callee
  identifier에 각각 SymbolQuery를 걸어 id로 짝짓기.
- **선택과 근거**: ②. 섀도잉·블록 스코프 재선언·동명 import가 전부
  TypeScript의 resolution으로 정리된다. 시그니처(어느 매개변수가 `val`인가)는
  `.rl` 원문에만 있는 rl의 사실이므로 계속 rl이 읽되, 표의 키만 이름 →
  심볼로 바뀐다. 같은 심볼이 서로 다른 시그니처의 선언 여럿을 가지면
  (TS 오버로드, `var` 병합) 그 callee는 검사하지 않는다 — 이름 표의
  ambiguity 규칙을 심볼 granularity로 옮긴 것.

### 결정 2: 수집 게이트는 "파일이 그 이름을 선언했는가"로 유지한다

- **상황**: 모든 호출의 모든 인자를 수집하면 질의가 호출 수에 비례해 는다.
- **선택과 근거**: verdict는 callee 심볼이 수집된 선언 심볼과 일치할 때만
  내려지므로, 같은 파일에 그 이름의 선언이 하나도 없는 호출은 어떤 선언과도
  일치할 수 없다 — 게이트는 순수 최적화이고 correctness에 기여하지 않는다
  (TASK-084의 mutator 정책 생략과 같은 원리). 단 종전과 달리 "이름이
  ambiguous해도" 수집한다 — ambiguity는 이제 심볼 쪽 개념이다.

### 결정 3: 인자의 매개변수 대응은 verdict로 미룬다

- **상황**: 종전 probes는 수집 시점에 매개변수를 찾아 `val` 여부·설명
  문자열까지 결정해 담았다.
- **선택과 근거**: 어느 선언이 불렸는지 모르는 채 매개변수를 고를 수 없으니,
  pass는 (인자 root 위치, callee 위치, 인자 index)만 나르고, 매개변수
  선택·`val` 검사·메시지의 매개변수 표기는 전부 verdict(`check.rs`)에서
  한다. 진단 메시지 형식은 종전과 동일하다.

## 작업 내역

- 2026-08-19: `src/val.rs` — `collect_signatures`를 선언 단위 수집
  (`collect_declarations` → `FnDecl{name, ident, params}`)과 이름 표 파생으로
  분리(untyped 경로는 표를 그대로 사용 — 동작 불변). 공개 타입 `ValFn`/
  `ValParam` 추가, `ValProbes.functions` 추가, `ValPass`를
  `{offset, name, callee, callee_at, arg_index}`로 재정의. probes 모드의
  호출 수집을 `probe_call`(무판정 수집)로 교체.
- 2026-08-19: `src/typescript/project.rs` — 선언 identifier마다 SymbolQuery
  (`FnAnchor{root, params}`), pass마다 인자 root + callee 두 SymbolQuery
  (`PassAnchor{root, callee_symbol, arg_index, ...}`).
- 2026-08-19: `src/typescript/check.rs` — 선언 심볼 id → 매개변수 목록 표
  (충돌 시 ambiguous=None), pass verdict를 "root ∈ val 심볼 && callee 심볼의
  선언 표 조회 && 해당 index 매개변수가 `val` 아님"으로.
- 2026-08-19: `docs/reference/language.md` §10.5 — 기본 경로(이름 대응,
  ambiguity 제외 규칙)와 typed 경로(심볼 pairing, 오버로드·불일치 제외)를
  구분해 규범화.
- 2026-08-19: 테스트 — `tests/compile.rs`
  `val_probes_carry_the_callee_and_the_declarations_it_may_name`(수집 계약),
  `tests/native.rs` `a_call_is_checked_against_the_declaration_it_resolves_to`
  (동명 함수 2개: 바깥 호출은 mutable 매개변수 선언으로 → 에러 1건, 블록
  안 호출은 `val` 매개변수 화살표로 → 통과. 이름 표는 둘 다 포기했던
  케이스).

## 이슈 및 해결

### 이슈 1: clippy `missing_docs`

- **증상**: `ValFn.params` 필드 문서 누락으로 `-D warnings` 실패.
- **해결**: 문서 추가. 그 외 없음.

## 동작 변화 (의도된 것)

typed 경로에서만, 같은 이름의 선언이 여럿인 경우에 한해 판정이 바뀐다:
종전에는 검사가 통째로 빠지거나 이름이 우연히 같은 선언의 시그니처가
적용됐고, 이제는 각 호출이 실제로 가리키는 선언으로 검사된다. 이름이 하나뿐인
코드(기존 테스트 전부)와 untyped 경로·방출 TypeScript는 바이트 단위로
불변이다.

## 검증 게이트

- `cargo fmt --check` — 통과
- `cargo clippy --all-targets -- -D warnings` — 통과
- `cargo test` — 449개 전부 통과 (native 24개 포함, 실 toolchain 구동)

## 변경 파일

- `src/val.rs`, `src/lib.rs` — 선언 단위 수집, `ValFn`/`ValParam`/`ValPass`
- `src/typescript/project.rs`, `src/typescript/check.rs` — 심볼 pairing
- `docs/reference/language.md` — §10.5 규범 갱신
- `tests/compile.rs`, `tests/native.rs` — 수집 계약·섀도잉 회귀 테스트
