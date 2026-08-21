# TASK-144: 구조화된 타입 진단과 공통 렌더링

- **상태**: 완료
- **시작일**: 2026-08-21
- **완료일**: 2026-08-21
- **커밋**: —

## 목적

TypeScript 진단 문자열에 RL 이름을 덧붙이는 현재 방식을 구조화된 진단 모델로 대체한다. RL 구문 종류와 무관하게 기대 타입, 실제 타입, 타입 차이와 원인 범위를 계산하고 CLI·서버·에디터가 같은 진단을 소비하게 한다.

## 범위

- 포함: 공통 진단 IR, TypeScript 타입 사실 수집, 기대/실제 타입 차이 렌더링, 원인 범위 선택, CLI·에디터 공통 직렬화, 테스트와 규범 문서
- 제외: TypeScript 자체 타입 규칙 변경, 특정 데모 함수명·행·문구에 의존한 예외, 새 RL 문법

## 의사결정

### 진단 문자열이 아니라 checker 사실을 경계에서 정규화한다

- 상황: 기존 구현은 TypeScript의 중첩된 TS2322 문장을 그대로 싣고 RL 이름으로
  바꾼 문장을 한 번 더 붙여 핵심 타입 차이를 찾기 어려웠다.
- 대안: TS2322 문자열을 파싱하거나 `Result` 전용 진단을 추가하는 방법은 현재
  문구와 문법에 결합된다. TypeScript checker가 비교한 타입 객체를 받는 방법은
  반환식·타입 주석·인자·향후 lowering에 같은 규칙을 적용할 수 있다.
