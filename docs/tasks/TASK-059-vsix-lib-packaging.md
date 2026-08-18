# TASK-059: VSIX 패키징 검증 — TypeScript `lib*.d.ts` 누락 확정과 절차 문서화

- **상태**: 완료
- **시작일**: 2026-08-18
- **완료일**: 2026-08-18
- **커밋**: —

## 목적

TASK-058은 `TS2488` 오탐의 **메커니즘**(기본 라이브러리가 로드되지 않은
프로그램)까지만 확정하고, "왜 사용자 설치본에서 lib이 없었는가"라는
**트리거**는 미확정으로 남겼다. 사용자 보고로 트리거가 VSIX 패키징이라는
제보가 들어왔다. 이 태스크는 그 트리거를 측정으로 확정하고, TASK-058이 넣은
`.vscodeignore` 수정이 실제로 문제를 해결하는지 패키징 산출물 수준에서
검증하며, 재발을 막을 패키징 절차를 문서화한다.

## 범위

- 포함:
  - `vsce ls` / 실제 vsix 산출물로 lib 포함 여부 측정 (수정 전/후).
  - `.vscodeignore` 패턴 교정 — TASK-058의 세 줄을 실제로 필요한 한 줄로.
  - 패키징된 트리로 `TS2488` 재현/해소를 E2E 확인.
  - `editors/vscode/README.md`에 패키징 절차와 사후 확인 명령 기록.
- 제외:
  - `tsproject.ts`/`server.ts` 런타임 코드 변경 (TASK-058에서 끝났다).
  - 워크스페이스 `tsconfig.json` 반영 — TASK-060으로 분리.
  - vsix 용량 다이어트(`tsc.js`/`tsserver.js` 제외) — 아래 이슈 2.

## 의사결정

### 결정 1: 제보를 그대로 받지 않고 저장소 상태에서 다시 측정한다

- **상황**: 제보는 "`.vscodeignore`를 **새로 추가**하면 카운트가 0 → 100이
  된다"고 했다. 그러나 이 저장소에는 `.vscodeignore`가 이미 있고, TASK-058이
  이미 손댄 상태였다. 제보의 전제와 저장소 상태가 어긋난다.
- **검토한 대안**: (A) 제보의 diff를 그대로 적용한다. (B) 저장소 현재
  상태에서 카운트를 직접 재고, 필요한 만큼만 고친다.
- **선택과 근거**: (B). 실측 결과 TASK-058 이후 상태에서 이미 99개가
  들어가고 있었다 — 제보가 요청한 수정은 대부분 이미 되어 있었다. 그대로
  적용했다면 중복 패턴이 쌓였을 것이다. 남은 1개(`lib.d.ts`)만이 실제 결함이었다
  (아래 결정 3).

### 결정 2: 트리거가 확정됐다 — `**/*.ts`가 선언 파일까지 걷어낸다

- **상황**: TASK-058은 "패키징 사고가 가장 그럴듯하다"까지만 말했다.
- **측정**: TASK-058 이전 `.vscodeignore`로 되돌려 `vsce ls --no-dependencies`
  실행:
  - `typescript/lib/lib*.d.ts` → **0개**
  - `typescript/lib/typescript.js` → **1개(포함됨)**
  - 총 530파일 / 4.44MB (제보의 "530파일/4.43MB"와 일치)
- **선택과 근거**: 이 조합이 결정적이다. 모듈 본체는 들어가므로 **언어 서버는
  정상적으로 뜨고**, 오직 lib만 없다 — TASK-058이 재현한 "lib 없는 프로그램"
  상태와 정확히 같다. 서버가 아예 죽는 것이 아니라 조용히 오탐만 내는 이유가
  이것으로 설명된다.
- 원인 규칙도 확정했다: `node_modules/**`는 **루트** `node_modules`만
  매칭하므로 `server/node_modules/typescript`에는 애초에 걸리지 않는다.
  실제로 걷어내는 것은 `**/*.ts`이고, `.d.ts`가 여기 걸린다.

