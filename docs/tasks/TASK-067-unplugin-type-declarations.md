# TASK-067: unplugin-rl 타입 선언 — 소비자의 `vite.config.ts` 타입 검사

- **상태**: 완료
- **시작일**: 2026-08-18
- **완료일**: 2026-08-18
- **커밋**: —

## 목적

`unplugin-rl`은 npm에 배포되는 패키지인데 타입 선언을 싣지 않는다. 소비자가
빌드 설정(`vite.config.ts`)을 타입 검사 대상에 넣으면 바로 `TS7016`이 나고,
플러그인 옵션(`compiler`·`verify`)도 타입으로 받을 수 없다.

```
vite.config.ts(1,16): error TS7016: Could not find a declaration file for
module 'unplugin-rl/vite'. '.../integrations/unplugin/vite.js' implicitly
has an 'any' type.
```

새 예제(`source/rl-todo`)를 만들면서 드러났다. 기존 예제 둘은 `tsconfig.json`의
`include`가 소스만 담아(`calculator-cli`는 `src/**/*.ts`, `rl-interop`은
`build/**/*.ts`) 이 경로를 밟지 않았을 뿐이다.

## 범위

- 포함: `integrations/unplugin/`에 `index.d.ts`와 서브패스 7개(`vite`·`rollup`·
  `rolldown`·`webpack`·`rspack`·`esbuild`·`farm`)의 `.d.ts`, `package.json`의
  `types`·`exports.*.types`·`files` 갱신, README에 한 줄.
- 제외: JSDoc에서 선언을 생성하는 빌드 단계 도입. 선언은 손으로 유지한다.
- 제외: 소비자 쪽 `tsconfig` 관례 변경 — 예제가 빌드 설정을 타입 검사에 넣을지는
  각 예제의 선택으로 둔다.

## 의사결정

### 결정 1: 선언을 손으로 쓴다 (JSDoc 자동 생성이나 소비자 `allowJs`가 아니라)

- **상황**: 구현은 `index.js` 하나에 JSDoc(`@typedef Options`,
  `@type {UnpluginFactory<...>}`)이 붙어 있다. 타입을 소비자에게 전달하는
  방법이 셋이다.
- **검토한 대안**:
  - 소비자가 `allowJs`를 켠다: 패키지는 그대로 두고 JSDoc이 읽힌다. 대신
    이 패키지 하나 때문에 소비자 전체의 컴파일러 설정이 바뀌고, 배포된
    패키지를 쓰는 쪽에서는 `node_modules`가 타입 검사에 들어와 부담이 는다.
  - `tsc --allowJs --emitDeclarationOnly`로 생성: JSDoc과 자동으로 일치한다.
    대신 이 패키지에 빌드 단계와 devDependency(typescript)가 생기고, 지금까지
    "빌드 없는 순수 JS 패키지"였던 성질이 깨진다.
  - 손으로 쓴 `.d.ts`: 표면이 작다 — 옵션 2개와 unplugin 인스턴스 8개뿐이고,
    실제 타입은 전부 `unplugin`이 제공하는 제네릭에서 온다.
- **선택과 근거**: 손으로 쓴 `.d.ts`. 이 패키지의 공개 표면은 unplugin 팩토리
  하나와 서브패스 엔트리들이라 선언이 20줄대로 끝나고, 빌드 단계 없이
  `files`에 얹으면 된다. 표면이 늘면 그때 생성으로 옮긴다.

### 결정 2: `exports` 맵에 서브패스별 `types` 조건을 넣는다

- **상황**: 소비자는 `unplugin-rl/vite`처럼 서브패스로만 import한다. 루트
  `types` 필드만으로는 서브패스가 해석되지 않는다.
- **검토한 대안**: 루트 `types`만 두기 / `typesVersions` 매핑 / `exports`의
  각 항목을 `{ types, default }` 객체로.
- **선택과 근거**: 세 번째. `exports`를 이미 쓰고 있고, `moduleResolution`이
  `bundler`·`node16`·`nodenext` 어디든 같은 방식으로 해석된다.
  `typesVersions`는 `exports`가 없는 패키지를 위한 옛 관례다. 루트 `types`도
  `moduleResolution: node10` 소비자를 위해 함께 남겼다.

## 작업 내역

- 2026-08-18: 새 예제 `source/rl-todo`의 `tsconfig.json` `include`에
  `vite.config.ts`를 넣은 상태에서 `npx tsc --noEmit`을 돌려 `TS7016`을
  확인했다.
- 2026-08-18: `integrations/unplugin/index.d.ts` 작성 — `Options`
  인터페이스(`compiler`·`verify`), `unpluginFactory`,`unplugin`, 기본 export,
  번들러별 이름 7개. 타입은 `unplugin`의 `UnpluginFactory`/`UnpluginInstance`
  제네릭에서 가져온다.
- 2026-08-18: 서브패스 7개의 `.d.ts` 작성 — 각각
  `UnpluginInstance<Options | undefined>["<번들러>"]`를 기본 export.
- 2026-08-18: `package.json` 갱신 — `types`, `exports` 각 항목을
  `{ types, default }`로, `files`에 `.d.ts` 8개 추가.
- 2026-08-18: 확인.
  ```
  $ cd ../rl-todo && npx tsc --noEmit      # 무출력 (vite.config.ts 포함)
  $ cd ../rl-interop && npm run build      # ✓ 4 modules transformed
  ```

## 이슈 및 해결

없음.

## 검증

Rust 변경이 없으므로 컴파일러 게이트는 해당 없다.

- [x] 소비자 타입 검사 — `rl-todo`에서 `vite.config.ts` 포함 `tsc --noEmit` 통과
- [x] 회귀 — `rl-interop` vite 빌드 재확인 (출력 동일)
- [ ] `cargo fmt --check` / `clippy` / `cargo test` — 해당 없음

AI 제공 문서(`docs/ai/rl.md`)는 번들러 대안으로 `unplugin-rl`의 존재와 import
형태만 다루고 옵션 타입은 언급하지 않으므로 갱신할 내용이 없다.

## 결과

- 추가: `integrations/unplugin/index.d.ts`, 서브패스 `.d.ts` 7개,
  `docs/tasks/TASK-067-unplugin-type-declarations.md`
- 수정: `integrations/unplugin/package.json`, `integrations/unplugin/README.md`,
  `docs/tasks/INDEX.md`

후속: 선언과 구현이 어긋나지 않는지 확인하는 수단이 없다. 표면이 늘면
JSDoc 기반 생성(`tsc --allowJs --emitDeclarationOnly`)으로 옮기고 CI에서
`git diff --exit-code`로 지키는 방법을 검토한다.
