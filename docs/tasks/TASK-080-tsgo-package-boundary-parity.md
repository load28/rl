# TASK-080: tsgo native backend package boundary parity

- **상태**: 완료
- **시작일**: 2026-08-19
- **완료일**: 2026-08-19
- **커밋**: 미커밋

## 목적

tsgo native backend가 `.rl`/`.ts` mixed project 안의 상대 경로만이 아니라
`node_modules` package boundary도 기존 TypeScript resolver와 같은 방식으로 통과하는지
테스트로 고정한다. native backend 전환은 프로젝트 내부 VFS뿐 아니라 사용자가 설치한
패키지 선언을 함께 읽을 수 있어야 한다.

## 범위

- 포함: `RLC_TS_BACKEND=tsgo` 통합 테스트 추가.
- 포함: project-local `node_modules/<pkg>`의 `package.json`/`.d.ts` 선언 resolution 확인.
- 제외: npm install 또는 외부 네트워크 의존 fixture.
- 제외: package `exports` 조건 전부의 exhaustive coverage.

## 의사결정

### 결정 1: 실제 `node_modules` fixture를 테스트 안에서 직접 만든다

- **상황**: package boundary parity를 보려면 host가 overlay VFS 밖의 project-local
  package files를 읽는지 확인해야 한다.
- **검토한 대안**:
  - 실제 npm package를 설치해 테스트한다. 장점은 현실성이 높다. 단점은 네트워크와
    lockfile 상태에 의존한다.
  - tmpdir 안에 최소 `node_modules/domain-lib` package를 직접 만든다. 장점은
    deterministic하고 외부 의존이 없다. 단점은 package exports 조건 전체를 덮지는
    않는다.
- **선택과 근거**: 두 번째를 선택한다. 이번 목표는 package boundary의 기본 resolver
  접근성 확인이고, 세부 exports 조건은 후속 fixture로 확장할 수 있다.

## 작업 내역

- 2026-08-19: TASK-080을 등록했다.
- 2026-08-19: `tests/integration.rs`에
  `cli_types_tsgo_resolves_project_local_package_types`를 추가했다.
- 2026-08-19: tmpdir 안에 `node_modules/domain-lib/package.json`과 `index.d.ts`를 직접
  만든 뒤, `.rl` source가 `import type { Money } from "domain-lib"`를 통해 package
  type을 참조하도록 fixture를 구성했다.
- 2026-08-19: `RLC_TS_BACKEND=tsgo` / `RLC_TSGO_ROOT=<checkout>`로 `rlc --types src`를
  실행하고, 생성된 `.rl-types/price.rl.d.ts`가 `domain-lib` import와 `Money` type을
  보존하는지 확인했다.

## 이슈 및 해결

없음.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test cli_types_tsgo_resolves_project_local_package_types --test integration -- --nocapture`
- [x] `cargo test`

## 결과

tsgo native backend가 overlay VFS에 없는 project-local `node_modules` package declarations도
읽어 `.rl` source declaration emit에 반영한다는 점을 통합 테스트로 고정했다. 이 변경은
테스트 coverage 추가이며 사용자 언어 표면, CLI 동작, 방출 규칙 변경이 아니므로
`docs/ai/rl.md` 갱신은 필요 없다.
