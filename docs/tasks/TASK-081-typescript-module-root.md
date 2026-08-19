# TASK-081: TypeScript backend module root 이동

- **상태**: 완료
- **시작일**: 2026-08-19
- **완료일**: 2026-08-19
- **커밋**: `8231619`

## 목적

TASK-077~080으로 `src/typescript_backend.rs`에 모은 TypeScript backend 경계를 설계
문서의 목표 배치인 `src/typescript/` 계층으로 옮긴다. 이후 `backend.rs`,
`protocol.rs`, `mapper.rs`, `semantic.rs` 같은 하위 모듈로 나누기 위한 root module을
먼저 만든다.

## 범위

- 포함: `src/typescript_backend.rs`를 `src/typescript/mod.rs`로 이동.
- 포함: `main.rs` module 선언/import 갱신.
- 포함: embedded host script include path 갱신.
- 제외: 하위 파일 단위 split.
- 제외: trait 도입 또는 native IPC 직접 구현.

## 의사결정

### 결정 1: root module 이동을 먼저 하고 세부 split은 후속 태스크로 둔다

- **상황**: 현재 `src/typescript_backend.rs`는 600줄 이상이며 backend 선택, protocol,
  mapper, host runner를 모두 담고 있다. 설계 문서는 `src/typescript/` 하위 module tree를
  목표로 한다.
- **검토한 대안**:
  - 한 번에 `backend.rs`, `protocol.rs`, `mapper.rs`, `semantic.rs`로 모두 나눈다.
    장점은 목표 구조에 바로 도달한다. 단점은 대형 이동과 visibility 조정이 한 커밋에
    섞인다.
  - 먼저 `src/typescript/mod.rs`로 root만 옮긴다. 장점은 작은 변경으로 public module
    path를 설계와 맞추고, 후속 split의 diff를 작게 만든다.
- **선택과 근거**: 두 번째를 선택한다. 동작 변화 없는 파일 이동을 먼저 커밋해 이후
  세부 split에서 실제 ownership 조정을 더 잘 볼 수 있게 한다.

## 작업 내역

- 2026-08-19: TASK-081을 등록했다.
- 2026-08-19: `src/typescript_backend.rs`를 `src/typescript/mod.rs`로 이동했다.
- 2026-08-19: `src/main.rs`의 module 선언을 `mod typescript;`로 바꾸고 import path를
  `typescript::{...}`로 갱신했다.
- 2026-08-19: `src/typescript/mod.rs` 안의 embedded host script include path를
  `../types_host.mjs`, `../tsgo_host.mjs`로 조정했다.
- 2026-08-19: 테스트 모듈의 diagnostic helper import도 `crate::typescript` 경로로
  갱신했다.

## 이슈 및 해결

없음.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`

## 결과

TypeScript backend root가 설계 문서의 목표 위치인 `src/typescript/mod.rs`로 이동했다.
아직 하위 module split은 하지 않았고, 다음 태스크에서 `protocol.rs`, `mapper.rs`,
`host.rs` 등으로 책임을 나눌 수 있다. 사용자 언어 표면, CLI 동작, 방출 코드 변화가
없는 내부 파일 이동이므로 `docs/ai/rl.md` 갱신은 필요 없다.
