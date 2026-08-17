# TASK-038: unplugin-rl — 번들러 어댑터를 하나의 구현으로

- **상태**: 완료
- **시작일**: 2026-08-17
- **완료일**: 2026-08-17
- **커밋**: —

## 목적

번들러 통합이 Vite(+Rollup) 하나뿐이라, 다른 번들러를 쓰는 프로젝트는
`resolve.alias` 같은 번들러별 설정을 직접 써야 했다. 구현 하나를 번들러별
엔트리로 내보내 사용자 설정을 "플러그인 한 줄"로 줄인다.

## 범위

- 포함: `integrations/unplugin/`(`unplugin-rl`) 신설 — unplugin 팩토리와
  서브패스 엔트리 7개, README. `integrations/vite` 제거. `rl-interop` 전환.
- 제외: Rollup·webpack·Rspack·Farm 경로의 실측(어댑터는 넣었지만 검증은
  Vite·esbuild까지).
- 제외: 베이스 tsconfig 패키지(`@rl/tsconfig`) — 별도 태스크.

## 의사결정

### 결정 1: 손으로 어댑터를 쓰지 않고 unplugin을 쓴다

- **상황**: 번들러마다 플러그인 API가 다르다. 직접 어댑터를 쓸지, 통합
  레이어를 쓸지.
- **검토한 대안**:
  - 번들러별 수동 어댑터: 의존성이 없다. 대신 9종을 직접 떠안고, 훅 차이와
    버전 문제를 계속 따라가야 한다.
  - unplugin: 팩토리 하나를 `unplugin.vite`/`.webpack`/`.esbuild`/... 로
    변환해 준다. 서브패스 exports도 공식 관례로 문서화돼 있다.
- **선택과 근거**: unplugin. 기존 구현이 `resolveId`/`load` 두 훅만 쓰고,
  둘 다 unplugin의 공통 지원 훅 목록에 있어 그대로 옮겨졌다. 의존성이
  하나 늘지만, 어댑터 9종을 직접 유지하는 비용보다 작다.

### 결정 2: esbuild 경로는 `loader: "ts"`로 명시한다

- **상황**: unplugin 문서에 "esbuild의 `load`/`transform`은 JavaScript만
  반환할 수 있다"고 적혀 있다. rlc가 내놓는 것은 TypeScript다.
- **검토한 대안**: esbuild만 제외 / 플러그인이 직접 TS→JS 변환 / unplugin의
  esbuild 옵션으로 로더를 명시.
- **선택과 근거**: 세 번째. `esbuild: { loader: "ts", onResolveFilter,
  onLoadFilter }`만 더하면 되고, 플러그인이 변환기를 품지 않는다는 원래
  성질(TASK-033 결정 2)이 유지된다. `/tmp/rlesb`에서 번들·실행으로
  확인했다.

### 결정 3: 검증한 경로와 그렇지 않은 경로를 README에 구분해 적는다

- **상황**: 어댑터 7종을 내보내지만 실제로 돌려본 것은 둘이다.
- **선택과 근거**: 표에 상태를 적었다 — Vite(예제로 검증), esbuild(검증),
  나머지는 "unplugin이 제공하는 어댑터, 미검증". 검증하지 않은 것을 검증한
  것처럼 적지 않는다.

## 작업 내역

- 2026-08-17: unplugin 문서를 확인했다 (createUnplugin, 서브패스 exports
  관례, 지원 훅 표와 esbuild·`enforce` 제약).
- 2026-08-17: `integrations/unplugin/index.js` 작성 — 기존 Vite 플러그인의
  `resolveId`/`load`를 `UnpluginFactory`로 옮기고 `esbuild` 옵션을 더했다.
  `vite.js`·`rollup.js`·`rolldown.js`·`webpack.js`·`rspack.js`·`esbuild.js`·
  `farm.js` 엔트리와 `exports` 맵.
- 2026-08-17: `integrations/vite` 삭제, README를 새 패키지로 옮겨 다시 썼다.
  현재 문서(`README.md`, `cli.md`, `std.md`)의 참조를 갱신했다 — 과거 태스크
  문서는 그 시점의 기록이므로 그대로 뒀다.
- 2026-08-17: `rl-interop`을 `unplugin-rl/vite`로 전환했다.
- 2026-08-17: 확인.
  ```
  $ node -e '...'                 # 엔트리 로드
  named: default, esbuildPlugin, farmPlugin, rolldownPlugin, rollupPlugin,
         rspackPlugin, unplugin, unpluginFactory, vitePlugin, webpackPlugin
  $ cd rl-interop && npm run build   # vite
  ✓ 3 modules transformed.  dist/main.js 1.31 kB   → 실행 출력 동일
  $ node build.mjs                   # esbuild (/tmp/rlesb)
  esbuild ok → node out.js → 5       # half(10)
  ```

## 이슈 및 해결

없음. `file:` 링크로 설치한 패키지가 자기 의존성(`unplugin`)을 자기 경로에서
찾아야 하는 것(TASK-033 이슈 1과 같은 성질)은 패키지 디렉터리에서
`npm install`을 한 번 돌려 해결했다.

## 검증

Rust 변경이 없으므로 컴파일러 게이트는 해당 없다.

- [x] 엔트리 7개 로드 확인
- [x] Vite 경로 — `rl-interop` 빌드·실행, 출력 동일
- [x] esbuild 경로 — 번들·실행 (`/tmp/rlesb`)
- [ ] `cargo fmt --check` / `clippy` / `cargo test` — 해당 없음

## 결과

- 추가: `integrations/unplugin/{index.js,package.json,README.md}`,
  엔트리 7개, `docs/tasks/TASK-038-unplugin.md`
- 삭제: `integrations/vite/`
- 수정: `README.md`, `docs/reference/cli.md`, `docs/reference/std.md`,
  `docs/tasks/INDEX.md`

후속: 베이스 tsconfig 패키지로 타입 쪽 설정도 `extends` 한 줄로 줄이는 일,
그리고 나머지 어댑터(Rollup·webpack·Rspack·Farm)의 실측.
