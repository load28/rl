# TASK-078: TypeScript backend job/probe model 분리

- **상태**: 완료
- **시작일**: 2026-08-19
- **완료일**: 2026-08-19
- **커밋**: `8231619`

## 목적

TASK-077에서 output-side protocol result model을 분리한 데 이어, input-side host job
model도 `src/typescript_backend.rs`로 이동한다. CLI 진입점은 source tree 수집과 sidecar
쓰기 흐름에 집중하고, TypeScript backend module이 host와 주고받는 job/result shape를
함께 소유하게 만든다.

## 범위

- 포함: `VirtualModule`, literal match probe, `val` probe model 이동.
- 포함: emitted/source offset → TypeScript UTF-16 probe position 변환 이동.
- 포함: host job serialization 이동.
- 제외: host process 실행 책임 이동.
- 제외: sidecar file writing 이동.
- 제외: native IPC 직접 구현.

## 의사결정

### 결정 1: backend-neutral job model을 같은 module에 둔다

- **상황**: `main.rs`는 `VirtualModule`, literal/`val` probe model, host job JSON
  serialization을 갖고 있었다. TASK-077 이후 result model은 module로 이동했지만,
  input model이 CLI에 남아 있어 protocol ownership이 반쪽이었다.
- **검토한 대안**:
  - `src/typescript/protocol.rs` 같은 하위 module tree를 지금 만든다. 장점은 장기
    설계 문서와 구조가 맞다. 단점은 현재 파일 하나짜리 backend module에서 곧장
    여러 파일로 나뉘어 변경 범위가 커진다.
  - 기존 `src/typescript_backend.rs`에 job/probe model을 먼저 모은다. 장점은 작은
    이동으로 protocol ownership을 고정한다. 단점은 파일이 커진다.
- **선택과 근거**: 두 번째를 선택한다. 이번 작업은 동작 변경 없는 소유권 이동이고,
  하위 module tree 도입은 native IPC adapter가 실제로 생길 때 분리하는 편이 낫다.

## 작업 내역

- 2026-08-19: TASK-078을 등록했다.
- 2026-08-19: `VirtualModule`, `LiteralCheck`, `ValCheck`를
  `src/typescript_backend.rs`로 이동했다. `main.rs`는 해당 model을 import해 source tree
  수집과 sidecar 쓰기에만 사용한다.
- 2026-08-19: literal match probe 생성 로직을 `literal_checks()`로 이동했다. emitted
  byte offset을 TypeScript UTF-16 offset으로 바꾸는 helper도 함께 backend module로
  이동했다.
- 2026-08-19: `val` typed mutation probe 생성 로직을 `val_checks()`로 이동했다. 기존
  `ValMethodCall` field mapping(`name`, `name_end`, `method`, `binding`, `offset`)을
  유지했다.
- 2026-08-19: host job serialization(`types_job()`), compiler option JSON,
  probe literal JSON serialization을 `src/typescript_backend.rs`로 이동했다.
- 2026-08-19: `src/main.rs`에서 더 이상 직접 필요하지 않은 `Literal`과
  `ValMethodCall` import를 제거했다.

## 이슈 및 해결

없음.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`

## 결과

TypeScript backend module이 host input job model과 output result model을 모두 소유하게
됐다. `main.rs`에는 host process 실행, source tree 수집, sidecar file writing만 남았다.
사용자 언어 표면, CLI 동작, 방출 코드 변화가 없는 내부 리팩터링이므로
`docs/ai/rl.md` 갱신은 필요 없다. 다음 단계는 host process execution을 module API로
감싸거나, 설계 문서의 `src/typescript/` 하위 module tree로 분리하는 작업이다.
