# 프로젝트 프론트엔드 — rlc 역할 변경 제안

이 문서는 **제안**이다. 규범 문서가 아니다 (구현된 구조는
[`compiler-architecture.md`](./compiler-architecture.md), 모듈 그래프의
1~3단계는 [`module-graph.md`](./module-graph.md)에서 다뤘고 전부 구현됐다).
TASK-026에서 작성했다.

## 목표

소스에서 **확장자 경계를 없앤다.** `.rl`이 `.ts`를 import하든 `.ts`가 `.rl`을
import하든 사용자는 신경 쓰지 않고, rlc가 소스 트리 전체를 받아 TypeScript
트리를 낸다.

rlc는 "파일 하나를 전처리하는 도구"에서 **"프로젝트 프론트엔드"**로 바뀐다.

## 지금 어디까지 되나

`/tmp/rlmix`에서 실측한 결과다 (rlc 0.3.0).

| 방향 | 소스 표기 | 지금 |
|------|-----------|------|
| `.rl` → `.rl` | `./shape.rl` | ✅ `./shape.js`로 재작성 |
| `.ts` → `.rl` | `./shape.rl` | ✅ 재작성됨 — 단 그 `.ts`를 rlc 입력으로 **명시**해야 한다 |
| `.rl` → `.ts` | `./util.ts` | ❌ 그대로 남는다 |

`.ts`가 `.rl`을 가리키는 방향이 이미 동작하는 것은 계약 1 덕분이다 — 유효한
TypeScript는 그대로 통과하므로, rlc는 `.ts` 파일을 통과시키면서 지정자만
고칠 수 있다.

막는 것은 세 가지다.

1. 디렉터리 입력은 `.rl`만 수집한다 ([cli.md](../reference/cli.md) "입력 수집").
   프로젝트를 한 번에 돌릴 수 없다.
2. `-o` 없이 `.ts`를 컴파일하면 출력 경로가 입력과 같아 **원본을 덮어쓴다**
   (`rlc: inplace.ts → inplace.ts`, `@generated` 배너까지 붙는다).
3. `.rl` → `.ts` 방향 지정자를 다루는 규칙이 없다.

## 층 분리 — 각 도구는 자기 층의 확장자만 안다

핵심은 **rlc가 JS를 모르게 하는 것**이다. rlc의 산출물은 TypeScript이므로,
방출하는 지정자도 TypeScript 세계의 것이어야 한다.

| 층 | 입력 | 출력 | 지정자 |
|----|------|------|--------|
| rlc | `.rl` + `.ts` 소스 트리 | TS 트리 | `./x.rl` → `./x.ts` |
| tsc | TS 트리 | JS | `./x.ts` → `./x.js` |

이 배치에서 `.rl` → `.ts` 방향(`./util.ts`)은 **재작성할 필요가 없다.** 이미
최종 형태다. 위의 막는 것 3번이 사라진다.

### `.ts` 지정자가 성립하는 근거

`allowImportingTsExtensions`만 켜면 `TS5096`으로 emit이 막히지만,
**`rewriteRelativeImportExtensions`**(TypeScript 5.7+)를 함께 켜면 통과한다.
tsc 5.9.3에서 확인했다.

```jsonc
// 소비 측 tsconfig.json
{ "compilerOptions": {
    "allowImportingTsExtensions": true,
    "rewriteRelativeImportExtensions": true } }
```

```ts
// 입력  n.ts
import { x } from "./m.ts";
// 방출  out/n.js
require("./m.js")          // tsc가 emit 시점에 바꾼다
```

즉 확장자 재작성은 두 번 일어나고, 각 단계가 자기 몫만 한다:
`.rl` →(rlc)→ `.ts` →(tsc)→ `.js`.

## 필요한 변경

### 1. `--rewrite-imports ts` 모드

현재 값은 `js`(기본) / `bare` / `off`다. `ts`를 추가한다 — `./x.rl`을
`./x.ts`로 방출한다. 역할 변경이 완료되는 시점에 기본값을 `ts`로 옮긴다.

`js`와 `bare`는 유지한다. `rewriteRelativeImportExtensions`를 쓸 수 없는
프로젝트(TypeScript 5.7 미만, 번들러 해석)가 있기 때문이다.

### 2. 입력 수집에 `.ts` 포함

디렉터리 순회에서 `.rl`과 `.ts`를 모두 수집한다. `.ts`는 계약 1에 따라 통과
대상이고, 지정자 재작성만 적용된다.

수집 대상이 넓어지므로 제외 규칙이 필요하다 — 최소한 출력 디렉터리와
`node_modules`는 건너뛰어야 한다.

### 3. 소스 트리와 출력 트리 분리

`.ts`가 입력이 되면 "입력 파일 옆에 같은 이름의 `.ts`"라는 기본 출력 규칙이
곧 원본 덮어쓰기가 된다. 프로젝트 모드에서는 `-o`를 필수로 하거나, `.ts`
입력에 한해 제자리 출력을 거부해야 한다.

### 4. 그래프에 `.ts` 노드 포함

소진성 검사의 선언 수집(TASK-022)은 `.rl`만 따라간다. `.ts` 파일은 rl enum을
선언할 수 없으므로 선언 제공자로는 무의미하지만, **`.rl`을 재수출하는
경유지**가 될 수 있다. 전이 수집이 필요해지는 첫 사례다.

## 결정해야 하는 것

1. **기본값 전환** — `ts`를 언제 기본으로 삼을지. 전환하면 기존 사용자의
   tsconfig에 두 옵션이 필요해진다. 메이저 버전을 끊을지, 경고를 한 릴리스
   먼저 낼지.
2. **제자리 출력 금지 범위** — `.ts` 입력에만 막을지, 프로젝트 모드 전체에서
   `-o`를 필수로 할지.
3. **`.ts` 입력의 배너** — 지금은 통과 파일에도 `@generated` 배너가 붙어
   "유효한 TS는 자기 자신으로 컴파일된다"는 계약과 어긋나 보인다. 통과 파일에는
   배너를 생략할지 정해야 한다.
4. **tsconfig 요구사항의 위치** — 두 옵션을 문서로만 안내할지, rlc가 확인해
   경고할지. 후자는 rlc가 TypeScript 설정을 읽기 시작한다는 뜻이다.

## 범위 밖

- **타입 검사** — tsc의 책임으로 남긴다.
- **`node_modules` 해석** — 상대 경로만 다룬다.
- **`.tsx`** — 별개 제약이 있다 ([language.md §11](../reference/language.md#11-제한사항)).