### 결정 3: 패턴은 `lib*.d.ts` — `lib.*.d.ts`가 아니다

- **상황**: TASK-058이 쓴 `!.../lib/lib.*.d.ts`로는 100개 중 99개만 들어온다.
- **원인**: TypeScript의 lib 디렉터리에는 점 없는 `lib.d.ts`가 하나 있다
  (`target`이 ES5 이하일 때의 기본 라이브러리).
- **선택과 근거**: `lib*.d.ts`로 바꿔 100개 전부 포함. 현재 확장은
  `target: ESNext`라 `lib.d.ts`를 쓰지 않지만, `COMPILER_OPTIONS`의 target이
  바뀌는 순간 같은 버그가 조용히 재발한다. 한 글자로 막을 수 있는 함정을
  남길 이유가 없다.

### 결정 4: 불필요한 부정 패턴 두 줄을 지운다

- **상황**: TASK-058은 `!.../package.json`과 `!.../lib/typescript.js`도 함께
  넣고, 주석에 "tsc·tsserver 등 나머지는 뺀다"고 적었다.
- **측정**: 결정 2의 실측대로 그 파일들은 **애초에 제외된 적이 없다**
  (`node_modules/**`가 하위 경로에 안 걸리므로). 두 줄은 no-op이고 주석은
  사실과 다르다 — `bin/tsc`, `lib/tsserver.js`, `README.md`까지 전부 이미
  들어가 있었다.
- **선택과 근거**: 두 줄을 지우고 주석을 실제 메커니즘(`**/*.ts`가 선언
  파일을 걷어낸다)으로 고쳐 썼다. 틀린 주석은 없는 주석보다 나쁘다.

## 작업 내역

- 2026-08-18: `npm install --no-save @vscode/vsce`(3.9.2) 후
  `npx @vscode/vsce ls --no-dependencies | grep -c "typescript/lib/lib"`로
  현재 상태 측정 → **99**. TASK-058 이전 `.vscodeignore`로 되돌려 재측정 →
  **0**(단 `typescript.js`는 1). 디스크의 lib 파일은 100개이며 누락된 하나가
  `lib.d.ts`임을 `comm`으로 확인.
- 2026-08-18: `editors/vscode/.vscodeignore` — 부정 패턴 3줄을
  `!server/node_modules/typescript/lib/lib*.d.ts` 한 줄로 정리하고 주석을
  실제 메커니즘으로 교체. 재측정 → **100**,
  `lib.esnext.full.d.ts` 포함 확인, 총 628파일.
- 2026-08-18: 실제 vsix 두 개를 만들어 비교.
  - 수정 전: `/tmp/rl-base.vsix` 530파일 / 4.44MB, lib 0개.
  - 수정 후: `/tmp/rl-test.vsix` 630파일 / 4.92MB, lib 100개
    (`extension/server/node_modules/typescript/lib/lib.esnext.full.d.ts` 존재).
  - 각각 압축을 풀어 그 트리의 `typescript`로
    `ts.getDefaultLibFilePath({target: ESNext})`가 가리키는 파일의 존재를
    확인 → 수정 전 `false`, 수정 후 `true`.
- 2026-08-18: **E2E 확인** — 압축 푼 확장 트리의 `server/out/tsproject.js`를
  직접 `require`해, 신고 코드를 줄인 소스를 방출물로 서빙하고
  `diagnosticsFor()`를 호출하는 드라이버(`/tmp/e2e.mjs`)를 돌렸다.
  - 수정 전 트리 + TASK-058 이전 서버 코드(`getDefaultLibFileName`을 원래
    구현으로, 가드 제거):
    `TS2488 Type '[number, number]' must have a '[Symbol.iterator]()' method
    that returns an iterator.` **1건 재현**.
  - 수정 후 트리 + 현재 서버 코드: `env: sound`, **진단 0건**.
  - 참고로 수정 전 트리에 현재 서버 코드를 쓰면 진단 0건 + 환경 경고가
    나온다 — TASK-058 가드가 의도대로 동작함을 같이 확인.
