# TASK-024: 언어 서버 TS 위임 — rl 파일 전반의 심볼 이동

- **상태**: 완료
- **시작일**: 2026-08-17
- **완료일**: 2026-08-17
- **커밋**: — (구현 커밋 후 기록)

## 목적

`.rl` 파일은 TypeScript + 4구문인데, 언어 서버의 정의 이동·호버가 rl
enum/케이스에만 동작한다 — 변수·함수·타입·import된 값 같은 **일반 TS
심볼은 이동이 안 된다**. TypeScript 언어 서비스를 서버에 내장해, rl 전용
해석이 답하지 못하는 심볼을 TS 서비스에 위임한다. TS 파서는 오류 내성이
강해 rl 구문이 섞인 파일에서도 통과 영역(파일의 대부분)의 심볼을 정상
해석하고, `./x.rl` import는 커스텀 모듈 해석으로 `.rl` 파일에 연결한다.

## 범위

- 포함: `server/src/tsproject.ts`(TS LanguageService 호스트 — 열린 문서
  오버레이 + 디스크, `.rl` 지정자 해석, 정의/quickInfo 래퍼), 정의 이동과
  호버의 TS 폴백 통합, `typescript` 런타임 의존성, 테스트, 문서.
- 제외: TS 진단 표출(진단은 계속 rlc가 정본), TS 자동완성/참조 찾기/이름
  변경(후속 후보), `.ts` 파일 쪽에서 `.rl`을 여는 방향(내장 TS 확장의
  영역), rl 구문 내부 스팬의 완전한 해석 보장(TS 오류 복구에 의존 —
  통과 영역이 목표).

## 의사결정

### 결정 1: rl 소스를 그대로 TS 서비스에 먹인다 (컴파일 산출물/투영이 아니라)

- **상황**: TS 서비스에 무엇을 보여줄지 — ① rlc 컴파일 산출물(+오프셋
  매핑 필요, 소스맵 없음), ② 길이 보존 투영(rl 구문을 동일 길이 TS로
  치환 — 일반적으로 불가능), ③ 원본 그대로.
- **선택과 근거**: ③ 원본 그대로. TS 파서는 오류 복구가 강해 rl 구문이
  섞여도 통과 영역(파일의 대부분)의 심볼은 정상 해석된다 — 테스트
  `rl constructs in the file do not break nearby TS resolution`으로 고정.
  오프셋이 원본과 1:1이라 위치 매핑이 사라지고, rl 구문 내부 스팬의 해석이
  부분적일 수 있다는 한계는 README에 명시했다. TS 진단은 표출하지 않으므로
  (에러는 rlc 정본) 파스 에러가 사용자에게 새어 나가지 않는다.

### 결정 2: rl 해석 우선, TS는 폴백

- **상황**: 정의 이동·호버에서 두 해석기의 우선순위.
- **선택과 근거**: rl 전용 해석(enum·케이스, `--symbols` 크로스 파일
  포함)이 먼저 답하고, 못 찾으면 TS 서비스에 위임한다. rl enum은 방출
  형태·소진성 등 rl 고유 정보가 더 정확하기 때문. 내장 `Option`/`Result`
  이름은 rl이 위치를 모르므로(선언이 없음) TS 폴백으로 흘려보낸다 —
  사용자가 std 모듈에서 import했다면 TS가 그 선언으로 이동시킨다.

### 결정 3: `.rl` 지정자는 커스텀 리졸버, 파일은 TS로 서빙

- **상황**: `import { add } from "./util.rl"`을 TS 서비스가 해석해야 한다.
- **선택과 근거**: `resolveModuleNameLiterals`에서 상대 `.rl` 지정자를
  실제 `.rl` 파일로 해석하고(`extension: Ts`로 선언해 TS 콘텐츠로 취급),
  나머지는 표준 `ts.resolveModuleName`(Bundler 모드)에 위임한다.
  `getScriptKind`가 `.rl`을 `ScriptKind.TS`로 보고한다. 컴파일러의 지정자
  재작성(1단계)과 대칭 — 에디터에선 소스가 소스를 가리킨다.

### 결정 4: 호스트는 열린 문서 오버레이 + 디스크, 열린 rl 문서가 루트

