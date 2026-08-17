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
| 진단 | 편집할 때마다 **실제 컴파일러**(`rlc --check`)를 실행해 에러를 표시 — 에디터의 에러는 항상 컴파일러와 일치 |
| 자동완성 | match 암 위치의 케이스 태그(이미 덮은 태그 제외), `Enum.` 뒤 생성자(필드 탭스톱 스니펫), `Tag(` 안의 필드 바인딩, `enum`/`match`/`try`/`let-else` 스니펫. 그 외 위치·`obj.` 멤버 접근은 TypeScript 언어 서비스의 완성 목록(rl 항목이 위) |
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
서비스**가 맡습니다(`.rl` 파일을 TS로 서빙 + `./x.rl` 지정자 커스텀 해석).
TS 파서의 오류 복구 덕분에 rl 구문이 섞여 있어도 통과 영역의 해석은
정상 동작하지만, rl 구문 *내부* 스팬의 해석은 복구 품질에 따라 부분적일
수 있습니다. TS 쪽 **진단은 표출하지 않습니다** — 에러는 언제나 rlc가
정본입니다.

## 요구사항

진단에는 `rlc` 바이너리가 필요합니다. 탐색 순서:

1. `rl.compilerPath` 설정
2. 워크스페이스의 `target/release/rlc` → `target/debug/rlc`
3. PATH의 `rlc`

`rlc`가 없으면 진단만 꺼지고 나머지 기능은 그대로 동작합니다.

## 설정

| 설정 | 기본값 | 설명 |
|------|--------|------|
| `rl.compilerPath` | `""` | 진단에 사용할 rlc 경로 |
| `rl.verify` | `true` | `false`면 `rlc --check`에 `--no-verify` 전달 |
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

패키징(vsix)은 [`@vscode/vsce`](https://github.com/microsoft/vscode-vsce)로:

```sh
npx @vscode/vsce package
```
