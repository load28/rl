# TASK-025: TS 위임 확장 — 자동완성·참조 찾기·이름 변경

- **상태**: 완료
- **시작일**: 2026-08-17
- **완료일**: 2026-08-17
- **커밋**: — (구현 커밋 후 기록)

## 목적

TASK-024가 내장한 TypeScript 언어 서비스(`TsProject`) 위에 남은 세 기능을
연결한다: 일반 TS 심볼의 **자동완성**(멤버 접근 `obj.` 포함), **참조
찾기**, **이름 변경**. `.rl` 파일의 편집 경험을 `.ts`와 대등하게 만든다.

## 범위

- 포함: `TsProject`에 `completionsAt`/`referencesAt`/`renameAt` 래퍼 추가,
  서버 자동완성에 TS 항목 병합(rl 전용 컨텍스트는 유지), references/rename
  프로바이더 신설, 테스트, 문서.
- 제외: rl 심볼(enum·케이스 태그)의 이름 변경 — rl 인식 재작성(케이스
  태그는 방출물의 `kind` 문자열과 연동)이 필요하므로 거부(null)로 안전하게
  막고 후속 과제로 남긴다. completionResolve(상세 지연 로딩), 코드 렌즈.

## 의사결정

### 결정 1: 자동완성 병합 규칙 — rl 컨텍스트는 배타, 일반 위치는 rl 우선 병합

- **상황**: TS 완성 목록(전역 포함 수천 항목)을 어디에 어떻게 섞을지.
- **선택과 근거**: rl 전용 컨텍스트(match 암 패턴 위치, `Tag(` 바인딩,
  `Enum.` 생성자)는 rl 목록만 유지 — 그 자리엔 태그/필드만 유효해서 TS
  항목은 소음이다. `Enum.`이 아닌 멤버 접근(`obj.`)은 TS에 위임(기존엔 빈
  목록이었다), 일반 위치는 rl 항목(sortText `0`/`1`) 뒤에 TS 항목
  (`2`+TS sortText)을 붙이고 라벨 중복은 rl 쪽을 남긴다.

### 결정 2: rl 심볼의 이름 변경은 거부

- **상황**: 케이스 태그·enum 이름에 rename을 허용하면 TS가 절반만
  바꾼다(방출물의 `kind` 문자열·다른 파일의 match 암은 rl 의미론).
- **선택과 근거**: `symbolAt`이 rl 심볼로 해석하면 rename 요청에 null을
  반환해 막는다. 반쪽 rename보다 명시적 미지원이 안전하다. rl 인식
  rename(케이스 태그의 프로젝트 단위 재작성)은 후속 과제.

### 결정 3: `isDefinition`은 정의 스팬과 대조해 직접 계산

- **상황**: LSP references의 `includeDeclaration=false` 필터에 선언 여부가
  필요한데, 현행 TypeScript의 `ReferenceEntry.isDefinition`이 채워지지
  않았다(테스트에서 확인 — 전부 false).
- **선택과 근거**: `getDefinitionAtPosition` 결과와 (파일, 시작 오프셋)을
  대조해 계산한다. 정확하고 서비스 버전에 의존하지 않는다.

## 작업 내역

- 2026-08-17: TASK-025 등록.
- 2026-08-17: `tsproject.ts`에 `completionsAt`(ScriptElementKind 문자열
  그대로 반환 — `typescript` import는 이 모듈에만 격리),
  `referencesAt`(isDefinition 직접 계산), `renameAt`(`getRenameInfo`
  canRename 검사 + `findRenameLocations`) 추가.
- 2026-08-17: `server.ts` — capabilities에 references/rename 등록,
  `TS_COMPLETION_KINDS` 매핑과 `tsCompletions` 헬퍼, 자동완성 병합(결정 1),
  `onReferences`(전면 위임 + includeDeclaration 필터),
  `onRenameRequest`(rl 심볼 거부 → TS rename → 파일별 `WorkspaceEdit`).
- 2026-08-17: 테스트 3건 추가 — 일반 위치·`console.` 멤버 완성, `.rl`
  import 양방향 참조(선언 플래그 포함), 지역 심볼 rename 위치 2건.
- 2026-08-17: 문서 — 확장 README 기능 표(자동완성 갱신, 참조 찾기·이름
  변경 행 신설), CHANGELOG 항목 통합.

## 이슈 및 해결

### 이슈 1: `ReferenceEntry.isDefinition`이 항상 false

- **증상**: 참조 테스트의 `refs.some(r => r.isDefinition)` 단언 실패.
- **원인**: 현행 TypeScript(5.9)의 `getReferencesAtPosition`이
  `isDefinition`을 채우지 않는다 (해당 필드는 사실상 폐기 상태).
- **해결**: 결정 3 — 정의 스팬 대조로 직접 계산.

## 검증

- [x] Rust 게이트 — 이 태스크는 Rust 변경 없음 (fmt/clippy/test 재확인)
- [x] `editors/vscode`: `npm test` — 32개 통과 (기존 29 + 신규 3)

## 결과

- 추가: `docs/tasks/TASK-025-ts-completion-references-rename.md`
- 수정: `editors/vscode/server/src/{tsproject,server}.ts`,
  `editors/vscode/server/src/test/tsproject.test.ts`,
  `editors/vscode/README.md`, `CHANGELOG.md`, `docs/tasks/INDEX.md`
- 컴파일러(Rust)는 변경 없음.

후속 후보: rl 인식 이름 변경(케이스 태그를 선언·모든 match 암·`Enum.Tag`
사용처에서 함께 재작성 — `--symbols`가 이미 위치를 알므로 컴파일러 확장으로
가능), completionResolve로 TS 항목 상세 지연 로딩.
