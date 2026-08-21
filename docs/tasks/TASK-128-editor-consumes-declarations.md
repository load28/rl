# TASK-128: 에디터가 declarations를 소비하고 shadow를 삭제한다 (D6 2/2)

- **상태**: 완료
- **시작일**: 2026-08-21
- **완료일**: 2026-08-21
- **커밋**: —

## 목적

[TASK-127](./TASK-127-declarations-surface.md)의 후반부이자 컴파일러 중심부
완료 기준의 마지막 미해결 항목: VSCode 언어 서버가 컴파일러의
`declarations` 표면을 소비하고, `analysis.ts`의 정규식 기반 rl 의미론
재구현(GAP-3/D6 — `parseEnums`/`parseMatches`/`visibleEnums`/
`BUILTIN_ENUMS`/`caseSignature`와 `rlc --symbols` 병합 경로)을 삭제한다.
같은 규칙의 두 번째 구현이 에디터에서 사라진다.

## 범위

- 포함: `engine.ts`의 `declarations` 클라이언트(+타입), `server.ts`의
  소비 전환 — completion(`Enum.` 멤버·enum 목록·케이스 스니펫),
  documentSymbol, 소진성 quick fix(match 삽입 지점), `match` 키워드 hover
  — 과 `importedEnums`/`toImportedEnumInfos` 삭제, `analysis.ts`를 텍스트
  형태 유틸(`maskNonCode`/`wordAt`/`memberAccessAt`/`isIdent`)만 남기고
  삭제, 러스트 표면에 `body_open` 추가(quick fix의 한 줄 match 콤마
  판정), 테스트 갱신, `lsp-architecture.md` 갱신.
- 제외: `analysis.ts` 파일 자체의 제거(텍스트 유틸은 남는다 — 커서 문맥
  UI 보조이지 의미론이 아니다).

## 의사결정

### 결정 1: 삭제 경계는 "의미론"이다 — 텍스트 형태 유틸은 남긴다

- **상황**: D6의 원문은 "regex 기반 editor shadow semantics 제거".
- **선택과 근거**: 컴파일러와 **다른 답을 할 수 있는** 것(어느 enum이
  보이는가, match가 어디인가, 케이스의 필드가 무엇인가)이 shadow의
  해악이고, 전부 컴파일러 답으로 대체했다. 마스킹·단어 경계·`.` 판정은
  커서 문맥의 텍스트 성질이라 두 번째 의미론이 아니다 — 남긴다.

### 결정 2: sidecar 회귀 테스트의 픽스처를 blocking 에러로

- **상황**: "컴파일 안 되는 파일은 마지막 좋은 sidecar 유지" 테스트가
  중복 케이스를 픽스처로 썼는데, TASK-120 이후 중복 케이스는 **회복
  가능**해 방출·갱신된다(의도된 개선).
- **선택과 근거**: 테스트의 의도(낮출 수 없는 파일의 sidecar 보존)는
  유효하므로 픽스처만 진짜 blocking(stray `|>`)으로 교체.

## 작업 내역

- 2026-08-21: `engine.ts` — `EngineDeclarations` 타입군과 `declarations()`.
  `server.ts` — `declarationsOf`(버전 캐시; 형제 편집 시 무효화는 기존
  importedCache 규칙 승계), `analyze`를 masked 전용으로 축소,
  `caseSignature` 재구현(동일 포맷), completion·documentSymbol·
  codeAction·hover 전환, `importedEnums` 계열 삭제.
- 러스트: `MatchExpr`/`TupleMatchExpr`/`RlMatchSite`/서버 JSON에
  `body_open`(한 줄 match의 콤마 필요 판정 재료).
- `analysis.ts` — 633→257줄: 의미론 전부 삭제, 텍스트 유틸만.
  `analysis.test.ts` — 유틸 테스트로 재작성. `sidecar.test.ts` — 픽스처
  교체(결정 2).
- `lsp-architecture.md` — 소유 표·구조도·"Node에 남는 이유" 절 갱신.

## 이슈 및 해결

### 이슈 1: 확장 테스트 31건 실패 — npm TypeScript 7에 rlc의 API server가 없음

- **증상**: typed/service 계열 테스트가 "expected a completion answer"
  등으로 실패(이전 CI에서는 8건 skip이던 부류).
- **원인**: npm 배포본에는 rlc가 구동하는 tsgo API server가 없다(이 세션
  의 tsgo 직접 빌드 지시의 배경 그대로).
- **해결**: `RLC_TSGO_ROOT=../typescript-go`(직접 빌드)로 실행 —
  **74/74 전부 통과, skip 0** (이전 최고치는 skip 8).

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` (전 스위트 통과)
- [x] `editors/vscode`: `npx tsc -b` + `npm test` 74/74 (skip 0,
  RLC_TSGO_ROOT=직접 빌드 tsgo) + `npm run grammar:check`

## 결과

에디터의 rl 의미론이 컴파일러 단일 구현으로 통일됐다 — 컴파일러 중심부
완료 기준(compiler-core.md §14)의 "regex 기반 editor shadow semantics
제거" 충족. D6 종결.
