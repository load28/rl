# rl

> Rust 스타일 `enum`과 `match`를 더한, TypeScript로 컴파일되는 언어

[![CI](https://github.com/load28/rl/actions/workflows/ci.yml/badge.svg)](https://github.com/load28/rl/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)
[![Rust 1.88+](https://img.shields.io/badge/rust-1.88%2B-orange.svg)](./Cargo.toml)

## 목표

TypeScript에 **일곱 구문**과 **바인딩 수식자 하나**만 더합니다 — 태그드 유니언
`enum`, 패턴 매칭 `match`(태그·리터럴·튜플·중첩 패턴), 에러 전파 `try`, 값 추출
`let-else`와 `if let`, 파이프라인 `|>` (함수 합성 `flow` 포함), `Result` 계산
블록 `result`, 그리고 변경 금지 `val`. 그리고 두 가지를 지킵니다.

1. **모든 유효한 TypeScript 파일은 그대로 유효한 `.rl` 파일이고, 자기
   자신으로 컴파일된다.**
2. **방출되는 코드는 타입 트릭 없는 순수 TypeScript다.** 런타임을 주입하지
   않고, 빠진 `match` 암 같은 rl 수준 에러는 컴파일러가 `파일:행:열`과 함께
   직접 보고한다.

## 예시

```rl
// shapes.rl
export enum Shape {
  Circle(radius: number),
  Rect(width: number, height: number),
  Point,
}

export const area = (s: Shape): number =>
  match (s) {
    Circle(radius) => Math.PI * radius * radius,
    Rect(width, height) => width * height,
    Point => 0,
  };
```

```ts
// shapes.ts — rlc가 방출한 것
export type Shape =
  | { kind: "Circle"; radius: number }
  | { kind: "Rect"; width: number; height: number }
  | { kind: "Point" };
export const Shape = {
  Circle: (radius: number): Shape => ({ kind: "Circle", radius }),
  Rect: (width: number, height: number): Shape => ({ kind: "Rect", width, height }),
  Point: { kind: "Point" } as const,
};
export const area = (s: Shape): number =>
  ((() => {
  const $rl_m = (s);
  switch ($rl_m.kind) {
    case "Circle": { const { radius } = $rl_m; return (Math.PI * radius * radius); }
    case "Rect": { const { width, height } = $rl_m; return (width * height); }
    case "Point": { return (0); }
    default: { throw new Error("rl match: unexpected case " + JSON.stringify($rl_m)); }
  }
})());
```

암을 하나 빼면 tsc가 아니라 rlc가 잡습니다.

```
$ rlc shapes.rl
rlc: shapes.rl:9:3: match on enum Shape is not exhaustive: missing "Point"
     (add the missing arms or a final `_` arm)
```

`Option`/`Result`와 콤비네이터는 표준 라이브러리에 있습니다.

```rl
import { Option, Result } from "@rl/std";

function readPort(raw: string): Result<number, string> {
  const port = try parseNum(raw);          // Err면 여기서 바로 return
  const Some(value) = clamp(port) else {   // None이면 else로 이탈
    return Result.Err("out of range");
  };
  return Result.Ok(value);
}
```

`Result`를 여러 단계 잇는 코드는 `result` 블록으로 평탄하게 씁니다 — `<-`는
성공값을 묶고, 실패는 블록 전체의 값이 됩니다.

```rl
const view = (id: number) => result {
  const user <- getUser(id);                  // Err면 여기서 블록 종료
  const company <- getCompany(user.companyId);
  { user, company }                           // 마지막 식이 Ok로 감싸집니다
};
```

바꾸면 안 되는 바인딩에는 `val`을 붙입니다 — 그 바인딩에서 시작하는 경로의
변경을 컴파일 시점에 막습니다. 방출물에는 아무것도 남지 않습니다.

```rl
val const config = loadConfig();
config.retries = 3;                    // rlc: cannot mutate through val binding `config`

function inspect(val user: User) {     // 이 함수는 user를 못 바꿉니다
  return user.name;
}
```

수식자가 없으면 지금까지의 TypeScript 그대로입니다 — `const user = getUser();`는
여전히 `user.name = "Lee"`를 허용합니다.

## 설치와 사용

TypeScript처럼 npm으로 설치합니다. 프리빌트 바이너리가 함께 설치되므로
Rust 툴체인이 필요 없습니다.

```sh
npm install --save-dev rl-lang    # rlc 컴파일러 (프리빌트 바이너리)

npx rlc -o build src/             # 소스 트리 → TypeScript 트리
npx rlc --check src/              # 컴파일하지 않고 검사만
npx rlc --types src/              # 에디터·tsc용 타입 선언
npx rlc help match                # 내장 언어·워크플로 가이드 (주제별)
```

프리빌트 지원 플랫폼은 linux-x64 / linux-arm64 / darwin-x64 / darwin-arm64 /
win32-x64입니다. 그 밖의 플랫폼이거나 npm 없이 쓰려면 소스에서 빌드합니다:

```sh
cargo install --git https://github.com/load28/rl   # 또는 클론 후 --path .
```

번들러를 쓰면 [`unplugin-rl`](./integrations/unplugin)이 `.rl`을 직접 읽습니다.

```ts
import rl from "unplugin-rl/vite";   // /rollup, /webpack, /esbuild, ...
export default defineConfig({ plugins: [rl()] });
```

## 문서

| 문서 | 내용 |
|------|------|
| [`language.md`](./docs/reference/language.md) | 언어 레퍼런스 — 문법, 방출 코드, 소진성 검사, 제한사항 |
| [`cli.md`](./docs/reference/cli.md) | `rlc` 옵션과 파이프라인 |
| [`std.md`](./docs/reference/std.md) | `Option`/`Result` API |
| [`errors.md`](./docs/reference/errors.md) | 에러 메시지 전체 목록 |
| [`docs/ai/`](./docs/ai/) | AI 코딩 도구용 컨텍스트 문서 — Claude Code·Cursor·Copilot 등에 rl을 가르치기 |
| [`examples/shapes.rl`](./examples/shapes.rl) | 동작하는 예제 (→ [`shapes.ts`](./examples/shapes.ts)) |
| [`editors/vscode`](./editors/vscode) | VSCode 확장 (하이라이팅·진단·정의 이동) |
| [`docs/design/`](./docs/design/) | 설계 문서 |

라이브러리로 쓸 때는 [`compile`](./src/lib.rs)과 `Options`가 전부입니다.

## 기여하기

이 저장소의 모든 작업은 [`docs/tasks/`](./docs/tasks/INDEX.md)의 태스크 문서로
관리됩니다. 규칙은 [`CLAUDE.md`](./CLAUDE.md)와
[`CONTRIBUTING.md`](./CONTRIBUTING.md)를 참고하세요.

머지 전 검증 게이트:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

## 라이선스

[MIT](./LICENSE)
