# 기여 가이드

## 개발 환경

- Rust stable (MSRV: `Cargo.toml`의 `rust-version` 참조)
- 선택: Node.js + `typescript` (`npm i -g typescript`) — 없으면 tsc/node 통합
  테스트가 자동으로 skip됩니다. 온전한 검증을 위해 설치를 권장합니다.

```sh
cargo build
cargo test
```

## 로컬 개발 환경 (`scripts/setup`)

typed 경로(`--check-types`/`--types`/`--server`)까지 포함해 전부 로컬에서
돌려 보려면 setup 스크립트 하나면 됩니다. TypeScript 7 toolchain은 두 방식
중 하나로 연결합니다:

```sh
# A. 로컬 typescript-go 체크아웃을 쓸 때 (루트 경로 하나만 전달)
./scripts/setup --tsgo-root ~/dev/typescript-go

# B. TypeScript 7을 npm으로 쓸 때 (소비 프로젝트가 typescript@7을 설치)
./scripts/setup --tsgo-npm

# 이후에는 저장된 설정을 그대로 재사용
./scripts/setup
```

setup은 선택을 `.rl-dev/toolchain.json`에 저장하고(머신 로컬, 커밋 금지),
checkout 모드면 **현재 체크아웃된** typescript-go를 그 자리에서 빌드한 뒤
(git pull/checkout 같은 저장소 상태 변경은 절대 하지 않습니다 — 두 저장소의
갱신은 사용자가 직접), 현재 RL 체크아웃을 release 빌드하고, VSCode 확장을
빌드해 재설치(기존 설치는 삭제 후 설치)합니다.

테스트 프로젝트에서는 RL 전용 명령 없이 일반 패키지 매니저로 설치합니다:

```sh
pnpm add -D file:/path/to/rl/npm/rl-lang   # 재빌드 후에는 --force로 재설치
pnpm rlc --check-types src
```

launcher(`npm/rl-lang/bin/rlc.js`)가 저장소의 `target/release/rlc`를 실행하며,
checkout 모드의 `RLC_TSGO_*` 환경변수는 **그 child process에만** 주입됩니다 —
셸 프로파일은 건드리지 않습니다. VSCode 확장도 같은 `toolchain.json`을 읽어
CLI와 동일한 toolchain을 씁니다. 이 계층 전체는 임시 구조입니다: RL 패키지가
검증된 TypeScript 7을 직접 포함하게 되면 `scripts/setup`·`.rl-dev/`·
`npm/rl-lang/dev.js`를 함께 제거합니다 (`docs/tasks/TASK-090`).

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

언어 표면(구문, 판별 규칙, 에러 메시지, CLI 동작)을 바꾸는 변경은 컴파일러에
내장되는 [`docs/ai/rl.md`](./docs/ai/rl.md)를 함께 갱신해야 합니다. 사용자가
처음 접하는 기능이면 영문·한글 README에도 반영하세요. 공개 Rust API를 바꾸면
rustdoc과 doctest도 갱신하세요. doctest는 `cargo test`에서 함께 실행됩니다.
