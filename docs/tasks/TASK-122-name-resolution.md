# TASK-122: 선언 수집과 이름 해석 — Phase 2 (resolve)

- **상태**: 완료
- **시작일**: 2026-08-21
- **완료일**: 2026-08-21
- **커밋**: —

## 목적

컴파일러 중심부([TASK-119](./TASK-119-compiler-core-design.md), §5)의
resolve 단계: 한 파일이 보는 선언 세계(로컬 enum·import·builtin)를 안정
ID의 definition으로 수집하고, 패턴이 쓰는 모든 이름을 그 definition에
묶는다. 이 단계 이후 "이 태그가 그 케이스인가"는 문자열 비교가 아니라
**ID 비교**다 — 동명 이형 variant가 구분되고(D4 해소의 기반), rename/
references 합성(Phase 3+)의 전제가 선다.

## 범위

- 포함: `src/resolve/mod.rs` — `Definition`/`DefKind`(Enum + EnumValue,
  namespace당 하나)/`EnumDef`/`VariantDecl`/`FieldDecl`, 소유 기반 참조
  `VariantRef`/`FieldRef`, `Res`, `Namespace`(Type/Value) 분리, shadowing
  (local > import > builtin, 로컬 재선언은 later-wins), builtin
  `Option`/`Result`를 declaration identity로, subject
  identification과 사이트별 해석(`uses`/`unresolved`/`sites`), nested
  해석, 같은 domain 한정 suggestion, 단일 패턴 사이트의 1-edit 유일
  라이선스. `HirSourceMap::record_def`로 로컬 def 스팬 기록. 동등성
  테스트로 analysis와 고정.
- 제외: analysis/sema가 resolver를 소비하는 전환(Phase 3), local 바인딩
  스코프(`LocalId` — Phase 5의 flow와 함께), `Res::Ambiguous`의 실제 생산
  (현 식별 규칙은 유일 후보 또는 침묵).

## 의사결정

### 결정 1: variant/field 참조는 소유 기반 `VariantRef`/`FieldRef`

- **상황**: HIR의 `VariantId`/`FieldId`는 **파일 구문**의 arena ID인데,
  선언 세계에는 import·builtin처럼 구문이 없는 variant도 있다. 같은 ID
  타입을 두 arena에 쓰면 의미가 갈라진다.
- **검토한 대안**: (a) 선언 세계 전용 arena에 hir ID 타입 재사용 — 동일
  타입·다른 의미로 혼동 유발. (b) `(enum DefId, index)` 쌍의 소유 기반
  참조.
- **선택과 근거**: (b). 지시 설계의 `Variant { enum_def, variant }` /
  `Field { variant, field }` 구조와 동형이고, "variant의 identity는 소유
  enum에 묶인다"가 타입에 드러난다. 로컬 선언과의 다리는
  `VariantDecl::hir`(Option) 링크.

### 결정 2: subject identification 규칙은 analysis와 동일하게 두고 동등성 테스트로 고정

- **상황**: rlc는 scrutinee 타입을 모르므로 "어느 enum인가"는 태그 집합
  증거로 식별해야 한다(rustc에 없는 rl 고유 단계). 그 규칙의 구현이
  analysis(`Table::identify`)에 이미 있다 — 한 규칙 두 구현 금지 원칙과
  마이그레이션의 충돌.
- **검토한 대안**: (a) analysis의 Table을 즉시 resolve로 이동하고 전
  소비자 개조 — 한 커밋이 과대해짐(analysis 2,600줄의 소비자가 sema·
  engine·probe에 걸침). (b) resolve가 같은 규칙을 (HIR 위에서) 구현하고
  **동등성 테스트**(`resolution_matches_the_analysis_answer` — 스팬·이름·
  제안 완전 일치, 6개 시나리오)로 드리프트를 차단, Phase 3에서 analysis를
  resolver 소비로 전환하며 병존 해소.
- **선택과 근거**: (b). 편집 거리·제안 라이선스 함수(`nearest`/
  `nearest_within`)는 analysis 것을 `pub(crate)` 승격으로 **재사용**해
  중복을 최소화했다. 병존은 부채로 명시(아래 결과).

### 결정 3: enum은 type·value 두 definition을 만든다

- **상황**: rl enum은 방출에서 `type Shape = ...`와 `const Shape = {...}`
  둘이 된다.
- **선택과 근거**: 지시 설계대로 namespace당 def 하나(`DefKind::Enum` /
  `DefKind::EnumValue { enum_def }`). 이후 value 위치의 `Shape.Circle(...)`
  해석과 rename 합성이 이 구분을 소비한다.

## 작업 내역

- 2026-08-21: `src/resolve/mod.rs` 신설(위 구조 전부). `resolve_file`이
  `&mut HirFile`을 받아 로컬 def 스팬을 source map에 기록.
- `src/analysis/mod.rs`: `nearest`/`nearest_within`을 `pub(crate)`로 승격
  (동작 불변).
- `src/lib.rs`: `pub mod resolve` 등록.
- `tests/resolve.rs` 신설(10건): type/value 이중 def와 def 스팬, 동일
  spelling 상이 identity, local>import>builtin shadowing, import alias,
  같은 domain 한정 suggestion(이웃 enum의 동명 variant에 연결하지 않음),
  손 유니언 침묵, nested가 필드 선언 타입으로 해석, 단일 사이트 1-edit
  유일 라이선스, analysis 동등성(6 시나리오), 튜플 위치 독립 식별.

## 이슈 및 해결

### 이슈 1: 테스트 픽스처의 unit-only enum이 수집되지 않음

- **증상**: `same_spelling_is_not_same_identity`에서 두 `Empty`가 모두
  enum `A`로 해석; 튜플 테스트에서 subject가 `None`.
- **원인**: 픽스처의 `enum B { Empty, Fail }`이 unit-only + 무제네릭이라
  **plain TypeScript enum**으로 판정되어 통과(수집 대상 아님) — 언어의
  판별 규칙 그대로이며 resolver 버그가 아니었다(디버그 실행으로 defs에
  B가 없음을 확인).
- **해결**: 픽스처에 payload/빈 괄호 케이스를 넣어 rl enum으로 강제
  (`Fail(code: number)`, `X()`).

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` (603건 통과 — resolve 10건 추가, 기존 전부 유지)

## 결과

`src/resolve/` 신설 — 선언 수집·namespace·shadowing·identity 해석이 한
단계로 섰다. **남은 부채**: subject identification 규칙이 analysis와
병존한다(동등성 테스트로 고정) — Phase 3(TASK-123)에서 analysis가
resolver의 identity를 소비하며 해소한다.
