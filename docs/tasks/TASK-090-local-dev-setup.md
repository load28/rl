# TASK-090: 로컬 개발 setup — `scripts/setup`과 toolchain 자동 연결

- **상태**: 완료
- **시작일**: 2026-08-19
- **완료일**: 2026-08-19
- **커밋**: 이 커밋

## 목적

typed 경로는 TypeScript 7 native compiler/API(typescript-go)가 필요한데,
지금까지는 사용자가 `RLC_TSGO_ROOT` 등 환경변수를 직접 관리해야 했다.
공식 npm 배포 전까지의 로컬 테스트 설치 흐름을 명령 하나로 단순화한다:

```sh
# RL 저장소에서
./scripts/setup --tsgo-root ~/dev/typescript-go
# 테스트 프로젝트에서
pnpm add -D file:/path/to/rl/npm/rl-lang
```

테스트 프로젝트는 RL 전용 설치 명령·toolchain 설정을 전혀 알 필요가 없어야
하고, CLI와 VSCode 확장은 반드시 같은 toolchain을 써야 한다.

## 범위

- 포함:
  - `scripts/setup` — toolchain 해석·저장, typescript-go 빌드(checkout 모드),
    RL release 빌드, npm 로컬 패키지 스탬프, VSCode 확장 빌드·재설치.
  - toolchain 두 모드: **checkout**(`--tsgo-root`, 로컬 typescript-go 빌드)과
    **npm**(`--tsgo-npm`, 향후 TS7 정식 npm 배포 시 — 소비 프로젝트가
    `typescript@7`을 설치하고 rlc의 기존 해석 순서가 찾음). 두 모드 모두
    같은 setup·launcher 흐름을 쓴다.
  - `npm/rl-lang` launcher의 로컬 개발 모드(dev.js) — 저장소의
    `target/release/rlc` 실행 + `RLC_TSGO_*`를 child process에만 주입.
  - VSCode 확장 서버의 동일 toolchain 주입(`server/src/dev.ts`)과
    `file:` 설치된 dev 패키지의 rlc 탐색.
- 제외:
  - 컴파일러(Rust) 변경 — toolchain 해석 순서(`native.rs`)는 그대로.
    setup 계층은 기존 `RLC_TSGO_*` 계약 위에만 얹힌다.
  - RL 패키지에 TS7 동봉(§향후 구조) — 이 계층 전체가 그때 제거될 임시
    구조다.
  - `rl-dev update` 같은 별도 동기화 명령 — 재설치는 패키지 매니저의
    `--force` 재설치로 충분하다.

## 의사결정

### 결정 1: toolchain을 "checkout | npm" 두 모드로 모델링

- **상황**: 지금은 로컬 typescript-go 체크아웃이 필수지만, TS7이 npm에
  정식 배포되면 소비 프로젝트가 npm으로 참조해야 한다. 두 시점 모두 같은
  로컬 설치 UX(`scripts/setup` + `file:` 설치)가 유지되어야 한다.
- **검토한 대안**:
  - (A) checkout 전용으로 만들고 npm 시대에 스크립트를 다시 고친다 —
    지금은 단순하지만 전환 시점에 흐름이 바뀐다.
  - (B) `toolchain.json`에 `kind: "checkout" | "npm"`을 저장하고, npm
    모드는 "빌드·주입할 것 없음"으로 처리 — rlc의 기존 해석 4번(프로젝트
    위쪽 `node_modules`)이 이미 npm 설치 TypeScript를 찾으므로 추가
    메커니즘이 필요 없다.
- **선택과 근거**: (B). rlc는 이미 두 시대를 모두 지원한다
  (`src/typescript/native.rs`의 해석 순서). setup/launcher는 checkout
  모드에서만 env를 주입하고, npm 모드에서는 아무것도 하지 않으면 된다 —
  전환은 `./scripts/setup --tsgo-npm` 한 번이고 이후 흐름은 동일하다.
  `kind` 없이 `root`만 있는 구식 config는 checkout으로 읽어 하위 호환.

### 결정 2: 설정 파일 두 개, 각각 root 하나만 저장

