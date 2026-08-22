# TASK-162: 사용된 표준 라이브러리 멤버만 방출

- **상태**: 완료
- **시작일**: 2026-08-22
- **완료일**: 2026-08-23
- **커밋**: —

## 목적

`@rl/std`에서 `Option` 또는 `Result`를 가져오면 사용하지 않는 콤비네이터까지
하나의 객체 리터럴에 포함되어 최종 번들에 남는다. 표준 라이브러리를 멤버 단위
트리셰이킹이 가능한 ESM 표면으로 바꾸고 타입과 런타임 namespace의 이름을 분리한다.

## 범위

- 포함: 독립 ESM runtime export, `TOption`/`TResult` 타입 API, 런타임 서브패스,
  번들 크기 회귀 테스트, 사용자 문서와 마이그레이션 예제.
- 제외: 사용자 TypeScript의 일반 목적 트리셰이킹, 번들러 자체 최적화 정책 변경.

## 의사결정

### 결정 1: 컴파일러 사용량 분석보다 독립 ESM export를 우선한다

- **상황**: `Option.map` 같은 객체 접근을 유지하면서 rlc가 필요한 프로퍼티만
  방출할지, std 자체를 번들러가 이해하는 ESM 선언 단위로 바꿀지 선택해야 한다.
- **검토한 대안**: ① rlc가 전체 프로젝트 사용량을 분석해 선택 방출 — AOT에는
  가능하지만 동적 사용은 전체 보존이 필요하고 플러그인의 모듈 그래프 수명주기와
  중복된다. ② 객체 API 제거 — 최적 결과지만 기존 코드를 깨뜨린다. ③ 독립 named
  export를 원본으로 두고 객체 facade 병행 — 새 API는 가지치기되고 기존 API도
  유지된다.
- **선택과 근거**: 조사 단계에서는 ③을 우선안으로 정했다. Rolldown 1.2.5
  실측에서 `Some`만 사용한 축약 번들은 객체 API 163바이트, 독립 export의
  namespace API 45바이트였다. 세부 근거와 공식 문서 출처는
  [`stdlib-tree-shaking-research.md`](../design/stdlib-tree-shaking-research.md)에
  기록했다.

### 결정 2: 타입과 런타임 namespace를 분리하고 호환 facade를 제거한다

- **상황**: `Option<T>` 타입과 `import * as Option`은 같은 로컬 이름을 사용할 수
  없다. 기존 객체 facade를 남기면 해당 경로를 쓰는 번들은 계속 전체 멤버를
  포함한다.
- **검토한 대안**: ① 타입 이름을 유지하고 런타임 이름을 바꾼다. ② 타입을
  `TOption`/`TResult`로 명시하고 런타임 namespace를 `Option`/`Result`로 둔다.
  ③ 기존 객체 facade를 함께 제공한다.
- **선택과 근거**: ②를 선택하고 호환성은 제공하지 않는다. 사용자가 타입과 값의
  역할을 이름에서 구분할 수 있고, `Option.map`/`Result.andThen` 점 표기는 유지된다.
  모든 런타임 진입점이 독립 ESM export만 갖기 때문에 새 API의 목적도 우회 없이
  보장된다.

## 작업 내역

- 2026-08-22: 기존 std 방출 경로와 TASK-035 기록을 확인했다. 현재
  `STD_SOURCE` 12,486바이트가 프로젝트·번들러 가상 모듈에 항상 통째로
  제공되고, `Option`/`Result` 객체 프로퍼티는 번들러가 개별 제거하지 못한다.
- 2026-08-22: Rollup·esbuild·webpack·TypeScript 공식 문서를 조사하고,
  Rolldown 1.2.5로 객체/독립 export/병행 facade를 비교했다. 결과와 권장안을
  `docs/design/stdlib-tree-shaking-research.md`에 기록했다.
- 2026-08-23: std를 타입 전용 `@rl/std`와 런타임
  `@rl/std/option`·`@rl/std/result`의 세 물리 모듈로 분리했다. 타입은
  `TOption`·`TResult`·`TOk`·`TErr`·`TErrorOf`로 바꿨고 각 런타임 연산은 독립
  ESM export로 정의했다.
- 2026-08-23: parser·AST·HIR·codegen의 std 지정자를 모듈 단위로 모델링하고,
  CLI AOT 방출·선언 sidecar·native engine VFS·unplugin 가상 모듈을 세 경로에
  맞췄다.
- 2026-08-23: 컴파일·통합·native·emit-map·stdlib 테스트와 사용자 문서,
  VS Code 테스트 fixture, 웹사이트 예시를 새 API로 갱신했다.
- 2026-08-23: `src/stdlib/{types,option,result}.ts`를 추가하고 기존
  `src/stdlib/rl_std.ts`를 제거했다. `src/{ast,parser/imports,hir,core_ir,
  codegen,lib,main}.rs`에는 `StdModule`·`StdImports`와 모듈별 지정자 재작성을
  반영했다.
- 2026-08-23: `src/engine/{projection,language,semantics}.rs`가 세 std 모듈을
  native project와 선언 출력에 싣도록 바꿨다. `integrations/unplugin/index.js`는
  세 가상 모듈과 내부 type-only 상대 import를 해석하도록 바꿨다.
- 2026-08-23: `tests/stdlib.rs`에 실제 Rolldown 번들 회귀 테스트를 추가하고
  CI에 Rolldown 1.2.5 설치를 고정했다. `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test`를 실행했고 모두
  통과했다. 홈페이지는 `bun run typecheck`와 `bun run build`로 확인했다.

## 이슈 및 해결

- **증상**: 출력 루트와 std 디렉터리가 바로 맞닿은 경우 import가
  `rl/option.js`처럼 방출되어 TypeScript가 bare package로 해석했다.
  **원인**: 상대 경로가 `rl`일 때 `.`으로 시작하는지만 검사해 `./` 접두사를
  붙이지 않았다. **해결**: 상위 이동 경로가 아닌 모든 상대 경로에 `./`를 붙이는
  규칙으로 고치고 AOT 통합 테스트로 고정했다.
- **증상**: 홈페이지 빌드의 prerender 단계가 샌드박스에서
  `listen EPERM ::1`로 실패했다. **원인**: 검증용 preview 서버의 로컬 포트
  바인딩이 제한됐다. **해결**: 같은 `bun run build`를 승인된 샌드박스 외부에서
  다시 실행해 29개 페이지 prerender까지 확인했다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`

## 결과

타입 전용 루트와 두 런타임 서브모듈로 std를 분리했다. 런타임 namespace의 각
멤버는 독립 ESM export이므로 번들러가 실제 사용한 선언만 남긴다. Rolldown
1.2.5 회귀 테스트에서 `Option.Some`만 사용한 번들에 `None`이 남지 않음을
확인했다. Rust 전체 게이트, 웹사이트 typecheck·build, VS Code의 변경 관련 std
completion·signature 테스트가 통과했다.
