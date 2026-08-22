# 표준 라이브러리 멤버 단위 트리셰이킹 조사

- 조사일: 2026-08-22
- 관련 태스크: [TASK-162](../tasks/TASK-162-stdlib-member-pruning.md)
- 질문: 현재 `Option.map` / `Result.andThen` 객체 API를 유지하면서 번들러가
  실제로 사용된 멤버만 포함하게 할 수 있는가?

## 결론

현재 객체 API만으로는 일반적인 번들러의 멤버 단위 제거를 기대할 수 없다.
`Option.Some`을 읽으면 `Option` 객체 선언이 사용된 선언이 되고, 객체 리터럴에
함께 들어 있는 다른 프로퍼티도 남는다. 반면 각 연산을 독립 ESM export로 만들면
namespace import를 포함해 번들러가 사용하지 않은 선언을 제거할 수 있다.

따라서 rlc가 프로젝트 전체 사용량을 분석해 매번 std 소스를 잘라내는 방식보다,
std의 원본 표현을 독립 ESM export로 바꾸는 방식이 책임 경계와 도구 호환성 면에서
우선이다. 최종 API는 타입을 `TOption`/`TResult`로 구분하고 런타임 연산을
`@rl/std/option`과 `@rl/std/result`에서 namespace import한다.

**출처:**

- [Official Documentation] esbuild API: Tree shaking — tree shaking을
  declaration-level dead-code removal로 정의하고, namespace import의 미사용 export
  제거 예제를 제공한다.
  https://esbuild.github.io/api/#tree-shaking
- [Official Documentation] webpack: Tree Shaking — `usedExports`가 ESM export
  사용량을 표시하고 미사용 코드를 제거하는 구조를 설명한다.
  https://webpack.js.org/guides/tree-shaking/

## 관찰: 객체와 ESM export의 실제 차이

저장소에 설치된 Rolldown 1.2.5로 같은 기능을 세 형태로 묶었다. 입력은 `Some`
하나만 호출했고 모두 minify를 켰다.

| std 표현 | 소비 형태 | 결과 크기 | 미사용 멤버 |
|---|---|---:|---|
| 단일 객체 | `Option.Some(1)` | 163 B | 남음 |
| 독립 export | `import * as Option`; `Option.Some(1)` | 45 B | 제거됨 |
| 독립 export + 객체 facade | `import { Some }`; `Some(1)` | 45 B | 제거됨 |
| 독립 export + 객체 facade | `import { Option }`; `Option.Some(1)` | 157 B | 남음 |

재현 입력과 결과는 `/tmp/rl-std-research`에 두었다. 이 측정은 특정 축약 예제의
관찰이며 모든 번들러의 보장은 아니다. 다만 esbuild 공식 문서의 독립 선언 및
namespace import 예제와 같은 경계를 보인다.

Rollup의 `treeshake.propertyReadSideEffects`는 사용하지 않은 **프로퍼티 읽기**를
보존할지 정하는 옵션이다. 사용 중인 객체 리터럴에서 읽지 않은 프로퍼티 정의를
개별 export처럼 제거한다는 계약은 아니다. `@__PURE__`도 호출 전체를 제거할 수
있다는 표시이지, 사용 중인 호출의 인자나 반환 객체 프로퍼티를 API 사용량에 맞춰
분해한다는 표시가 아니다.

**출처:**

- [Official Documentation] Rollup Configuration:
  `treeshake.propertyReadSideEffects`
  https://rollupjs.org/configuration-options/#treeshake-propertyreadsideeffects
- [Official Documentation] Rollup Configuration: `treeshake.annotations`
  https://rollupjs.org/configuration-options/#treeshake-annotations
- [Official Documentation] esbuild API: Tree shaking
  https://esbuild.github.io/api/#tree-shaking

## 비교: 가능한 세 접근

### 1. rlc가 사용량을 분석해 객체를 선택 방출

AOT 빌드에서는 모든 입력을 읽으므로 구현할 수 있다. 그러나 값 자체 전달,
계산 프로퍼티, 재수출에서는 전체 객체 보존이 필요하다. 번들러 플러그인은 std
가상 모듈을 로드하는 시점에 전체 모듈 그래프가 확정됐다는 보장이 없으므로 같은
분석 결과를 안정적으로 만들기 어렵다. importer마다 다른 std 인스턴스를 만들면
같은 ESM 지정자가 하나의 모듈 인스턴스를 가리킨다는 사용자의 기대도 깨진다.

이 접근은 번들러가 이미 소유한 도달성 분석을 rlc와 플러그인에 중복 구현한다.
프로젝트의 TypeScript 통과 영역을 더 깊게 해석해야 하므로 rl의 “일반 TS는 그대로
통과” 계약과도 긴장이 생긴다.

### 2. 객체 API를 제거하고 독립 named export만 제공

가장 작은 번들을 안정적으로 만든다. `Option.map` 호출 형태는 유지하되 값은
`import * as Option from "@rl/std/option"`으로 가져온다. 타입은 `TOption<T>`로
이름을 분리하므로 namespace binding과 충돌하지 않는다.

TypeScript는 type-only import가 JavaScript 출력에서 제거됨을 보장한다. 따라서
타입과 런타임 함수를 독립 export로 제공하면 소비자는 필요한 런타임 선언만
가져오고 `TOption<T>` 타입은 비용 없이 유지할 수 있다.

### 3. 독립 named export를 원본으로 만들고 객체 facade를 병행

기존 코드는 그대로 동작한다. 새 API를 쓰는 코드만 즉시 멤버 단위 트리셰이킹을
얻는다. std 구현도 각 연산을 한 번만 정의하고 facade가 참조하므로 의미의 단일
원천을 유지할 수 있다. 단점은 두 API를 문서화해야 하고, 기존 객체 호출은 자동으로
작아지지 않는다는 점이다.

**출처:**

- [Official Documentation] TypeScript Modules Reference: Type-only imports
  and exports
  https://www.typescriptlang.org/docs/handbook/modules/reference.html#type-only-imports-and-exports
- [Official Documentation] esbuild API: ESM import/export 기반 tree shaking
  https://esbuild.github.io/api/#tree-shaking
- [Official Documentation] webpack: `sideEffects`와 `usedExports`의 차이
  https://webpack.js.org/guides/tree-shaking/#clarifying-tree-shaking-and-sideeffects

## 최종 선택

초기 조사에서는 단계적 이행을 위해 3번을 권장했다. 이후 API 형태를 검토하면서
호환성이 필요 없다는 제품 결정을 내렸으므로 2번을 선택했다. 타입 전용 루트는
`TOption`/`TResult`를 제공하고, 두 런타임 서브모듈은 생성자와 콤비네이터를 각각
독립 export한다. 소비자는 namespace import에 `Option`/`Result`라는 이름을 붙여
기존의 점 표기 사용감을 유지한다.

객체 facade와 컴파일러 사용량 분석은 도입하지 않는다. 번들러가 독립 ESM 선언의
도달성을 판단하는 하나의 책임 모델만 유지한다.

**출처:**

- [Official Documentation] esbuild API: namespace import에서도 독립 export를
  제거하는 예제
  https://esbuild.github.io/api/#tree-shaking
- [Official Documentation] TypeScript Modules: ESM 및 type-only import
  https://www.typescriptlang.org/docs/handbook/2/modules.html
