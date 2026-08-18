# rl Language — VSCode 확장

rl(`.rl`) 파일을 위한 VSCode 언어 서비스입니다. VSCode 공식
[LSP 확장 패턴](https://code.visualstudio.com/api/language-extensions/language-server-extension-guide)
(lsp-sample 구조)을 따릅니다: `client/`는 `vscode-languageclient`로 서버를
띄우고, `server/`는 `vscode-languageserver`로 LSP를 구현합니다.

## 기능

| 기능 | 설명 |
|------|------|
| 문법 하이라이팅 | rl 전용 구문(match 키워드, 암 태그, enum 케이스)을 칠하고 나머지는 VSCode 내장 TypeScript 문법(`source.ts`)에 위임 — 통과 원칙과 같은 구조 |
| 파일 아이콘 | 탐색기·탭에서 `.rl` 파일에 "RL" 배지 아이콘 표시 (라이트/다크 테마별). 언어 기본 아이콘을 지원하는 파일 아이콘 테마(기본 Seti 포함)에서 보이며, 자체 `.rl` 아이콘을 정의한 테마가 있으면 그쪽이 우선 |
| 진단 (rl) | 편집할 때마다 **실제 컴파일러**(`rlc --check`)를 실행해 에러를 표시 — 에디터의 에러는 항상 컴파일러와 일치 |
| 진단 (타입) | 버퍼가 컴파일된 TypeScript를 타입 검사해 `match` 암·`\|>` 파이프라인 **안의 타입 에러까지** 원본 위치에 표시 (`source: ts`). `rl.typeDiagnostics`로 끌 수 있음 |
| 자동완성 | match 암 위치의 케이스 태그(이미 덮은 태그 제외 — 대상 enum은 구조적 추론, 실패 시 스크루티니의 **TS 추론 타입**으로 특정), `Enum.` 뒤 생성자(필드 탭스톱 스니펫)**와 그 객체의 TS 멤버**(`Result.map`·`Option.unwrapOrElse` 등 표준 라이브러리 콤비네이터), `Tag(` 안의 필드 바인딩, `enum`/`match`/`try`/`flow`/`result`/`let-else` 스니펫. 그 외 위치·`obj.` 멤버 접근은 TypeScript 언어 서비스의 완성 목록(rl 항목이 위). 항목을 고르면 그 항목의 **타입 시그니처와 JSDoc**을 채워서 보여줌 |
| 시그니처 헬프 | 호출을 쓰는 동안 파라미터 힌트 — TypeScript 언어 서비스 위임이라 match 암·`\|>` 파이프라인 안에서도 동작 |
| 참조 찾기 | TypeScript 언어 서비스 위임 — `.rl` import 너머 선언·사용처 포함 |
| 이름 변경 | 일반 TS 심볼은 TypeScript 언어 서비스 위임. rl 심볼(enum·케이스 태그)은 방출물의 `kind` 문자열과 연동되므로 거부(안전) |
| 호버 | enum·케이스 선언 시그니처와 컴파일 형태 설명 (내장 `Option`/`Result`·import한 enum 포함). 그 외 심볼은 TypeScript 언어 서비스의 quick info |
| 정의로 이동 | rl 심볼(케이스 태그·enum 이름)은 선언 위치로 — **`.rl` import 너머까지**. **그 외 모든 심볼(변수·함수·타입·import된 값)은 TypeScript 언어 서비스에 위임** — `.ts` 파일에서처럼 동작하고, `./x.rl` import도 따라간다 |
| 문서 심볼 | Outline에 enum과 케이스 트리 표시 |
| 빠른 수정 | 소진되지 않은 match에 "빠진 암 추가" / "와일드카드 `_` 암 추가" (import한 enum 포함) |

심볼 해석은 컴파일러와 동일한 규칙을 따릅니다: 직접 `.rl` import의
exported enum이 자동완성·호버·정의 이동에 포함되고(별칭 반영, named import
한정 — `* as ns`는 아직), 섀도잉은 **로컬 > 임포트 > 내장** 순입니다.
크로스 파일 정보는 서버가 rl 문법을 다시 구현하지 않고 컴파일러의 심볼
인터페이스(`rlc --symbols`)를 소비해 얻습니다 — 저장된 파일 기준이므로
import 줄을 편집한 직후에는 저장 전까지 한 박자 늦을 수 있습니다.

rl 해석이 답하지 못하는 나머지 심볼은 서버에 내장된 **TypeScript 언어
서비스**가 맡습니다. 열린 `.rl` 버퍼는 컴파일러의 방출 결과를 **가상
TypeScript 문서**로 서빙하고(`rlc --emit-map`, 원본↔방출 오프셋 매핑으로
질의·결과를 왕복), 방출물이 순수 TS이므로 match 암 본문·스크루티니·
`try`/`let-else`/`if let` 식·파이프라인 스텝·`result` 블록 *내부*에서도 호버·완성·정의
이동이 온전한 타입 추론으로 동작합니다. match 암 자동완성은 스크루티니의
TS 추론 타입으로 대상 enum을 특정합니다(구조적 추론이 실패할 때).

타입이 `any`로 흘러내리지 않도록 세 가지가 더 보장됩니다:

- **`"@rl/std"`는 프로젝트 설정 없이도 해석됩니다.** 프로젝트가 그
  지정자를 직접 해석하면(`rlc --types` 산출물을 가리키는 tsconfig
  `paths` 등) 그쪽이 우선이고, 아니면 컴파일러 자신의 표준 라이브러리
  모듈(`rlc --emit-std`)이 대신 들어갑니다. 설정이 없다고 `Option`/
  `Result`가 `any`가 되지 않습니다.
- **import한 `.rl` 모듈도 방출물로 서빙됩니다.** 디스크의 `.rl`은
  열려 있지 않아도 `rlc --emit-map`으로 컴파일해서(파일 mtime 기준 캐시)
  넘깁니다 — 원문을 넘기면 rl `enum`이 TS `enum`으로 잘못 파싱되어 그
  import를 건너온 값의 타입이 전부 무너집니다.
- **호버·완성·정의 요청은 최신 방출물을 기다립니다.** 가상 문서는 진단과
  같은 디바운스로 갱신되지만, 그 사이에 들어온 요청은 방출을 한 번 돌려
  최신 문서로 답합니다(같은 버전의 동시 요청은 한 번만 컴파일). 이 대기가
  없으면 타이핑 직후의 호버가 원문으로 답해 `|>` 식 전체가 `any`가 됩니다.

컴파일러가 아예 없을 때만 원문을 그대로 서빙하는 예전 방식(TS 오류 복구)으로
폴백합니다.

**입력 중인 `.` — 프로브.** 완성은 `.`를 친 그 순간에 요청되는데, 그때
버퍼는 아직 멤버가 없는 상태(`x |> .`)라 컴파일 결과가 원문 그대로입니다 —
`|>`에서 TS 파싱이 무너지므로 멤버가 하나도 안 나오고, 에디터는 그 빈 목록을
캐시해 이후에도 안 나옵니다. 그래서 위임이 빈손일 때 커서 자리에 자리표시
식별자를 끼운 **프로브 소스**를 한 번 컴파일해서(`x |> .$rl_probe`) 그
방출물의 매핑된 위치에서 멤버를 얻습니다. 프로브는 완성 전용이며 그 질의
동안에만 서빙됩니다 — 사용자가 쓰지 않은 텍스트로 진단이 만들어지는 일은
없습니다.

### 타입 진단

TypeScript 언어 서비스가 보는 것이 방출물이므로, **그 타입 에러를 원본
`.rl` 위치로 되돌려 표시합니다**(`rl.typeDiagnostics`, 기본 켜짐). rl
구문 안에서만 드러나는 타입 에러 — 예를 들어 `|>` 스텝의 인자 타입이
head와 맞지 않아 콤비네이터 파라미터가 `unknown`으로 추론되는 경우 — 도
이제 편집기에서 바로 보입니다.

에러 계층은 그대로입니다:

- **rl 수준 에러**(중복 케이스, 소진되지 않은 match, 잘못된 필드 타입)는
  `rlc --check`만 냅니다 (`source: rlc`).
- **타입 에러**는 tsc만 냅니다 (`source: ts`, `code`는 TS 에러 번호).

안전장치 두 가지 때문에 잘못된 진단이 새어 나오지 않습니다.

- **가상 문서일 때만** 검사합니다. 원문 서빙 중(컴파일러 없음, 방출 실패)
  이면 TS가 보는 것은 rl 구문이 섞인 텍스트라 오류 복구가 지어낸 에러가
  쏟아지므로 한 건도 표출하지 않습니다. 방출물에 구문 에러가 있으면
  (컴파일되지 않은 버퍼) 그 파일의 타입 진단은 통째로 버립니다.
- **매핑되지 않는 스팬은 버립니다.** 컴파일러가 쓴 글루(switch IIFE,
  구조분해, `$rl_ap` 헬퍼)에 걸린 진단은 사용자 코드가 아니므로 표시하지
  않습니다 — 방출물 때문에 tsc 에러가 나면 그건 rlc의 버그입니다.
- **타입 환경이 온전할 때만** 검사합니다. TypeScript의 `lib.*.d.ts`를
  찾지 못하면 전역 타입이 통째로 없는 프로그램이 되어 멀쩡한 코드에
  엉뚱한 에러(예: 튜플 구조 분해에 `TS2488`)가 붙으므로, 그 상태에서는
  타입 진단을 전부 끄고 출력 채널에 사유를 한 번 기록합니다.

## 요구사항

진단에는 `rlc` 바이너리가 필요합니다. 탐색 순서:

1. `rl.compilerPath` 설정
2. 워크스페이스의 `target/release/rlc` → `target/debug/rlc`
3. PATH의 `rlc`

`rlc`가 없으면 진단과 가상 문서 서빙이 꺼지고, 나머지 기능은 원문 서빙
폴백으로 그대로 동작합니다.

## 설정

| 설정 | 기본값 | 설명 |
|------|--------|------|
| `rl.compilerPath` | `""` | 진단에 사용할 rlc 경로 |
| `rl.verify` | `true` | `false`면 `rlc --check`에 `--no-verify` 전달 |
| `rl.typeDiagnostics` | `true` | `.rl` 파일에 TypeScript 타입 에러 표시 (위 "타입 진단") |
| `rl.sidecar` | `refresh` | 저장 시 에디터 사이드카 갱신 — `refresh`(이미 있는 것만) / `always`(없으면 생성) / `off` |
| `rl.sidecarDir` | `""` | 사이드카를 쓸 디렉터리(워크스페이스 기준). 비우면 `.rl` 옆 |
| `rl.trace.server` | `off` | LSP 통신 트레이스 |

## `.ts`에서 `.rl` 가져다 쓰기

`.ts` 파일은 tsserver가 담당하는데 tsserver는 `.rl` 확장자를 모르므로,
`import { Notice } from "./notice.rl"`은 그대로 두면 `TS2307`이 됩니다.
`.rl` 옆에 **사이드카**(`notice.rl.d.ts` + `notice.rl.d.ts.map`)를 두면
해결됩니다 — 에러가 사라지고, 정의 이동이 `.d.ts`가 아니라 **원본 `.rl`의
해당 줄**로 갑니다.

사이드카는 `rlc --sidecar`가 만들고, 이 확장이 **저장할 때마다 갱신**합니다.
기본값 `refresh`는 이미 있는 사이드카만 다시 씁니다 — 프로젝트가
`rlc --sidecar`를 한 번 돌려 명시적으로 참여한 경우에만 파일이 생깁니다.
처음부터 자동으로 만들려면 `rl.sidecar`를 `always`로 두세요.

컴파일에 실패한 저장은 사이드카를 건드리지 않습니다. 편집 도중 선언이
사라지는 대신 마지막으로 성공한 상태가 유지됩니다.

사이드카를 읽으려면 그 `.ts` 파일을 포함하는 `tsconfig.json`이 있어야
합니다 — 추론 프로젝트로 열리면 tsserver가 선언 맵을 따라가지 않습니다.

### 소스 트리를 어지럽히지 않게

**권장: 사이드카를 별도 트리에 두세요.** `rl.sidecarDir`을 `.rl-types` 같은
값으로 두면 저장 시 그쪽에 쓰이고, 소스 트리에는 아무것도 생기지 않습니다.
소비 측 `tsconfig.json`에 `rootDirs`를 함께 두면 `"./x.rl"`이 그대로
해석되고 정의 이동도 원본으로 갑니다.

```jsonc
// .vscode/settings.json 또는 워크스페이스 설정
"rl.sidecarDir": ".rl-types"

// src/tsconfig.json
"rootDirs": [".", "../.rl-types"]
```

이 방식은 에디터와 무관하게 동작합니다 — 생성물이 소스와 섞이지 않으니
탐색기 설정이 필요 없습니다.

사이드카를 소스 옆에 두는 경우(`rl.sidecarDir`이 비어 있을 때)를 위해
이 확장이 보이는 방식을 정리해 둡니다.

| 기본값 | 효과 |
|--------|------|
| `explorer.fileNesting` | `notice.rl.d.ts`와 `.map`을 `notice.rl` 아래로 접어 넣습니다 |
| `search.exclude` | 검색 결과에서 뺍니다 |
| `files.readonlyInclude` | 생성물이므로 읽기 전용으로 엽니다 |

파일 자체도 `// @generated ... do not edit.` 배너로 시작합니다. 셋 다
사용자 설정으로 덮어쓸 수 있고, 아예 숨기려면 `files.exclude`에
`**/*.rl.d.ts`와 `**/*.rl.d.ts.map`을 추가하세요.

생성물이므로 `.gitignore`에 넣는 것을 권합니다.

```gitignore
*.rl.d.ts
*.rl.d.ts.map
```

## 개발

```sh
cd editors/vscode
npm install        # client/server 의존성까지 설치 (postinstall)
npm run compile    # tsc -b (client + server)
npm test           # 서버 분석 로직 단위 테스트 (node --test)
```

VSCode에서 `editors/vscode` 폴더를 열고 **F5** (Launch Extension)를 누르면
확장 개발 호스트가 뜹니다. `.rl` 파일을 열어 확인하세요.

### 패키징

[`@vscode/vsce`](https://github.com/microsoft/vscode-vsce)로 vsix를 만듭니다.
client/·server/가 각자 `package.json`을 갖는 레이아웃이라 `--no-dependencies`
로 패키징합니다 (`.vscodeignore`가 담을 것을 그대로 결정합니다):

```sh
npm ci && npx tsc -b
npx @vscode/vsce package --no-dependencies
```

**패키징 후 반드시 확인할 것** — 언어 서버는 런타임에 TypeScript로 타입을
검사하므로 TypeScript의 `lib*.d.ts`가 vsix 안에 들어가야 합니다.
`.vscodeignore`의 `**/*.ts`가 선언 파일까지 걸러내기 때문에, 뒤쪽의
`!server/node_modules/typescript/lib/lib*.d.ts` 한 줄이 그것들을 되살립니다.
이게 빠지면 확장은 **뜨긴 하지만 전역 타입이 하나도 없는 채로** 검사해
멀쩡한 코드에 거짓 에러(예: 튜플 구조 분해에 `TS2488`)를 답니다:

```sh
npx @vscode/vsce ls --no-dependencies | grep -c "typescript/lib/lib"
# → 100 (0이면 lib이 빠진 것 — 그 vsix는 배포하면 안 됩니다)
```
