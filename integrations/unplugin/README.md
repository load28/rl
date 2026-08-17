# unplugin-rl

`.rl` 모듈을 번들러에서 그대로 import합니다. 중간 `.ts` 트리를 만들지 않고,
번들러가 소스를 직접 읽습니다.

[unplugin](https://github.com/unjs/unplugin) 기반이라 구현은 하나이고 번들러별
서브패스로 갈라져 나갑니다.

```ts
// vite.config.ts
import { defineConfig } from "vite";
import rl from "unplugin-rl/vite";

export default defineConfig({ plugins: [rl()] });
```

```ts
// src/main.ts — 평범한 TypeScript
import { Notice, render } from "./notice.rl";
```

`rlc`가 PATH에 있어야 합니다 (`cargo install --path .`).

## 서브패스

| import | 상태 |
|--------|------|
| `unplugin-rl/vite` | 예제(`rl-interop`)로 검증 |
| `unplugin-rl/esbuild` | 번들·실행 검증 |
| `unplugin-rl/rollup`, `/rolldown`, `/webpack`, `/rspack`, `/farm` | unplugin이 제공하는 어댑터 — 미검증 |

`unplugin-rl`를 그대로 import하면 `unplugin` 객체와 `vitePlugin`·
`esbuildPlugin` 같은 이름들이 나옵니다.

## 동작

| 단계 | 하는 일 |
|------|---------|
| `resolveId` | `.rl` 지정자를 파일 경로로 풀고 `.ts`를 덧붙인 가상 id를 돌려줍니다. `@rl/std`는 가상 모듈 id로 바꿉니다 |
| `load` | `rlc -p --rewrite-imports off`(표준 라이브러리는 `rlc --emit-std`)의 출력을 돌려줍니다 |

id에 `.ts`를 붙이는 이유는 **호스트의 TypeScript 처리에 그대로 태우기**
위해서입니다. 덕분에 플러그인이 변환을 직접 하지 않습니다. 다만 esbuild의
`load`는 JavaScript만 반환할 수 있어서, 그 경로에는 `loader: "ts"`를
명시합니다.

`--rewrite-imports off`인 것도 의도입니다. 지정자 재작성은 미리 컴파일하는
파이프라인을 위한 기능이고, 여기서는 `.rl`이 그대로 남아야 이 플러그인이
다음 모듈도 잡습니다.

컴파일 에러는 rlc의 진단이 그대로 빌드 에러가 됩니다.

```
[unplugin-rl] src/notice.rl:22:16: match on enum Notice is not exhaustive:
              missing "Warn" (add the missing arms or a final `_` arm)
```

## 옵션

| 옵션 | 기본값 | 설명 |
|------|--------|------|
| `compiler` | `"rlc"` | rlc 실행 파일 경로 |
| `verify` | `true` | `false`면 `--no-verify`를 넘겨 방출물 자가 검사를 생략합니다 |

## 타입은 별도입니다

번들러 플러그인은 **런타임만** 해결합니다. `.ts` 파일이 `.rl`을 import할 때
타입 검사와 정의 이동이 동작하려면 사이드카가 필요하고, 그건 rlc가 만듭니다.

```sh
rlc --types src/        # .rl-types/<이름>.rl.d.ts + .map
```

자세한 내용은 [`docs/reference/cli.md`](../../docs/reference/cli.md)와
[VSCode 확장](../../editors/vscode/README.md)을 참조하세요. 확장은 저장할
때마다 사이드카를 갱신합니다.

## 알려진 제약

- `enforce: "pre"`는 Rollup·esbuild에서 무시됩니다 (unplugin 문서의 지원 훅
  표). 그 두 곳에서는 플러그인 순서를 직접 앞에 두세요.
- `resolveId`는 Rspack·Rsbuild에서 최신 버전을 요구합니다.