- **상황**: launcher(테스트 프로젝트에 복사·링크된 npm 패키지)와 확장이
  toolchain을 찾으려면 어딘가에 경로가 기록되어야 한다. bin/api 상세
  경로를 저장하면 중복이고, 저장 안 하면 매번 계산해야 한다.
- **검토한 대안**: bin/api 절대경로까지 저장 / root만 저장하고 파생
  경로는 사용 시점에 계산.
- **선택과 근거**: root만 저장(요구사항 6). 파생 경로는 항상
  `built/local/tsgo`·`_packages/native-preview/dist/api/sync/api.js`로
  계산한다 — `native.rs`의 `BIN_IN_TREE`/`API_IN_TREE`와 같은 값.
  - `.rl-dev/toolchain.json` (RL 저장소): `{kind, root}` — setup의 toolchain 선택.
  - `npm/rl-lang/rl-dev.local.json`: `{root: <RL 저장소 절대경로>}` — 이
    패키지가 로컬 개발 설치임을 표시. pnpm의 `file:`은 디렉터리를
    복사하므로 상대경로 역참조가 불가능해 절대경로가 필요하다. 둘 다
    gitignore(머신 로컬).

### 결정 3: env 주입은 child process 한정, 컴파일러는 무변경

- **상황**: CLI·확장·엔진이 같은 typescript-go를 쓰게 하는 방법.
- **검토한 대안**:
  - (A) rlc 자신이 `.rl-dev/toolchain.json`을 읽게 한다(실행 파일 위치
    기준) — 주입이 아예 필요 없지만, 컴파일러의 해석 순서(규범 문서
    `cli.md`)가 임시 개발 설정에 오염되고, 나중에 제거할 때 컴파일러를
    다시 고쳐야 한다.
  - (B) launcher(dev.js)와 확장 서버(dev.ts)가 `RLC_TSGO_*`를 **스폰하는
    child process에만** 넣는다 — 셸 프로파일·VSCode 환경 무변경, 컴파일러
    무변경.
- **선택과 근거**: (B). 임시 구조는 컴파일러 밖에 격리한다(§향후 제거).
  확장 서버는 rlc 스폰 지점 전부(엔진 `--server`, `--check`/`--symbols`/
  `--check-types` one-shot, `--types` 사이드카)에 같은 env를 준다 —
  해석 로직은 `server/src/dev.ts` 한 곳. 빌드 안 된 체크아웃은 주입하지
  않는다(`RLC_TSGO_ROOT`가 설정되면 rlc의 폴백 해석이 멈추므로, 있는
  그대로 두는 쪽이 동작을 보존).

### 결정 4: npm 패키지는 기존 `npm/rl-lang`을 그대로 사용

- **상황**: 제안서 예시는 `file:.../rl/npm` 설치를 보여주지만 실제
  패키지는 `npm/rl-lang`이다(`npm/scripts/`가 배포 스크립트로 같은
  디렉터리를 참조).
- **검토한 대안**: 패키지를 `npm/` 루트로 옮기기 / 기존 위치 유지.
- **선택과 근거**: 유지. 배포 스크립트(`stamp-version.mjs` 등)와
  `repository.directory` 메타데이터가 `npm/rl-lang`을 가리키고 있고,
  설치 명령은 setup이 성공 출력에서 정확한 경로
  (`pnpm add -D file:<repo>/npm/rl-lang`)로 알려주므로 사용자 부담이 없다.

### 결정 5: dev 스탬프를 launcher 탐색 순서의 두 번째로

- **상황**: 게시된 설치와 로컬 개발 설치가 같은 launcher 코드를 쓴다.
- **선택과 근거**: `RLC_BINARY`(명시적 오버라이드) > dev 스탬프 >
  플랫폼 패키지. 스탬프가 있는데 바이너리가 없으면(빌드 전/스탬프만
  남음) 조용히 다른 rlc로 넘어가지 않고 `./scripts/setup`을 안내하며
  실패한다 — 잘못된 컴파일러로 조용히 도는 것이 더 나쁘다.
  `rl-dev.local.json`을 package.json `files`에 넣어 pnpm의 pack 기반
  `file:` 설치에도 포함되게 했다(배포 시에는 clean checkout이라 파일이
  없고, npm pack은 files 목록의 부재 항목을 무시하므로 게시본에 새지
  않는다).

