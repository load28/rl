# TASK-048: npm 패키징 — `npm install rl-lang`으로 rlc 설치

- **상태**: 완료
- **시작일**: 2026-08-17
- **완료일**: 2026-08-17
- **커밋**: —

## 목적

지금 rlc를 쓰려면 Rust 툴체인을 깔고 `cargo install --path .`을 해야 한다.
rl의 사용자는 TypeScript 개발자이므로, TypeScript처럼
`npm install --save-dev rl-lang` 한 번으로 `rlc` CLI가 설치되게 한다.

## 범위

- 포함:
  - npm 메인 패키지 `rl-lang` (bin `rlc` 런처 + `binaryPath()` 공개 API)
  - 플랫폼별 프리빌트 바이너리 패키지 5종
    (`rl-lang-linux-x64`, `rl-lang-linux-arm64`, `rl-lang-darwin-x64`,
    `rl-lang-darwin-arm64`, `rl-lang-win32-x64`) — optionalDependencies로 연결
  - 릴리스 워크플로 (`.github/workflows/release.yml`) — `v*` 태그 푸시 시
    5개 타깃 빌드 → npm 배포 → GitHub Release 자산 업로드
  - `unplugin-rl`이 설치된 `rl-lang`의 바이너리를 자동으로 찾도록 연동
  - README / `docs/reference/cli.md` 설치 문서 갱신
- 제외:
  - 실제 npm 배포 실행 (저장소 시크릿 `NPM_TOKEN` 등록과 태그 푸시는
    저장소 소유자의 몫)
  - crates.io 배포
  - VSCode 확장의 rlc 자동 탐색 (별도 태스크로)
  - WASM 빌드 (아래 결정 1에서 배제)

## 의사결정

### 결정 1: 배포 방식 — 플랫폼별 프리빌트 바이너리 (esbuild/swc 방식)

- **상황**: Rust 바이너리를 npm으로 배포하는 방식을 골라야 한다.
- **검토한 대안**:
  - **A. postinstall에서 GitHub Release 바이너리 다운로드**: npm 패키지가
    1개라 관리가 단순하지만, install 스크립트는 보안 정책으로 차단되는
    환경이 많고(`--ignore-scripts`) 오프라인/프록시 환경에서 설치가 깨진다.
  - **B. WASM(wasm32-wasip1) 단일 패키지**: 플랫폼 무관 단일 패키지가
    되지만, `--types`가 tsc를 자식 프로세스로 실행하고 `-w`가 파일 감시를
    쓰므로 WASI로는 CLI 전체를 담을 수 없다. 성능도 네이티브보다 느리다.
  - **C. 플랫폼별 바이너리 패키지 + optionalDependencies**: esbuild·swc·
    Biome이 쓰는 현행 표준. npm이 os/cpu가 맞는 패키지만 내려받고, install
    스크립트가 전혀 없다. 패키지가 6개가 되는 게 비용.
- **선택과 근거**: C. install 스크립트 없이 모든 기능(프로세스 생성, 파일
  감시 포함)이 그대로 동작하는 유일한 방식이고, 대형 프로젝트들이 검증한
  경로다. 패키지 6개 관리는 릴리스 워크플로가 전부 자동화하므로 실질 비용이
  낮다.

### 결정 2: 패키지 이름 — `rl-lang` (bin은 `rlc`)

- **상황**: npm 이름을 정해야 한다. TypeScript의 유례(패키지 `typescript`,
  bin `tsc`)를 따라 언어 이름 계열이 바람직하다.
- **검토한 대안**: `rl`(readline 계열 기존 패키지 존재), `rlc`(2018-07-27
  unpublish 이력 — npm은 unpublish된 이름의 재사용을 제한한다),
  `@rl/...` 스코프(스코프 소유 불가 — `@rl/std`는 컴파일러가 실체화하는
  가상 지정자라 npm 소유가 필요 없지만 실제 배포 이름으로는 쓸 수 없다),
  `rl-lang`(비어 있음 — `npm view rl-lang` → 404 Not Found로 확인).
- **선택과 근거**: `rl-lang`. 확인 시점(2026-08-17)에 메인·플랫폼 5종
  전부(`npm view rl-lang-linux-x64` 등 → 404) 사용 가능했고, 언어 이름을
  담으면서 스코프 소유 문제가 없다.

