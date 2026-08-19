# TASK-077: 언어 서비스를 네이티브 백엔드로

- **상태**: 진행 중
- **시작일**: 2026-08-19
- **완료일**: —
- **커밋**: —

## 목적

`editors/vscode/server/src/tsproject.ts`는 아직 **인프로세스 TypeScript 언어
서비스**(`import * as ts from "typescript"`)를 돌린다. TypeScript 7에는 그
JS API가 없으므로, hover·정의 이동·자동완성·참조 찾기·타입 진단을 네이티브
컴파일러 쪽으로 옮겨야 한다.

## 범위

- 포함: 어느 경로로 붙을지 결정(API 프리미티브 조립 vs tsgo LSP 서버),
  `.rl` 가상 문서 처리, 위치 매핑 재사용, 기능별 이전과 동등성 테스트.
- 제외: 이름 바꾸기 — 현재 API에 편집 목록을 만드는 표면이 없다.

## 실측: LSP 경로가 성립하는가 (2026-08-19)

"억지로 맞추는 것 아니냐"는 질문에 답하기 위해, tsgo의 LSP 서버(`tsgo --lsp
-stdio`)에 **디스크에 없는 문서**(에디터 버퍼에만 있는 lowering 결과)를
`textDocument/didOpen`으로 열고 기능별로 물어봤다. 스파이크는
`/home/user/lspspike`.

| 요청 | 가상 문서에 대한 결과 |
|------|----------------------|
| `hover` | `(alias) function describe(s: State): string` |
| `definition` | 디스크의 진짜 `src/user.ts`의 정확한 range로 이동 |
| `completion` | 1045개 항목 |
| `references` | 가상 문서 안의 range들 |
| `rename` | `WorkspaceEdit`(`changes`)로 편집 목록 반환 |
| `diagnostic`(pull) | `TS2322` + 정확한 range |

즉 **크로스 파일 해석까지 포함해 전부 답한다.** 파일이 디스크에 없어도
`didOpen`으로 준 내용이 곧 그 파일이다 — 이것은 LSP의 본래 동작이지 우회가
아니고, VS Code의 native-preview 확장이 tsgo를 구동하는 방식과 같은 경로다.

선언된 provider (요약): `hover / definition / typeDefinition / implementation
/ references / completion / signatureHelp / documentHighlight / documentSymbol
/ codeAction / codeLens / workspaceSymbol / formatting / rename / foldingRange
/ semanticTokens / inlayHint / diagnostic / callHierarchy`.

**API 경로와의 차이**: API에는 편집 목록을 만드는 표면이 없어 이름 바꾸기가
불가능했는데(TASK-074 조사), LSP에는 `renameProvider`가 있다. 그래서 이
태스크는 LSP 경로로 간다.

### 그래도 남는 진짜 제약

1. **rl 구문 위에서는 답이 없다.** `match` 키워드, 패턴 태그, `val` 같은
   rl 고유 구문은 lowering 결과에 대응하는 위치가 없다(`EmitMapping`은
   그대로 복사된 조각만 매핑한다). 그 자리의 hover/definition은 rl 자신의
   분석(`server/src/analysis.ts`)이 답해야 한다 — 지금도 그렇다.
2. **rename의 편집을 되돌려 매핑해야 한다.** 반환된 편집은 lowering 좌표다.
   글루 코드에 떨어진 편집은 `.rl`로 옮길 수 없으므로, **하나라도 매핑에
   실패하면 이름 바꾸기를 거부**해야 한다. 조용히 일부만 적용하는 것이
   최악이다.
3. **클라이언트가 서버 요청에 답해야 한다.** 스파이크에서 모든 요청이
   타임아웃했는데 원인은 서버가 보낸 `client/registerCapability`에 응답하지
   않은 것이었다. 진단이 아니라 클라이언트 구현 실수였다.
4. **전송이 둘로 갈린다.** rlc의 배치 검사는 API 서버를, 에디터는 LSP를 쓰게
   된다. 같은 컴파일러에 두 경로다 — 배치 질의와 대화형 기능은 모양이 다르니
   방어할 수 있지만, 설계 결정으로 명시해야 한다.

## 배경 (TASK-074 조사)

| 기능 | TS 7 API |
|------|----------|
| hover | 프리미티브 조립 (`getTypeAtPosition` + `typeToString` + 문서 주석) |
| 정의 이동 | `symbol.declarations` → NodeHandle(`path`, `resolve()` → `pos`/`end`) |
| 자동완성 | `getCompletionsAtPosition` |
| 참조 찾기 | `getReferencesToSymbolInFile` / `getReferencedSymbolsForNode` |
| 시그니처 도움말 | `getResolvedSignature` / `getSignaturesOfType` |
| 이름 바꾸기 | 없음 |

