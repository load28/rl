# rlc CLI 레퍼런스

`rlc`는 **소스 트리를 완결된 TypeScript 트리로 만듭니다.** `.rl`은 컴파일되고,
손으로 쓴 `.ts`는 바이트 그대로 통과하며(상대 경로 `.rl` 지정자만 재작성),
`@rl/std`를 import하면 표준 라이브러리가 실체화됩니다.

언어는 [`language.md`](./language.md), 에러 메시지는
[`errors.md`](./errors.md)를 보세요.

```
rlc [options] <file | dir> ...
rlc help [topic]
```

## 설치

```sh
npm install --save-dev rl-lang    # 프리빌트 바이너리, `npx rlc`로 실행
```

npm 패키지 [`rl-lang`](https://www.npmjs.com/package/rl-lang)은 플랫폼별
프리빌트 바이너리(linux-x64/arm64 · darwin-x64/arm64 · win32-x64)를
optionalDependencies로 함께 설치합니다. 그 밖의 플랫폼은
`cargo install --git https://github.com/load28/rl`로 빌드한 뒤, npm 런처를
쓰려면 `RLC_BINARY` 환경 변수로 바이너리를 가리키면 됩니다.

릴리스는 태그 `vX.Y.Z` 푸시로 이루어집니다 — `.github/workflows/release.yml`이
태그와 `Cargo.toml` 버전 일치를 검증하고, 5개 타깃을 빌드해 npm 배포와
GitHub Release 업로드까지 수행합니다.

## 옵션

| 옵션 | 설명 |
|------|------|
| `-o, --out-dir <dir>` | 출력을 `<dir>` 아래에 씁니다 (중간 디렉터리 자동 생성) |
| `-w, --watch` | 계속 지켜보며 바뀐 파일을 다시 처리합니다 ([감시 모드](#감시-모드--w)) |
| `-j, --jobs <n>` | 한 번에 컴파일할 파일 수 (기본: 코어 수, `1`이면 순차) ([병렬 컴파일](#병렬-컴파일---jobs)) |
| `--check` | 컴파일만 하고 아무것도 쓰지 않습니다 (rl 수준 검사, TypeScript 불필요) |
| `--check-types` | 여기에 **타입 검사**까지 ([타입 검사](#타입-검사---check-types---types)) |
| `--types` | `--check-types`에 더해 **타입 사이드카**를 씁니다 (같은 절) |
| `--project <tsconfig>` | 위 두 모드가 검사할 `tsconfig.json` (기본: 입력 위쪽에서 탐색) |
| `--overlay <path>` | `<path>`의 내용을 **stdin에서** 받습니다 — 저장되지 않은 버퍼를 프로젝트의 일부로 검사 ([에디터의 타입 검사](#에디터의-타입-검사---overlay---rl-only)) |
| `--rl-only` | rl 수준 진단만 보고하고 타입 에러(`ts(코드)`)는 생략 (같은 절) |
| `--node <path>` | TypeScript 컴파일러의 클라이언트를 돌릴 node 바이너리 (기본: PATH의 `node`) |
| `-h, --help` / `-v, --version` | 출력하고 종료 (코드 0) |

도구용 — 번들러 플러그인·에디터가 호출합니다.

| 옵션 | 설명 |
|------|------|
| `-p, --print` | 컴파일 결과를 stdout으로 |
| `--emit-std` | 표준 라이브러리 모듈([`std.md`](./std.md))을 stdout으로 출력하고 종료. 입력과 조합되지 않습니다 |
| `--no-banner` | `@generated` 배너 주석 생략 |
| `--no-verify` | 필드 타입 검사와 생성물 자가 검사 생략 |
| `--rewrite-imports <js\|ts\|off>` | `.rl` 지정자의 방출 형태 ([`language.md` §9.2](./language.md#92-방출-형태---rewrite-imports)). 그 외 값은 에러 |
| `--sidecar <dir>` | 선언을 받아 사이드카만 씁니다 ([아래](#에디터-사이드카---sidecar-저수준)) |
| `--symbols` | rl enum 선언과 `.rl` import를 JSON으로 ([아래](#심볼-출력---symbols)) |
| `--emit-map` | 방출 TypeScript와 원본↔출력 바이트 매핑을 JSON으로 ([아래](#방출-매핑---emit-map)) |
| `--server` | 엔진을 살려 두고 stdin/stdout의 JSON 라인으로 `check`/`emitMap`/`typedCheck` 요청에 답합니다 ([아래](#엔진-서버---server)) |

옵션과 입력은 순서 무관하게 섞을 수 있습니다. `-`로 시작하는 알 수 없는 인자는
에러이고, `--`(옵션 종료)와 짧은 옵션 병합(`-po`)은 지원하지 않습니다.

## 주제별 헬프 (`rlc help`)

바이너리에 임베드된 언어·워크플로 가이드([`docs/ai/rl.md`](../ai/rl.md), 빌드
시점에 포함)를 주제별로 stdout에 출력합니다. 네트워크나 저장소 없이 문법을
찾아볼 수 있어 AI 코딩 도구가 자기 서비스로 쓰기에 적합합니다.

```sh
rlc help              # 주제 목록
rlc help match        # 해당 섹션만 출력
rlc help all          # 가이드 전체 (별칭: guide)
```

| 주제 | 별칭 |
|------|------|
| `overview` | `contracts`, `intro` |
| `enum` | `enums` |
| `match` | `tuple`, `patterns` |
| `try` | — |
| `let-else` | `letelse` |
| `if-let` | `iflet` |
| `pipe` | `pipeline`, `\|>`, `flow` |
| `result` | `do`, `result-block` |
| `val` | `mutation`, `readonly` |
| `std` | `option` |
| `modules` | `imports` |
| `install` | `update` |
| `setup` | `init` |
| `workflow` | `dev`, `build` |
| `errors` | — |
| `checklist` | — |

- 주제는 대소문자 무관, 한 번에 하나만 받습니다 (둘 이상은 에러).
- `help`는 **첫 번째 인자일 때만** 서브커맨드입니다 — `help`라는 이름의
  파일은 `./help`처럼 경로로 넘기면 입력으로 처리됩니다.
- 알 수 없는 주제는 stderr 에러와 종료 코드 1입니다
  ([`errors.md`](./errors.md)).

## 입력 수집

| 입력 | 동작 |
|------|------|
| 파일 | 확장자와 무관하게 대상에 추가 |
| 디렉터리 | 재귀 순회 — `.rl`과, 컴파일 계열 모드(빌드/`--check`/`--types`)에서는 `.ts`/`.mts`/`.cts`까지. 도구 모드(`--symbols`/`--emit-map`/`--sidecar`)는 `.rl`만 |
| `.`으로 시작하는 디렉터리, `node_modules` | 순회하지 않음 |
| 없는 경로 | 즉시 에러 |
| 결과가 빈 경우 | `rlc: no sources found` |

컴파일할 때 각 파일의 **직접 상대 경로 `.rl` import**를 추가로 읽어 enum 선언을
수집합니다 (소진성 검사용,
[`language.md` §9.3](./language.md#93-선언-수집과-프로젝트-단위-소진성)).
읽을 수 없는 지정자는 조용히 건너뜁니다 — 모듈 해석은 tsc의 책임입니다.

## 출력 경로

| 상황 | 출력 |
|------|------|
| `-o` 없음 | 입력 옆 (`src/a.rl` → `src/a.ts`) |
| `-o out/` + 파일 | `out/<파일명>` |
| `-o out/` + 디렉터리 | 입력 기준 상대 경로를 `out/` 아래에 미러 |

`.rl`은 같은 이름의 `.ts`가 되고 통과하는 `.ts`는 이름을 유지합니다. 기존
파일은 덮어쓰지만 **출력이 입력 자신이 되는 경우**는 거부합니다.

```
rlc: src/main.ts: output would overwrite the input — pass -o <dir>
```

## 병렬 컴파일 (`-j, --jobs`)

파일 단위 컴파일은 서로 독립적이므로 rlc는 입력을 **코어 수만큼 동시에**
처리합니다. `-j <n>`으로 스레드 수를 직접 정할 수 있고 `-j 1`은 순차 실행입니다.

**출력과 진단은 스레드 수와 무관하게 항상 동일합니다.** 진단은 입력 순서대로
모아서 출력하고, 두 입력이 같은 출력 경로를 요구하는 경우(`a.rl`과 손으로 쓴
`a.ts`가 모두 `a.ts`로) 그 쓰기만 순서대로 처리하므로 "뒤에 오는 입력이 이긴다"는
규칙도 그대로입니다. 다만 진단은 실행 도중이 아니라 **끝난 뒤 한꺼번에** 나옵니다.

```sh
$ rlc -j 1 -o build src/     # 순차 (재현 가능한 CI 로그)
$ rlc -o build src/          # 기본 — 코어 수만큼 동시에
```

## 표준 라이브러리 자동 방출

입력 중 하나라도 `@rl/std`를 import하면 모듈이 출력 트리에 자동으로 쓰이고
지정자가 그것을 가리키도록 재작성됩니다.

```sh
$ rlc -o build src/
rlc: std → build/rl.ts
rlc: src/main.rl → build/main.ts                 # import ... from "./rl.js"
rlc: src/deep/nested.rl → build/deep/nested.ts   # "../rl.js"
```

위치는 `-o` 디렉터리(없으면 출력들의 공통 상위)의 `rl.ts`이고, 지정자 형태는
`--rewrite-imports`를 따릅니다(`off`면 `@rl/std` 그대로 — 번들러 플러그인이
직접 해석할 때).

## 타입 검사 (`--check-types` / `--types`)

`.ts` 파일이 `"./x.rl"`이나 `"@rl/std"`를 import하면 tsc는 그 지정자를 몰라
`TS2307`을 냅니다. 그리고 `.rl` 안의 타입 에러는 `--check`가 보지 않습니다 —
그것은 TypeScript의 몫이니까요. 이 두 모드가 **진짜 TypeScript 컴파일러**를
데려옵니다.

```sh
rlc --check-types src                   # 검사만
rlc --types src                         # 검사 + 사이드카 (기본 -o .rl-types)
rlc --check-types src --project ./tsconfig.app.json
rlc --check-types src -w                # 감시 (컴파일러를 살려 둔다)
```

`.rl`을 ordinary TypeScript로 낮춘 뒤 **사용자의 실제 TypeScript 프로젝트**에
넣어 TypeScript 7 네이티브 컴파일러(typescript-go)에게 묻습니다. `.ts`와
`.rl`이 하나의 프로그램 안에 있으므로 서로를 봅니다. 낮춘 모듈은
**메모리에만** 있습니다 — 어떤 소스도 복사되지 않고 중간 트리도 만들지
않습니다.

- **설정이 필요 없습니다.** `src/token.rl`은 프로그램 안에서 `src/token.rl.ts`가
  되므로, 사람이 쓴 `.ts`의 `import "./token.rl"`이 평범한 TypeScript 해석으로
  그 모듈을 찾습니다. `paths`도 `allowImportingTsExtensions`도 필요 없습니다.
  `@rl/std`는 가상 `node_modules/@rl/std`로 해석되므로 지정자가 바 상태로
  남습니다.
- **그래프는 프로젝트 전체**입니다 — 인자로 준 파일이 프로젝트의 다른 `.rl`을
  import해도 해석됩니다. 인자는 *무엇을 쓸지*만 정합니다. `tsconfig.json`이
  있으면 그 `include`가 손으로 쓴 `.ts`의 범위를 정하고, 없으면 프로젝트의
  `.ts`도 함께 열어 검사합니다.
- **소진성과 `val`을 체커가 답합니다.** match 위치에서 실제로 좁혀진 타입을
  쓰므로, 앞선 가드가 제거한 케이스는 요구하지 않습니다. `val`은 심볼 동일성으로
  바인딩을 짝짓고, 내장 메서드 판정도 컴파일러가 합니다.
- `-j, --jobs`는 이 모드에 영향이 없습니다 — 프로그램은 하나고, 시간은 대부분
  체커가 씁니다.

### 진단

rl 수준 에러는 `--check`와 **똑같이** 읽히고, 타입 에러만 `ts(코드):`로
구분됩니다. 둘 다 원본 `.rl`의 위치를 가리키고, 둘 다 stderr입니다.

```
rlc: src/eval.rl:12:31: ts(2339): Property 'length' does not exist on type 'number'.
rlc: src/main.rl:3:10: match on literal union is not exhaustive: missing "south"
     (add the missing arms or a final `_` arm)
rlc: src/main.rl:2:1: cannot call mutating method `set` through val binding `map`
     (the binding is declared with `val`, so every access path from it is read-only)
```

- 손으로 쓴 `.ts`의 에러는 원래 위치 그대로 나옵니다.
- 글루(switch IIFE, `$rl_ap` 헬퍼 등)에 걸린 진단은 원본 대응이 없으므로 그
  구문의 위치로 보고하고 `(in code rlc generated for this construct)`를 덧붙입니다.
  애초에 방출물 때문에 tsc 에러가 나면 그건 rlc의 버그입니다
  ([`errors.md`](./errors.md) 에러 계층).
- **소진성 메시지가 `--check`와 다릅니다.** `--check`는 자기 선언 표에서
  답하므로 enum 이름을 댈 수 있고(`match on enum Shape is not exhaustive`),
  이 모드는 *타입*에서 답하므로 이름 없이 `match is not exhaustive`라고
  합니다. 대신 좁혀진 타입을 쓰므로 더 정확합니다. 리터럴 유니언은 어느
  선언 표에도 없으므로 이 모드만 검사합니다
  ([`language.md` §3.9](./language.md#39-리터럴-유니언-소진성---types)).
- **`val` 경로의 built-in 변경 메서드도 여기서만 검사합니다.** 타입 체커에게 그
  메서드의 선언을 물어, TypeScript 자신이 선언한 메서드일 때만 보고합니다.
  같은 이름의 사용자 정의 메서드는 걸리지 않고, 수신자를 확정할 수 없으면
  검사하지 않습니다
  ([`language.md` §10.4](./language.md#104-built-in-변경-메서드---types)).

### 종료 코드

| 코드 | 의미 |
|------|------|
| 0 | 아무것도 보고되지 않음 |
| 1 | 무언가 보고됨. `--types`라면 **사이드카는 갱신된 상태** — 낡은 사이드카보다 타입 에러가 있는 코드의 사이드카가 낫습니다 |
| 2 | 검사를 시작할 수 없었음 (rl 수준 에러로 낮출 것이 없음) — **아무것도 쓰지 않았으므로** 이전 결과를 들고 있는 쪽은 그대로 두면 됩니다 |

### 에디터의 타입 검사 (`--overlay`, `--rl-only`)

`val` 변경과 타입 기반 소진성은 이 모드만 답합니다. 편집 중에 그것을 보려면
에디터는 **저장되지 않은 버퍼**를 물어야 하고, 그 버퍼는 자기 프로젝트 안에
있어야 합니다. 두 옵션이 그것을 가능하게 합니다 — `--check-types` 전용이고,
`--types`와는 조합되지 않습니다(저장되지 않은 텍스트가 사이드카에 들어가면
안 되므로).

```sh
rlc --check-types --rl-only --overlay src/main.rl src < buffer.txt
```

- `--overlay <path>`는 `<path>`가 프로젝트에서 차지하는 자리를 그대로 두고
  **내용만** stdin의 텍스트로 바꿉니다. 임시 파일이 아니라 원래 경로이므로,
  그 파일의 import도 그 파일을 import하는 쪽도 디스크에서와 똑같이 해석됩니다.
  `<path>`는 **실재해야** 합니다 — 아직 저장된 적 없는 버퍼는 프로젝트 그래프에
  자리가 없습니다. `--watch`와는 조합되지 않습니다(감시는 디스크를 다시 읽는데
  stdin의 텍스트는 영원히 그대로이므로).
- `--rl-only`는 타입 계층을 생략하고 rl 계층만 남깁니다. 살아 있는 언어 서버를
  이미 들고 있는 소비자는 타입 에러를 그쪽에서 받으므로, 여기서도 내면 같은
  에러가 두 번 그려집니다.

두 옵션 모두 진단의 **문안·위치·형식을 바꾸지 않습니다**. 에디터는 rlc가 쓴
문장을 그대로 옮깁니다 ([`errors.md`](./errors.md) 에러 계층).

### 사이드카 (`--types`)

`-o`가 없으면 `.rl-types/`에 씁니다.

```sh
rlc --types src/          # → .rl-types/<이름>.rl.d.ts (+ .map, rl.d.ts)
rlc --types -w src/       # 감시하며 갱신
```

소비 측 `tsconfig.json`은 두 가지만 선언하면 됩니다.

```jsonc
{
  "compilerOptions": {
    "rootDirs": ["./src", "./.rl-types"],
    "paths": { "@rl/std": ["./.rl-types/rl.d.ts"] }
  }
}
```

| 파일 | 역할 |
|------|------|
| `<이름>.rl.d.ts` | tsserver/tsc가 `"./<이름>.rl"`을 해결하는 근거 |
| `<이름>.rl.d.ts.map` | `sources`가 원본 `.rl` — **정의 이동이 원본으로** 갑니다 |
| `rl.d.ts` | 표준 라이브러리(`@rl/std`)의 선언 |

`.rl-types/`는 생성물이므로 gitignore에 넣으세요.

### 감시 (`-w`)

**컴파일러를 살려 둡니다.** 프로젝트는 한 번만 열고, 이후에는 바뀐 파일만
알려 재검사합니다. 매 패스마다 걸린 시간을 stderr에 적습니다.

```
rlc: 1 file(s), 0 reported in 183 ms — watching   ← 첫 패스(컴파일러 기동 + 프로젝트 열기)
rlc: 1 file(s), 1 reported in 8 ms — watching     ← 편집 후 재검사
```

### 컴파일러 해석

먼저 나오는 것을 씁니다.

| 순서 | 무엇 |
|------|------|
| 1 | `RLC_TSGO_API` (+ 선택적 `RLC_TSGO_BIN`) |
| 2 | `RLC_TSGO_ROOT` — 빌드된 typescript-go 체크아웃 |
| 3 | `../typescript-go` — 마찬가지로 빌드된 것 |
| 4 | 프로젝트 위쪽의 `node_modules/typescript` 또는 `@typescript/native-preview` |

4번(설치된 패키지)은 API 클라이언트와 네이티브 실행 파일을 함께 배포하므로
`npm i -D typescript@7`만으로 동작합니다. 다만 **선언 emit은 아직 릴리스에
없어** `--types`의 사이드카 쓰기에는 빌드된 체크아웃이 필요합니다 — 그 경우
rlc가 그렇게 말합니다. 아무것도 해석되지 않으면
`rlc: no TypeScript compiler found — install one (npm i -D typescript@7)`.


## 감시 모드 (`-w`)

```sh
rlc -w -o build src/          # 바뀔 때마다 다시 컴파일
rlc -w --check src/           # 검사만 (tsc --noEmit --watch에 해당)
```

`--check-types`/`--types`의 감시는 컴파일러를 살려 두는 별도 구조입니다 —
[타입 검사 §감시](#감시--w).

- 입력을 매 회차 다시 수집하므로 **새로 생긴 `.rl`도** 잡힙니다.
- **바뀐 파일의 importer도 함께** 다시 컴파일합니다 — 다른 파일의 enum에
  케이스가 늘면 그것을 `match`하는 쪽에서 에러가 나야 하기 때문입니다.

```
rlc: watching 2 file(s) — Ctrl-C to stop
rlc: ./a.rl → out/a.ts
rlc: ./b.rl:4:3: match on enum E (imported from "./a.rl") is not exhaustive:
     missing "C" (add the missing arms or a final `_` arm)
rlc: 2 file(s) failed — watching
```

파일 시각을 300ms마다 확인하므로 외부 의존성이 없고 네트워크 파일 시스템에서도
동작합니다. `--symbols`·`--sidecar`와는 조합되지 않습니다.

## 심볼 출력 (`--symbols`)

언어 도구가 rl 문법을 다시 구현하지 않도록 심볼 정보를 JSON 배열(입력 파일당
한 항목)로 냅니다.

```jsonc
[{
  "file": "parser.rl",
  "enums": [
    { "name": "Local", "exported": false, "generics": "",
      "line": 3, "col": 6,              // 이름 위치 (1-기반, UTF-8 코드포인트)
      "cases": [
        { "tag": "A", "line": 3, "col": 14,
          "fields": [ { "name": "x", "optional": false, "type": "number" } ] },
        { "tag": "B", "line": 3, "col": 30, "fields": null }   // null = 유닛
      ] }
  ],
  "imports": [
    { "specifier": "./token.rl",
      "names": { "kind": "named",       // "namespace" { name } / "none" 도 가능
                 "entries": [ { "name": "Token", "alias": "Tok" } ] },
      "resolved": "./token.rl",         // 읽지 못하면 null (enums는 [])
      "enums": [ /* 참조 파일의 exported enum, 같은 형태 */ ] }
  ]
}]
```

import 수집은 소진성 검사와 같은 1-홉입니다. 컴파일 모드와 조합되지 않고
(`-o`/`-p`/`--check` 무시), 입력을 읽지 못하면 종료 코드 1입니다.

## 방출 매핑 (`--emit-map`)

에디터가 방출 결과를 **가상 TypeScript 문서**로 TS 언어 서비스에 서빙할 수
있도록, 입력 파일마다 방출 코드와 원본↔출력 바이트 매핑을 JSON 배열로 냅니다.

```jsonc
[{
  "file": "demo.rl",
  "code": "…방출된 TypeScript 전체…",
  "mappings": [
    { "src": 0, "out": 0, "len": 108 },   // 원본 src부터 len바이트가
    { "src": 115, "out": 135, "len": 5 }  // 출력 out에 그대로 복사됨
  ]
}]
```

- **파싱 + 방출만** 수행합니다 — rl 수준 검사(소진성 등)와 생성물 검증을
  생략하므로 편집 중인(오류가 있는) 버퍼도 항상 방출됩니다. 진단은 여전히
  `--check`의 몫입니다.
- 상대 `.rl` 지정자와 `"@rl/std"`는 **그대로** 둡니다(`--rewrite-imports off`
  의미) — 소비자(에디터의 모듈 해석기)가 직접 해석합니다.
- 매핑은 원본에서 바이트 그대로 복사된 조각만 담습니다: 통과 구간, match
  스크루티니·암 본문·가드, `try`/let-else/`if let` 식, 파이프라인 스텝,
  `result` 블록의 `<-` 바인딩 이름(타입 주석·구조 분해 포함)과 그 식, 템플릿
  조각. 구문이 **새로 도입하는 이름**도 마찬가지입니다 — `try` 선언형의
  바인딩(타입 주석·구조 분해 포함)과 패턴 바인딩(match 암·let-else·`if let`,
  중첩 패턴 포함)의 필드 이름·별칭이 각각 방출된 구조 분해로 매핑됩니다.
  컴파일러가 쓴 글루(IIFE 골격, enum 방출, 구조 분해의 `const { … } =` 골격)는
  매핑이 없습니다.
- 예외는 **or-패턴**(`A(x) | B(x)`)의 바인딩입니다: 방출되는 구조 분해 하나가
  모든 대안을 대표하므로 어느 한 대안의 바이트라고 주장할 수 없어, 이름들이
  글루로 남습니다.
- 오프셋은 **바이트** 기준이고, 매핑된 조각은 원본과 출력에서 내용이
  동일합니다(도구가 재파싱 없이 양방향 변환하는 근거). 조각은 어느 좌표계에서도
  겹치지 않습니다.

컴파일 모드와 조합되지 않으며, 입력을 읽지 못하면 종료 코드 1입니다.

## 엔진 서버 (`--server`)

한 프로세스를 살려 두고 stdin의 JSON 라인 요청에 stdout의 JSON 라인으로
답합니다. 에디터처럼 같은 질문을 계속 묻는 도구용입니다 — 답은 one-shot
모드와 **동일**하고, 다른 것은 상태 재사용뿐입니다: 프로젝트당 엔진 세션
하나(그 뒤의 TypeScript 컴파일러 포함)가 요청 사이에 유지되므로, 첫
`typedCheck`가 프로젝트를 연 뒤에는 재검사가 밀리초로 답합니다.

```
→ { "id": 1, "method": "check",      "params": { "text", "filename"?, "verify"? } }
← { "id": 1, "result": { "diagnostics": [{ "line", "col", "message" }] } }

→ { "id": 2, "method": "emitMap",    "params": { "text" } }
← { "id": 2, "result": { "code", "mappings": [{ "src", "out", "len" }] } }

→ { "id": 3, "method": "typedCheck", "params": { "path", "text" } }
← { "id": 3, "result": { "blocked", "diagnostics": [{ "path", "line", "col", "message" }] } }

← { "id": N, "error": "문장" }        // 요청 실패 — 세션은 살아 있음
```

- `check`는 `--check`처럼 텍스트만으로 판정합니다(상대 import는 해석되지
  않음 — one-shot이 임시 파일에서 도는 것과 같음). `emitMap`은
  `--emit-map`의 항목 하나와 같습니다. `typedCheck`는
  `--check-types --rl-only --overlay <path>`와 같고, `blocked`가
  종료 코드 2에 해당합니다.
- 진단의 문안·위치는 one-shot과 동일합니다. `typedCheck`의 `path`는
  절대 경로로 돌아오고, 위치가 없는 진단은 `line`/`col`이 0입니다.
- 요청 하나의 실패는 그 요청의 `error`로 답하고 세션은 유지됩니다.
  stdin이 닫히면 종료 코드 0으로 끝납니다.
- 다른 입력·모드와 조합되지 않습니다. VSCode 확장이 이 모드를 쓰며,
  서버가 없는 구형 rlc에는 one-shot 명령으로 폴백합니다.

## 에디터 사이드카 (`--sidecar`, 저수준)

`--types`의 마지막 단계만 따로 실행합니다. 선언 방출을 자체적으로 수행하는
도구(VSCode 확장의 저장 시 갱신)가 씁니다 — 일반 사용은 `--types`로 충분합니다.

```sh
rlc --sidecar types src/notice.rl                # src/notice.rl.d.ts + .map
rlc --sidecar types -o .rl-types src/notice.rl   # .rl-types/notice.rl.d.ts
```

선언 본문은 `<dir>/<이름>.d.ts`에서 가져오고, rlc는 그 선언이 원본의 어디에서
왔는지만 맵으로 채웁니다. 맵의 `sources`는 사이드카 위치 기준 상대 경로라 어느
배치에서든 정의 이동이 원본으로 갑니다.

에디터가 맵을 따라가려면 그 `.ts`를 포함하는 `tsconfig.json`이 있어야 합니다 —
추론 프로젝트로 열리면 맵 추적이 동작하지 않습니다.

## 출력과 종료 코드

- 쓴 파일마다 stderr에 진행 로그(`rlc: src/a.rl → src/a.ts`). `-p`/`--check`는
  로그 없음.
- 에러는 stderr에 `rlc: 파일:행:열: 메시지` ([`errors.md`](./errors.md)) —
  타입 검사 모드의 진단도 같습니다.
- stdout은 `-p`·`--emit-std`·`--symbols`·`--emit-map`·`--server`·`help`·`-h`·`-v` 전용이라 파이프로 안전합니다.
- 기본적으로 출력 첫 줄에 `// @generated from <파일> by rlc — do not edit
  directly.`가 붙습니다 (`--no-banner`로 생략).

| 코드 | 의미 |
|------|------|
| 0 | 전부 성공 |
| 1 | 인자 에러, 없는 입력 경로, 또는 하나 이상 실패 |
| 2 | 타입 검사 모드에서만: 검사를 시작할 수 없었음 ([위](#종료-코드)) |

한 파일이 실패해도 나머지는 계속 처리하고 마지막에 1로 종료합니다. 인자 에러와
없는 경로는 시작 전에 즉시 1입니다.

## 두 모드, 한 파이프라인

```jsonc
// 단독 (tsc)
{ "scripts": {
    "build": "rlc -o build src && tsc",
    "types": "rlc --types src",
    "check": "rlc --check-types src" } }
```

```jsonc
// 번들러 (vite)
{ "scripts": {
    "build": "vite build",
    "types": "rlc --types src" } }
```

소스도, 타입을 만드는 명령도 두 모드에서 같습니다. 다른 것은 `.rl` 지정자를
런타임에 해석하는 주체(방출 시 재작성 vs 플러그인)뿐입니다.