### 결정 6: VSCode 확장 설치는 항상 uninstall → install

- **상황**: 확장 버전이 고정(0.1.0)이라 덮어쓰기 설치는 VSCode가 캐시를
  유지할 수 있다.
- **선택과 근거**: `code --list-extensions`로 설치 여부를 확인해 있으면
  `--uninstall-extension` 후 `--install-extension`(요구사항 12). `code`
  CLI가 없는 머신(헤드리스 등)에서는 설치만 건너뛰고 수동 설치 명령을
  안내한다 — vsix 빌드 실패는 여전히 전체 실패다. vsce가 LICENSE 부재
  시 대화형 프롬프트로 멈추므로 저장소 LICENSE를 `editors/vscode/`에
  복사해 커밋했다(확장이 자기 라이선스를 동봉하는 것이 어차피 올바르다).

## 작업 내역

- 2026-08-19: 저장소 구조 조사 — `npm/rl-lang` launcher(`bin/rlc.js`,
  `index.js`), 확장 서버의 rlc 스폰 지점(`engine.ts`/`rlc.ts`/`sidecar.ts`),
  `native.rs`의 toolchain 해석 순서와 `BIN_IN_TREE`/`API_IN_TREE` 확인.
- `scripts/setup` 신규 작성: 인자 파싱(`--tsgo-root`/`--tsgo-npm`/재실행) →
  root 절대경로 정규화(`~` 확장 포함) → `.rl-dev/toolchain.json` 저장 →
  checkout 모드면 `go build -o built/local/tsgo ./cmd/tsgo` + (lockfile이
  더 새것일 때만) `npm ci` + `npx tsc -b _packages/native-preview` → bin/api
  산출물 검증 → `cargo build --release` + rlc 검증 →
  `npm/rl-lang/rl-dev.local.json` 스탬프 → 확장 `npm ci`(stale 시)/`tsc -b`/
  `vsce package --no-dependencies` → 기존 확장 uninstall 후 install →
  성공 요약 출력. git 상태는 어떤 단계에서도 건드리지 않는다.
- `npm/rl-lang/dev.js` 신규: `devEnvironment()` — 스탬프에서 RL root를
  읽어 release rlc와 `RLC_TSGO_*` env(checkout 모드 한정)를 파생.
  `bin/rlc.js`가 이를 사용(env는 spawnSync의 child에만), `index.js`
  `binaryPath()`도 dev 바이너리를 두 번째 순위로 해석(unplugin 등 API
  소비자도 동일 동작). `package.json` `files`에 `dev.js`·
  `rl-dev.local.json` 추가.
- `editors/vscode/server/src/dev.ts` 신규: `toolchainEnv`(컴파일러
  위치에서 위로 `.rl-dev/toolchain.json` 탐색)·`rlcSpawnEnv`·
  `devPackageCompiler`(워크스페이스 `node_modules/rl-lang` 스탬프).
  `rlc.ts`(탐색 순서에 dev 패키지 추가 + one-shot 3곳 env), `engine.ts`
  (`--server` 스폰 env), `sidecar.ts`(`--types` env)에 연결.
- 테스트: `server/src/test/dev.test.ts` 7건 — checkout env 계산, 구식
  config(root만) 호환, npm 모드 무주입, 미빌드 체크아웃 무주입, PATH
  컴파일러 무주입, env 레이어링, dev 패키지 rlc 탐색(스탬프 stale 포함).
- launcher 수동 검증(가짜 rlc/tsgo 트리): checkout 모드에서 `RLC_TSGO_*`
  3종이 child에 주입, npm 모드에서 미주입, 스탬프만 있고 바이너리가 없으면
  안내와 함께 실패, 스탬프 없는 게시형 설치는 기존 에러 그대로.
- `scripts/setup` 수동 검증: `bash -n`, 미설정 실행/존재하지 않는 root/
  플래그 충돌/`--help` 각각 기대 메시지로 종료. `./scripts/setup --tsgo-npm`
  전체 실행으로 release 빌드→스탬프→확장 빌드→vsix 패키징까지 통과
  (`code` CLI 없는 환경이라 설치는 안내와 함께 skip — 의도된 동작).
