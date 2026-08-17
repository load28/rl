# rlc CLI 레퍼런스

rl 소스를 TypeScript로 컴파일하는 커맨드라인 도구 `rlc`의 사용법입니다.
언어 자체는 [`language.md`](./language.md), 진단 메시지는
[`errors.md`](./errors.md) 참조.

## 시놉시스

```
rlc [options] <file.rl | dir> ...
```

## 설치

```sh
cargo install --path .        # 또는
cargo build --release        # → target/release/rlc
```

## 옵션

| 옵션 | 설명 |
|------|------|
| `-o, --out-dir <dir>` | 출력을 `<dir>` 아래에 씁니다 (경로 규칙은 아래). 필요한 중간 디렉터리는 자동 생성됩니다. |
| `-p, --print` | 파일을 쓰는 대신 컴파일 결과를 stdout으로 출력합니다. |
| `--check` | 컴파일만 하고 아무것도 쓰지 않습니다 (문법·소진성 검사 용도). |
| `--emit-std <file>` | 표준 라이브러리 모듈(`Option`/`Result` + 콤비네이터, [`std.md`](./std.md))을 `<file>`에 씁니다. 입력 없이 단독으로 쓸 수도, 컴파일과 함께 쓸 수도 있습니다. 배너가 붙으며 `--no-banner`로 생략합니다. |
| `--no-banner` | 출력 첫 줄의 "generated" 배너 주석을 생략합니다. |
| `--no-verify` | 필드 타입 검사와 생성물 자가 검사를 생략합니다. 검증기가 아직 모르는 최신 TS 문법을 쓴 코드를 위한 탈출구입니다. |
| `--rewrite-imports <js\|ts\|bare\|off>` | 상대 경로 `.rl` import 지정자의 방출 형태 ([`language.md` §7](./language.md#7-모듈-rl-import-지정자-재작성)): `js`(기본) = `./x.js`, `ts` = `./x.ts`(tsc의 `rewriteRelativeImportExtensions` 필요), `bare` = `./x`, `off` = 재작성 끔. 그 외 값은 에러입니다. |
| `--sidecar <dir>` | 컴파일하지 않고 `<이름>.rl.d.ts`와 `.map`을 씁니다. 선언 본문은 `<dir>/<이름>.d.ts`(tsc `--emitDeclarationOnly` 산출물)에서 가져오고, 출력 위치는 `-o`가 없으면 입력 옆, 있으면 그 트리입니다 (아래 "에디터 사이드카"). |
| `--symbols` | 컴파일하지 않고 각 입력 파일의 rl enum 선언(위치 포함)과 직접 `.rl` import를 JSON으로 stdout에 출력합니다 (아래 "심볼 출력"). 언어 도구용. |
| `-h, --help` | 도움말을 출력하고 종료합니다 (종료 코드 0). |
| `-v, --version` | 버전만 출력하고 종료합니다 (종료 코드 0). |

- 옵션과 입력 인자는 순서 무관하게 섞어 쓸 수 있습니다.
- `-`로 시작하는 알 수 없는 인자는 에러입니다 (`rlc: unknown option ...`).
- `--`(옵션 종료 구분자)와 짧은 옵션 병합(`-po`)은 지원하지 않습니다.

## 입력 수집

각 입력 인자에 대해:

- **파일**이면 확장자와 무관하게 컴파일 대상에 추가됩니다.
  (`.rl`이 아닌 파일도 명시적으로 지정하면 컴파일을 시도합니다.)
- **디렉터리**면 재귀적으로 순회하며 **확장자가 `.rl`인 파일만** 수집합니다.
  하위 디렉터리 포함, 항목은 경로 기준 정렬 순서로 처리됩니다.
- 존재하지 않는 경로는 즉시 에러이며 컴파일을 시작하지 않습니다.
- 수집 결과가 비어 있으면 `rlc: no .rl files found` 에러입니다.

컴파일할 때 각 파일의 **직접 상대 경로 `.rl` import**를 추가로 읽어 enum
선언을 수집합니다 — import한 enum의 match 소진성 검사용입니다
([`language.md` §7.3](./language.md#73-선언-수집과-프로젝트-단위-소진성)).
읽을 수 없는 지정자는 조용히 건너뜁니다 (모듈 해석 에러는 tsc `TS2307`의
영역).

## 출력 경로 규칙

| 상황 | 출력 위치 |
|------|-----------|
| 기본 (`-o` 없음) | 입력 파일 옆에 같은 이름의 `.ts` (`src/a.rl` → `src/a.ts`) |
| `-o out/` + 파일 입력 | `out/<파일명>.ts` (`rlc -o out src/a.rl` → `out/a.ts`) |
| `-o out/` + 디렉터리 입력 | 입력 디렉터리 기준 상대 경로를 `out/` 아래에 미러 (`rlc -o out src/`에서 `src/x/b.rl` → `out/x/b.ts`) |

기존 파일은 덮어씁니다. `-p`가 있으면 파일을 쓰지 않고, `--check`면 아무
출력도 만들지 않습니다.

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

## 에디터 사이드카 (`--sidecar`)

`.ts` 파일이 `"./notice.rl"`을 import하면 tsserver가 확장자를 몰라
`TS2307`을 냅니다. TypeScript의 탈출구는 그 에러 메시지에 있습니다 —
"or its corresponding type declarations". `.rl` 옆에 선언 파일을 두면
해결됩니다.

```sh
tsc -p tsconfig.types.json          # rlc 출력에서 선언만 뽑아 types/에
rlc --sidecar types src/notice.rl   # src/notice.rl.d.ts + .map 생성
```

`-o`를 함께 주면 사이드카를 **별도 트리**에 씁니다 (입력 구조를 미러합니다).
소스 트리에 생성물을 남기지 않는 쪽이 에디터와 무관하게 깔끔합니다.

```sh
rlc --sidecar types -o .rl-types src/notice.rl   # .rl-types/notice.rl.d.ts
```

이때 소비 측 `tsconfig.json`에 **`rootDirs`**를 두어 두 디렉터리를 하나로
합쳐야 `"./notice.rl"`이 해석됩니다. 맵의 `sources`는 rlc가 사이드카 위치
기준 상대 경로(`../src/notice.rl`)로 적어 주므로 정의 이동은 그대로
원본으로 갑니다.

```jsonc
// src/tsconfig.json
"rootDirs": [".", "../.rl-types"]
```

두 파일이 생깁니다.

| 파일 | 역할 |
|------|------|
| `<이름>.rl.d.ts` | tsserver가 `"./<이름>.rl"`을 해결하는 근거 — 에러가 사라지고 자동완성·타입이 살아납니다 |
| `<이름>.rl.d.ts.map` | `sources`가 원본 `.rl` — **정의 이동이 `.d.ts`가 아니라 원본으로** 갑니다 |

선언 본문에는 타입 추론이 필요하므로 tsc가 만들고(`--emitDeclarationOnly`),
rlc는 그 선언들이 원본 `.rl`의 어디에서 왔는지만 채웁니다. rl `enum`의
위치는 파싱 결과에서 정확히 가져오고, 통과 영역의 선언은 이름으로 찾습니다.

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
rlc file.rl                 # file.ts 생성
rlc src/                    # src/ 아래 모든 .rl 재귀 컴파일 (제자리)
rlc -o dist/ src/           # dist/ 아래에 트리 미러
rlc -p file.rl > out.ts     # stdout으로 출력
rlc --check src/            # CI용: 검사만, 쓰기 없음
rlc --no-verify file.rl     # swc 검증 생략
rlc --emit-std src/rl.ts    # 표준 라이브러리 모듈 생성
rlc --rewrite-imports bare src/   # .rl import를 확장자 없이 방출 (번들러용)
rlc --symbols file.rl             # 심볼 JSON 출력 (언어 도구용)
```

빌드 파이프라인에서는 tsc 앞 단계로 실행합니다 (표준 라이브러리를 쓴다면
모듈 생성을 앞에 둡니다):

```jsonc
// package.json
{ "scripts": { "build": "rlc --emit-std src/rl.ts && rlc src/ && tsc" } }
```

`--emit-std` 성공 시 stderr에 `rlc: std → <파일>` 진행 로그가 출력됩니다.
