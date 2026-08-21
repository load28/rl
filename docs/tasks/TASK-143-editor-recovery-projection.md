# TASK-143: 에디터가 typed recovery projection을 공유

- **상태**: 완료
- **시작일**: 2026-08-21
- **완료일**: 2026-08-21
- **커밋**: —

## 목적

CLI typed 검사에서 복구된 파일을 에디터 TypeScript 서비스도 같은 projection으로 검사하게 한다. malformed rl 구문이 있어도 `tryNonResult` 같은 독립 타입 진단을 에디터에 표시한다.

## 범위

- 포함: language service projection 공유, recovery 범위 연쇄 진단 억제, `try` 표현식 배치 진단, 회귀 테스트와 문서
- 제외: 정상 컴파일 출력 변경, 새 문법

## 의사결정

### 결정 1: 에디터 서비스와 배치 typed 검사가 같은 projection 생성기를 쓴다

- **상황**: 배치 typed 검사는 TASK-142의 오류 노드 복구를 사용하지만 에디터 TypeScript 서비스는 복구 전 `emit_mapped()`를 별도로 호출했다. 이후 출력 검증이 실패하면 파일 전체 진단을 폐기했다.
- **검토한 대안**: 에디터에서 malformed 파일의 타입 진단을 계속 모두 버리면 독립 오류가 사라진다. Node LSP에서 문자열을 고치면 컴파일러와 복구 규칙이 중복된다. Rust 엔진의 공통 projection 생성기를 쓰면 파서의 오류 노드와 source map을 그대로 공유한다.
- **선택과 근거**: `serve_one()`과 문서 전용 서빙이 `compile_projection_report()`를 호출하도록 통합했다. 복구되지 않는 입력만 기존 raw projection과 파일 단위 안전장치로 폴백한다.

### 결정 2: 복구 연쇄 진단은 오류 노드와 교차할 때만 억제한다

- **상황**: placeholder가 만든 TypeScript 진단은 숨겨야 하지만 같은 파일의 `tryNonResult`는 유지해야 한다.
- **검토한 대안**: 파일 전체 억제는 기존 누락을 재현한다. TypeScript 오류 코드별 억제는 새 코드가 추가될 때 규칙이 새고 원인 범위를 표현하지 못한다.
- **선택과 근거**: 서비스 문서에 parser-owned recovery span을 보존했다. TypeScript 진단을 원본 UTF-16 좌표로 역매핑한 뒤 그 범위가 recovery span과 교차할 때만 제외한다.

### 결정 3: `try` claim 전에 문장 위치를 구조적으로 판정한다

- **상황**: `return try a()`를 bare `try` 문으로 잘못 claim하면 codegen이 표현식 일부를 문장으로 방출해 일반 출력 검증 오류만 남긴다.
- **검토한 대안**: `return` 문자열만 특별 처리하면 화살표 함수 본문과 삼항식에서 같은 결함이 남는다. TypeScript 오류에 맡기면 rlc가 만든 잘못된 출력 때문에 진단 위치와 원인이 흐려진다.
- **선택과 근거**: 파서가 앞 토큰의 문장 경계, 제어문 head, 삼항식 상태를 확인한 뒤 bare `try`를 claim한다. 표현식 위치이면 `try <식>` 범위의 `try-placement` 오류 노드를 만들며, return·arrow·ternary 회귀 테스트로 확인했다.

## 작업 내역

- 2026-08-21: 설치 경로와 실행 프로세스를 확인하고 에디터 누락을 `service_diagnostics()`의 복구 전 `emit_mapped()` projection으로 재현했다.
- 2026-08-21: 에디터 서비스 projection을 `compile_projection_report()`로 통합하고 recovery span 교차 진단만 억제했다.
- 2026-08-21: `return try a()`를 포함한 표현식 위치의 bare `try`에 정밀한 `try-placement` 진단을 추가했다.
- 2026-08-21: Rust 단위·통합 테스트와 VS Code 엔진·LSP 회귀 테스트를 추가하고 규범·AI 문서를 갱신했다.

## 이슈 및 해결

### 이슈 1: 에디터 TypeScript 서비스가 파일 전체 진단을 거부함

- **증상**: CLI는 `_errors-demo.rl:242`의 `tryNonResult` 진단을 보고하지만 VS Code에는 표시되지 않는다.
- **원인**: 조사 결과 `serve_one()`이 복구 전 가상 문서를 제공하고 `projection_accepts_diagnostics()`가 malformed 출력의 모든 TypeScript 진단을 버린다.
- **해결**: 에디터 서비스가 배치 typed 경로와 같은 복구 projection을 사용하고, recovery span과 교차하는 연쇄 진단만 제외하도록 변경했다.

### 이슈 2: `return try a()`가 일반 출력 검증 오류로 바뀜

- **증상**: 에디터에 `try`의 배치 오류가 없고, 컴파일러는 생성 TypeScript parse 실패만 보고했다.
- **원인**: bare `try` sub-parser가 식의 선행 문맥을 확인하지 않아 `return` 뒤의 `try a();`를 독립 문장으로 claim했다.
- **해결**: claim 전에 문장 시작 여부를 판정하고 표현식 위치에는 `TryPlacement` 오류 노드를 남겼다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`
- [x] VS Code TypeScript 빌드 및 추가한 엔진·LSP 회귀 테스트

## 결과

에디터와 배치 typed 검사가 같은 오류 노드 복구 projection을 사용한다. malformed rl 구문이 있어도 독립적인 `try` 타입 오류가 표시되며, `return try a()`는 생성 코드 검증 오류가 아니라 원본 `try a()` 범위의 안정된 `try-placement` 진단으로 표시된다.
