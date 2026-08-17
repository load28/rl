# rl Language — VSCode 확장

rl(`.rl`) 파일을 위한 VSCode 언어 서비스입니다. VSCode 공식
[LSP 확장 패턴](https://code.visualstudio.com/api/language-extensions/language-server-extension-guide)
(lsp-sample 구조)을 따릅니다: `client/`는 `vscode-languageclient`로 서버를
띄우고, `server/`는 `vscode-languageserver`로 LSP를 구현합니다.

## 기능

| 기능 | 설명 |
|------|------|
| 문법 하이라이팅 | rl 전용 구문(match 키워드, 암 태그, enum 케이스)을 칠하고 나머지는 VSCode 내장 TypeScript 문법(`source.ts`)에 위임 — 통과 원칙과 같은 구조 |
| 진단 | 편집할 때마다 **실제 컴파일러**(`rlc --check`)를 실행해 에러를 표시 — 에디터의 에러는 항상 컴파일러와 일치 |
| 자동완성 | match 암 위치의 케이스 태그(이미 덮은 태그 제외), `Enum.` 뒤 생성자(필드 탭스톱 스니펫), `Tag(` 안의 필드 바인딩, `enum`/`match`/`try`/`let-else` 스니펫 |
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
| `rl.trace.server` | `off` | LSP 통신 트레이스 |

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
