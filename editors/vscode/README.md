# rl Language — VSCode 확장

rl(`.rl`) 파일을 위한 VSCode 언어 서비스입니다. VSCode 공식
[LSP 확장 패턴](https://code.visualstudio.com/api/language-extensions/language-server-extension-guide)
(lsp-sample 구조)을 따릅니다: `client/`는 `vscode-languageclient`로 서버를
띄우고, `server/`는 `vscode-languageserver`로 LSP를 구현합니다.

## 기능

| 기능 | 설명 |
|------|------|
| 문법 하이라이팅 | TypeScript 문법의 **완전 확장**(TSX 방식) — 업스트림 TS 문법 전체에 rl 규칙(enum·match·try·let-else·if let·`\|>`·`result`·`val`)을 짜 넣은 독립 문법이라, 함수 본문·초기화식 등 **모든 중첩 위치**에서 rl 구문이 칠해지고 순수 TS는 TS 문법과 동일하게 칠해진다. `syntaxes/rl.tmLanguage.json`은 생성물 — `syntaxes/src/`(vendored TS 문법 + rl 규칙)에서 `npm run grammar`로 재생성한다 |
| 파일 아이콘 | 탐색기·탭에서 `.rl` 파일에 "RL" 배지 아이콘 표시 (라이트/다크 테마별). 언어 기본 아이콘을 지원하는 파일 아이콘 테마(기본 Seti 포함)에서 보이며, 자체 `.rl` 아이콘을 정의한 테마가 있으면 그쪽이 우선 |
| 진단 (rl) | 편집할 때마다 **실제 컴파일러**(`rlc --check`)를 실행해 에러를 표시 — 에디터의 에러는 항상 컴파일러와 일치 |
| 진단 (타입) | 버퍼가 컴파일된 TypeScript를 타입 검사해 `match` 암·`\|>` 파이프라인 **안의 타입 에러까지** 원본 위치에 표시 (`source: ts`). `rl.typeDiagnostics`로 끌 수 있음 |
| 진단 (타입 기반 rl) | 타입이 있어야 판정되는 **rl 에러** — `val` 바인딩을 통한 변경, 스크루티니의 실제 타입 기준 소진성 — 을 편집 중에 표시. 서버가 버퍼를 자기 프로젝트의 일부로 두고 `rlc --check-types`를 돌리므로 문안은 컴파일러의 것 그대로. `rl.typedChecks`로 끌 수 있음 |
| 자동완성 | match 암 위치의 케이스 태그(이미 덮은 태그 제외 — 대상 enum은 구조적 추론, 실패 시 보이는 enum 전체), `Enum.` 뒤 생성자(필드 탭스톱 스니펫)**와 그 객체의 TS 멤버**(`Result.map`·`Option.unwrapOrElse` 등 표준 라이브러리 콤비네이터), `Tag(` 안의 필드 바인딩, `enum`/`match`/`try`/`flow`/`result`/`let-else` 스니펫. 그 외 위치·`obj.` 멤버 접근은 TypeScript 언어 서비스의 완성 목록(rl 항목이 위). 항목을 고르면 그 항목의 **타입 시그니처와 JSDoc**을 채워서 보여줌 |
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

rl 해석이 답하지 못하는 나머지 심볼은 **rl 엔진**(`rlc --server`)이
맡습니다. 서버는 열린 버퍼를 엔진에 전달하고(didOpen/didChange/didClose),
질문을 `.rl` 위치로 보내면 답도 `.rl` 위치로 돌아옵니다 — projection
(원본↔방출 TS 매핑), TypeScript 세션, 프로브까지 전부 엔진 안의 일입니다
([`lsp-architecture.md`](../../docs/design/lsp-architecture.md)). 방출물이
순수 TS이므로 match 암 본문·스크루티니·`try`/`let-else`/`if let` 식·
파이프라인 스텝·`result` 블록 *내부*에서도 호버·완성·정의 이동이 온전한
타입 추론으로 동작합니다.

타입이 `any`로 흘러내리지 않도록 두 가지가 더 보장됩니다:

- **`"@rl/std"`는 프로젝트 설정 없이도 해석됩니다.** 프로젝트가 그
  지정자를 직접 해석하면(`rlc --types` 산출물을 가리키는 tsconfig
  `paths` 등) 그쪽이 우선이고, 아니면 엔진이 표준 라이브러리 모듈을
  대신 넣어 둡니다. 설정이 없다고 `Option`/`Result`가 `any`가 되지
  않습니다.
- **import한 `.rl` 모듈도 방출물로 서빙됩니다.** 디스크의 `.rl`은 열려
  있지 않아도 엔진이 projection해서 넘깁니다(재-export 포함, 내용 기준
  캐시) — 원문을 넘기면 rl `enum`이 TS `enum`으로 잘못 파싱되어 그
  import를 건너온 값의 타입이 전부 무너집니다.

**입력 중인 `.` — 프로브.** 완성은 `.`를 친 그 순간에 요청되는데, 그때
버퍼는 아직 멤버가 없는 상태(`x |> .`)라 컴파일 결과가 원문 그대로입니다 —
`|>`에서 TS 파싱이 무너지므로 멤버가 하나도 안 나오고, 에디터는 그 빈 목록을
캐시해 이후에도 안 나옵니다. 그래서 위임이 빈손일 때 엔진이 커서 자리에
자리표시 식별자를 끼운 **프로브 소스**를 만들어(`x |> .$rl_probe`) 그
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

- **매핑되지 않는 스팬은 버립니다.** 컴파일러가 쓴 글루(switch IIFE,
  구조분해, `$rl_ap` 헬퍼)에 걸린 진단은 사용자 코드가 아니므로 표시하지
  않습니다 — 방출물 때문에 tsc 에러가 나면 그건 rlc의 버그입니다.
- **파싱되는 텍스트일 때만** 검사합니다. 진단에 파스 에러(TypeScript가
  1000–1999번을 쓰는 구문 에러)가 하나라도 있으면 그 파일의 진단은 통째로
  버립니다 — 미완성 rl 구문은 원문 그대로 projection되어 TypeScript가
  아니고, 체커의 오류 복구가 지어낸 에러가 뒤따르기 때문입니다.

### 타입 기반 rl 진단

`val` 경로의 built-in 변경 메서드 판정과 리터럴/좁혀진 타입 기준 소진성은
**타입이 있어야** 답할 수 있어 `rlc --check`가 내지 못합니다
([`language.md` §10.4](../../docs/reference/language.md#104-built-in-변경-메서드---types)).
그래서 서버는 편집 중인 버퍼를 그 파일이 프로젝트에서 차지하는 자리에
그대로 얹어(`rlc --check-types --rl-only --overlay <path>`) 컴파일러에게 묻고,
돌아온 문장을 **그대로** 표시합니다 — 무엇이 변경인지는 에디터가 판단하지
않습니다 (`source: rlc`).

이 검사는 프로젝트를 열고 TypeScript 컴파일러를 띄우므로 나머지 진단보다
느립니다. 그래서 별도의 긴 디바운스(약 1.2초)로 돌고, 끝나면 다시 게시합니다.
세 가지가 보장됩니다:

- **버전이 맞을 때만 표시합니다.** 검사가 끝난 사이에 버퍼가 바뀌었으면 그
  결과는 버립니다 — 위치가 어긋난 `val` 에러는 늦게 오는 것보다 나쁩니다.
- **한 위치에 한 진단.** 소진성은 `rlc --check`와 이 검사가 둘 다, 같은
  위치에 보고합니다. 먼저 도착한 쪽(사용자가 이미 읽고 있는 문장)이 남습니다.
- **답할 수 없으면 아무것도 바꾸지 않습니다.** TypeScript가 설치되지 않은
  프로젝트, 저장된 적 없는 버퍼, 검사가 시작조차 못한 경우 — 전부 "모름"이지
  "깨끗함"이 아니므로, 기존 진단을 그대로 둡니다. 이유는 출력 채널에 한 번만
  적습니다.

## 요구사항

### `rlc`

진단에는 `rlc` 바이너리가 필요합니다. 탐색 순서:

1. `rl.compilerPath` 설정
2. 워크스페이스의 `target/release/rlc` → `target/debug/rlc`
3. 워크스페이스에 `file:`로 설치된 로컬 개발용 `rl-lang` 패키지의 rlc
   (RL 저장소에서 `scripts/setup`을 돌린 경우 — `server/src/dev.ts`)
4. PATH의 `rlc`

`rlc`가 없으면 진단과 엔진 위임 기능이 꺼지고, rl 구문 계층(enum·케이스
태그·문서 심볼·빠른 수정)은 그대로 동작합니다.

### TypeScript (`tsgo`)

위 표에서 "TypeScript 언어 서비스 위임"이라고 적은 기능 — 호버·정의 이동·
참조 찾기·이름 변경·자동완성·시그니처 헬프·타입 진단 — 은 rl 엔진이
**TypeScript 컴파일러 자신의 언어 서버**(`tsgo --lsp`)를 몰아서 답합니다.
TypeScript 7에는 인프로세스 JS 언어 서비스 API가 없기 때문이며, 확장
프로그램은 TypeScript를 번들하지 않습니다. 엔진의 탐색 순서:

1. `RLC_TSGO_BIN` / `RLC_TSGO_ROOT` 환경 변수 — 직접 빌드한
   typescript-go 체크아웃 (`built/local/tsgo`)
2. 프로젝트에서 위로 올라가며 찾는 `node_modules/@typescript/
   typescript-<platform>/lib/tsc` (또는 `native-preview-<platform>`)

RL 저장소에서 `scripts/setup`으로 toolchain을 연결해 뒀다면(체크아웃 모드,
`.rl-dev/toolchain.json`) 이 서버가 rlc를 띄울 때 그 `RLC_TSGO_*` 변수를
**그 child process에만** 주입해 CLI launcher와 동일한 toolchain을 쓰게
합니다 — 셸이나 VSCode 환경은 건드리지 않습니다 (`server/src/dev.ts`).
npm 모드(`--tsgo-npm`)면 아무것도 주입하지 않고 위 순서 그대로 각
프로젝트의 TypeScript를 씁니다.

즉 **프로젝트가 `typescript@7`을 설치해 두면 그대로 동작합니다.** 찾지
못하면 위 위임 기능들이 답하지 않고, rl 자신이 아는 것(enum·케이스 태그·
소진성)만 동작합니다. TypeScript 세션이 죽으면 다음 질문이 새로
시작합니다 — 기능이 영구히 침묵하는 일은 없습니다.

표준 라이브러리(`@rl/std`)는 모듈 해석이 디스크를 보므로, 엔진이
프로젝트의 `node_modules/@rl/std`에 표준 라이브러리 모듈을 한 번 써
둡니다 (이미 있는 패키지는 건드리지 않습니다).

## 설정

| 설정 | 기본값 | 설명 |
|------|--------|------|
| `rl.compilerPath` | `""` | 진단에 사용할 rlc 경로 |
| `rl.verify` | `true` | `false`면 `rlc --check`에 `--no-verify` 전달 |
| `rl.typeDiagnostics` | `true` | `.rl` 파일에 TypeScript 타입 에러 표시 (위 "타입 진단") |
| `rl.typedChecks` | `true` | 타입이 있어야 판정되는 rl 진단 표시 (위 "타입 기반 rl 진단") |
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

저장소 루트의 `./scripts/setup`은 이 확장을 빌드해 vsix로 만들고 실제
VSCode에 설치까지 합니다 — 업데이트는 항상 기존 설치를 삭제한 뒤 새로
설치합니다 ([`CONTRIBUTING.md`](../../CONTRIBUTING.md)의 "로컬 개발 환경").

### 패키징

[`@vscode/vsce`](https://github.com/microsoft/vscode-vsce)로 vsix를 만듭니다.
client/·server/가 각자 `package.json`을 갖는 레이아웃이라 `--no-dependencies`
로 패키징합니다 (`.vscodeignore`가 담을 것을 그대로 결정합니다):

```sh
npm ci && npx tsc -b
npx @vscode/vsce package --no-dependencies
```

확장은 TypeScript를 번들하지 않습니다 — 타입은 rl 엔진이 프로젝트의
TypeScript 7(`tsgo`)로 검사하고, `tsgo`는 표준 라이브러리 선언을 실행
파일 안에 갖고 있습니다. vsix에 담기는 것은 컴파일된 서버/클라이언트와
LSP 런타임 패키지뿐입니다 (`.vscodeignore`가 그대로 결정합니다).
