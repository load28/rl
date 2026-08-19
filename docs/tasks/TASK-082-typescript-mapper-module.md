# TASK-082: TypeScript diagnostic mapper 모듈 분리

- **상태**: 완료
- **시작일**: 2026-08-19
- **완료일**: 2026-08-19
- **커밋**: `8231619`

## 목적

`src/typescript/mod.rs`에 남아 있는 TypeScript diagnostic source mapping 책임을
`src/typescript/mapper.rs`로 분리한다. 설계 문서의 `mapper.rs` 목표에 맞춰
TypeScript UTF-16 좌표를 `.rl` source byte offset으로 되돌리는 로직을 protocol/host
runner와 분리한다.

## 범위

- 포함: `TypeOrigin`, `TypeDiagnostic`, `utf16_offset()`, `source_offset()` 이동.
- 포함: module root re-export로 기존 call site 유지.
- 제외: host result parser 분리.
- 제외: Content Mapper API adapter 도입.

## 의사결정

### 결정 1: diagnostic model과 mapper render를 함께 옮긴다

- **상황**: `TypeDiagnostic`은 host result shape이지만, 현재 유의미한 동작은
  `render()`가 source mapping을 적용하는 데 있다.
- **검토한 대안**:
  - `TypeDiagnostic`은 protocol module에 두고 `render()`만 mapper에 둔다. 장점은 raw
    protocol shape ownership이 선명하다. 단점은 지금은 protocol module이 아직 없어서
    순환적인 작은 파일이 생긴다.
  - `TypeDiagnostic`과 mapper helper를 함께 `mapper.rs`에 둔다. 장점은 현재 책임이
    응집된다. 단점은 parser가 mapper type을 직접 생성한다.
- **선택과 근거**: 두 번째를 선택한다. 후속 TASK에서 `protocol.rs`를 분리할 때 raw
  result type과 mapped diagnostic type을 다시 나눌 수 있다.

## 작업 내역

- 2026-08-19: TASK-082를 등록했다.
- 2026-08-19: `src/typescript/mapper.rs`를 추가했다.
- 2026-08-19: `TypeOrigin`, `TypeDiagnostic`, `TypeDiagnostic::render()`를
  `mapper.rs`로 이동했다.
- 2026-08-19: TypeScript UTF-16 line/column을 emitted byte offset으로 바꾸는
  `utf16_offset()`과 emitted offset을 `.rl` source offset으로 되돌리는
  `source_offset()`을 `mapper.rs`로 이동했다.
- 2026-08-19: `src/typescript/mod.rs`에서 `TypeOrigin`/`TypeDiagnostic`을 re-export하고,
  테스트 전용 helper인 `utf16_offset()`/`source_offset()`은 `#[cfg(test)]`에서만
  re-export하도록 정리했다.

## 이슈 및 해결

### 이슈 1: 일반 빌드에서 테스트 전용 helper re-export가 unused warning을 냄

- **증상**: `cargo check`가 `source_offset`과 `utf16_offset` re-export에 대해
  unused import warning을 냈다. clippy gate에서는 warning이 실패가 된다.
- **원인**: 두 helper는 `main.rs`의 unit test에서만 직접 import하고, 일반 binary
  compile path에서는 사용하지 않는다.
- **해결**: `TypeOrigin`/`TypeDiagnostic`은 항상 re-export하고,
  `source_offset`/`utf16_offset`은 `#[cfg(test)]` re-export로 제한했다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`

## 결과

TypeScript diagnostic source mapping 책임이 `src/typescript/mapper.rs`로 분리됐다.
사용자 언어 표면, CLI 동작, 방출 코드 변화가 없는 내부 구조 변경이므로
`docs/ai/rl.md` 갱신은 필요 없다. 다음 단계는 host protocol parsing/serialization을
`protocol.rs`로 분리하는 것이다.
