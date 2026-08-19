# TASK-075: TypeScript backend 선택 경계와 default export parity

- **상태**: 완료
- **시작일**: 2026-08-19
- **완료일**: 2026-08-19
- **커밋**: `8231619`

## 목적

`rlc --types`의 legacy JS host와 tsgo host 선택을 명시적인 backend 경계로 분리하고,
TASK-074의 `.d.rl.ts` shim 방식이 default export를 포함한 `.rl` import에도 동작하게
확장한다.

## 범위

- 포함: `RLC_TS_BACKEND` 해석을 backend enum/method로 분리.
- 포함: host script 선택, Node args, 실패 메시지를 backend별로 정리.
- 포함: tsgo arbitrary-extension shim의 default export parity.
- 포함: native backend default import 회귀 테스트.
- 제외: legacy JS host 제거.
- 제외: Rust가 tsgo IPC를 직접 말하는 구현.
- 제외: editor language service 전환.

## 의사결정

### 결정 1: `RLC_TS_BACKEND`는 enum 경계로 해석한다

- **상황**: TASK-073/074의 첫 구현은 `run_types_host()` 안의 `use_tsgo` boolean으로
  script 선택, Node args, 실패 메시지를 모두 처리했다. 다음 단계에서 backend adapter를
  `src/typescript/`로 옮기려면 선택 로직이 한 곳에 모여 있어야 한다.
- **검토한 대안**:
  - boolean을 유지한다. 장점은 코드가 짧다. 단점은 backend별 동작이 늘어날수록
    CLI 함수에 조건문이 퍼진다.
  - `TypesBackend` enum을 두고 script name/body, Node args를 method로 제공한다.
    장점은 현재 변경이 작고, 이후 trait/module로 빼기 쉽다.
- **선택과 근거**: `TypesBackend` enum을 추가했다. 이 단계에서는 binary 내부 경계로
  충분하고, production adapter 분리는 후속 task에서 파일 단위로 진행한다.

### 결정 2: default export shim은 generated text에 default export가 있을 때만 만든다

- **상황**: TASK-074의 `x.d.rl.ts` shim은 `export * from "./x"`만 내보냈다. TypeScript의
  `export *`는 default export를 다시 내보내지 않으므로 `import label from "./labels.rl"`
  같은 source가 native backend에서 깨질 수 있다.
- **검토한 대안**:
  - 모든 shim에 `export { default } from "./x"`를 추가한다. 장점은 단순하다. 단점은
    default export가 없는 module에서 불필요한 TS diagnostic을 만들 수 있다.
  - generated text가 default export를 포함할 때만 default re-export를 추가한다.
    장점은 named-only module의 noise를 피한다. 단점은 현재는 host의 lightweight text
    판정에 의존한다.
- **선택과 근거**: 두 번째를 선택했다. `export default`와 `export { default ... }`
  형태를 감지하고, native default import fixture로 동작을 고정했다.

## 작업 내역

- 2026-08-19: TASK-075를 등록했다.
- 2026-08-19: `src/main.rs`에 `TypesBackend` enum을 추가하고, `RLC_TS_BACKEND=tsgo`
  해석, host script 이름/본문, Node 실행 인자를 method로 분리했다.
- 2026-08-19: `run_types_host()`가 `TypesBackend`를 통해 host를 실행하도록 바꿨다.
  기존 legacy JS host는 기본값으로 유지했다.
- 2026-08-19: `src/tsgo_host.mjs`의 `.d.rl.ts` shim 생성이 default export를 감지하면
  `export { default } from "./x"`도 포함하도록 확장했다.
- 2026-08-19: `tests/integration.rs`에
  `cli_types_tsgo_resolves_default_imports_from_rl_modules`를 추가해
  `import label from "./labels.rl"`이 native backend에서 통과하고 sidecar가 string
  declaration을 내는지 확인했다.
- 2026-08-19: `docs/design/tsgo-native-backend.md`의 project graph parity 설명을
  named/default export shim으로 갱신했다.

## 이슈 및 해결

없음.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`
- [x] `cargo test cli_types_tsgo_resolves_default_imports_from_rl_modules --test integration -- --nocapture`
- [x] `cargo test types_tsgo --test cli -- --nocapture`

## 결과

`rlc --types`의 backend 선택이 `TypesBackend` 경계로 정리됐고, tsgo arbitrary-extension
shim이 default export import까지 처리한다. legacy JS host는 기본값으로 유지된다.
남은 후속 작업은 package boundary fixture와 Rust-side backend module 분리다.
