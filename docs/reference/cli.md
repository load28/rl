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
| `--check` | 컴파일만 하고 아무것도 쓰지 않습니다 |
| `--types` | 빌드 대신 **타입 사이드카**를 만듭니다 ([타입 생성](#타입-생성---types)) |
| `--node <path>` | `--types`가 쓸 node 바이너리 (기본: PATH의 `node`) |
| `-h, --help` / `-v, --version` | 출력하고 종료 (코드 0) |

도구용 — 번들러 플러그인·에디터가 호출합니다.

| 옵션 | 설명 |
|------|------|
| `-p, --print` | 컴파일 결과를 stdout으로 |
| `--emit-std` | 표준 라이브러리 모듈([`std.md`](./std.md))을 stdout으로 출력하고 종료. 입력과 조합되지 않습니다 |
| `--no-banner` | `@generated` 배너 주석 생략 |
| `--no-verify` | 필드 타입 검사와 생성물 자가 검사 생략 |
| `--rewrite-imports <js\|ts\|off>` | `.rl` 지정자의 방출 형태 ([`language.md` §8.2](./language.md#82-방출-형태---rewrite-imports)). 그 외 값은 에러 |
| `--sidecar <dir>` | 선언을 받아 사이드카만 씁니다 ([아래](#에디터-사이드카---sidecar-저수준)) |
| `--symbols` | rl enum 선언과 `.rl` import를 JSON으로 ([아래](#심볼-출력---symbols)) |
| `--emit-map` | 방출 TypeScript와 원본↔출력 바이트 매핑을 JSON으로 ([아래](#방출-매핑---emit-map)) |

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
| `pipe` | `pipeline`, `\|>` |
| `std` | `option`, `result` |
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
[`language.md` §8.3](./language.md#83-선언-수집과-프로젝트-단위-소진성)).
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

## 타입 생성 (`--types`)

`.ts` 파일이 `"./x.rl"`이나 `"@rl/std"`를 import하면 tsserver/tsc는 그 지정자를
몰라 `TS2307`을 냅니다. `--types`가 그 간극을 메우는 선언을 만듭니다 — 단독
파이프라인이든 번들러든 타입은 이 명령 하나로 나옵니다.

```sh
rlc --types src/          # → .rl-types/<이름>.rl.d.ts (+ .map, rl.d.ts)
rlc --types -w src/       # 감시하며 갱신
```

선언은 **메모리에서** 방출됩니다 — 각 `.rl`을 컴파일해 내장 호스트
스크립트(node)에 넘기고, 손으로 쓴 `.ts`는 디스크에서 제자리에서 읽습니다.
**어떤 소스도 복사되지 않고 중간 트리도 만들지 않습니다.** 그 `.ts` 파일들도
프로그램에 참여하므로 그쪽 타입 에러도 함께 보고됩니다.

- TypeScript는 프로젝트의 `node_modules`에서 해석합니다 (없으면 PATH의
  `tsc`가 속한 패키지). 없으면
  `rlc: typescript not found — install it (npm i -D typescript)`.
- **TypeScript 5·6이 필요합니다.** TypeScript 7은 네이티브(Go) 컴파일러라
  npm 패키지에 JS 컴파일러 API가 없어 `--types`가 구동할 수 없습니다 —
  7만 해석되는 환경에서는 API가 있는 버전을 찾을 때까지 건너뛰고, 끝내
  없으면 `rlc: the resolved typescript has no JS compiler API ...
  (npm i -D typescript@6)`로 안내합니다.
- 타입 에러가 있어도 선언은 방출되므로 사이드카는 갱신되고 종료 코드만 1입니다.

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

`.rl-types/`는 생성물이므로 gitignore에 넣으세요.

## 감시 모드 (`-w`)

```sh
rlc -w -o build src/          # 바뀔 때마다 다시 컴파일
rlc -w --check src/           # 검사만 (tsc --noEmit --watch에 해당)
rlc -w --types src/           # 사이드카를 계속 갱신
```

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
  스크루티니·암 본문·가드, `try`/let-else/`if let` 식, 파이프라인 스텝, 템플릿
  조각. 컴파일러가 쓴 글루(IIFE 골격, 구조 분해, enum 방출)는 매핑이 없습니다.
- 오프셋은 **바이트** 기준이고, 매핑된 조각은 원본과 출력에서 내용이
  동일합니다(도구가 재파싱 없이 양방향 변환하는 근거). 조각은 어느 좌표계에서도
  겹치지 않습니다.

컴파일 모드와 조합되지 않으며, 입력을 읽지 못하면 종료 코드 1입니다.

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
- 에러는 stderr에 `rlc: 파일:행:열: 메시지` ([`errors.md`](./errors.md)).
- stdout은 `-p`·`--emit-std`·`--symbols`·`--emit-map`·`help`·`-h`·`-v` 전용이라 파이프로 안전합니다.
- 기본적으로 출력 첫 줄에 `// @generated from <파일> by rlc — do not edit
  directly.`가 붙습니다 (`--no-banner`로 생략).

| 코드 | 의미 |
|------|------|
| 0 | 전부 성공 |
| 1 | 인자 에러, 없는 입력 경로, 또는 하나 이상 실패 |

한 파일이 실패해도 나머지는 계속 처리하고 마지막에 1로 종료합니다. 인자 에러와
없는 경로는 시작 전에 즉시 1입니다.

## 두 모드, 한 파이프라인

```jsonc
// 단독 (tsc)
{ "scripts": {
    "build": "rlc -o build src && tsc",
    "types": "rlc --types src",
    "check": "rlc --check src && tsc --noEmit" } }
```

```jsonc
// 번들러 (vite)
{ "scripts": {
    "build": "vite build",
    "types": "rlc --types src" } }
```

소스도, 타입을 만드는 명령도 두 모드에서 같습니다. 다른 것은 `.rl` 지정자를
런타임에 해석하는 주체(방출 시 재작성 vs 플러그인)뿐입니다.
