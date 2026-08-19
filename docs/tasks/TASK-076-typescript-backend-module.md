# TASK-076: TypeScript backend 모듈 분리

- **상태**: 완료
- **시작일**: 2026-08-19
- **완료일**: 2026-08-19
- **커밋**: 미커밋

## 목적

TASK-075에서 `main.rs` 내부 enum으로 만든 TypeScript backend 선택 경계를 별도 모듈로
분리한다. 이후 native IPC adapter나 legacy 제거 작업이 CLI 진입점과 덜 얽히게 한다.

## 범위

- 포함: backend enum과 embedded host script constants를 별도 Rust module로 이동.
- 포함: `run_types_host()`가 module API만 사용하도록 조정.
- 제외: JSON protocol 전체 data model 이동.
- 제외: native IPC 직접 구현.

## 의사결정

### 결정 1: host script ownership부터 모듈 밖으로 뺀다

- **상황**: `main.rs`가 CLI, type pipeline, backend 선택, embedded host script까지
  모두 소유하고 있었다. 전체 protocol data model까지 한 번에 옮기면 변경 범위가
  커지고 TASK-075의 parity 확인과 섞인다.
- **검토한 대안**:
  - `VirtualModule`, diagnostics parser, JSON protocol까지 모두 새 module로 옮긴다.
    장점은 최종 구조에 가깝다. 단점은 한 번에 너무 많은 private type 경계가 생긴다.
  - backend enum과 embedded host script constants만 먼저 새 module로 옮긴다.
    장점은 작은 변경으로 CLI와 backend 선택 책임을 분리한다.
- **선택과 근거**: 두 번째를 선택했다. `run_types_host()`의 protocol은 아직
  `main.rs`에 남기되, backend별 script/args 선택은 `typescript_backend` module이
  담당하게 했다.

## 작업 내역

- 2026-08-19: TASK-076을 등록했다.
- 2026-08-19: `src/typescript_backend.rs`를 추가했다.
- 2026-08-19: `TYPES_HOST`, `TSGO_HOST`, `TypesBackend` enum과 script/Node args methods를
  `src/typescript_backend.rs`로 이동했다.
- 2026-08-19: `src/main.rs`가 `mod typescript_backend`와 `TypesBackend` import를 통해
  backend 선택을 사용하도록 바꿨다.

## 이슈 및 해결

없음.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`
- [x] `cargo test cli_types_tsgo_resolves_default_imports_from_rl_modules --test integration -- --nocapture`

## 결과

TypeScript backend 선택과 embedded host script ownership이 `src/typescript_backend.rs`로
분리됐다. 다음 작업은 protocol data model과 diagnostics mapping을 `src/typescript/`
하위 모듈로 옮기는 더 큰 분리 단계다.
