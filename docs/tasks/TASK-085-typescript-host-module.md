# TASK-085: TypeScript backend 모듈 경계 정리

- **상태**: 완료
- **시작일**: 2026-08-19
- **완료일**: 2026-08-19
- **커밋**: `8231619`

## 목적

TypeScript 타입 백엔드 루트 모듈에 남은 호스트 실행 책임과 공통 모델 책임을 한 번에
정리한다. protocol, mapper, semantic probe 분리에 이어 Node/tsgo 실행 경계와 host
입력 모델을 독립시켜 루트 모듈의 역할을 타입 백엔드 public surface 재수출로 좁힌다.

## 범위

- 포함: `TypesBackend`, embedded host script 상수, `run_types_host`를
  `src/typescript/host.rs`로 이동한다.
- 포함: `VirtualModule`을 `src/typescript/model.rs`로 이동한다.
- 포함: `src/typescript/mod.rs`는 하위 모듈 선언과 필요한 crate-visible 재수출만 맡는다.
- 제외: host process 실행 방식, 환경 변수 이름, 오류 메시지, protocol JSON shape는
  바꾸지 않는다.

## 의사결정

### 결정 1: 실행 경계를 host 모듈로 분리

- **상황**: mapper/protocol/semantic이 분리된 뒤 `src/typescript/mod.rs`에 남은 큰
  구현은 Node host script 생성과 child process 실행이었다.
- **검토한 대안**: root에 유지하면 import가 단순하지만 루트 모듈이 계속 실행 세부사항을
  포함한다. host 모듈로 이동하면 root는 공통 모델과 재수출만 맡고, legacy JS/tsgo
  선택 로직이 한 파일에 모인다.
- **선택과 근거**: `host.rs`로 이동한다. 이 파일은 embedded script 선택, command
  구성, stdout/stderr 처리까지 실제 host process boundary를 전담한다.

### 결정 2: `TypesBackend`는 private 구현 세부사항으로 둔다

- **상황**: backend 선택은 현재 `run_types_host` 내부에서 `RLC_TS_BACKEND`를 읽어
  결정하며, crate 외부 호출자는 backend enum을 직접 다루지 않는다.
- **검토한 대안**: root에서 `TypesBackend`를 계속 공개하면 테스트나 후속 코드에서
  직접 사용할 수 있지만 현재 공개 surface가 불필요하게 넓다. host 내부 private로
  두면 backend 선택 정책이 실행 모듈에 캡슐화된다.
- **선택과 근거**: `TypesBackend`는 `host.rs` private type으로 이동한다. 기존
  외부 호출 지점은 `run_types_host`만 사용하므로 API 변화 없이 구현 세부사항을
  숨길 수 있다.

### 결정 3: 너무 작은 후속 태스크 생성을 중단하고 관련 경계 정리를 묶는다

- **상황**: TASK-077부터 모듈 분리를 작은 파일 단위로 진행해 왔으나, 이후 작업은
  사용자가 보기에는 같은 TypeScript backend 구조 정리의 연속이다.
- **검토한 대안**: 파일 하나마다 계속 TASK를 만들면 기록은 세밀하지만 커밋과 태스크가
  과도하게 늘어난다. 현재 TASK-085에 host 실행과 공통 모델 정리를 묶으면 하나의
  의미 있는 리팩터링 단위로 닫을 수 있다.
- **선택과 근거**: TASK-085 범위를 넓혀 남은 루트 모듈 경계 정리를 함께 수행한다.
  이후에는 독립적인 사용자 가치나 검증 의미가 있는 단위로만 새 태스크를 만든다.

## 작업 내역

실제로 수행한 작업을 시간순으로, 재현 가능할 만큼 구체적으로 기록한다.
무엇을 어떤 파일에 어떻게 바꿨는지, 어떤 명령으로 확인했는지 포함.

- 2026-08-19: `docs/tasks/INDEX.md`에 TASK-085를 진행 중으로 등록하고 다음 번호를
  TASK-086으로 갱신했다.
- 2026-08-19: `TypesBackend`와 `run_types_host`를 `src/typescript/host.rs`로 이동하기로
  했다.
- 2026-08-19: 태스크 단위가 지나치게 작아지지 않도록 TASK-085 범위를 host runner와
  공통 모델 경계 정리를 포함하는 단위로 확장했다.
- 2026-08-19: `src/typescript/host.rs`를 추가하고 embedded JS/tsgo host script 선택,
  Node child process 실행, stdout/stderr 처리, host result parsing 호출을 이동했다.
- 2026-08-19: `TypesBackend`를 `host.rs` 내부 private enum으로 바꿔 backend 선택 정책을
  실행 경계 안에 캡슐화했다.
- 2026-08-19: `src/typescript/model.rs`를 추가하고 `VirtualModule`을 이동했다.
- 2026-08-19: `src/typescript/mod.rs`를 하위 모듈 선언과 crate-visible 재수출만 담는
  루트로 정리했다.
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

TypeScript 타입 백엔드 루트 모듈이 `host`, `model`, `mapper`, `protocol`, `semantic`
하위 모듈을 선언하고 필요한 항목을 재수출하는 얇은 경계로 정리됐다. 사용자 언어
표면, CLI 동작, 방출 코드 변화가 없는 내부 구조 변경이므로 `docs/ai/rl.md` 갱신은
필요 없다.