완성된 LS 진입점(`getQuickInfoAtPosition` 등)은 없다. 대안은 tsgo의 LSP
서버(`internal/lsp`)에 클라이언트로 붙는 것이며, 그 경우 `.rl`을 lowering한
문서를 `textDocument/didOpen`으로 열어야 한다.

## 선행 조건

- TASK-076(증분화)이 먼저면 좋다 — 편집마다 프로젝트를 새로 여는 구조로는
  에디터에서 쓸 수 없다.

## 의사결정

### 결정 1: LSP 경로 (API 프리미티브 조립이 아니라)

위 실측대로 LSP는 가상 문서에 대해 전 기능을 답하고, **이름 바꾸기는 LSP에만
있다**. API로 조립하면 rename을 못 만든다.

### 결정 2: 문서 이름은 CLI 백엔드와 같게 — `<원본>.rl.ts`

에디터와 CLI가 "lowering된 모듈의 이름"에 대해 같은 규약을 쓴다. 덕분에
`import "./x.rl"`이 양쪽에서 똑같이 해석된다.

### 결정 3: 인터페이스는 기존 `TsProject`의 표면을 그대로 (offset 기반)

`server.ts`가 쓰는 모양(`quickInfoAt`/`definitionsAt`/...)을 그대로 구현해,
교체가 국소적으로 끝나게 한다. LSP의 line/character ↔ offset 변환은
`lsp.ts`가 담당한다.

### 결정 4: rename은 "하나라도 매핑 실패하면 전체 거부"

반환되는 편집은 lowering 좌표다. 글루 코드에 떨어진 편집은 `.rl`로 옮길 수
없으므로 **일부만 적용하지 않는다**. 매핑은 호출자(`server.ts`)가 하므로
거부도 거기서 강제한다 — `tsgo.ts`는 위치를 하나도 빠뜨리지 않고 돌려주는
것으로 그 판단을 가능하게 한다.

## 작업 내역

- 2026-08-19: LSP 경로 실측 (위 표).
- 2026-08-19: `server/src/lsp.ts` — 최소 LSP 클라이언트(프레이밍, 요청/응답,
  문서 동기화, **서버 요청 응답**, 요청 타임아웃).
- 2026-08-19: `server/src/tsgo.ts` — `TsProject`와 같은 표면을 LSP로 구현:
  hover / definition / references / completion / diagnostics(pull) /
  rename / signature help.
- 2026-08-19: `server/src/test/tsgo.test.ts` 10건 — 실제 tsgo 상대로 통과.
  디스크에 없는 버퍼에 대해 hover가 답하고, definition이 디스크의 `.ts`로
  건너가고, 편집 후 재질의가 새 텍스트로 답하는 것을 확인.

### 막힌 지점: 타입의 *이름*을 구조적으로 얻을 방법이 없다

`server.ts`의 `typeAt` 호출부(match 스크루티니가 어느 rl enum인지 고르는 자리)
는 `{ name, declFile }`을 요구한다. TS 7에서 이것만 대체가 안 된다.

| 시도 | 결과 |
|------|------|
| LSP `textDocument/typeDefinition` | 유니언 **구성원**들의 위치로 감 (`{ kind: "Circle" }`), 별칭 이름은 없음. 파일(`declFile`)은 얻어짐 |
| API `getTypeAtPosition(...).getSymbol()` | 유니언 별칭에는 심볼이 **없다** (`(none)`) |
| API 와이어(`TypeResponse`) | `aliasSymbol` 필드 자체가 없다 |
| `typeToString` | `"Shape"` / `"Option<string>"` — **렌더된 문자열** |

즉 이름을 얻는 유일한 길이 타입 문자열이고, 그것을 파싱해 의미를 정하는 것은
이 재설계가 처음부터 금지한 것이다. 여기가 "억지로 맞추게 되는" 지점이다.

### 재측정: 이름 없이 정확히 된다 (2026-08-19)

"정확하게 구현 못하냐"는 물음에 다시 재봤다. 이름이 아니라 **판별자(`kind`)
리터럴 집합**으로 고르면 정확하다:

```
Shape           -> tags: ["Circle","Point"]
Option<string>  -> tags: ["None","Some"]     ← 제네릭 인스턴스도 같은 집합
Token           -> tags: ["Eof","Num"]
number          -> tags: null                 ← 태그드 유니언이 아니면 무답
```

