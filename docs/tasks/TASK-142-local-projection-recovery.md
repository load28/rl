# TASK-142: 오류 구문 단위 typed projection 복구

- **상태**: 완료
- **시작일**: 2026-08-21
- **완료일**: 2026-08-21
- **커밋**: —

## 목적

한 파일의 복구 불가능한 rl 구문이 같은 파일의 독립적인 TypeScript 진단을 가리는 문제를 해결한다. 오류가 있는 구문만 안전한 TypeScript로 복구해 프로젝트 typed 검사를 계속한다.

## 범위

- 포함: projection 전용 구문 복구 표현, 원본 좌표 매핑 보존, 같은 파일의 RL·TS 진단 병합, 회귀 테스트와 규범 문서 갱신
- 제외: 정상 컴파일 출력 변경, 오류 진단 문구 변경, 새로운 rl 문법 추가

## 의사결정

### 결정 1: 오류 문자열이 아니라 파서 AST에 복구 노드를 둔다

- **상황**: `stray_*`의 진단 위치만으로 구문 전체를 추정하면 다음 독립 문장까지 지우거나 불완전한 TypeScript를 만들 수 있다.
- **검토한 대안**: 진단 코드·문구에 따라 문자열을 치환하는 방식은 빠르지만 구문 경계를 중복 추론한다. 파일 전체를 typed 프로젝트에서 제외하는 방식은 기존 문제를 유지한다. 파서가 span과 placeholder 종류를 가진 오류 노드를 만드는 방식은 구문 판정과 복구 경계를 한 단계가 소유한다.
- **선택과 근거**: `Program::recoveries`에 `RecoveryNode { span, kind }`를 추가했다. `match`·`enum`·`result`·`if let`·파이프라인과 중첩 `<-`를 판정한 파서가 동기화 경계를 함께 기록한다. rustc의 `ExprKind::Err(ErrorGuaranteed)`가 잘못된 식을 AST placeholder로 보존하는 구조와 같은 단계 분리를 따른다.

### 결정 2: 정상 방출과 typed projection 복구를 분리한다

- **상황**: 프로젝트 typed 검사는 계속되어야 하지만, 정상 컴파일은 완전히 파싱되지 않은 구문을 변환하지 않는 passthrough 계약을 지켜야 한다.
- **검토한 대안**: 일반 codegen에서 오류 노드를 항상 placeholder로 내보내면 정상 방출 계약이 바뀐다. projection 전용 복구는 사용자 빌드 출력에는 영향을 주지 않는다.
- **선택과 근거**: `compile_report()`는 그대로 두고 엔진 전용 `compile_projection_report()`를 추가했다. 복구 소스는 원본과 같은 바이트 길이를 유지해 기존 source mapping 좌표를 보존한다. `tests/compile.rs`의 emission-withholding 계약과 56개 passthrough 테스트가 모두 통과했다.

### 결정 3: 연쇄 진단 억제를 오류 노드와 교차하는 범위로 제한한다

- **상황**: placeholder 자체에서 생긴 TypeScript 진단은 원인의 반복이지만 match나 파일 전체를 억제하면 독립 진단도 사라진다.
- **검토한 대안**: 파일 단위 억제는 `bindNonResult`를 숨긴다. match 전체 typed-poison도 내부 사용자 오류를 숨길 수 있다. 진단 범위와 오류 노드 span의 교차 판정은 원인 범위만 제외한다.
- **선택과 근거**: emitted 진단의 시작·끝을 원본으로 역매핑하고 recovery span과 교차할 때만 제외한다. 투어 전체에서 malformed·중첩 바인딩 진단을 유지하면서 `bindNonResult`의 TS2322가 267행에 함께 보고됨을 확인했다.

## 작업 내역

- 2026-08-21: `bindNonResult`의 TS2322가 같은 파일의 malformed·nested result 오류 때문에 사라지는 현상을 재현하고 TASK-142를 등록했다.
- 2026-08-21: 파서의 Claim과 result 분석에 구문 전체 recovery span을 추가하고 중첩 AST의 오류 노드를 수집했다.
- 2026-08-21: 엔진 전용 byte-length-preserving projection 복구와 recovery span 기반 TypeScript 진단 억제를 구현했다.
- 2026-08-21: Snapshot 테스트를 파일 제외 계약에서 오류 노드 격리 계약으로 갱신하고 native 회귀 테스트를 추가했다.
- 2026-08-21: `docs/reference/errors.md`, `docs/design/compiler-core.md`, `docs/ai/rl.md`에 typed projection 복구 계약을 반영했다.

## 이슈 및 해결

### 이슈 1: 파일 단위 projection 차단이 독립 진단을 제거함

- **증상**: `_errors-demo.rl` 전체를 검사하면 `bindNonResult`의 TS2322가 보고되지 않지만 차단 구문들을 주석 처리하면 보고된다.
- **원인**: `compile_report()`가 emit을 보류하면 `ProjectedDocument` 생성이 실패하고, Snapshot이 그 파일 전체를 typed query의 modules에서 제외했다.
- **해결**: 파서 오류 노드만 placeholder로 내보내는 projection 전용 보고서를 추가해 해당 파일을 Snapshot에 유지했다.

### 이슈 2: placeholder의 타입 오류가 오류 구문 앞의 `return`에 보고됨

- **증상**: malformed `result`를 `undefined`로 복구한 뒤 TS2322가 `return` 시작 위치에 나타나 시작점만 비교한 억제를 우회했다.
- **원인**: TypeScript 진단 범위가 반환문 전체를 덮어 시작점은 recovery span 밖이고 끝점만 안에 들어갔다.
- **해결**: 진단의 시작·끝을 모두 원본으로 역매핑하고 recovery span과 범위가 교차하는지 판정하도록 바꿨다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`

## 결과

파서가 소유한 오류 노드 단위로 typed projection을 복구한다. rl 오류가 여러 개 있는 한 파일에서도 독립적인 TypeScript 진단이 계속 보고되며, 정상 컴파일과 passthrough 출력은 바뀌지 않는다.
