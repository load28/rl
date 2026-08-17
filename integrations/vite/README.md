# vite-plugin-rl

Vite와 Rollup에서 `.rl` 모듈을 그대로 import합니다. 중간 `.ts` 트리를 만들지
않고, 번들러가 소스를 직접 읽습니다.

```ts
// vite.config.ts
import { defineConfig } from "vite";
import { rl } from "vite-plugin-rl";

export default defineConfig({ plugins: [rl()] });
```

```ts
// src/main.ts — 평범한 TypeScript
import { Notice, render } from "./notice.rl";
```

`rlc`가 PATH에 있어야 합니다 (`cargo install --path .`).

## 동작

| 단계 | 하는 일 |
|------|---------|
| `resolveId` | `.rl` 지정자를 파일 경로로 풀고 `.ts`를 덧붙인 가상 id를 돌려줍니다 |
| `load` | `rlc -p --rewrite-imports off`로 컴파일한 TypeScript를 돌려줍니다 |

id에 `.ts`를 붙이는 이유는 **호스트의 TypeScript 처리에 그대로 태우기**
위해서입니다. 덕분에 플러그인은 번들러 API를 전혀 쓰지 않고, Vite와 순수
Rollup(+ TypeScript 플러그인)에서 같은 코드로 동작합니다.

`--rewrite-imports off`인 것도 의도입니다. 지정자 재작성은 미리 컴파일하는
파이프라인을 위한 기능이고, 여기서는 `.rl`이 그대로 남아야 이 플러그인이
다음 모듈도 잡습니다.

컴파일 에러는 rlc의 진단이 그대로 빌드 에러가 됩니다.

```
[vite-plugin-rl] src/notice.rl:22:16: match on enum Notice is not exhaustive:
                 missing "Warn" (add the missing arms or a final `_` arm)
```

## 옵션

| 옵션 | 기본값 | 설명 |
|------|--------|------|
| `compiler` | `"rlc"` | rlc 실행 파일 경로 |
| `verify` | `true` | `false`면 `--no-verify`를 넘겨 방출물 자가 검사를 생략합니다 |

## 타입은 별도입니다

번들러 플러그인은 **런타임만** 해결합니다. `.ts` 파일이 `.rl`을 import할 때
타입 검사와 정의 이동이 동작하려면 사이드카가 필요합니다.

```sh
rlc --sidecar <선언 디렉터리> -o .rl-types src/x.rl
```

자세한 내용은 [`docs/reference/cli.md`](../../docs/reference/cli.md)의
"에디터 사이드카" 절과 [VSCode 확장](../../editors/vscode/README.md)을
참조하세요. 확장은 저장할 때마다 사이드카를 갱신합니다.