- 문서: `CONTRIBUTING.md`에 "로컬 개발 환경" 절, `editors/vscode/README.md`
  (컴파일러 탐색 순서·toolchain 주입·setup 언급), `npm/rl-lang/README.md`
  (local development install 절). `.gitignore`에 `.rl-dev/`와
  `npm/rl-lang/rl-dev.local.json` 추가.
- AI 제공 문서(`docs/ai/rl.md`) 확인: 언어 표면·CLI 동작 무변경(컴파일러
  는 건드리지 않음)이므로 갱신 불필요. 레퍼런스(`docs/reference/`)도 동일
  이유로 무변경 — toolchain 해석 순서는 그대로고, 이 계층은 그 위의 임시
  개발 편의 구조다.

## 이슈 및 해결

### 이슈 1: pnpm `file:` 설치가 저장소로의 상대 참조를 끊음

- **증상**: pnpm은 `file:` 디렉터리를 저장소 밖으로 복사/팩하므로, 설치된
  패키지에서 `../..`로 RL 저장소를 되짚을 수 없다.
- **원인**: pnpm의 `file:` 프로토콜 동작(팩 후 설치). npm은 심링크라
  동작이 다르지만 둘 다 지원해야 한다.
- **해결**: setup이 RL root **절대경로**를 `rl-dev.local.json`에 스탬프하고
  `files` 목록에 넣어 pack에 포함시킨다. 파일 부재(게시 빌드)는 npm pack이
  무시하므로 게시본에는 존재하지 않는다.

### 이슈 2: vsce의 LICENSE 부재 대화형 프롬프트

- **증상**: `vsce package`가 확장 디렉터리에 LICENSE가 없으면 계속할지
  대화형으로 물어 비대화형 setup이 멈출 수 있다.
- **원인**: vsce의 기본 동작.
- **해결**: 저장소 MIT LICENSE를 `editors/vscode/LICENSE`로 복사해 커밋
  (확장 패키지에 라이선스가 동봉되는 부수 효과도 올바름).

### 이슈 3: 빌드 안 된 checkout을 env로 지목하면 rlc 폴백이 멈춤

- **증상**: `RLC_TSGO_ROOT`가 설정되면 rlc는 그 트리에서 실패를 보고하고
  다른 해석으로 넘어가지 않는다(의도된 계약).
- **원인**: `native.rs` 해석 순서 2번은 명시 지정이라 폴백하지 않는다.
- **해결**: launcher·확장 모두 bin/api 산출물이 실재할 때만 주입한다.
  setup 직후에는 항상 실재하고, 사용자가 typescript-go를 지웠다면 rlc가
  스스로의 순서(프로젝트 node_modules 등)로 해석한다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`
- [x] `editors/vscode`: `npx tsc -b` + `npm test` (신규 dev.test.ts 7건 포함
  전건 통과; rlc가 PATH에 없어 컴파일러 의존 테스트는 기존대로 skip)

## 결과

로컬 개발 설치가 명령 하나로 준비된다: `./scripts/setup --tsgo-root <경로>`
(또는 TS7 npm 시대의 `--tsgo-npm`), 테스트 프로젝트에서는
`pnpm add -D file:<repo>/npm/rl-lang`만. CLI launcher와 VSCode 확장이
같은 `.rl-dev/toolchain.json`을 읽어 동일 toolchain을 쓰고, 환경변수는
스폰되는 rlc child process에만 존재한다. 변경 파일: `scripts/setup`(신규),
`npm/rl-lang/{dev.js(신규),bin/rlc.js,index.js,package.json,README.md}`,
`editors/vscode/server/src/{dev.ts(신규),rlc.ts,engine.ts,sidecar.ts}`,
`editors/vscode/server/src/test/dev.test.ts`(신규),
`editors/vscode/{LICENSE(신규),README.md}`, `CONTRIBUTING.md`, `.gitignore`,
태스크 문서. 이 계층 전체는 임시 구조로, RL 패키지가 검증된 TS7을 동봉하면
`scripts/setup`·`.rl-dev/`·`dev.js`·`dev.ts`를 함께 제거한다.
