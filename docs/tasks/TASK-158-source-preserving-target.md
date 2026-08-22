# TASK-158: source-preserving target과 printer

- **상태**: 완료
- **시작일**: 2026-08-22
- **완료일**: 2026-08-22
- **커밋**: —

## 목적

기존 Rope의 source/glue 구조를 provenance가 명시된 Structured TypeScript target으로
승격한다. codegen이 target printer를 경유하도록 바꾸되 출력 bytes, mapping, mark,
anchor는 기존 결과와 완전히 같게 유지한다.

## 범위

- 포함: TargetPiece, SourceOrigin, TargetFile, 구조·source range validator, printer,
  helper emission 통합, byte-identical/mapping 회귀 테스트
- 제외: Evaluation IR 기반 direct control flow, IIFE 출력 변경, 새 hygiene/effect 분석

## 의사결정

### 결정 1: Rope를 버리지 않고 target builder 경계로 사용한다

- **상황**: 기존 Rope는 source/glue/mark/anchor 조각과 trim·append 의미를 이미 보존한다.
  별도 emitter를 동시에 만들면 같은 Core lowering을 두 번 구현해야 한다.
- **검토한 대안**: Evaluation IR에서 새 target을 바로 만들면 최종 구조에는 가깝지만 Phase 3의
  byte-identical 기준선을 증명하기 전에 codegen 의미까지 함께 바뀐다. Rope를 그대로
  printer로 두면 generated provenance를 검증할 계층이 없다.
- **선택과 근거**: Rope를 변경 가능한 조립 계층으로 유지하고, 소비 시 불변 `TargetFile`로
  변환한다. 기존 Core emitter는 그대로이며 target printer만 최종 `Flat`을 만든다.

### 결정 2: source와 generated origin을 타입으로 구분한다

- **상황**: 원본 bytes와 생성 glue는 진단·navigation 계약이 다르며 같은 span 타입으로
  섞으면 generated text가 양방향 mapping을 가질 수 있다.
- **검토한 대안**: 모든 piece에 optional source span을 두면 `None`의 의미가 모호하다.
  anchor 안의 glue만 origin으로 인정하면 enum, import rewrite, helper 같은 비-anchor 생성물이
  provenance 없이 남는다.
- **선택과 근거**: source piece는 `ExactOrigin`만 가질 수 있다. generated piece는
  `SourceOrigin::Construct` 또는 source parent가 있는 `Synthetic`만 가질 수 있다. 타입이
  generated text의 exact mapping 생성을 막는다.

### 결정 3: target printer를 실제 출력 경로로 사용한다

- **상황**: shadow target만 만들고 기존 flatten을 계속 쓰면 byte 동일성은 확인해도 새
  printer의 mapping·anchor 동작이 실제 compile corpus에서 검증되지 않는다.
- **검토한 대안**: 두 printer 출력을 매번 비교하면 안전하지만 문자열과 metadata를 두 번
  할당한다. target을 바로 사용하면 단일 출력 경로가 되며 기존 회귀 테스트 전체가
  동등성 게이트가 된다.
- **선택과 근거**: `Rope → TargetFile → Flat`을 실제 경로로 전환했다. validator는 debug/test
  빌드에서 즉시 실패하고 release 출력에는 새 사용자 오류를 만들지 않는다.

## 작업 내역

- 2026-08-22: 기존 Rope piece와 Flat printer, helper 후처리, mapping·mark·anchor 계약을
  확인했다.
- 2026-08-22: Rust Enterprise Type Programming Guide의 provenance 합 타입, typed error,
  불변 target 원칙을 적용하기로 했다.
- 2026-08-22: `ExactOrigin`, `SourceOrigin`, `SyntheticReason`, `TargetPiece`, `TargetFile`,
  `TargetError`를 구현했다.
- 2026-08-22: target validator가 출력 길이, source/construct/synthetic parent 범위,
  anchor 균형을 검사하도록 했다.
- 2026-08-22: 기존 Rope flatten 로직을 target printer로 옮겨 mapping 병합, mark 정렬,
  nested anchor 순서를 그대로 유지했다.
- 2026-08-22: pipeline과 flow helper를 `Flat.code` 사후 변경에서 Rope 조립 단계로 옮겨
  전체 출력이 target provenance와 validator를 통과하도록 했다.
- 2026-08-22: provenance, source 범위, anchor 불균형 단위 테스트와 전체 회귀 게이트를
  통과했다.

## 이슈 및 해결

### 이슈 1: file helper가 target 밖에서 출력에 추가됨

- **증상**: pipeline·flow helper는 기존 Rope를 flatten한 뒤 `Flat.code.push_str`로 붙어서
  TargetPiece, provenance, validator, mapping printer를 통과하지 않았다.
- **원인**: helper 사용 여부가 body emission 후에 확정됐고 기존 구현이 완성된 문자열에
  후처리하는 구조였다.
- **해결**: Rope에 `ends_with_newline` 조회를 추가하고 helper와 필요한 newline을 generated
  piece로 body Rope에 붙인 뒤 한 번만 target printer를 실행하도록 순서를 바꿨다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`

## 결과

전체 codegen 출력이 provenance를 가진 source-preserving `TargetFile`과 단일 printer를
통과한다. 기존 출력 bytes, mapping, scrutinee/payload mark, anchor, 진단과 runtime 동작은
전체 회귀 테스트에서 유지됐다. 다음 태스크는 boundary가 없는 Evaluation region부터
direct control flow target을 생성하는 Phase 4다.
