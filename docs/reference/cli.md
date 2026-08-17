# rlc CLI 레퍼런스

rl 소스를 TypeScript로 컴파일하는 커맨드라인 도구 `rlc`의 사용법입니다.
언어 자체는 [`language.md`](./language.md), 진단 메시지는
[`errors.md`](./errors.md) 참조.

rlc의 정신 모델은 한 문장입니다: **소스 트리를 완결된 TypeScript 트리로
만든다.** `.rl` 파일은 컴파일되고, 손으로 쓴 `.ts` 파일은 바이트 그대로
통과하며(상대 경로 `.rl` import 지정자만 재작성), `@rl/std`를 import하면
표준 라이브러리가 실체화됩니다. 소스는 단독(tsc) 파이프라인이든 번들러
플러그인이든 **같은 모양**으로 쓰고 — `.ts`에서도 `"./x.rl"`을 그대로
import합니다 — 타입은 어느 쪽이든 같은 명령 `--types`가 만듭니다. 두 모드의
차이는 `.rl` 지정자를 런타임에 누가 해석하느냐(방출 시 재작성 vs 플러그인)
하나뿐입니다.

## 시놉시스

```
rlc [options] <file | dir> ...
```

## 설치

```sh
cargo install --path .        # 또는
cargo build --release        # → target/release/rlc
```

## 옵션

사용자용:

| 옵션 | 설명 |
|------|------|
| `-o, --out-dir <dir>` | 출력을 `<dir>` 아래에 씁니다 (경로 규칙은 아래). 필요한 중간 디렉터리는 자동 생성됩니다. |
| `-w, --watch` | 한 번 실행한 뒤 계속 지켜보며 바뀐 파일을 다시 처리합니다 (아래 "감시 모드"). `--types`와도 조합됩니다. |
| `--check` | 컴파일만 하고 아무것도 쓰지 않습니다 (문법·소진성 검사 용도). |
| `--types` | 빌드 대신 **타입 사이드카**를 만듭니다: 트리를 `.rl-build/`로 컴파일하고, tsc `--emitDeclarationOnly`를 실행하고, `<이름>.rl.d.ts`(+`.map`)를 `-o`(기본 `.rl-types/`) 아래에 씁니다 (아래 "타입 생성"). |
| `--tsc <path>` | `--types`가 실행할 tsc 바이너리. 기본 탐색은 `node_modules/.bin/tsc` → PATH의 `tsc`. |
| `-h, --help` | 도움말을 출력하고 종료합니다 (종료 코드 0). |
| `-v, --version` | 버전만 출력하고 종료합니다 (종료 코드 0). |

도구용 (번들러 플러그인·에디터가 호출합니다 — 직접 쓸 일은 드뭅니다):