- 2026-08-18: `editors/vscode/README.md` "개발"에 "### 패키징" 절 추가 —
  `npm ci && npx tsc -b && npx @vscode/vsce package --no-dependencies`와
  사후 확인 명령(`vsce ls | grep -c` → 100), 그리고 0이면 배포 금지.
- 2026-08-18: 확장 테스트 재실행
  (`PATH=<repo>/target/release:$PATH node --test "server/out/test/*.test.js"`)
  → 59/59 통과. rlc 게이트도 재실행.

## 이슈 및 해결

### 이슈 1: 제보의 `--no-dependencies` 필수 주장이 재현되지 않음

- **증상**: 제보는 vsce 기본(의존성) 모드가 `Extension entrypoint(s) missing`
  으로 **실패**한다고 했다.
- **원인**: 이 환경(vsce 3.9.2, `npm install` 직후 레이아웃)에서는 기본
  모드도 exit 0으로 성공했고, 산출물도 `--no-dependencies`와 동일한
  630파일 / 4.92MB였다. 제보자 쪽 vsce 버전이나 설치 레이아웃 차이로 보이며,
  여기서 원인을 특정할 수는 없었다.
- **해결**: README에는 양쪽 환경에서 확실히 동작하는 `--no-dependencies`를
  기록했다. 기본 모드가 실패한다고 단정하는 문장은 쓰지 않았다 — 확인하지
  못한 것을 문서에 규범으로 적지 않는다.

### 이슈 2: vsix에 확장이 쓰지 않는 TypeScript 진입점이 들어간다

- **증상**: `bin/tsc`, `lib/tsc.js`, `lib/tsserver.js`, `lib/typingsInstaller.js`
  등 언어 서버가 전혀 쓰지 않는 파일이 패키지에 포함된다(결정 4에서 확인).
- **원인**: `node_modules/**`가 하위 디렉터리의 `node_modules`에 걸리지
  않으므로 `server/node_modules/*`는 통째로 포함된다. TASK-058 이전부터
  그랬고 이번 버그와는 무관하다.
- **해결**: **고치지 않았다.** 이번 변경은 "거짓 진단 제거"가 목적이고,
  제외 패턴을 새로 넣는 것은 잘못 빼면 확장이 조용히 죽는 종류의 위험이다
  (이번 버그가 정확히 그런 사고였다). 용량 최적화는 별도 판단으로 다룬다 —
  남은 부채로 여기 기록한다.

### 이슈 3: TASK-058 문서의 결정 5가 사실과 다름

- **증상**: TASK-058 결정 5는 "런타임에 필요한 것만 넣고 `tsc.js`/`tsserver.js`
  등은 뺀다"고 적었으나, 결정 4의 실측대로 그 파일들은 제외된 적이 없다.
- **원인**: `node_modules/**`의 매칭 범위를 확인하지 않고 추정으로 적었다.
- **해결**: TASK-058 문서에 이 태스크를 가리키는 정정 주석을 달았다(완료된
  태스크의 기록은 고쳐 쓰지 않고 정정을 덧붙인다). 실제 패턴은 이 태스크에서
  바로잡았다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`
- [x] `npx tsc -b` + `node --test "server/out/test/*.test.js"` → 59/59 통과
- [x] `npx @vscode/vsce ls --no-dependencies | grep -c "typescript/lib/lib"`
      → **100** (수정 전 0)
- [x] 패키징된 트리 E2E — 수정 전 `TS2488` 1건 재현 / 수정 후 0건

## 결과

| 파일 | 변경 |
|------|------|
| `editors/vscode/.vscodeignore` | 부정 패턴 3줄 → `!server/node_modules/typescript/lib/lib*.d.ts` 한 줄, 주석 교정 |
| `editors/vscode/README.md` | "### 패키징" — 절차와 사후 확인 명령 |
| `docs/tasks/TASK-058-...md` | 결정 5에 정정 주석 |

후속: [TASK-060](./TASK-060-editor-tsconfig-adoption.md) — 워크스페이스
`tsconfig.json` 반영 검토 (대기).
