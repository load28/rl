# TASK-079: TypeScript backend host runner 분리

- **상태**: 완료
- **시작일**: 2026-08-19
- **완료일**: 2026-08-19
- **커밋**: `8231619`

## 목적

TASK-077/078에서 TypeScript backend protocol model을 분리한 데 이어, embedded Node
host 실행 책임도 `src/typescript_backend.rs`로 옮긴다. `main.rs`는 `--types` pipeline의
source 수집, 진단 출력, sidecar 쓰기 흐름만 담당하고, backend module이 host script
선택·job serialization·process execution·result parsing을 한 경계에서 처리하게 한다.

## 범위

- 포함: `run_types_host()` 이동.
- 포함: backend별 Node args, host script materialization, failure message 처리 이동.
- 포함: `main.rs`의 backend-specific import 제거.
- 제외: sidecar writing 이동.
- 제외: TypeScript backend trait/hierarchy 도입.
- 제외: native IPC 직접 구현.

## 의사결정

### 결정 1: host runner는 일단 기존 error printing 계약을 유지한다

- **상황**: `run_types_host()`는 CLI-facing error message와 `ExitCode`를 직접 반환한다.
  더 순수한 module API를 만들려면 error enum을 새로 설계하고 `main.rs`에서 출력하게
  할 수 있지만, 그러면 동작 변경 위험이 생긴다.
- **검토한 대안**:
  - `TypeScriptBackendError` enum을 새로 만들고 CLI 출력은 `main.rs`로 되돌린다.
    장점은 library-like module 경계가 깨끗하다. 단점은 기존 메시지/exit 흐름이
    변경될 수 있다.
  - 기존 `Result<EmittedTypes, ExitCode>`와 `eprintln!` 계약을 유지한 채 함수를
    module로 이동한다. 장점은 동작 변경 없이 host 실행 ownership만 이동한다.
- **선택과 근거**: 두 번째를 선택한다. 이번 단계는 구조 이동이며, error type 정리는
  native IPC adapter 도입 시 별도 태스크로 처리하는 편이 검증 범위가 작다.

## 작업 내역

- 2026-08-19: TASK-079를 등록했다.
- 2026-08-19: `run_types_host()`를 `src/main.rs`에서
  `src/typescript_backend.rs`로 이동했다.
- 2026-08-19: embedded host script를 temp dir에 쓰고 Node process를 실행하는 책임을
  backend module로 옮겼다.
- 2026-08-19: backend별 failure message(`typescript` 미설치, `typescript-go` checkout
  누락, TypeScript 7 JS API 부재)를 기존 문자열 그대로 backend module에 유지했다.
- 2026-08-19: `src/main.rs`에서 `Command`, `TypesBackend`, `types_job`,
  `parse_types_result`, `EmittedTypes` import를 제거했다.

## 이슈 및 해결

없음.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`

## 결과

`src/typescript_backend.rs`가 backend selection, host script ownership, input job
serialization, process execution, output result parsing, diagnostic source mapping을 모두
소유하게 됐다. `main.rs`는 `--types` source collection, typed diagnostic rendering
call site, sidecar writing을 계속 담당한다. 사용자 언어 표면, CLI 동작, 방출 코드 변화가
없는 내부 리팩터링이므로 `docs/ai/rl.md` 갱신은 필요 없다.