- **상황**: TS 프로젝트의 파일 집합과 버전 관리.
- **선택과 근거**: 루트 = 열린 rl 문서들, 그 import가 끌어오는 파일은
  디스크에서 읽는다(mtime 버전). 열린 문서는 버퍼 내용을 서빙하므로
  미저장 편집도 즉시 반영된다 — `--symbols` 기반 rl 크로스 파일 기능과
  달리 저장 지연이 없다. tsconfig.json은 읽지 않는다(해석 전용이라 소비
  옵션과 무관, rlc가 TS 설정을 해석하지 않는 원칙과 일치).

## 작업 내역

- 2026-08-17: TASK-024 등록.
- 2026-08-17: `server/src/tsproject.ts` 신설 — `TsProject`(LanguageService
  호스트: 열린 문서 오버레이/디스크 스냅숏, `.rl` 커스텀 모듈 해석,
  `definitionsAt`/`quickInfoAt` 래퍼 — 서비스 예외는 빈 결과로 열화),
  `positionAt`(UTF-16 오프셋 → LSP Position). `typescript`를 서버 런타임
  의존성으로 추가.
- 2026-08-17: `server.ts` — `getTsProject()` 지연 생성, `onDefinition`을
  "rl 해석 성공 시 그 위치, 아니면 `tsDefinitions`"로 재구성(내장 enum도
  TS 폴백으로), `onHover`에 `tsHover`(quick info 시그니처 + 문서) 폴백.
- 2026-08-17: 테스트 `tsproject.test.ts` 4건 — `.rl` import 너머 함수 정의
  이동(스팬이 선언 이름과 일치), 파일 내 지역 변수 정의, quick info
  시그니처, match 구문이 섞인 파일에서의 통과 영역 해석.
- 2026-08-17: 문서 — 확장 README 기능 표·위임 설명·한계, CHANGELOG.

## 이슈 및 해결

### 이슈 1: `.rl` 루트 파일이 TS 프로그램에서 조용히 탈락

- **증상**: `getDefinitionAtPosition`이
  `Could not find source file: '/tmp/.../main.rl'`을 던져 모든 위임 결과가
  비어 있었다 (테스트 4건 전부 실패).
- **원인**: TS 프로그램 구성은 루트 파일을 확장자로 필터링한다 —
  `.ts`/`.tsx`/`.js`가 아닌 `.rl`은 `getScriptFileNames`에 넣어도
  버려진다. 디버그 스크립트로 서비스 호출을 직접 재현해 확인.
- **해결**: 내장 TS 도구들(VSCode 자체, Volar, Svelte)이 쓰는 내부 옵션
  `allowNonTsExtensions: true`를 컴파일러 옵션에 추가(공개 타입에 없어
  캐스트로 주입, 주석으로 출처 명시). 적용 후 4건 전부 통과.

### 이슈 2: `ts.getScriptKindFromFileName`은 내부 API

- **증상**: tsc 컴파일 에러 `TS2339: Property 'getScriptKindFromFileName'
  does not exist on type 'typeof ts'`.
- **원인**: 확장자→ScriptKind 매핑 함수가 공개 표면에 없다.
- **해결**: 확장자 분기(`.tsx`/`.jsx`/`.m|cjs`/`.json`/기본 TS)를 직접
  구현했다.

## 검증

- [x] Rust 게이트 — 이 태스크는 Rust 변경 없음. `cargo fmt --check` /
  `clippy -D warnings` / `cargo test` 재실행으로 무변경 확인.
- [x] `editors/vscode`: `npm test` — tsc 컴파일 + node --test 29개 통과
  (기존 25 + tsproject 4)

## 결과

- 추가: `editors/vscode/server/src/tsproject.ts`,
  `editors/vscode/server/src/test/tsproject.test.ts`,
  `docs/tasks/TASK-024-ts-language-service-delegation.md`
- 수정: `editors/vscode/server/src/server.ts`,
  `editors/vscode/server/package.json`(+`typescript` 의존성,
  package-lock 갱신), `editors/vscode/README.md`, `CHANGELOG.md`,
  `docs/tasks/INDEX.md`
- 컴파일러(Rust)는 변경 없음.

후속 후보: TS 자동완성/참조 찾기/이름 변경 위임 (같은 `TsProject` 위에서
`getCompletionsAtPosition`/`getReferencesAtPosition`/`findRenameLocations`
연결만 남음).
