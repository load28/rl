# 기여 가이드

## 개발 환경

- Rust stable (MSRV: `Cargo.toml`의 `rust-version` 참조)
- 선택: Node.js + `typescript` (`npm i -g typescript`) — 없으면 tsc/node 통합
  테스트가 자동으로 skip됩니다. 온전한 검증을 위해 설치를 권장합니다.

```sh
cargo build
cargo test
```

## 절대 불변 원칙

어떤 변경도 이 두 계약을 깨뜨릴 수 없습니다 (자세한 내용은 [`CLAUDE.md`](./CLAUDE.md)):

1. 모든 유효한 TypeScript 파일은 그대로 유효한 `.rl` 파일이다 (바이트 단위 통과).
2. rl 수준 에러는 rlc가 직접 보고하고, 방출 코드는 타입 트릭 없는 순수
   TypeScript다 — rlc가 방출한 코드가 tsc 에러를 만들면 안 된다.

## 작업 절차 (필수)

모든 작업은 태스크 문서로 관리됩니다:

1. `docs/tasks/INDEX.md`에서 다음 번호를 확인하고 `docs/tasks/TEMPLATE.md`로
   태스크 문서를 만든 뒤 INDEX에 등록합니다.
2. 작업 중 결정·문제·범위 변경을 태스크 문서에 기록합니다.
3. 완료 시 검증 결과를 기록하고 상태를 갱신합니다.
4. 커밋 메시지는 태스크 ID로 시작합니다: `TASK-012: ...`.

## 머지 전 검증 게이트

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

CI가 동일한 게이트를 강제합니다. 새 기능에는 반드시 테스트를 추가하세요:

- 출력 형태 → `tests/compile.rs`
- TS 통과 계약 → `tests/passthrough.rs`
- 타입/런타임 의미 → `tests/integration.rs`

언어 표면(구문, 판별 규칙, 방출 코드, 에러 메시지, CLI 동작)을 바꾸는 변경은
규범 레퍼런스인 [`docs/reference/`](./docs/reference/)의 해당 문서를 함께
갱신해야 합니다. 공개 Rust API를 바꾸면 rustdoc(과 doctest)도 갱신하세요 —
doctest는 `cargo test`에서 함께 실행됩니다.