| 옵션 | 설명 |
|------|------|
| `-p, --print` | 파일을 쓰는 대신 컴파일 결과를 stdout으로 출력합니다. |
| `--emit-std` | 표준 라이브러리 모듈([`std.md`](./std.md))을 stdout으로 출력하고 종료합니다. 번들러 플러그인이 `@rl/std`를 가상 모듈로 서빙할 때 씁니다 — 빌드는 자동 방출(아래)이 대신하므로 입력과 조합되지 않습니다. |
| `--no-banner` | 출력 첫 줄의 "generated" 배너 주석을 생략합니다. |
| `--no-verify` | 필드 타입 검사와 생성물 자가 검사를 생략합니다. 검증기가 아직 모르는 최신 TS 문법을 쓴 코드를 위한 탈출구입니다. |
| `--rewrite-imports <js\|ts\|off>` | 상대 경로 `.rl` import 지정자의 방출 형태 ([`language.md` §7](./language.md#7-모듈-rl-import-지정자-재작성)): `js`(기본) = `./x.js`, `ts` = `./x.ts`(tsc의 `rewriteRelativeImportExtensions` 필요), `off` = 재작성 끔 (번들러 플러그인이 지정자를 직접 해석할 때). 그 외 값은 에러입니다. |
| `--sidecar <dir>` | 컴파일하지 않고 `<이름>.rl.d.ts`와 `.map`을 씁니다. 선언 본문은 `<dir>/<이름>.d.ts`(tsc `--emitDeclarationOnly` 산출물)에서 가져옵니다. `--types`가 이 단계를 대신 실행해 주므로 저수준 훅입니다 (VSCode 확장이 씁니다). |
| `--symbols` | 컴파일하지 않고 각 입력 파일의 rl enum 선언(위치 포함)과 직접 `.rl` import를 JSON으로 stdout에 출력합니다 (아래 "심볼 출력"). 언어 도구용. |

- 옵션과 입력 인자는 순서 무관하게 섞어 쓸 수 있습니다.
- `-`로 시작하는 알 수 없는 인자는 에러입니다 (`rlc: unknown option ...`).
- `--`(옵션 종료 구분자)와 짧은 옵션 병합(`-po`)은 지원하지 않습니다.

## 입력 수집

각 입력 인자에 대해:

- **파일**이면 확장자와 무관하게 컴파일 대상에 추가됩니다.
  (`.rl`이 아닌 파일도 명시적으로 지정하면 컴파일을 시도합니다.)
- **디렉터리**면 재귀적으로 순회하며 **`.rl` 파일과, 컴파일 계열
  모드(빌드/`--check`/`--types`)에서는 손으로 쓴 TypeScript
  (`.ts`/`.mts`/`.cts`)까지** 수집합니다 — 출력 트리가 그 자체로 완결되도록.
  도구 모드(`--symbols`/`--sidecar`)는 `.rl`만 수집합니다. 항목은 경로 기준
  정렬 순서로 처리됩니다.
- 이름이 `.`으로 시작하는 디렉터리(`.git`, `.rl-build`, `.rl-types`, ...)와
  `node_modules`는 순회하지 않습니다 — 생성물이나 벤더 코드는 소스가
  아닙니다.
- 존재하지 않는 경로는 즉시 에러이며 컴파일을 시작하지 않습니다.
- 수집 결과가 비어 있으면 `rlc: no sources found` 에러입니다.

수집된 `.ts` 파일은 통과 계약 그대로 **바이트 단위로 통과**하며, 상대 경로
`.rl` import 지정자(그리고 `@rl/std`)만 재작성됩니다 — 손으로 쓴 `.ts`도
소스에서는 `"./x.rl"`을 import하고, 방출 트리에서는 그것이 컴파일된 이웃을
가리키게 됩니다.

컴파일할 때 각 파일의 **직접 상대 경로 `.rl` import**를 추가로 읽어 enum
선언을 수집합니다 — import한 enum의 match 소진성 검사용입니다
([`language.md` §7.3](./language.md#73-선언-수집과-프로젝트-단위-소진성)).
읽을 수 없는 지정자는 조용히 건너뜁니다 (모듈 해석 에러는 tsc `TS2307`의
영역).

## 출력 경로 규칙

| 상황 | 출력 위치 |
|------|-----------|
| 기본 (`-o` 없음) | 입력 파일 옆 (`src/a.rl` → `src/a.ts`) |
| `-o out/` + 파일 입력 | `out/<파일명>` (`rlc -o out src/a.rl` → `out/a.ts`) |
| `-o out/` + 디렉터리 입력 | 입력 디렉터리 기준 상대 경로를 `out/` 아래에 미러 (`rlc -o out src/`에서 `src/x/b.rl` → `out/x/b.ts`) |

`.rl`은 같은 이름의 `.ts`가 되고, 통과하는 `.ts`는 이름을 그대로
유지합니다. 기존 파일은 덮어쓰지만, **출력이 입력 파일 자신이 되는 경우**
(예: `-o` 없이 `.ts`를 통과시키는 경우 — 지정자가 재작성된 채 소스를
덮어쓰게 됩니다)는 파일 단위 에러로 거부합니다:

```
rlc: src/main.ts: output would overwrite the input — pass -o <dir>
```

`-p`가 있으면 파일을 쓰지 않고, `--check`면 아무 출력도 만들지 않습니다.

## 타입 생성 (`--types`)

`.ts` 파일이 `"./x.rl"`이나 `"@rl/std"`를 import하면 tsserver/tsc는 그
지정자를 몰라 `TS2307`을 냅니다. `--types` 한 명령이 그 간극을 메우는
선언들을 만듭니다 — 단독(tsc) 파이프라인이든 번들러 플러그인이든 타입은
이 명령 하나로 동일하게 나옵니다.

```sh
rlc --types src/          # → .rl-types/<이름>.rl.d.ts (+ .map, rl.d.ts)
rlc --types -w src/       # 감시하며 계속 갱신
```

내부적으로 세 단계를 실행합니다:

1. 트리 전체(`.rl` + 통과 `.ts`)를 캐시 트리 `.rl-build/`로 컴파일합니다.
   지정자는 **소스 그대로**(`off`) 둡니다 — 선언 방출은 지정자를 보존하므로,
   그래야 사이드카가 소비 측에서 그대로 해석되는 지정자를 담습니다. 캐시
   안에서의 해석은 rlc가 합성하는 `tsconfig.json`이 맡습니다
   (`allowArbitraryExtensions` + 모듈별 `<이름>.d.rl.ts` 심,
   `paths`의 `@rl/std` 매핑).
2. tsc를 `--emitDeclarationOnly`로 실행합니다 (`--tsc` → 프로젝트의
   `node_modules/.bin/tsc` → PATH 순으로 탐색; 없으면
   `rlc: tsc not found ...` 에러). tsc가 타입 에러를 보고하면 그대로
   중계하고 종료 코드 1이 되지만, 선언은 그래도 방출되므로 사이드카는
   갱신됩니다.
3. 각 `.rl` 입력의 선언을 에디터 사이드카(`<이름>.rl.d.ts` + `.map`)로
   바꿔 `-o`(기본 `.rl-types/`) 아래에 입력 구조를 미러하며 씁니다.
   `@rl/std`를 쓰면 그 선언도 `rl.d.ts`로 함께 나옵니다.

소비 측 `tsconfig.json`은 두 가지만 선언하면 됩니다 — 사이드카 트리를
소스와 합치는 `rootDirs`, 표준 라이브러리를 매핑하는 `paths`:

```jsonc
{
  "compilerOptions": {
    "rootDirs": ["./src", "./.rl-types"],
    "paths": { "@rl/std": ["./.rl-types/rl.d.ts"] }
  }
}
```

이러면 소스 트리에서 `tsc --noEmit`이 그대로 동작하고, 에디터의 자동완성·
타입·정의 이동(맵의 `sources`가 원본 `.rl`을 가리킵니다)이 살아납니다.
`.rl-build/`와 `.rl-types/`는 생성물이므로 gitignore에 넣으세요.

두 파일의 역할:

| 파일 | 역할 |
|------|------|
| `<이름>.rl.d.ts` | tsserver/tsc가 `"./<이름>.rl"`을 해결하는 근거 — 에러가 사라지고 자동완성·타입이 살아납니다 |
| `<이름>.rl.d.ts.map` | `sources`가 원본 `.rl` — **정의 이동이 `.d.ts`가 아니라 원본으로** 갑니다 |

## 심볼 출력 (`--symbols`)

언어 도구(VSCode 확장 등)가 rl 문법을 다시 구현하지 않도록, 컴파일러가
심볼 정보를 JSON 배열(입력 파일당 한 항목)로 내보냅니다:

```jsonc
[{
  "file": "parser.rl",
  "enums": [                       // 이 파일의 rl enum (exported 여부 포함)
    { "name": "Local", "exported": false, "generics": "",
      "line": 3, "col": 6,         // 이름 위치 (1-기반, 열은 UTF-8 코드포인트)
      "cases": [
        { "tag": "A", "line": 3, "col": 14,
          "fields": [ { "name": "x", "optional": false, "type": "number" } ] },
        { "tag": "B", "line": 3, "col": 30, "fields": null }  // null = 유닛 케이스
      ] }
  ],
  "imports": [                     // 직접 상대 경로 .rl import/re-export
    { "specifier": "./token.rl",
      "names": { "kind": "named",  // "namespace" { name } / "none" 도 가능
                 "entries": [ { "name": "Token", "alias": "Tok" } ] },
      "resolved": "./token.rl",    // 읽지 못하면 null (enums는 [])
      "enums": [ /* 참조 파일의 exported enum, 같은 형태 */ ] }
  ]
}]
```

- 위치는 에러 보고와 같은 규약입니다: 1-기반 행, UTF-8 코드포인트 단위 열.
- import 수집은 소진성 검사와 같은 **1-홉**입니다
  ([`language.md` §7.3](./language.md#73-선언-수집과-프로젝트-단위-소진성)).
- `--symbols`는 컴파일 모드와 조합되지 않습니다 — 지정하면 심볼 출력만
  하고 종료합니다 (`-o`/`-p`/`--check` 무시). 입력 파일을 읽지 못하면
  종료 코드 1입니다.

## 표준 라이브러리 자동 방출

입력 중 하나라도 `@rl/std`를 import하면, 컴파일할 때 표준 라이브러리 모듈이
**출력 트리에 자동으로** 쓰이고 각 출력의 지정자가 그것을 가리키도록
재작성됩니다 — `--emit-std`를 따로 부를 필요가 없습니다.

```sh
$ rlc -o build src/
rlc: std → build/rl.ts
rlc: src/main.rl → build/main.ts        # import ... from "./rl.js"
rlc: src/deep/nested.rl → build/deep/nested.ts   # "../rl.js"
```

- 위치는 `-o` 디렉터리(없으면 출력들의 공통 상위)이고 파일 이름은 `rl.ts`입니다.
- 지정자의 형태는 `--rewrite-imports`를 따릅니다: `js`(기본) → `./rl.js`,
  `ts` → `./rl.ts`, `off` → `@rl/std` 그대로.
- `off`로 두는 것은 번들러 플러그인이 이 지정자를 직접 해석할 때입니다
  ([`integrations/unplugin`](../../integrations/unplugin/README.md)) — 플러그인은
  `--emit-std`(stdout)로 모듈 본문을 받아 가상 모듈로 서빙합니다.

## 감시 모드 (`-w`)

한 번 컴파일한 뒤 종료하지 않고 입력을 계속 지켜봅니다. Ctrl-C로 멈춥니다.

```sh
rlc -w -o build src/          # 바뀔 때마다 다시 컴파일
rlc -w --check src/           # 쓰지 않고 검사만 (tsc --noEmit --watch에 해당)
rlc -w --types src/           # 사이드카를 계속 갱신 (변경 시 파이프라인 재실행)
```

- 입력은 매 회차 다시 수집하므로, 감시 중인 디렉터리에 **새로 생긴 `.rl`도**
  잡힙니다.
- **바뀐 파일의 importer도 함께 다시 컴파일합니다.** 다른 파일의 enum에
  케이스가 늘면 그것을 `match`하는 쪽에서 소진성 에러가 나야 하기 때문입니다
  ([language.md §7.3](./language.md#73-선언-수집과-프로젝트-단위-소진성)).

```
rlc: watching 2 file(s) — Ctrl-C to stop
rlc: ./a.rl → out/a.ts
rlc: ./b.rl:4:3: match on enum E (imported from "./a.rl") is not exhaustive:
     missing "C" (add the missing arms or a final `_` arm)
rlc: 2 file(s) failed — watching
```

파일 시각을 300ms마다 확인하는 방식이라 외부 의존성이 없고, 네트워크
파일 시스템에서도 동작합니다. `--symbols`·`--sidecar`처럼 컴파일하지 않는
모드와는 조합되지 않습니다.

## 에디터 사이드카 (`--sidecar`, 저수준)

`--types`의 마지막 단계를 따로 실행하는 저수준 훅입니다 — 일반 사용은
`--types`로 충분하고, 이 옵션은 선언 방출을 자체적으로 수행하는 도구
(VSCode 확장의 저장 시 갱신)가 씁니다.

```sh
rlc --sidecar types src/notice.rl                # src/notice.rl.d.ts + .map
rlc --sidecar types -o .rl-types src/notice.rl   # .rl-types/notice.rl.d.ts
```

선언 본문은 `<dir>/<이름>.d.ts`(tsc `--emitDeclarationOnly` 산출물)에서
가져오고, rlc는 그 선언들이 원본 `.rl`의 어디에서 왔는지만 맵으로
채웁니다. rl `enum`의 위치는 파싱 결과에서 정확히 가져오고, 통과 영역의
선언은 이름으로 찾습니다. `-o`가 없으면 입력 옆에, 있으면 그 트리에(입력
구조 미러) 씁니다. 맵의 `sources`는 사이드카 위치 기준 상대 경로로 적히므로
정의 이동은 어느 배치에서든 원본으로 갑니다.

에디터가 선언 맵을 따라가려면 그 `.ts` 파일을 포함하는 `tsconfig.json`이
있어야 합니다 — 추론 프로젝트로 열리면 맵 추적이 동작하지 않습니다.

## 배너

기본적으로 출력 첫 줄에 다음 주석이 붙습니다 (`--no-banner`로 생략):

```ts
// @generated from <입력 파일명> by rlc — do not edit directly.
```

## 진단 출력

- 성공적으로 쓴 파일마다 stderr에 진행 로그: `rlc: src/a.rl → src/a.ts`
  (`-p`/`--check`에서는 출력 없음).
- 컴파일 에러는 stderr에 `rlc: 파일:행:열: 메시지` 형식으로 출력됩니다.
  형식과 전체 목록은 [`errors.md`](./errors.md).
- stdout은 `-p`의 컴파일 결과(그리고 `-h`/`-v` 출력) 전용입니다 — 파이프로
  안전하게 받을 수 있습니다.

## 종료 코드

| 코드 | 의미 |
|------|------|
| 0 | 모든 파일 컴파일 성공 (또는 `-h`/`-v`) |
| 1 | 인자 에러, 입력 경로 없음, 또는 하나 이상의 파일이 컴파일/IO 실패 |

여러 파일을 처리할 때 한 파일이 실패해도 **나머지 파일은 계속 처리**하고,
마지막에 실패가 하나라도 있었으면 1로 종료합니다. 인자 에러와 존재하지 않는
입력 경로는 처리 시작 전에 즉시 1로 종료합니다.

## 사용 예

```sh
rlc -o build src/           # 소스 트리(.rl + .ts)를 build/ 아래 완결 트리로
rlc file.rl                 # file.ts 생성 (제자리 — .rl만)
rlc --types src/            # 타입 사이드카 생성 (.rl-types/)
rlc --check src/            # CI용: 검사만, 쓰기 없음
rlc -w -o build src/        # 감시하며 다시 빌드
rlc -p file.rl > out.ts     # stdout으로 출력 (도구용)
rlc --no-verify file.rl     # swc 검증 생략
rlc --symbols file.rl       # 심볼 JSON 출력 (언어 도구용)
```

두 모드, 한 파이프라인:

```jsonc
// 단독 (tsc) — rlc가 완결 트리를 만들고 tsc가 JS/타입검사를 맡습니다
{ "scripts": {
    "build": "rlc -o build src && tsc",
    "types": "rlc --types src",
    "check": "rlc --check src && tsc --noEmit" } }
```

```jsonc
// 번들러 (vite) — 플러그인이 같은 컴파일러를 모듈 단위로 호출합니다
{ "scripts": {
    "build": "vite build",
    "types": "rlc --types src" } }
```

소스도, 타입을 만드는 명령도 두 모드에서 동일합니다 — 다른 것은 `.rl`
지정자를 런타임에 해석하는 주체(방출 시 재작성 vs 플러그인)뿐입니다.
