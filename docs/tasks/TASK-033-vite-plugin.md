# TASK-033: vite-plugin-rl — 번들러가 `.rl`을 직접 읽는다

- **상태**: 완료
- **시작일**: 2026-08-17
- **완료일**: 2026-08-17
- **커밋**: —

## 목적

지금까지는 `.rl`을 쓰려면 rlc가 **미리** 전체 트리를 `.ts`로 방출해야 했고,
그 트리를 완결시키려고 손으로 쓴 `.ts` 파일까지 출력 디렉터리에 복사됐다.
번들러 플러그인으로 그 중간 트리를 없앤다.

## 범위

- 포함: `integrations/vite/`(플러그인 패키지 + README), `rl-interop` 예제를
  Vite 빌드로 전환.
- 제외: dev 서버 시나리오. 예제는 CLI라 `vite build`(SSR 타깃)만 쓴다.
- 제외: 타입. 번들러 플러그인은 런타임만 해결하고, 에디터·tsc용 타입은
  사이드카가 계속 담당한다.

## 의사결정

### 결정 1: esbuild 대신 Vite 플러그인으로 만든다

- **상황**: 예제가 esbuild로 번들하고 있었고, 플러그인을 어느 쪽에 붙일지
  정해야 했다.
- **검토한 대안**: esbuild 플러그인 / Vite(Rollup) 플러그인.
- **선택과 근거**: Vite. 플러그인 API가 Rollup 호환이라 Vite와 순수
  Rollup 양쪽에서 쓰이고, 별도 패키지로 배포하기도 자연스럽다. esbuild
  플러그인은 esbuild 전용이다.

### 결정 2: 플러그인이 번들러 API를 쓰지 않게 만든다

- **상황**: rlc의 출력은 TypeScript이므로 JS로 한 번 더 변환해야 한다.
  처음에는 `vite`의 `transformWithEsbuild`를 불렀는데, `file:` 링크로
  설치하면 플러그인의 실제 경로에서 `vite`를 찾지 못해 실패했다
  (아래 이슈 1).
- **검토한 대안**:
  - `vite`를 dependency로 선언: 플러그인이 번들러 버전에 묶인다.
  - 소비 측에서 `vite`를 resolve: `createRequire`로 우회 — 취약하다.
  - 가상 id에 `.ts`를 붙여 **호스트의 TypeScript 처리에 태운다**.
- **선택과 근거**: 세 번째. `resolveId`가 `/abs/notice.rl.ts`를 돌려주면
  Vite의 esbuild 패스가 그 모듈을 TypeScript로 처리한다. 플러그인은
  `child_process`와 `path`만 쓰므로 의존성이 없고, Rollup에서도 TypeScript
  플러그인만 있으면 같은 코드가 동작한다.

### 결정 3: 플러그인 경로에서는 `--rewrite-imports off`를 쓴다

- **상황**: 기본값 `js`는 `.rl` 지정자를 `.js`로 바꾼다.
- **선택과 근거**: 플러그인이 `.rl` 지정자를 직접 해석하므로 재작성하면
  안 된다. 재작성은 미리 컴파일하는 파이프라인을 위한 기능이고, 번들러
  경로에서는 `.rl`이 그대로 남아야 다음 모듈도 플러그인이 잡는다.

### 결정 4: 사이드카 파이프라인은 남기되 캐시 디렉터리로 옮긴다

- **상황**: 번들에는 중간 트리가 필요 없어졌지만, 사이드카의 재료(선언)는
  여전히 tsc가 rlc 출력에서 뽑아야 한다.
- **선택과 근거**: 중간 산출물을 `.rl-build/`(gitignore)로 보내고, 그
  단계에만 `.rl`과 그것이 참조하는 `.ts`를 넘긴다. 소스 트리와 번들 경로
  어디에도 복사본이 남지 않는다. 사이드카는 별도 `.rl-types/`에 쓴다
  ([TASK-032](./TASK-032-sidecar-out-dir.md)).

## 작업 내역

- 2026-08-17: `integrations/vite/index.js`·`package.json`·`README.md` 작성.
  `resolveId`가 `.rl` → `<파일>.rl.ts` 가상 id, `load`가
  `rlc -p --rewrite-imports off` 결과를 돌려준다. 실패 시 `this.error`로
  rlc 진단을 그대로 올린다. `addWatchFile`로 원본을 감시 대상에 넣는다.
- 2026-08-17: `rl-interop`을 전환 — `vite.config.ts`(SSR 타깃),
  `vite-plugin-rl`을 `file:` 의존성으로, esbuild 제거,
  `tsconfig.types.json`을 `.rl-build` 기준으로 재작성.
- 2026-08-17: 확인.
  ```
  ✓ 3 modules transformed.  dist/main.js  1.31 kB
  $ node dist/main.js        (출력은 전환 전과 동일)
  진단: 없음   정의 이동 → src/notice.rl:21:17
  ```
- 2026-08-17: 컴파일 에러 전달도 확인 — `Warn` 암을 지우고 빌드하면
  `[vite-plugin-rl] src/notice.rl:22:16: match on enum Notice is not
  exhaustive: missing "Warn"`가 빌드 에러로 나온다.

## 이슈 및 해결

### 이슈 1: 플러그인이 `vite`를 찾지 못했다

- **증상**: `Cannot find package 'vite' imported from
  .../integrations/vite/index.js`로 빌드가 실패했다.
- **원인**: 플러그인이 `transformWithEsbuild`를 쓰려고 `vite`를 import했는데,
  `file:` 의존성은 심링크라 Node가 **플러그인의 실제 경로**에서 `vite`를
  찾는다. 그 경로에는 `node_modules`가 없다.
- **해결**: vite import를 없앴다(결정 2). 가상 id에 `.ts`를 붙여 호스트가
  TypeScript로 처리하게 하니 플러그인이 의존성 없는 순수 Node 코드가 됐다.

### 이슈 2: 선언 출력 경로가 한 겹 더 들어갔다

- **증상**: 사이드카 단계가 `.rl-build/types/notice.d.ts: No such file`로
  실패했다. 실제 파일은 `.rl-build/types/.rl-build/notice.d.ts`에 있었다.
- **원인**: `tsconfig.types.json`에 `rootDirs`를 넣어 `src/format.ts`가
  프로그램에 딸려 들어오자 tsc가 공통 루트를 프로젝트 루트로 잡았다.
- **해결**: `rootDirs`를 빼고 `rootDir: ".rl-build"`로 고정한 뒤, 그 단계에
  `.rl`과 참조되는 `.ts`를 함께 넘겨 `.rl-build` 안에서 해석되게 했다.

## 검증

Rust 변경이 없으므로 컴파일러 게이트는 해당 없다.

- [x] `vite build` — 3 modules, `dist/main.js` 1.31 kB
- [x] `node dist/main.js` — 출력이 전환 전과 동일
- [x] tsserver 구동 — 진단 없음, 정의 이동 `→ src/notice.rl:21:17`
- [x] 컴파일 에러가 빌드 에러로 전달되는지 확인
- [ ] `cargo fmt --check` / `clippy` / `cargo test` — 해당 없음

## 결과

- 추가: `integrations/vite/{index.js,package.json,README.md}`,
  `docs/tasks/TASK-033-vite-plugin.md`
- 수정: `docs/tasks/INDEX.md`

후속: Rollup·webpack 통합, dev 서버(HMR)에서의 동작 확인, 그리고 사이드카
생성을 플러그인이 빌드 시 함께 해 주는 옵션.
