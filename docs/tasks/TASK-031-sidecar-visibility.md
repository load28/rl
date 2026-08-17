# TASK-031: 사이드카가 소스 트리를 어지럽히지 않게

- **상태**: 완료
- **시작일**: 2026-08-17
- **완료일**: 2026-08-17
- **커밋**: 5e55a97

## 목적

[TASK-029](./TASK-029-sidecar-mode.md)·[TASK-030](./TASK-030-sidecar-on-save.md)
이후 `.rl` 옆에 `.rl.d.ts`와 `.rl.d.ts.map`이 소스와 같은 층에 늘어서
탐색기가 지저분해졌다. 파일 위치를 바꿀 수 있는지 확인하고, 바꿀 수 없다면
보이는 방식을 정리한다.

## 범위

- 포함: 확장의 `contributes.configurationDefaults`(파일 중첩·검색 제외·읽기
  전용), 사이드카 파일의 `@generated` 배너, 확장 README 갱신.
- 제외: 사이드카 위치 변경 — 아래 결정 1의 이유로 불가능하다.

## 의사결정

### 결정 1: 위치는 바꾸지 않는다 (바꿀 수 없다)

- **상황**: 사이드카를 `types/` 같은 별도 디렉터리로 옮기고 `tsconfig.json`
  `paths`로 연결할 수 있는지 확인이 필요했다.
- **검토한 대안**: `paths` 매핑 / 파일명 앞에 `.` 붙이기 / 그대로 두기.
- **선택과 근거**: 그대로 둔다. `paths`는 **상대 경로 지정자에 적용되지
  않는다**. `/tmp/rlpaths`에서 `"./notice.rl": ["types/notice.rl.d.ts"]`와
  `"*.rl": ["types/*.rl.d.ts"]`를 모두 넣고 확인했지만 결과는 그대로였다.
  ```
  main.ts(1,24): error TS2307: Cannot find module './notice.rl' or its
                 corresponding type declarations.
  ```
  TypeScript가 `./x.rl`을 해결하는 경로는 형제 `x.rl.d.ts` 하나뿐이므로,
  남는 선택지는 "어떻게 보이게 할 것인가"다.

### 결정 2: 확장이 에디터 기본값을 제공한다

- **상황**: 사용자가 설정을 직접 찾아 넣게 할지, 확장이 기본값을 줄지.
- **검토한 대안**: README로만 안내 / `contributes.configurationDefaults`로
  기본값 제공.
- **선택과 근거**: 후자. 확장을 설치한 것만으로 정리된 상태가 되고, 사용자
  설정이 언제나 우선하므로 강요가 되지 않는다. 세 가지를 넣었다.
  - `explorer.fileNesting` — `.rl.d.ts`와 `.map`을 `.rl` 아래로 접는다.
  - `search.exclude` — 검색 결과에서 뺀다.
  - `files.readonlyInclude` — 생성물이니 읽기 전용으로 연다.
  `files.exclude`(완전히 숨김)는 기본값에 넣지 않았다. 문제를 진단할 때
  파일을 열어봐야 하는 경우가 있어 "접어두기"가 "감추기"보다 낫다고 봤다.

### 결정 3: 사이드카 파일에 `@generated` 배너를 붙인다

- **상황**: 탐색기에서 접혀 있어도 파일을 열면 손으로 고쳐도 되는지 알 수
  없다.
- **선택과 근거**: 배너 한 줄을 맨 위에 붙인다. rlc의 컴파일 출력이 이미
  같은 방식을 쓰고 있어 일관적이다. 배너가 생성 파일의 줄 번호를 하나
  밀기 때문에 `mappings` 앞에 빈 줄 세그먼트(`;`)를 넣어 보정했고, 정의
  이동이 그대로 원본에 착지하는지 tsserver로 재확인했다.

## 작업 내역

- 2026-08-17: `/tmp/rlpaths`에서 `paths` 우회 가능 여부를 확인했다 (결정 1).
- 2026-08-17: `editors/vscode/package.json`에
  `contributes.configurationDefaults` 추가.
- 2026-08-17: `src/sidecar.rs` — 선언 파일 맨 위에
  `// @generated from <파일> by rlc --sidecar — do not edit.`를 붙이고,
  `mappings`를 `;`로 한 줄 밀었다.
- 2026-08-17: `tests/sidecar.rs`에 배너 테스트를 더하고, 생성 줄 번호를
  기대하는 테스트를 한 줄씩 밀었다 (7개).
- 2026-08-17: `editors/vscode/README.md`에 "소스 트리를 어지럽히지 않게"
  절 추가 — 기본값 표, `files.exclude` 안내, `.gitignore` 예시.
- 2026-08-17: 확장 재패키징·설치 후 설치본에 기본값이 들어갔는지 확인했다.
- 2026-08-17: `rl-interop`에서 사이드카를 다시 만들어 정의 이동을 재확인했다.
  ```
  진단: 없음
  main.ts:23  render  → src/notice.rl:21:17
  main.ts:10  Notice  → src/notice.rl:9:13
  ```

## 이슈 및 해결

### 이슈 1: clippy가 `format!` 중첩을 거부했다

- **증상**: `error: format! in format! args --> src/sidecar.rs:70:15`으로
  clippy 게이트가 실패했다 (`-D warnings`).
- **원인**: `mappings` 값을 만들면서 바깥 `format!`의 인자 자리에
  `format!(";{}", ...)`를 그대로 넣었다.
- **해결**: `mappings`를 앞줄에서 `let`으로 만들어 넘겼다. 배너 보정이
  왜 필요한지 주석으로 남겼다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` — 90 + 21 + 35 + 7 + 2 + 7 전부 통과
- [x] 확장 `npm test` — 37개 통과
- [x] tsserver 구동으로 정의 이동 재확인 (배너 보정 후에도 동일)
- [x] 설치본 `package.json`에 `configurationDefaults` 확인

## 결과

- 수정: `src/sidecar.rs`, `tests/sidecar.rs`,
  `editors/vscode/package.json`, `editors/vscode/README.md`,
  `docs/tasks/INDEX.md`
- 추가: `docs/tasks/TASK-031-sidecar-visibility.md`

후속: 사이드카를 워크스페이스 전체에 한 번에 만드는 명령(지금은 파일마다
`rlc --sidecar` 또는 저장이 필요하다).
