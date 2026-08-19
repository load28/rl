# TASK-082: TS7 semantic unification 제안서 검토와 개선 계획 확정

- **상태**: 완료
- **시작일**: 2026-08-19
- **완료일**: 2026-08-19
- **커밋**: (커밋 후 기입)

## 목적

"val binding resolution을 TypeScript symbol identity로 통일하고, host의
semantic query를 batch화하고, TS adapter를 Node 중심으로 정리한다"는 외부
제안서를 현재 main 구현과 최신 `microsoft/typescript-go` main의 실제 API
표면에 대조해 검증하고, 사실관계에 맞게 고쳐 쓴 실행 계획을 확정한다.
이 태스크의 산출물은 코드가 아니라 검증된 계획 문서
(`docs/design/ts7-semantic-unification.md`)다.

## 범위

- 포함: 제안서 전 항목의 사실 검증(코드 대조 + tsgo HEAD API 대조),
  잘못된 전제의 교정, 우선순위 재배열, 계획 문서 작성.
- 제외: 계획의 구현(후속 태스크 P1~P4로 진행), mutator 정책 목록 보강
  (`Date#setHours` 등 — 언어 표면 변경이므로 별도 태스크).

## 의사결정

### 결정 1: 제안서 P1("val 완전 통일")의 범위를 typed 경로로 좁힌다

- **상황**: 제안서는 `val.rs`의 scope model 제거를 최우선으로 놓았다.
- **검토한 대안**: ① untyped 경로 포함 전면 제거(= val 검사를
  `--check-types` 전용으로 만들거나 tsgo를 필수 의존성으로),
  ② typed 경로만 대상으로 좁히고 untyped scope model은 문서화된 근사로
  유지.
- **선택과 근거**: ②. 코드 대조 결과 typed 경로(`defer_to_checker`)는
  이미 `val::check`를 건너뛰고 `ValProbes` + symbol id pairing으로
  동작한다(`lib.rs:629-631`, `check.rs:334-420`). ①은 제안서 자신의 완료
  기준 7(기존 semantics 불변) 및 독립 실행 컴파일러 설계와 모순.

### 결정 2: 항목 4(Node+Location 중심 전환)는 뒤집는다

- **상황**: 제안서는 position 계열 API를 legacy로 전제했다.
- **검토한 대안**: ① Node 기반 batch로 전환, ② position 기반 batch 유지.
- **선택과 근거**: ②. tsgo HEAD(`c6b013f5`, 2026-08-19)의
  `api/sync/api.ts`에서 `getTypeAtPosition(file, positions[])` /
  `getSymbolAtPosition(file, positions[])`가 1급 batch endpoint
  (`getTypesAtPositions`/`getSymbolsAtPositions`)임을 확인. Node 경유는
  `getSourceFile` 바이너리 AST 전송이 선행돼야 하므로 offset을 이미 아는
  rlc에게 순수 오버헤드.

### 결정 3: mutator 이름 목록은 제거가 아니라 판정 시점으로 이동

- **상황**: 제안서 P3은 이름 prefilter를 "correctness-critical 구조에서
  제거"하라고 했다.
- **검토한 대안**: ① 목록 완전 제거(builtin 판정만으로 verdict),
  ② 수집은 무필터·판정에서 `builtin && mutator_name` 적용.
- **선택과 근거**: ②. 현재 verdict(`check.rs:349-355`)는 `builtin`만
  보므로 ①은 `map.get()`·`arr.at()` 같은 non-mutating builtin을 전부
  오탐으로 만든다. TypeScript는 mutation effect를 제공하지 않으므로
  이름 정책 자체는 RL이 소유해야 하고(제안서 2절도 인정), 옮기면 목록
  누락이 오탐을 만들 수 없는 구조(미탐만 가능)가 된다.

### 결정 4: typed 경로에 남은 이름 기반 근사로 callee resolution을 지목

- **상황**: 제안서가 나열한 제거 후보(lexical scope, shadowing,
  redeclaration pairing)는 typed 경로에서 이미 제거돼 있어, 실제 남은
  근사를 코드에서 다시 찾아야 했다.
- **검토한 대안**: 없음(사실 확인).
- **선택과 근거**: `collect_signatures`/`check_call`(`val.rs:535`,
  `val.rs:1264`)이 call-capability 검사의 callee를 같은 파일 안에서
  **이름으로** 매칭하고, probes 모드에서도 동일하다. 이것이 typed 경로의
  마지막 name-resolution 근사이므로 P3(callee symbol pairing)로 등재.

### 결정 5: 산출물은 design 문서 + 태스크 문서로 커밋한다

- **상황**: 검토 결과를 어디에 남길지.
- **선택과 근거**: 선례(TASK-079의 `docs/design/tsgo-native-backend.md`)를
  따라 "검토 기록이자 제안, 규범 아님" 헤더를 단 design 문서로 편입.
  구현은 후속 태스크에서 이 문서를 참조한다.

## 작업 내역

- 2026-08-19: 제안서를 main 코드와 대조 — `src/typescript/backend.rs`
  (batch Query 이미 존재), `src/typescript/check.rs`(symbol id pairing 이미
  존재), `src/typescript/host.mjs`(check당 개별 IPC 확인),
  `src/val.rs`(Sink 3모드, 이름 prefilter는 probes 수집 조건임을 확인),
  `src/lib.rs`(`defer_to_checker`가 `val::check`를 건너뜀을 확인).
- 2026-08-19: `microsoft/typescript-go` HEAD `c6b013f5`를 shallow clone,
  `_packages/native-preview/src/api/sync/api.ts`·`proto.ts`에서 batch
  overload·`isTypeAssignableTo`·`runWithTemporaryFileUpdate`·
  `getSourceFileMetadata`·`SymbolResponse`/`TypeResponse`/`NodeHandle`
  구조를 확인 (문서 §1에 기록).
- 2026-08-19: 기존 테스트 커버리지 확인 — `tests/native.rs`에 shadowing·
  parameter 경계·사용자 정의 mutator·narrowed exhaustiveness 테스트가
  이미 있음을 확인하고 문서 §4에 회귀 게이트로 등재.
- 2026-08-19: `docs/design/ts7-semantic-unification.md` 작성, INDEX 갱신.

## 이슈 및 해결

### 이슈 1: 제안서의 현재 상태 서술이 한 세대 이전 구현을 가리킴

- **증상**: 제안서가 "목표 구조"로 그린 것(unpaired probes + symbol
  pairing, batch Query)이 이미 main에 있음.
- **원인**: TASK-070~081(특히 071·073~077)이 제안서 작성 기준 시점 이후에
  같은 방향을 이미 구현.
- **해결**: 문서 §0에 "현재 구현 기준선"을 명시하고, 남은 실제 작업만
  P1~P5로 재배열.

## 검증 게이트

문서만 변경한 태스크이나 규칙대로 실행:

- `cargo fmt --check` — 통과
- `cargo clippy --all-targets -- -D warnings` — 통과
- `cargo test` — 통과 (tsc/node 있는 환경, 통합 테스트 포함)

## 변경 파일

- `docs/design/ts7-semantic-unification.md` (신규)
- `docs/tasks/TASK-082-ts7-semantic-unification-review.md` (신규)
- `docs/tasks/INDEX.md` (TASK-082 등록, 다음 번호 083)
