# TASK-086: typed val 예시 프로젝트와 Cursor 확장 갱신

- **상태**: 완료
- **시작일**: 2026-08-19
- **완료일**: 2026-08-19
- **커밋**: 미커밋

## 목적

이번 브랜치에서 정리한 TypeScript 타입 백엔드와 에디터의 타입 기반 `val` 진단을
작게 확인할 수 있는 독립 예시 프로젝트를 `examples/` 아래에 추가한다. 같은 변경이
Cursor에서 바로 동작하도록 로컬 Cursor 확장 설치본도 최신 빌드로 갱신한다.

## 범위

- 포함: 실행 가능한 `rl` 예시 프로젝트, 빌드 스크립트, README를 추가한다.
- 포함: 타입 기반 `val` built-in mutator 진단을 확인하는 별도 예시 파일을 추가한다.
- 포함: 루트 README의 예시 링크를 갱신한다.
- 포함: `editors/vscode` 확장을 패키징하고 Cursor extension directory의 설치본을
  갱신한다.
- 제외: 언어 기능이나 컴파일러 동작 변경.
- 제외: 예시 실행 산출물을 저장소에 커밋하지 않는다.
- 제외: 확장 버전 번호 변경. 저장소 규칙상 버전은 릴리스 단위로만 올린다.

## 의사결정

### 결정 1: 성공 빌드 파일과 진단용 파일을 분리한다

- **상황**: 사용자는 예시 프로젝트를 요청했고, 이번 브랜치의 핵심 사용자 체감 변경은
  에디터/`--types`가 `val` binding을 통한 built-in mutator 호출을 잡는 것이다.
- **검토한 대안**: 기본 프로젝트에 의도적 오류 파일을 포함하면 진단을 즉시 볼 수 있지만
  `rlc src`와 `npm run build`가 기본적으로 실패한다. 오류 파일을 `.rl.example`로 두면
  기본 프로젝트는 실행 가능하고, 사용자가 원할 때 파일명을 바꿔 진단을 확인할 수 있다.
- **선택과 근거**: 성공 빌드용 `src/main.rl`과 진단 확인용 `src/diagnostics.rl.example`을
  분리한다. 예시는 기본적으로 빌드 가능해야 하며, 진단 시나리오는 README 명령으로
  명확히 안내한다.

### 결정 2: repo 내부 예시이므로 현재 worktree의 `target/debug/rlc`를 기본 스크립트로 쓴다

- **상황**: 이 예시는 이 브랜치에서 작업한 내용을 기반으로 저장소 내부에 만드는 예시다.
- **검토한 대안**: `rlc` PATH 명령을 쓰면 npm 설치본이나 사용자 로컬 설치본을 잡아
  현재 브랜치의 동작을 보장하지 못한다. `../../target/debug/rlc`를 쓰면 `cargo build` 후
  현재 worktree의 컴파일러로 예시를 실행한다.
- **선택과 근거**: package script는 `../../target/debug/rlc`를 사용한다. README에서
  먼저 repo root에서 `cargo build`를 실행하라고 안내한다.

### 결정 3: Cursor 갱신은 VSIX를 Cursor extensions dir에 설치하는 방식으로 한다

- **상황**: 이 환경에는 `cursor` CLI가 없고, Cursor extension directory에는 기존
  `rl-lang.rl-language-0.1.0` 설치본이 있다.
- **검토한 대안**: 설치 폴더를 직접 복사하면 빠르지만 `.vscodeignore` 패키징 결과와
  차이가 날 수 있다. VSIX를 만들고 `code --extensions-dir ~/.cursor/extensions
  --install-extension --force`를 쓰면 VSCode 호환 확장 설치 절차를 Cursor 폴더에
  적용할 수 있다.
- **선택과 근거**: VSIX 패키징 후 Cursor extension directory를 지정해 강제 설치한다.
  패키징 전후 `typescript/lib/lib*.d.ts` 포함 여부도 확인한다.

## 작업 내역

- 2026-08-19: TASK-086을 등록했다.
- 2026-08-19: 요청에 따라 TASK-086 범위를 Cursor 확장 최신 빌드 설치까지 확장했다.
- 2026-08-19: `examples/typed-val-demo/`를 추가했다. `src/main.rl`은 `enum`,
  `match`, `result`, `Result`, pipeline, `val`을 사용하는 실행 가능한 예시이고,
  `src/diagnostics.rl.example`은 typed `val` 진단 확인용 파일이다.