### 결정 3: 리눅스 빌드는 musl 정적 링크

- **상황**: glibc 타깃으로 빌드하면 빌드 러너의 glibc 버전(ubuntu-latest =
  2.39+)보다 오래된 배포판에서 실행이 깨진다.
- **검토한 대안**: 오래된 러너(글리브시 하한 확보 — 러너 수명에 종속됨) /
  musl 정적 링크(어느 배포판에서든 동작, swc·Biome이 같은 방식).
- **선택과 근거**: musl 정적 링크. C 의존성이 `psm`/`stacker`(cc로 컴파일되는
  어셈블리)뿐이라 `musl-tools`(musl-gcc)로 충분함을 Cargo.lock 조사로
  확인했다. 정적 바이너리는 glibc 배포판에서도 그대로 돌므로 플랫폼 패키지에
  libc 구분(패키지 분리)이 필요 없다.

### 결정 4: 버전은 릴리스 태그에서 스탬프

- **상황**: npm 패키지 6개 + Cargo.toml의 버전을 어긋나지 않게 유지해야
  한다.
- **검토한 대안**: 저장소에 실제 버전을 커밋하고 릴리스마다 6곳 수동 갱신
  (드리프트 위험) / 저장소에는 플레이스홀더(`0.0.0-dev`)를 두고 릴리스
  워크플로가 태그에서 버전을 스탬프.
- **선택과 근거**: 스탬프 방식. 워크플로가 태그 `vX.Y.Z`와 `Cargo.toml`
  버전이 일치하는지 검증한 뒤 `npm/scripts/stamp-version.mjs`로 메인
  패키지 버전과 optionalDependencies를 한 번에 채운다. 진실 소스는
  Cargo.toml 하나가 된다.

### 결정 5: unplugin-rl은 설치된 rl-lang을 자동 탐색

- **상황**: 번들러 사용자가 `rl-lang`을 devDependency로 설치했다면 PATH에
  rlc가 없어도 동작해야 한다.
- **검토한 대안**: 런처(`npx rlc`)를 자식 프로세스로 실행(호출마다 node
  프로세스 1개가 더 뜸) / `rl-lang`의 `binaryPath()`로 네이티브 바이너리
  경로를 직접 얻어 실행.
- **선택과 근거**: `binaryPath()` 직접 호출. 컴파일 호출마다 프로세스 생성
  오버헤드가 없고, `compiler` 옵션을 명시하면 기존처럼 그 값을 쓴다.
  rl-lang이 없으면 종전대로 PATH의 `rlc`로 폴백하므로 기존 사용자에게
  동작 변화가 없다.

## 작업 내역

- 2026-08-17: npm 이름 가용성 확인 (`npm view` — 결정 2), Cargo.lock C 의존성
  조사 (결정 3), 태스크 문서 작성.
- 2026-08-17: `npm/rl-lang/` 작성 — `package.json`(bin `rlc`,
  optionalDependencies 5종), `bin/rlc.js`(플랫폼 패키지 해석 → spawnSync,
  실패 시 지원 플랫폼과 소스 빌드 안내), `index.js`(`binaryPath()`),
  `README.md`.
- 2026-08-17: `npm/scripts/make-platform-package.mjs`(플랫폼 패키지 생성)와
  `npm/scripts/stamp-version.mjs`(버전 스탬프) 작성.
- 2026-08-17: `.github/workflows/release.yml` 작성 — 태그 검증 → 5개 타깃
  빌드(matrix) → 아티팩트 수집 → npm publish(플랫폼 → 메인 순) →
  GitHub Release 생성.
- 2026-08-17: `integrations/unplugin/index.js`에 rl-lang 자동 탐색 추가,
  README 갱신.
- 2026-08-17: `README.md`·`docs/reference/cli.md` 설치 문서를 npm 우선으로
  갱신, `CHANGELOG.md` 기록.
- 2026-08-17: 로컬 종단 검증 — release 바이너리로 플랫폼 패키지를 만들어
  임시 프로젝트에 `npm install`(파일 경로) 후 `npx rlc` 스모크 테스트,
  unplugin 폴백/자동 탐색 경로 확인 (아래 검증 참조).

## 이슈 및 해결

### 이슈 1: 로컬에서 musl 빌드 검증 불가

