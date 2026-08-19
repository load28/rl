# TASK-077: 언어 서비스를 네이티브 백엔드로

- **상태**: 대기
- **시작일**: —
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

*작업 시작 시 기록.*

## 작업 내역

*작업 시작 시 기록.*

## 이슈 및 해결

*작업 시작 시 기록.*

## 검증

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo test`
- [ ] 확장 프로그램 `node --test`

## 결과

*작업 완료 시 기록.*
