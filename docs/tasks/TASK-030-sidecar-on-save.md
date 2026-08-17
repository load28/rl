# TASK-030: 저장 시 사이드카 갱신 (언어 서버)

- **상태**: 완료
- **시작일**: 2026-08-17
- **완료일**: 2026-08-17
- **커밋**: ffeaaf9

## 목적

[TASK-029](./TASK-029-sidecar-mode.md)의 `rlc --sidecar`는 빌드 스텝이라
`.rl`을 고쳐도 저장 즉시 반영되지 않는다. 언어 서버가 저장 시점에 사이드카를
다시 만들어, `.ts` 쪽 타입과 정의 이동이 편집과 함께 따라오게 한다.

## 범위

- 포함: `editors/vscode/server/src/sidecar.ts`(저장 시 재생성), 저장 알림
  수신, `rl.sidecar` 설정, 서버 테스트 5개, 확장 README 갱신.
- 제외: 저장 없이(타이핑 중) 갱신. 진단과 달리 파일을 쓰는 동작이라
  저장 시점으로 제한한다.
- 제외: `rlc` 쪽 변경. TASK-029의 `--sidecar`를 그대로 호출한다.

## 의사결정

### 결정 1: 선언 본문은 서버가 TypeScript API로 메모리에서 만든다

- **상황**: `rlc --sidecar`는 tsc가 만든 `.d.ts`를 입력으로 받는다. 저장할
  때마다 사용자가 `tsc -p tsconfig.types.json`을 돌릴 수는 없다.
- **검토한 대안**:
  - 서버가 `tsc` 프로세스를 띄운다: 설정 파일과 출력 디렉터리를 관리해야
    하고 느리다.
  - 서버가 번들된 `typescript`로 선언만 방출한다: 서버는 이미 TS 위임
    (TASK-024)을 위해 `typescript`를 의존성으로 갖고 있다.
- **선택과 근거**: 후자. `rlc -p --rewrite-imports ts`로 컴파일 결과를 받아,
  그 텍스트를 **rlc가 썼을 경로**(`<dir>/<이름>.ts`)에 얹은
  `ts.createProgram`으로 `emitDeclarationOnly` 방출한다. 경로를 그대로
  맞추기 때문에 상대 import가 실제 이웃 파일로 해석되어 타입이 정확하다.
  기존 `TsProject`를 쓰지 않은 이유는 그쪽이 `.rl` 원문을 TS로 취급하기
  때문이다 — 선언 방출에는 rlc가 변환한 결과가 필요하다.

### 결정 2: 기본값은 "이미 있는 사이드카만 갱신"

- **상황**: 저장할 때마다 워크스페이스에 파일 두 개를 쓰는 동작이라 기본값을
  정해야 했다.
- **검토한 대안**:
  - 항상 생성: 바로 동작한다. 대신 사이드카를 원치 않는 프로젝트에도 파일이
    생기고 git 상태가 지저분해진다.
  - 기본 off: 안전하지만 사용자가 설정을 찾아내야 한다.
  - 이미 있는 것만 갱신: 프로젝트가 `rlc --sidecar`를 한 번 돌려 참여를
    표시하면 그다음부터 자동으로 따라온다.
- **선택과 근거**: 세 번째(`refresh`). 파일 존재 자체가 옵트인 신호라
  설정을 몰라도 되고, 원치 않는 워크스페이스에는 아무것도 만들지 않는다.
  `always`와 `off`도 설정으로 남겼다.

### 결정 3: 컴파일 실패 시 사이드카를 건드리지 않는다

- **상황**: 편집 도중 저장하면 `.rl`이 일시적으로 컴파일되지 않을 수 있다.
- **검토한 대안**: 실패 시 사이드카 삭제 / 그대로 두기.
- **선택과 근거**: 그대로 두기. 삭제하면 편집 중에 `.ts` 쪽 타입이 통째로
  사라져 무관한 에러가 쏟아진다. 마지막으로 성공한 선언을 유지하는 편이
  낫고, 실패는 서버 로그에 경고로 남긴다. 테스트로 고정했다
  (`a file that no longer compiles keeps its last good sidecar`).

## 작업 내역

- 2026-08-17: `server/src/sidecar.ts` 작성 — `refreshSidecar(compiler,
  rlPath, mode)`가 ① 모드 확인 ② `rlc -p --rewrite-imports ts` ③ 메모리에서
  선언 방출 ④ 임시 디렉터리에 `.d.ts`를 쓰고 `rlc --sidecar <tmp> <rl>` 실행
  ⑤ 임시 디렉터리 정리 순으로 동작한다.
- 2026-08-17: `server/src/server.ts` — `textDocumentSync`를 객체 형태로 바꿔
  `save` 알림을 켜고(`includeText: false`, 저장 시점엔 디스크가 최신),
  `documents.onDidSave`에서 `rebuildSidecar`를 호출한다. `RlSettings`에
  `sidecar` 추가.
- 2026-08-17: `package.json`에 `rl.sidecar` 설정
  (`refresh`(기본)/`always`/`off`) 추가.
- 2026-08-17: `server/src/test/sidecar.test.ts` 5개 추가. `rlc`가 PATH에
  없으면 skip한다.
- 2026-08-17: 확장 README에 설정 행과 "`.ts`에서 `.rl` 가져다 쓰기" 절 추가.
- 2026-08-17: 확장 재패키징(4.43MB, 527 files) 후 VSCode에 설치.

## 이슈 및 해결

### 이슈 1: 갱신 테스트가 컴파일 에러로 실패했다

- **증상**: "refresh 모드가 기존 사이드카를 갱신한다" 테스트가
  `match on enum Notice is not exhaustive: missing "Debug"`로 실패했다.
- **원인**: 테스트가 갱신을 확인하려고 enum에 케이스를 추가했는데, 그러면
  같은 파일의 `match`가 소진되지 않아 컴파일이 실패한다. 컴파일러가 옳고
  테스트 시나리오가 틀렸다.
- **해결**: 갱신 확인은 exported 함수 추가로 바꾸고, 원래 시나리오는
  "컴파일 실패 시 마지막 사이드카가 유지된다"는 별도 테스트로 살렸다.
  결정 3의 근거가 그 과정에서 실측으로 확인됐다.

## 검증

- [x] `npm run compile` (client + server)
- [x] `npm test` — 37개 통과 (기존 32 + 신규 5)
- [x] 확장 패키징·설치 (`code --install-extension --force`)
- [ ] `cargo fmt --check` / `clippy` / `cargo test` — 해당 없음 (Rust 변경
      없음)

## 결과

- 추가: `editors/vscode/server/src/sidecar.ts`,
  `editors/vscode/server/src/test/sidecar.test.ts`,
  `docs/tasks/TASK-030-sidecar-on-save.md`
- 수정: `editors/vscode/server/src/server.ts`,
  `editors/vscode/package.json`, `editors/vscode/README.md`,
  `docs/tasks/INDEX.md`

후속: 같은 이름의 중복 선언을 다루는 전체 오프셋 대응표(TASK-029 후속),
그리고 사이드카를 워크스페이스 전체에 한 번에 만드는 명령.
