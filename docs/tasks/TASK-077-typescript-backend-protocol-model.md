# TASK-077: TypeScript backend protocol result model 분리

- **상태**: 완료
- **시작일**: 2026-08-19
- **완료일**: 2026-08-19
- **커밋**: 미커밋

## 목적

TASK-076에서 `src/typescript_backend.rs`로 옮긴 backend 선택 경계를 한 단계 더
넓힌다. `main.rs`에 남아 있던 host result data model, diagnostic source mapping,
result parser를 TypeScript backend module이 소유하게 만들어 다음 native IPC adapter
작업의 경계를 줄인다.

## 범위

- 포함: `EmittedTypes`, `TypeDiagnostic`, literal/`val` host answer model 이동.
- 포함: TypeScript diagnostic UTF-16 position → `.rl` source position 변환 이동.
- 포함: host stdout result parser 이동.
- 제외: host job serialization 전체 이동.
- 제외: `run_types_host()` 실행 책임 이동.
- 제외: native IPC 직접 구현.

## 의사결정

### 결정 1: result protocol부터 옮기고 job serialization은 남긴다

- **상황**: `main.rs`는 CLI 흐름, host 실행, job serialization, host result parsing,
  diagnostic mapping을 함께 들고 있었다. TASK-076은 backend enum과 embedded script만
  분리했으므로 protocol data model이 여전히 CLI 파일에 남아 있었다.
- **검토한 대안**:
  - host 실행과 job serialization까지 모두 한 번에 `typescript_backend`로 옮긴다.
    장점은 최종 구조에 더 가깝다. 단점은 `VirtualModule`, `LiteralCheck`,
    `ValCheck`, `TypesOptions`의 공개 범위를 한 번에 흔든다.
  - host result model, parser, diagnostic mapping만 먼저 옮긴다. 장점은 동작 변경 없이
    TypeScript backend가 반환하는 data shape ownership을 분명히 한다. 단점은
    `run_types_host()`와 `types_job()`이 아직 `main.rs`에 남는다.
- **선택과 근거**: 두 번째를 선택한다. 이번 단계의 목적은 native IPC 전환 전에
  output-side protocol boundary를 먼저 고정하는 것이다. input-side job serialization은
  후속 태스크에서 `VirtualModule`/probe model 이동과 함께 처리하는 편이 작다.

## 작업 내역

- 2026-08-19: TASK-077을 등록했다.
- 2026-08-19: `src/typescript_backend.rs`로 host result model을 이동했다.
  `EmittedTypes`, `TypeDiagnostic`, literal match 누락 응답, `val` mutation 응답을
  backend module이 소유하게 했다.
- 2026-08-19: TypeScript diagnostic의 UTF-16 line/column을 emitted byte offset으로
  바꾸고 `.rl` source offset으로 되돌리는 `utf16_offset()`/`source_offset()` 로직을
  `src/typescript_backend.rs`로 이동했다.
- 2026-08-19: host stdout parser(`parse_types_result()`와 내부 JSON scanner)를
  `src/typescript_backend.rs`로 이동했다.
- 2026-08-19: `src/main.rs`는 `TypeOrigin`을 만들어 diagnostics render에 넘기고,
  host 실행과 job serialization만 계속 담당하도록 정리했다.
- 2026-08-19: 기존 `type_diagnostic_tests`를 이동된 module API를 참조하도록 조정했다.

## 이슈 및 해결

없음.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`

## 결과

TypeScript backend module이 host result protocol과 diagnostic source mapping을 소유하게
됐다. `main.rs`에는 host process 실행과 input-side job serialization이 남아 있으며,
이는 후속 TASK에서 `VirtualModule`/probe model 이동과 함께 분리한다. 사용자 언어 표면,
CLI 동작, 방출 코드 변화가 없는 내부 리팩터링이므로 `docs/ai/rl.md` 갱신은 필요 없다.
