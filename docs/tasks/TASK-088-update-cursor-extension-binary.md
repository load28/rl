# TASK-088: Cursor 확장과 rlc 바이너리 갱신

- **상태**: 완료
- **시작일**: 2026-08-19
- **완료일**: 2026-08-19
- **커밋**: 미커밋

## 목적

현재 브랜치의 최신 컴파일러 변경을 Cursor에서 바로 확인할 수 있도록 로컬 `rlc`
바이너리와 Cursor에 설치된 `rl-language` 확장을 갱신한다.

## 범위

- 포함: 현재 소스 기준으로 `rlc` 바이너리를 빌드한다.
- 포함: `editors/vscode` 확장을 다시 컴파일/패키징하고 Cursor extension directory에
  설치한다.
- 포함: 설치 결과와 버전/바이너리 동작을 확인한다.
- 제외: 확장 버전 번호 변경. 이 저장소는 릴리스 태스크가 아닐 때 버전을 올리지 않는다.
- 제외: 생성된 `.vsix` 파일이나 빌드 산출물을 커밋하지 않는다.

## 의사결정

### 결정 1: 릴리스 버전은 유지하고 로컬 설치본만 교체한다

- **상황**: 사용자는 Cursor 확장과 바이너리를 최신 작업 내용으로 업데이트하길 원했다.
- **검토한 대안**: package version을 올리고 배포 산출물처럼 커밋할 수 있지만, 저장소
  버저닝 가이드는 릴리스 단위가 아닐 때 버전을 올리지 않도록 한다. 로컬 VSIX 재설치와
  바이너리 빌드는 사용자 환경 갱신에는 충분하다.
- **선택과 근거**: `0.1.0` 버전은 유지하고, 현재 소스에서 빌드한 VSIX를 Cursor에
  `--force` 재설치한다. 컴파일러는 workspace 우선 탐색 경로인 `target/release/rlc`와
  `target/debug/rlc`를 최신으로 빌드한다.

## 작업 내역

- 2026-08-19: TASK-088을 등록했다.
- 2026-08-19: `cargo build`로 `target/debug/rlc`를 최신 소스 기준으로 빌드했다.
- 2026-08-19: `cargo build --release`로 `target/release/rlc`를 최신 소스 기준으로
  빌드했다.
- 2026-08-19: `PATH=.../target/debug:$PATH npm test`를 `editors/vscode`에서 실행해
  확장 서버 테스트 76개 통과를 확인했다.
- 2026-08-19: `npx @vscode/vsce package`로 `rl-language-0.1.0.vsix`를 생성했다.
- 2026-08-19: `code --extensions-dir "$HOME/.cursor/extensions" --install-extension
  rl-language-0.1.0.vsix --force`로 Cursor extension directory에 재설치했다.
- 2026-08-19: `cargo install --path . --force`로 `/Users/seominyeong/.cargo/bin/rlc`를
  갱신했다.
- 2026-08-19: 현재 셸의 `rlc`가 `/Users/seominyeong/.local/bin/rlc`를 우선 잡는 것을
  확인했고, 이 파일도 `target/release/rlc`로 교체했다.
- 2026-08-19: 설치용 임시 파일 `editors/vscode/rl-language-0.1.0.vsix`를 삭제했다.

## 이슈 및 해결

- **증상**: `cargo install --path . --force` 후에도 `command -v rlc`는
  `/Users/seominyeong/.local/bin/rlc`를 가리켰다.
- **원인**: 사용자 PATH에서 `/Users/seominyeong/.local/bin`이
  `/Users/seominyeong/.cargo/bin`보다 앞서 있었다.
- **해결**: `target/release/rlc`와 `/Users/seominyeong/.local/bin/rlc`의 SHA-256 해시가
  다른 것을 확인한 뒤, `install -m 755 target/release/rlc
  "$HOME/.local/bin/rlc"`로 PATH 우선 바이너리도 교체했다. 교체 후 두 파일의
  해시가 같고 `rlc --version`이 `0.3.0`을 출력하는 것을 확인했다.

## 검증

- [x] `cargo build`
- [x] `cargo build --release`
- [x] `PATH=/Users/seominyeong/orca/workspaces/rl/tsgo-frontend-review/target/debug:$PATH npm test` (`editors/vscode`)
- [x] VSIX 패키징: `npx @vscode/vsce package`
- [x] Cursor extension directory 재설치:
  `code --extensions-dir "$HOME/.cursor/extensions" --install-extension
  rl-language-0.1.0.vsix --force`
- [x] 설치된 확장 확인:
  `code --extensions-dir "$HOME/.cursor/extensions" --list-extensions --show-versions`
  → `rl-lang.rl-language@0.1.0`
- [x] `target/release/rlc`, `/Users/seominyeong/.cargo/bin/rlc`,
  `/Users/seominyeong/.local/bin/rlc` 해시 일치 확인
- [x] `rlc --version` → `0.3.0`

## 결과

Cursor extension directory의 `rl-language@0.1.0` 설치본을 현재 브랜치에서 패키징한
VSIX로 갱신했다. workspace 우선 경로의 debug/release 바이너리와 PATH 우선 바이너리
(`/Users/seominyeong/.local/bin/rlc`)도 현재 release 빌드와 동일하게 맞췄다. VSIX와
빌드 산출물은 커밋 대상에서 제외했다.
