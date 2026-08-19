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