- **증상**: 작업 컨테이너에 `musl-gcc`가 없어 `x86_64-unknown-linux-musl`
  빌드를 로컬에서 돌릴 수 없다.
- **원인**: 기본 이미지에 musl-tools 미설치, 네트워크 정책상 툴체인 추가
  설치가 제한적.
- **해결**: 로컬 검증은 gnu 타깃 바이너리로 패키지 조립·설치·실행 경로를
  종단 확인하고(패키지 구조는 타깃과 무관), musl 빌드 자체는 릴리스
  워크플로의 `musl-tools` 설치 단계에 맡긴다. swc가 동일 의존성 구성으로
  musl 빌드를 배포하고 있어 리스크는 낮다. 첫 릴리스 태그에서 확인 필요
  (남은 부채).

### 이슈 2: `file:` 의존성 설치에서 `binaryPath()` 해석 실패

- **증상**: 스모크 테스트에서 `npm install <디렉터리 경로>`로 두 패키지를
  설치하자 `rl-lang: cannot find the rlc binary: the rl-lang-linux-x64
  package is not installed` — 플랫폼 패키지가 트리에 있는데도 해석 실패.
- **원인**: `file:` 설치는 심링크를 만들고, Node의 require 해석은 심링크의
  **실제 경로** 기준으로 walk하므로 프로젝트의 `node_modules`에 절대
  도달하지 못한다. 레지스트리 설치(tarball 복사)와 pnpm 가상 스토어
  (의존성이 같은 레벨에 링크됨)에서는 발생하지 않는 테스트 방식의 아티팩트.
- **해결**: `npm pack`으로 tarball을 만들어 설치하는 방식으로 스모크
  테스트를 바꿨고 정상 동작을 확인했다. 코드 변경은 불필요
  (esbuild가 동일 패턴으로 배포 중).

### 이슈 3: 스모크 스니펫이 통과 계약에 걸려 무의미

- **증상**: 처음 쓴 스니펫 `enum E { A }`가 컴파일 출력에 그대로 나왔다.
- **원인**: 필드 없는 케이스만 있는 enum은 유효한 TS enum이므로 설계 계약
  1(유효한 TS는 바이트 그대로 통과)에 따라 변환되지 않는다 — 올바른
  동작이지만 "컴파일이 됐다"는 스모크 증거로는 부적합.
- **해결**: 릴리스 워크플로의 스모크를 rl 고유 구문
  `enum E { A(x: number) }`로 바꾸고 출력에서 `kind: "A"`를 grep해
  실제 변환을 확인하도록 했다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` — 238개 통과 (tsc/node 있는 환경, 통합 테스트 포함)
- [x] `node --check` — npm 런처·스크립트·unplugin 전체
- [x] 종단 스모크: `npm pack` tarball 설치 → `npx rlc --version` /
      `-p` 컴파일 / 종료 코드 전달 / `binaryPath()` 확인
- [x] unplugin 자동 탐색: PATH에 rlc가 없는 상태에서 `rl-lang`·
      `unplugin-rl`·esbuild만 설치한 임시 프로젝트가 `.rl` import를
      번들·실행 (`node out.js` → `hi npm`)

## 결과

`npm install --save-dev rl-lang`으로 rlc가 프리빌트 바이너리로 설치된다.
릴리스는 `Cargo.toml` 버전을 올리고 `vX.Y.Z` 태그를 푸시하면 워크플로가
빌드·배포한다. **첫 배포 전에 저장소 소유자가 할 일**: ① npm 계정에서
`NPM_TOKEN`(automation token) 발급 후 저장소 시크릿으로 등록, ② 태그 푸시.

- 추가: `npm/rl-lang/`(package.json, `bin/rlc.js`, `index.js`, README),
  `npm/scripts/make-platform-package.mjs`, `npm/scripts/stamp-version.mjs`,
  `.github/workflows/release.yml`
- 수정: `integrations/unplugin/index.js`(+README) — rl-lang 자동 탐색,
  `README.md`·`docs/reference/cli.md` 설치 문서, `CHANGELOG.md`
- 남은 부채: musl 실빌드는 첫 릴리스 태그에서 최종 확인 (이슈 1). VSCode
  확장의 rl-lang 자동 탐색은 별도 태스크로.