- 선택: backend가 contextual expected/found와 `isTypeAssignableTo` 결과를
  `TypeMismatch`로 직렬화한다. 심볼 ID가 같은 제네릭·유니언 구성요소를 재귀적으로
  따라가 최소 불일치 쌍을 구한다. 깊이 제한 뒤에는 전체 타입 쌍으로 안전하게
  후퇴한다. TypeScript Compiler API의 semantic diagnostic·type checker 경계는
  [공식 Compiler API 문서](https://github.com/microsoft/TypeScript/wiki/Using-the-Compiler-API)를
  기준으로 확인했다.

### 원인 진단을 결과 진단보다 우선한다

- 상황: 하나의 잘못된 `try`가 대입 불일치와 생성 글루의 프로퍼티·비교 오류를
  함께 만들었다. 정확한 RL 필드 오타도 더 넓은 구조적 타입 오류를 동반했다.
- 대안: TS 코드별 억제 목록은 새 lowering마다 늘어난다. match 전체를 poison하면
  내부의 독립적인 사용자 타입 오류까지 가릴 수 있다.
- 선택: 같은 lowering anchor에서 구조화된 expected/found를 원인으로 선택하고
  나머지 글루 진단을 결과로 억제한다. 정확한 RL 진단 span과 겹치는 구조화된
  타입 결과도 RL 원인에 양보한다. rustc의 구조화된 진단과 expected/found 모델은
  [rustc diagnostic 문서](https://doc.rust-lang.org/stable/nightly-rustc/rustc_errors/diagnostic/index.html)와
  [Diag API](https://doc.rust-lang.org/nightly/nightly-rustc/rustc_errors/struct.Diag.html)를
  기준으로 삼았다.

### 에디터의 빠른 답은 잠정값, compiler pass는 권위값으로 둔다

- 상황: 에디터는 tsgo LSP의 빠른 원문 진단을 표시했지만 CLI의 구조화된 렌더링과
  최종 문구가 달랐다.
- 대안: LSP 문구를 에디터에서 다시 변환하면 CLI와 구현이 중복된다. 타입 진단을
  editor typed pass에 포함하면 compiler의 동일한 report를 재사용할 수 있다.
- 선택: `typedCheck.includeTypes`를 추가하고, 권위 있는 응답이 도착하면 같은 문서
  버전의 잠정 TS 진단 계층 전체를 교체한다. 위치·코드·메시지는 서버가 직렬화한
  공통 진단을 그대로 사용한다. Language Service의 증분 서비스 역할은
  [공식 Language Service API 문서](https://github.com/microsoft/TypeScript/wiki/Using-the-Language-Service-API),
  LSP 진단의 range/code 표면은
  [LSP 3.18 명세](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.18/specification/)를
  확인했다.

## 작업 내역

- 2026-08-21: 중첩된 TS2322 원문과 RL 이름 치환문이 중복되어 핵심 `RangeError` 차이를 가리는 현상을 `_errors-demo.rl`에서 재현했다.
- 2026-08-21: rustc의 구조화된 `Diagnostic`/`Subdiagnostic`/expected-found 모델과 TypeScript Compiler·Language Service의 semantic diagnostics 경계를 공식 문서로 조사했다.
- 2026-08-21: `src/typescript/host.mjs`에서 진단 범위를 포함하는 AST의 contextual
  expression을 찾고 expected/found·최소 차이를 수집하도록 했다. Rust backend
  protocol과 parser에 `TypeMismatch`/`TypeDifference`를 추가했다.
- 2026-08-21: `src/engine/semantics.rs`에 구문 중립 렌더러와 원인 우선순위를
  추가했다. 구조화된 expression span을 원본 위치로 투영하고 RL 선언 이름을
  적용했다.
- 2026-08-21: 서버 `typedCheck`에 `includeTypes`를 추가하고 VSCode의 잠정
  language-service 타입 진단을 compiler 결과로 교체하도록 했다.
- 2026-08-21: 반환식·타입 주석·함수 인자·`try` lowering·RL 필드 오타 겹침을
  회귀 테스트로 추가하고 기존 원문 문구 의존 테스트를 구조화 계약으로 바꿨다.
- 2026-08-21: `docs/reference/errors.md`, `docs/design/compiler-core.md`,
  `docs/ai/rl.md`를 새 진단 계약에 맞췄다.

## 이슈 및 해결

### TS2322의 중첩 문구가 핵심 차이를 가림

- 증상: `Result<number, InputError>`에 `Err<RangeError>`가 섞인 경우 유니언과
  객체 구조 설명이 반복되고 `RangeError → InputError`가 뒤에 묻혔다.
- 원인: backend가 `diagnostic.text`만 전달해 rlc가 타입 비교 사실을 알 수 없었다.
- 해결: checker의 contextual type과 실제 expression type을 직접 비교하고 최소
  불일치 leaf와 전체 obligation을 분리해 렌더링했다.

### Err-only `try`가 세 개의 서로 다른 오류처럼 보임

- 증상: `try (() => Result.Err(10))()`에서 대입 오류 외에 `.value` 프로퍼티와
  `kind` 비교 오류가 함께 나왔다.
- 원인: 하나의 lowering이 만든 TypeScript 연산을 checker가 독립적으로 진단했다.
- 해결: 같은 anchor의 구조화된 assignability 진단을 원인으로 선택하고 나머지를
  결과로 억제했다. 출력은 `expected string, found number` 한 건이 됐다.

### VSCode 테스트가 설치된 이전 컴파일러를 사용함

- 증상: 새 typed-check 테스트가 빈 배열을 받고 기존 parser/editor 테스트도 현재
  소스와 다른 결과를 냈다.
- 원인: 테스트의 `COMPILER`는 PATH의 `rlc`를 사용했고 저장소 build가 아니었다.
- 해결: `scripts/setup`으로 release 컴파일러와 확장을 재설치하고, 검증에서는
  저장소 `target/release`를 PATH 앞에 두었다.

### VSCode 전체 테스트의 기존 환경 실패

- 증상: 전체 103개 중 6개가 실패했다. 네 건은 macOS `/var`와
  `/private/var` 실경로 비교였고, completion 한 건과 sidecar 한 건은 이번 변경과
  무관한 기존 기대값 차이였다.
- 원인: 해당 테스트는 이번에 수정한 typed diagnostic 경로 밖의 경로 정규화와
  기존 TypeScript 도구 동작을 검사한다.
- 해결: 이번 변경 경계인 TypeScript compile과 `typedcheck.test.ts` 네 건을 별도로
  실행해 모두 통과시켰다. 저장소 필수 Rust 검증 게이트는 전체 실행한다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` — 663개 통과
- [x] VSCode TypeScript 빌드와 `typedcheck.test.ts` 4개 통과
- [x] `scripts/setup`으로 release 컴파일러·VSCode 확장 재설치
- [x] rl-tour `npm install`과 `npx rlc --check-types src` 확인

## 결과

TypeScript checker의 구조화된 assignability 사실이 backend 경계를 넘어 공통
진단으로 전달된다. CLI·서버·에디터는 같은 최소 expected/found 문구와 타입
expression 범위를 사용한다. lowering의 파생 오류와 정확한 RL 원인 뒤의 중복
타입 오류는 원인 범위 단위로 억제된다.