`getTypeAtPosition` + `getPropertyOfType("kind")` + 리터럴 값 — 전부 구조적이고,
문자열 파싱이 없다. 이것은 CLI의 tag 소진성이 이미 하는 질문과 **같은 질문**이다.
유일한 비정밀성은 태그 집합이 완전히 같은 서로 다른 enum 둘인데, `match`의
관점에서는 팔 구성이 같으므로 구별할 필요가 없다.

즉 아래 (B)(문자열 파싱)와 (C)(TS 5 잔존)는 필요 없다. **(A)로 간다.**

### 왜 LSP인가 — API로는 에디터를 못 덮는다 (실측)

"LSP를 쓰는 것 자체가 이상하다"는 지적에 대해, API만으로 에디터 기능을 만들 수
있는지 재봤다.

| 기능 | API로 시도한 결과 |
|------|-------------------|
| hover | `getTypeAtPosition` + `typeToString` — 표시용 문자열로는 됨 |
| definition | `getSymbolAtPosition`이 **별칭(import) 심볼**을 주어, 진짜 선언까지 `getAliasedSymbol` 홉이 더 필요 |
| references | `getReferencesToSymbolInFile` — **한 파일 안만**. 크로스 파일은 직접 순회해야 함 |
| completions | `getCompletionsAtPosition`이 **예외**를 던진다: `completion list needs auto imports` |
| rename | 없음 |

그리고 API의 `LanguageService` 표면은 다섯 개뿐이다 (import 편집 2, 노드 참조,
시그니처 사용처, 자동완성). 즉 **TS 7은 의도적으로 나눠 놓았다** — API는 컴파일러
질문(타입·심볼·진단·emit)용이고, **에디터 조작은 언어 서버(LSP)의 몫**이다.

따라서 배치는 억지가 아니라 제품의 설계를 따르는 것이다:

- **rl이 주인인 질문**(어느 enum인가, 소진성, `val`) → rlc → **API**. 구조적.
- **TypeScript 에디터 조작**(hover/definition/completion/references/rename)
  → **LSP**.

API로 에디터 기능을 재구현하는 쪽이야말로 LSP가 이미 하는 일을 다시 만드는
억지가 된다.

**남은 선택지 기록** (위 재측정으로 (A) 확정):

- (A) **이름 없이 고른다** — 후보 enum을 `kind` 프로퍼티의 리터럴 집합으로
  고른다. 구조적이고, CLI의 tag 소진성이 이미 하는 것과 같은 질문이다.
  다만 LSP에는 타입 구조 질의가 없으므로 이 질문은 rlc에게 가야 한다
  (그 질문의 주인이 rlc이기도 하다). 비용: 에디터용 rlc 질의 통로가 필요.
- (B) `typeToString`의 선두 식별자를 **힌트**로 쓰고 `declFile`로 확인 —
  오늘 당장 되지만, 금지한 그 문자열 파싱이다.
- (C) 이 질의만 `tsproject.ts`(TS 5)에 남긴다 — JS API 의존이 영원히 남는다.

## 이슈 및 해결

### 이슈 1: 모든 요청이 타임아웃

- **증상**: initialize는 답하는데 hover/definition/diagnostic이 전부 타임아웃.
- **원인**: 서버가 시작 중에 보내는 `client/registerCapability` **요청**에
  클라이언트가 응답하지 않았다. 서버는 그 응답을 기다리며 이후 요청을 처리하지
  않는다. 에러도 로그도 없다.
- **해결**: `dispatch`가 서버 요청에 항상 답한다 (`workspace/configuration`은
  `[{}]`, 나머지는 `null`).

### 이슈 2: 사이드카 종료 코드가 "막힘"과 "보고됨"을 구분하지 못했다

- **증상**: 중복 케이스 같은 rl 에러로 lowering이 막혔는데 `--native-sidecar`가
  0으로 끝나, 확장 프로그램이 "성공"으로 보고 이전 사이드카를 버릴 수 있었다.
  확장 테스트 1건이 이걸 잡았다.
- **원인**: TASK-076에서 한 패스의 결과를 `reported: usize` 하나로 합치면서
  "아무것도 못 썼다"가 "1건 보고했다"와 같아졌다.
- **해결**: `Report { reported, blocked }`로 분리. 막힌 패스는 실패로 끝난다.

## 검증

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo test`
- [ ] 확장 프로그램 `node --test`

## 결과

*작업 완료 시 기록.*
