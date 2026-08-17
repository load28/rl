# TASK-032: 사이드카를 별도 트리로 — 소스/출력 완전 분리

- **상태**: 완료
- **시작일**: 2026-08-17
- **완료일**: 2026-08-17
- **커밋**: —

## 목적

[TASK-031](./TASK-031-sidecar-visibility.md)은 사이드카가 소스 옆에 있어야
한다는 전제 위에서 "보이는 방식"만 정리했고, 그 해법(파일 중첩·숨김)은
VSCode 전용이었다. 생성물을 소스 트리에서 완전히 빼내 **에디터와 무관하게**
깔끔한 상태를 만든다. 타입 선언도 포함이다.

## 범위

- 포함: `rlc --sidecar`의 `-o` 지원과 상대 경로 `sources`, 언어 서버의
  `rl.sidecarDir` 설정, 예제 두 개의 트리 재배치, 문서 갱신.
- 제외: rlc 자체의 입력 수집 확대(TASK-026의 남은 항목).

## 의사결정

### 결정 1: `rootDirs`로 사이드카를 소스 트리 밖으로 뺀다

- **상황**: TASK-031에서 "사이드카는 반드시 `.rl` 옆에 있어야 한다"고
  결론지었다. 근거는 `paths`가 상대 경로 지정자에 적용되지 않는다는 실측
  (`TS2307`)이었다. 전제를 다시 검토했다.
- **검토한 대안**:
  - `paths` 매핑: 상대 경로에 적용되지 않는다 (TASK-031에서 확인).
  - `rootDirs`: 여러 디렉터리를 **하나의 가상 디렉터리로 합쳐** 상대 경로를
    해석한다. 원래 생성 파일을 위해 만들어진 기능이다.
- **선택과 근거**: `rootDirs`. `/tmp/rlroot`에서 확인했다 —
  `rootDirs: ["src", ".rl-types"]`를 두고 `src/main.ts`가
  `"./notice.rl"`을 import하면 `.rl-types/notice.rl.d.ts`로 해석되고,
  tsserver의 정의 이동도 `src/notice.rl`로 갔다. 즉 TASK-031의 전제가
  틀렸고, 소스 트리를 완전히 비울 수 있다.

### 결정 2: 위치는 `-o`로 정한다

- **상황**: 사이드카 출력 위치를 새 플래그로 받을지, 기존 `-o`를 쓸지.
- **선택과 근거**: `-o`. 이미 "출력을 어디에 쓸지"를 뜻하고 입력 트리를
  미러하는 규칙도 그대로 쓸 수 있다. `--sidecar <선언 디렉터리>`는 입력
  (tsc가 만든 `.d.ts`가 어디 있는지), `-o`는 출력이라는 역할 구분도
  분명하다.

### 결정 3: `sources`는 사이드카 기준 상대 경로로 적는다

- **상황**: 맵의 `sources`가 원본을 찾지 못하면 정의 이동이 `.d.ts`에
  선다. 사이드카가 다른 트리로 가면 파일 이름만으로는 부족하다.
- **선택과 근거**: `build_sidecar`의 세 번째 인자를 "사이드카 위치 기준
  `.rl` 상대 경로"로 바꿨다(`../src/notice.rl`). 맵의 `file`과 배너,
  `sourceMappingURL`은 그 경로의 **파일 이름**만 쓰므로 두 배치에서 모두
  같은 이름이 나온다. CLI가 경로 차이를 계산해 넘긴다(`relative_path`).

### 결정 4: 예제는 생성물을 한 곳에 모은다

- **상황**: 두 예제가 서로 다른 방식으로 지저분했다. `rl-calc`은 `.rl` 옆에
  컴파일된 `.ts`가 쌓였고, `rl-interop`은 사이드카가 소스와 섞였다.
- **선택과 근거**: 둘 다 소스 트리를 손으로 쓰는 파일만 남기도록 바꿨다.
  `rl-calc`은 `rlc -o build src/`(표준 라이브러리도 `build/rl.ts`),
  `rl-interop`은 `rlc --sidecar types -o .rl-types`. 그 결과 TASK-031에서
  넣었던 예제의 `.vscode/settings.json`이 **둘 다 필요 없어져 삭제**했다.
  에디터별 설정 없이 어디서 열어도 같은 모습이 된다.

## 작업 내역

- 2026-08-17: `/tmp/rlroot`에서 `rootDirs` 해석과 정의 이동을 확인했다
  (결정 1).
- 2026-08-17: `src/sidecar.rs` — `build_sidecar`의 세 번째 인자를 상대
  경로로 재정의하고, 파일 이름은 그 경로에서 뽑도록 했다.
- 2026-08-17: `src/main.rs` — `sidecar_mode`가 `job.out_path` 기준으로 쓰고
  (디렉터리는 필요 시 생성), `relative_path` 헬퍼로 `sources`를 계산한다.
- 2026-08-17: `tests/sidecar.rs`에 상대 경로 케이스를 추가했다 (8개).
- 2026-08-17: 언어 서버 — `refreshSidecar`에 `outDir` 인자, `rl.sidecarDir`
  설정과 워크스페이스 기준 경로 해석(`resolveSidecarDir`), 서버 테스트 2개
  추가 (39개).
- 2026-08-17: 예제 재배치. `rl-calc` → `src/`에 `.rl` 다섯 개만,
  `rl-interop` → `src/`에 손으로 쓴 파일 넷만, 사이드카는 `.rl-types/`.
  두 프로젝트의 `.vscode/settings.json` 삭제.
- 2026-08-17: `cli.md`, 확장 README, 예제 README 갱신. 확장 재패키징·설치.

## 이슈 및 해결

### 이슈 1: 서버 테스트가 옛 `rlc`를 불러 실패했다

- **증상**: "declarations can live in their own tree"가
  `fs.existsSync(`${rl}.d.ts`)`에서 `true !== false`로 실패했다 — 사이드카가
  `-o` 대상이 아니라 소스 옆에 생겼다.
- **원인**: 서버 테스트는 PATH의 `rlc`를 실행하는데, `cargo build`만 하고
  `cargo install`을 하지 않아 옛 바이너리가 남아 있었다. 옛 버전은 `-o`를
  유효한 옵션으로 받되 사이드카 모드에서 쓰지 않는다.
- **해결**: `cargo install --path . --force` 후 재실행해 39개 전부 통과.
  PATH 바이너리에 의존하는 테스트의 특성이므로 코드 변경은 없다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` — 90 + 21 + 35 + 8 + 2 + 7 전부 통과
- [x] 확장 `npm test` — 39개 통과 (기존 37 + 신규 2)
- [x] `rl-interop`에서 tsserver 구동 — 진단 없음,
      정의 이동 `→ src/notice.rl:21:17` (사이드카가 `.rl-types/`에 있는 상태)
- [x] `rl-calc`·`rl-interop` 빌드·실행 결과가 재배치 전과 동일

## 결과

- 수정: `src/sidecar.rs`, `src/main.rs`, `tests/sidecar.rs`,
  `docs/reference/cli.md`, `editors/vscode/package.json`,
  `editors/vscode/README.md`, `editors/vscode/server/src/sidecar.ts`,
  `editors/vscode/server/src/server.ts`,
  `editors/vscode/server/src/test/sidecar.test.ts`, `docs/tasks/INDEX.md`
- 추가: `docs/tasks/TASK-032-sidecar-out-dir.md`

후속: TASK-026의 남은 항목(디렉터리 수집에 `.ts` 포함), 그리고 언어 서버를
`editors/vscode/` 밖으로 분리해 다른 에디터에서 쓰기 쉽게 하는 일.
