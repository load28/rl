# TASK-084: TypeScript semantic probe 모듈 분리

- **상태**: 완료
- **시작일**: 2026-08-19
- **완료일**: 2026-08-19
- **커밋**: 미커밋

## 목적

TypeScript 타입 백엔드 루트 모듈에 남아 있는 리터럴 match/`val` 타입 프로브 모델과
생성 로직을 별도 모듈로 분리한다. 프로토콜 직렬화, 진단 매핑, 호스트 실행과 의미
프로브 생성 책임을 나누어 이후 백엔드 정리 작업의 변경 범위를 줄인다.

## 범위

- 포함: `LiteralCheck`, `ValCheck`, `literal_checks`, `val_checks`와 보조 오프셋
  함수를 `src/typescript/semantic.rs`로 이동한다.
- 포함: `src/typescript/mod.rs`는 semantic 모듈을 선언하고 기존 외부 사용 지점이
  동일한 이름을 가져갈 수 있게 재수출한다.
- 제외: 타입 백엔드 호스트 실행 방식, JSON 프로토콜 형태, 의미 검사 규칙, 출력
  코드 형식은 바꾸지 않는다.

## 의사결정

### 결정 1: 의미 프로브 생성만 semantic 모듈로 분리

- **상황**: `src/typescript/mod.rs`가 호스트 실행, 가상 모듈 모델, 의미 프로브 생성,
  모듈 재수출을 함께 맡고 있었다. TASK-082/083에서 mapper/protocol을 분리했으므로
  남은 구조 로직도 단계별로 줄일 필요가 있었다.
- **검토한 대안**: 호스트 실행까지 한 번에 분리하면 루트 모듈을 더 작게 만들 수
  있지만 변경 범위가 커지고 검증 실패 시 원인 범위가 넓어진다. 프로브 생성만
  분리하면 이번 변경은 Rust 모듈 경계와 가시성 조정에 한정된다.
- **선택과 근거**: 프로브 모델과 생성 함수만 `semantic.rs`로 옮긴다. 리터럴
  소진성/`val` 변경 판정은 TypeScript checker에 던지는 의미 질의라는 공통성이 있고,
  호스트 실행·프로토콜 파싱과 분리해도 기존 호출 인터페이스를 유지할 수 있다.

### 결정 2: `VirtualModule`은 루트에 유지

- **상황**: `VirtualModule`은 프로브가 아니라 호스트에 넘기는 컴파일 산출 모듈 모델이다.
- **검토한 대안**: `VirtualModule`까지 semantic 모듈로 옮기면 타입 백엔드 모델을 한
  파일에 모을 수 있지만 semantic이라는 이름과 책임이 맞지 않는다. 새 project/model
  모듈을 만들 수도 있으나 이번 태스크의 범위를 넘어선다.
- **선택과 근거**: `VirtualModule`은 일단 루트 모듈에 둔다. 이 결정으로 semantic
  모듈은 타입 checker에 요청할 의미 질의 모델만 가진다.

## 작업 내역

실제로 수행한 작업을 시간순으로, 재현 가능할 만큼 구체적으로 기록한다.
무엇을 어떤 파일에 어떻게 바꿨는지, 어떤 명령으로 확인했는지 포함.

- 2026-08-19: `docs/tasks/INDEX.md`에 TASK-084를 진행 중으로 등록하고 다음 번호를
  TASK-085로 갱신했다.
- 2026-08-19: `src/typescript/semantic.rs`를 추가해 리터럴 match/`val` 타입 프로브
  구조체와 생성 함수를 이동했다.
- 2026-08-19: `src/typescript/mod.rs`에서 의미 프로브 모델/생성 구현을 제거하고
  semantic 모듈 선언과 기존 crate-visible 재수출만 남겼다.
- 2026-08-19: `LiteralCheck.covered`는 sibling module인 `protocol.rs`가 JSON job
  직렬화 때 읽어야 하므로 `pub(crate)` 필드로 조정했다.
- 2026-08-19: `cargo fmt --check`, `cargo check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test`로 변경을 검증했다.

## 이슈 및 해결

없음.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`

## 결과

TypeScript 타입 백엔드의 의미 프로브 모델과 생성 로직이
`src/typescript/semantic.rs`로 분리됐다. 사용자 언어 표면, CLI 동작, 방출 코드 변화가
없는 내부 구조 변경이므로 `docs/ai/rl.md` 갱신은 필요 없다. 다음 단계는 호스트 실행
경계를 별도 모듈로 정리하는 것이다.
