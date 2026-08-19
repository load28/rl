# TASK-083: TypeScript backend protocol 모듈 분리

- **상태**: 완료
- **시작일**: 2026-08-19
- **완료일**: 2026-08-19
- **커밋**: 미커밋

## 목적

`src/typescript/mod.rs`에 남은 host job serialization과 result parsing을
`src/typescript/protocol.rs`로 분리한다. protocol wire shape를 host runner와 diagnostic
mapper에서 떼어내어, 이후 legacy JS host와 tsgo/native IPC adapter가 같은 protocol
boundary를 공유할 수 있게 한다.

## 범위

- 포함: `types_job()` 이동.
- 포함: `EmittedTypes`, `LiteralMissing`, `ValMutation` result model 이동.
- 포함: `parse_types_result()`와 내부 JSON scanner 이동.
- 제외: `VirtualModule`/probe model 이동.
- 제외: host runner 이동.
- 제외: serde 도입.

## 의사결정

### 결정 1: 기존 minimal JSON scanner를 유지한다

- **상황**: host protocol은 우리 script가 만든 고정 JSON shape이며, 기존 구현은 serde
  dependency 없이 minimal scanner로 parsing한다.
- **검토한 대안**:
  - `serde_json`을 도입해 job/result를 구조화한다. 장점은 일반 JSON 처리 안정성이
    높다. 단점은 dependency와 변경 범위가 커지고, 이번 작업의 목적이 파일 분리에서
    serialization rewrite로 확장된다.
  - 기존 string serializer/scanner를 그대로 `protocol.rs`로 옮긴다. 장점은 동작 변화
    없이 ownership만 분리한다. 단점은 hand-written JSON 처리는 계속 남는다.
- **선택과 근거**: 두 번째를 선택한다. dependency 도입 여부는 native IPC protocol이
  실제로 안정화될 때 별도 태스크에서 판단한다.

## 작업 내역

- 2026-08-19: TASK-083을 등록했다.
- 2026-08-19: `src/typescript/protocol.rs`를 추가했다.
- 2026-08-19: host job serializer인 `types_job()`와 compiler option JSON을
  `protocol.rs`로 이동했다.
- 2026-08-19: host result model인 `EmittedTypes`, `LiteralMissing`, `ValMutation`을
  `protocol.rs`로 이동했다.
- 2026-08-19: host stdout parser `parse_types_result()`와 hand-written JSON scanner
  helper들을 `protocol.rs`로 이동했다.
- 2026-08-19: `src/typescript/mod.rs`가 `protocol::{types_job, parse_types_result}`를
  내부 사용하고 `EmittedTypes`만 crate-visible하게 re-export하도록 정리했다.

## 이슈 및 해결

### 이슈 1: `TypeDiagnostic` re-export가 일반 빌드에서 unused warning을 냄

- **증상**: `cargo check`가 `TypeDiagnostic` re-export에 대해 unused import warning을
  냈다. clippy gate에서는 warning이 실패가 된다.
- **원인**: `TypeDiagnostic`은 protocol parser 내부와 `main.rs` unit test에서 쓰지만,
  일반 `main.rs` call site는 직접 타입 이름을 import하지 않는다.
- **해결**: root module의 `TypeDiagnostic` re-export를 `#[cfg(test)]`로 제한했다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`

## 결과

TypeScript backend wire protocol의 job serialization과 result parsing이
`src/typescript/protocol.rs`로 분리됐다. 사용자 언어 표면, CLI 동작, 방출 코드 변화가
없는 내부 구조 변경이므로 `docs/ai/rl.md` 갱신은 필요 없다. 다음 단계는 semantic probe
model/generation을 `semantic.rs`로 분리하는 것이다.