- 2026-08-19: 예시 `package.json`에 repo-local `../../target/debug/rlc` 기반 스크립트
  (`check:rl`, `build:rl`, `types:rl`, `build`, `start`)를 추가했다.
- 2026-08-19: 예시 산출물(`build/`, `dist/`)을 `.gitignore`에 추가하고 루트 README의
  예시 목록에 `typed-val-demo`를 연결했다.
- 2026-08-19: `npm install --no-package-lock` 후 예시 `npm run check:rl`,
  `npm run build`, `npm start`, `npm run types:rl`을 실행했다.
- 2026-08-19: 진단용 파일을 임시 디렉터리에 `.rl`로 복사해
  `../../target/debug/rlc --types`가 `Map#set` typed `val` 진단을 내는지 확인했다.
- 2026-08-19: `editors/vscode`에서 `npm run compile`과 현재 worktree compiler를 PATH
  앞에 둔 `npm test`를 실행했다.
- 2026-08-19: `npx @vscode/vsce package --no-dependencies`로 VSIX를 만들고,
  `npx @vscode/vsce ls --no-dependencies | grep -c "typescript/lib/lib"`가 100임을
  확인했다.
- 2026-08-19: `code --extensions-dir "$HOME/.cursor/extensions" --install-extension
  rl-language-0.1.0.vsix --force`로 Cursor extension directory의
  `rl-lang.rl-language-0.1.0` 설치본을 갱신했다.
- 2026-08-19: Cursor 설치본의 `server/out/valdiag.js` 존재와 TypeScript
  `lib*.d.ts` 100개 포함을 확인했다.
- 2026-08-19: Rust 필수 게이트(`cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test`)를 실행했다.

## 이슈 및 해결

### 이슈 1: 예시 Result 패턴의 필드명이 표준 라이브러리와 달랐다

- **증상**: 예시 `npm run build`가 생성된 TypeScript에서
  `Property 'task' does not exist on type 'Ok<Task>'`,
  `Property 'message' does not exist on type 'Err<string>'`로 실패했다.
- **원인**: 표준 `Result` 변종 필드는 `Ok(value)`와 `Err(error)`인데 예시에서
  `Ok(task)`, `Err(message)`처럼 필드 alias 없이 원하는 로컬 이름을 직접 썼다.
- **해결**: 패턴을 `Ok(value: task)`, `Err(error: message)`로 바꿔 표준 필드명을
  사용하면서 로컬 변수 이름은 예시 의도대로 유지했다.

### 이슈 2: 확장 테스트가 PATH의 오래된 `rlc`를 잡을 수 있다

- **증상**: `editors/vscode`에서 plain `npm test`를 실행하면 completion std signature와
  typed val emit-map 테스트가 실패했다.
- **원인**: 테스트의 `COMPILER` 상수는 `rlc`이고, 기본 PATH는 현재 worktree의
  `target/debug/rlc`보다 `/Users/seominyeong/.local/bin/rlc`를 먼저 가리켰다.
- **해결**: `cargo build` 후
  `PATH="/Users/seominyeong/orca/workspaces/rl/tsgo-frontend-review/target/debug:$PATH"
  npm test`로 현재 브랜치 compiler를 사용해 확장 테스트를 실행했다.

## 검증

- [x] `npm run check:rl` (`examples/typed-val-demo`)
- [x] `npm run build` (`examples/typed-val-demo`)
- [x] `npm start` (`examples/typed-val-demo`)
- [x] `npm run types:rl` (`examples/typed-val-demo`)
- [x] 임시 `diagnostics.rl`에 대한 `../../target/debug/rlc --types`
- [x] `npm run compile` (`editors/vscode`)
- [x] `PATH="/Users/seominyeong/orca/workspaces/rl/tsgo-frontend-review/target/debug:$PATH" npm test` (`editors/vscode`)
- [x] `npx @vscode/vsce package --no-dependencies`
- [x] `npx @vscode/vsce ls --no-dependencies | grep -c "typescript/lib/lib"` → 100
- [x] Cursor extension directory 설치 확인
- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`

## 결과

`examples/typed-val-demo`가 추가됐다. 기본 프로젝트는 빌드/실행 가능하며,
`diagnostics.rl.example`을 통해 이번 브랜치의 typed `val` 진단을 확인할 수 있다.
Cursor의 로컬 `rl-lang.rl-language-0.1.0` 확장 설치본도 최신 빌드로 갱신됐다.
언어 표면이나 CLI 동작 변경은 없으므로 `docs/reference/`와 `docs/ai/rl.md` 갱신은
필요 없다.
