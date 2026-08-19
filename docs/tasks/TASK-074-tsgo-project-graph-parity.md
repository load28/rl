# TASK-074: tsgo native backend project graph parity 1단계

- **상태**: 완료
- **시작일**: 2026-08-19
- **완료일**: 2026-08-19
- **커밋**: `8231619`

## 목적

TASK-073에서 붙인 `RLC_TS_BACKEND=tsgo` opt-in path를 실제 project graph parity로
전진시킨다. 첫 목표는 기존 JS host가 처리하던 `@rl/std`, hand-written `.ts`,
relative `.rl` imports, typed literal/`val` query fixture를 tsgo backend에서도 같은
테스트로 검증할 수 있게 만드는 것이다.

## 범위

- 포함: tsgo host의 relative `.rl` import resolution parity 1단계.
- 포함: 기존 `--types` fixture를 재사용하는 opt-in native backend 테스트 추가.
- 포함: tsgo checkout이 없을 때 테스트를 skip하는 guard 추가.
- 포함: TASK-073에서 남긴 experimental host의 known limitation 갱신.
- 제외: legacy JS host 제거.
- 제외: editor language service 전환.
- 제외: Rust native IPC 직접 구현.

## 의사결정

### 결정 1: relative `.rl` import는 VFS의 arbitrary-extension shim으로 처리한다

- **상황**: legacy `types_host.mjs`는 JS Compiler API의 `resolveModuleNames` hook으로
  `"./level.rl"`을 generated virtual `"./level.ts"`에 직접 매핑한다. tsgo source API의
  public surface에는 같은 hook이 없고, 대신 VFS callback과 `allowArbitraryExtensions`
  compiler option이 있다.
- **검토한 대안**:
  - host 안에서 generated TS source의 import specifier를 `.rl`에서 `.ts`로 바꾼다.
    장점은 resolver가 쉽게 통과한다. 단점은 declaration emit이 source specifier를
    잃어 sidecar 계약을 깨뜨린다.
  - `.rl` source path 자체를 generated TS text로 overlay한다. 장점은 단순하다.
    단점은 TypeScript resolver가 arbitrary source extension을 그대로 source file로
    열지 않아 `Cannot find module './level.rl'`가 계속 발생했다.
  - TS arbitrary-extension 규칙에 맞춰 `level.d.rl.ts` shim을 VFS에 추가하고,
    그 shim이 generated `level.ts`를 re-export하게 한다.
- **선택과 근거**: 세 번째를 선택했다. 이 방식은 source specifier `"./level.rl"`을
  declaration emit에 그대로 남기면서, tsgo project graph가 imported declarations를
  실제로 resolve하게 한다. `RLC_TS_BACKEND=tsgo` mixed project fixture에서 확인했다.

### 결정 2: native tests는 tsgo가 없으면 skip한다

- **상황**: tsgo는 아직 npm dependency가 아니라 별도 `typescript-go` checkout/build에
  의존한다. 전체 CI가 항상 이 checkout을 갖는다고 가정할 수 없다.
- **검토한 대안**:
  - tests가 항상 tsgo를 요구하게 한다. 장점은 coverage가 강하다. 단점은 일반 개발
    환경과 CI에서 불필요하게 깨진다.
  - `RLC_TSGO_ROOT` 또는 로컬 기본 checkout이 있을 때만 native tests를 실행한다.
- **선택과 근거**: skip guard를 둔다. legacy tests는 계속 항상 기존 조건으로 돌고,
  native parity는 tsgo가 준비된 환경에서만 추가로 검증한다.

## 작업 내역

- 2026-08-19: TASK-073 이후 전체 전환 계획을 계속 수행하기 위해 TASK-074를 등록했다.
- 2026-08-19: `src/tsgo_host.mjs`에서 `job.rlModules`를 순회하며 각 `.rl` source path를
  generated virtual `.ts` text로 overlay하고, `allowArbitraryExtensions: true`를
  compiler options에 추가했다.
- 2026-08-19: `.rl` source overlay만으로는 `Cannot find module './level.rl'`가
  해결되지 않음을 확인했다.
- 2026-08-19: 각 `.rl` source path에 대응하는 `x.d.rl.ts` shim을 overlay에 추가했다.
  shim은 generated virtual `.ts` module을 extension 없는 specifier로 re-export한다.
- 2026-08-19: 임시 mixed project fixture에서
  `RLC_TS_BACKEND=tsgo cargo run -- --types ...`가 `level.rl.d.ts`,
  `notice.rl.d.ts`를 만들고 `notice` sidecar가 `from "./level.rl"`을 보존함을 확인했다.
- 2026-08-19: `tests/integration.rs`에
  `cli_types_tsgo_sidecars_typecheck_the_source_tree`를 추가했다. 기존 legacy fixture와
  같은 source tree로 `@rl/std`, hand-written `.ts`, relative `.rl` import sidecar를
  native backend에서 확인한다.
- 2026-08-19: `tests/cli.rs`에 native backend typed query tests를 추가했다. finite
  literal union 누락, `Map#set` mutation error, user-defined `set` no error를
  `RLC_TS_BACKEND=tsgo`로 확인한다.
- 2026-08-19: `docs/design/tsgo-native-backend.md`의 project graph parity 절을
  `x.d.rl.ts` shim 기반 1차 해결로 갱신하고, default export parity는 후속 fixture로
  남겼다.

## 이슈 및 해결

### 이슈 1: `.rl` source overlay만으로는 resolver가 import를 찾지 못함

- **증상**: native backend mixed fixture가
  `Cannot find module './level.rl' or its corresponding type declarations.`를 보고했다.
- **원인**: tsgo/TypeScript resolver는 arbitrary source extension 파일을 그대로
  TypeScript source로 채택하지 않고, arbitrary-extension declaration naming 규칙을
  따른다.
- **해결**: `allowArbitraryExtensions`를 켠 뒤 `level.d.rl.ts` shim을 VFS overlay에
  추가해 `"./level.rl"` import가 generated virtual module declarations를 보게 했다.

### 이슈 2: native test가 불필요하게 `tsc` 존재를 요구함

- **증상**: targeted native integration test가 `skipping: tsc/node not available`로
  실제 tsgo path를 실행하지 않았다.
- **원인**: 기존 mixed source-tree test의 `require_toolchain!()`를 그대로 복사해,
  native backend에는 필요 없는 `tsc`까지 guard에 포함했다.
- **해결**: `require_node!()`와 `require_tsgo_native_backend!()`를 분리해 native test가
  Node와 built `typescript-go` checkout만 요구하도록 고쳤다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`
- [x] `cargo test types_tsgo --test cli -- --nocapture`
- [x] `cargo test cli_types_tsgo_sidecars_typecheck_the_source_tree --test integration -- --nocapture`

## 결과

tsgo native backend가 relative `.rl` import를 포함한 mixed project fixture에서
declaration sidecar를 생성하고, source specifier를 보존한다. typed literal match와
typed `val` query도 native backend 전용 tests로 고정했다. 남은 후속 작업은 Phase 2의
Rust-side backend abstraction과 Phase 3의 default export/package-boundary fixture다.
