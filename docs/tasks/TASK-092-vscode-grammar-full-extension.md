# TASK-092: VS Code 하이라이팅 재구축 — TS 문법 전체 확장(TSX 방식) 생성 파이프라인

- **상태**: 완료
- **시작일**: 2026-08-20
- **완료일**: 2026-08-20
- **커밋**: (후속 커밋에서 기록)

## 목적

`.rl` 하이라이팅이 중첩 컨텍스트(함수 본문·초기화식·클래스 등)에서 전면적으로
깨진다. 원인은 LSP가 아니라 TextMate 문법의 구조다: 기존 문법은 최상위
`patterns`에 rl 규칙을 나열하고 `source.ts`를 통째로 include하는데, TextMate의
include는 완전 위임이라 `source.ts`가 영역(함수 본문 `meta.block.ts` 등)을 열면
그 내부에서 rl 규칙은 시도조차 되지 않는다. 그 결과 함수 안의
`result`/`<-`/`val`/`match`/`enum`/`|>`가 전부 순수 TS로 오해석되고, 특히
`result { ... }`는 객체 리터럴로 파싱되어 뒤 코드까지 연쇄 오염된다
(vscode-textmate 엔진으로 토크나이즈해 검증 — LSP semantic tokens는 서버
capabilities에 아예 없으므로 색상과 무관하다는 것도 함께 확인).

TSX가 하는 방식(TS 문법의 완전 확장 — `source.tsx`는 `source.ts`를 include하지
않는 독립 문법)을 채택하되, 손으로 유지하는 포크 대신 **업스트림 TS 문법 +
rl 규칙 + 접합 명세로부터 문법을 생성하는 빌드 스크립트**로 유지비를 흡수한다.
이후 새 구문이 추가될 때는 rl 규칙 정의와 접합 지점 선언만 늘리면 된다.

## 범위

- 포함: 업스트림 TypeScript TextMate 문법 vendoring, 생성 스크립트
  (`editors/vscode/syntaxes/build.mjs`), rl 규칙 소스, 생성된
  `rl.tmLanguage.json`, vscode-textmate 기반 토크나이즈 테스트(중첩 컨텍스트 +
  순수 TS 동등성 + look-alike 비오탐 + 생성물 최신성).
- 제외: LSP semantic tokens(후속 태스크 — `match(...)`가 rl match인지 순수 TS
  호출인지 같은 파서 수준 판별의 정밀 보강), 언어 표면·컴파일러 변경 없음.
  `docs/reference/`·`docs/ai/rl.md`는 언어 표면이 변하지 않으므로 갱신 대상
  아님.

## 의사결정

### 결정 1: 패턴 선택 — injection이 아닌 완전 문법 확장(TSX 방식)

- **상황**: 중첩 컨텍스트에서 rl 규칙이 죽는 구조 결함의 수정 방식 선택.
- **검토한 대안**:
  - A. 완전 문법 확장 (TSX 방식): TS 문법 전체를 기반으로 rl 규칙을 각
    repository 지점에 짜 넣은 독립 문법. 가장 정확 — 문장/식/매개변수 등
    올바른 문맥 위치에 규칙이 놓여 오탐이 최소화되고, `result` 블록 본문을
    문장 컨텍스트로 소유해 연쇄 오염을 원천 차단. 단점: TS 문법(146개
    repository 규칙) 추적 유지비.
  - B. injection(`injections` 섹션 + `L:` 셀렉터): Svelte가
    `L:(source.ts, ...)`로 하는 방식. 저비용이지만 규칙이 모든 위치에
    주입되어 문맥 구분(암 본문 vs 패턴 위치 등)이 어렵고, TS 규칙이 먼저
    소비하는 형태(`const r = result {`)와의 우선순위 제어가 거칠다.
  - C. semantic tokens 단독: VS Code 공식 가이드상 문법 하이라이팅의
    "addition"이며 테마/설정에 따라 비활성 — 서버 부재 시 깨진 기본색이
    그대로 노출.
- **선택과 근거**: A. 사용자 결정 — 이후 추가될 문법도 안정적으로 수용하는
  것이 우선. 유지비는 Microsoft의 방식(YAML 마스터에서 TS/TSX 문법 생성)을
  차용한 생성 스크립트로 흡수: 업스트림 문법은 무수정 vendoring, rl 규칙과
  접합 지점은 별도 소스로 분리해 "업스트림 갱신 = 파일 교체 + 재생성"이
  되게 한다. 접합 지점이 업스트림에서 사라지거나 모양이 바뀌면 build.mjs가
  생성 시점에 실패한다(조용한 열화 방지).

### 결정 2: 스코프 네이밍 — 루트는 `source.rl`, TS 유래 스코프는 `.ts` 유지

- **상황**: 생성 문법의 스코프 접미사를 `.rl`로 전면 개명할지.
- **검토한 대안**: 전면 `.rl` 개명(정체성 명확하나 `.ts` 스코프를 겨냥하는
  테마와의 호환 상실 + 업스트림 diff 전량 발생) vs TS 유래는 `.ts` 유지.
- **선택과 근거**: `.ts` 유지. 테마 호환성이 우선이고(테마는 접두사
  `variable.other.enummember` 등으로 매칭하므로 접미사는 rl 고유 개념
  표시용), 업스트림 무수정 vendoring 원칙과 일치. rl 고유 개념만 `.rl`
  (`keyword.control.match.rl`, `storage.modifier.val.rl`,
  `keyword.operator.result-bind.rl`, `keyword.operator.pipeline.rl` 등).

### 결정 3: 업스트림 소스 — tm-grammars 1.32.5의 typescript 문법을 vendoring

- **상황**: 기반 TS 문법을 어디서 가져와 어떻게 고정할지.
- **검토한 대안**: microsoft/TypeScript-TmLanguage 직접(원전이지만 YAML 빌드
  필요) / VS Code 내장 사본(추출 경로 불편) / tm-grammars 패키지(위 원전을
  JSON으로 배포, shiki 생태계 표준, MIT).
- **선택과 근거**: tm-grammars 1.32.5의 `grammars/typescript.json`을
  `syntaxes/src/typescript.tmLanguage.json`으로 무수정 커밋. 외부 include가
  0개인 자기완결 문법임을 확인했다(`source.ts#` 참조 0건). 런타임/빌드
  의존성은 없다 — vendoring이므로 npm 네트워크 없이 재생성 가능.

### 결정 4: 접합 명세 — repository 항목 이름(+패턴 경로)에 include 앞삽입

- **상황**: rl 규칙을 TS 문법 어디에 어떻게 끼울지의 표현.
- **검토한 대안**: TS 문법 JSON을 직접 수정(diff 추적 불가) / 정규식 치환
  (취약) / 접합 명세 선언(`splices: {대상: [include...]}`) + 앞삽입.
- **선택과 근거**: 접합 명세. TextMate는 같은 오프셋에서 매치가 겹치면 먼저
  나열된 규칙이 이기므로 **patterns 맨 앞 삽입 = rl 우선**이 정확한 의미가
  된다. 대상 표기는 `이름` 또는 `이름:i:j...`(중첩 patterns 인덱스 —
  `for-loop:2`처럼 하위 괄호 영역에 끼울 때). 접합 지점: `statements`(enum·
  let-else·if let·val 선언 수식자·catch val), `expression`(match·result·
  flow·try 식), `expression-operators`(`|>`), `function-parameters-body`·
  `expression-inside-possibly-arrow-parens`·`paren-expression`(val 매개변수),
  `for-loop:2`(for 헤드의 val).

### 결정 5: 테스트 — vscode-textmate + vscode-oniguruma로 실제 토크나이즈

- **상황**: 문법 회귀를 어떻게 게이트할지.
- **검토한 대안**: 수동 확인(회귀 무방비) / shiki(간편하나 자체 번들과
  뒤섞임) / vscode-textmate+vscode-oniguruma 직접(VS Code와 동일 엔진).
- **선택과 근거**: vscode-textmate 직접. 기존 서버 테스트 러너(`node --test`
  `server/out/test/*.test.js`)에 편승해 CI 변경 없이 돈다. 테스트 축 4개:
  ① 신고된 회귀(함수 안 result 블록·val — 연쇄 오염 부재까지 단언)
  ② 전 구문의 중첩 컨텍스트 동작 ③ look-alike 비오탐(`const enum`,
  `a < -b`, 객체 키 `result:`) ④ 순수 TS 스코프가 TS 문법과 완전 동일
  (통과 계약의 하이라이팅판) + 생성물 최신성(`build.mjs --check`).

## 작업 내역

- 2026-08-20: 원인 분석 — `server.ts` capabilities에 semanticTokensProvider
  부재 확인(색상은 전적으로 TextMate). vscode-textmate 엔진으로 신고된
  코드를 토크나이즈해 재현: 함수 본문 안에서 rl 규칙 발동 0건,
  `result {`가 객체 리터럴로 오해석되어 이후 함수 시그니처까지 연쇄 오염.
  최상위에서도 `const r = result {...}`·`const x = match(...)`는 TS `const`
  규칙이 먼저 영역을 열어 실패함을 확인.
- 2026-08-20: 기술조사 — TSX는 `source.ts` include 없는 독립 문법
  (repository 160개), Svelte는 `L:(source.ts,...)` injection, rust-analyzer는
  보수적 TextMate + semantic tokens. VS Code 공식 가이드로 semantic tokens가
  "addition"임을 확인. 사용자 결정으로 패턴 A(완전 확장) 채택.
- 2026-08-20: 구현 —
  - `syntaxes/src/typescript.tmLanguage.json`: tm-grammars 1.32.5 vendoring.
  - `syntaxes/src/rl.rules.json`: rl 규칙 19개(repository)와 접합 명세 8개.
    enum(페이로드 필드·제네릭·TS enum 초기화 `=` 지원), match(스크루티니/
    암 패턴/가드/or-패턴/리터럴/중첩 `필드: 태그(...)` 패턴/와일드카드/
    암 본문 `=>` 영역), let-else·if let(태그 패턴 + `=` 이후 식 영역 +
    else), result(블록이 `#statements`를 소유 — 객체 리터럴 오해석 차단,
    바인딩은 단순명/타입 주석/구조 분해 셋 다), `|>`·`flow`, try 식,
    val(선언 수식자/매개변수/for 헤드/catch — catch는 `val`이 있을 때만
    발동하는 전용 규칙으로 TS의 arrow 휴리스틱 붕괴를 대체).
  - `syntaxes/build.mjs`: 검증(이름 접두사·충돌·접합 지점 존재·include
    해석 가능) 후 합성, `--check` 모드로 생성물 드리프트 검사.
  - `server/src/test/grammar.test.ts`: 테스트 5개(위 결정 5). 서버
    devDependencies에 vscode-textmate·vscode-oniguruma 추가.
  - `package.json`에 `grammar`/`grammar:check` 스크립트, `.vscodeignore`에
    소스 제외(생성물만 패키징), README 기능표 갱신.
- 2026-08-20: 검증 — 아래 게이트 전부 통과. 확장 스위트에서 신규 문법
  테스트 5개 포함 40 pass / 4 fail — 실패 4개는 completion 테스트로 tsgo
  부재 환경의 기존 실패(HEAD에서 동일 재현, 본 변경과 무관; CI는 tsgo를
  설치하므로 CI에서는 통과 경로).

## 이슈 및 해결

### 이슈 1: match 패턴의 `true`/`false`가 enum 태그로 색칠

- **증상**: `match (b) { true => 1 }`에서 `true`가
  `variable.other.enummember`로 나옴.
- **원인**: `rl-match-arm-pattern`에서 태그 패턴 규칙이 불리언 리터럴
  규칙보다 먼저 나열되어 동오프셋 경쟁에서 이김.
- **해결**: 불리언 규칙을 태그 패턴 앞으로 이동.

### 이슈 2: `for (val const item of items)`의 `val` 미적용

- **증상**: for 헤드의 `val`이 평범한 식별자로 남음.
- **원인**: 접합을 `for-loop` 최상위 patterns에 했는데, 괄호 내부는
  for-loop의 **중첩 인라인 영역**(patterns[2])이 별도 규칙 목록으로 파싱.
- **해결**: 접합 명세에 경로 표기(`for-loop:2`)를 도입해 중첩 patterns에
  끼울 수 있게 build.mjs 확장(존재하지 않는 경로는 생성 실패).

### 이슈 3: `catch (val error: unknown)`에서 타입 주석이 무색

- **증상**: `error`·`unknown`이 매개변수/타입 스코프를 못 받음.
- **원인**: 순수 TS의 catch 절은 TS 문법의 arrow 휴리스틱이 매개변수로
  파싱하는데, `val`이 끼면 그 휴리스틱의 lookahead가 실패해
  paren-expression으로 떨어짐.
- **해결**: `val`이 있을 때만 발동하는 `rl-catch-val-parameters` 영역 추가
  (`#parameter-name`·`#parameter-type-annotation` 재사용). `val` 없는 catch는
  TS 경로 그대로(패리티 유지).

### 이슈 4: look-alike 테스트가 루트 스코프에 오탐

- **증상**: `const enum`의 모든 토큰이 "rl 스코프 누출"로 실패.
- **원인**: 단언이 `.rl`로 끝나는 스코프 전체를 검사했는데 루트 스코프
  `source.rl` 자체가 매치.
- **해결**: 루트 스코프(첫 항목)를 제외하고 검사.

### 이슈 5: vscode-oniguruma d.ts의 WebAssembly 네임스페이스로 tsc 실패

- **증상**: `error TS2503: Cannot find namespace 'WebAssembly'`.
- **원인**: 프로젝트 lib이 es2020뿐이라 dom/webworker의 WebAssembly 타입이
  없음.
- **해결**: `server/tsconfig.json`에 `skipLibCheck: true`(선언 파일 검사만
  생략, 소스 검사는 그대로).

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` (전 스위트 통과)
- [x] `editors/vscode`: `npx tsc -b` 클린 +
      `node --test server/out/test/*.test.js` — 문법 테스트 5/5 통과,
      나머지 실패 4개는 tsgo 부재 환경의 기존 실패(HEAD 동일)
- [x] `node syntaxes/build.mjs --check` 통과 (생성물 = 소스)

## 결과

- `editors/vscode/syntaxes/src/typescript.tmLanguage.json` (신규, vendored)
- `editors/vscode/syntaxes/src/rl.rules.json` (신규 — rl 규칙 + 접합 명세)
- `editors/vscode/syntaxes/build.mjs` (신규 — 생성/검증 스크립트)
- `editors/vscode/syntaxes/rl.tmLanguage.json` (재생성 — TS 완전 확장 문법)
- `editors/vscode/server/src/test/grammar.test.ts` (신규 — 토크나이즈 테스트)
- `editors/vscode/server/package.json`·`package-lock.json` (textmate devDeps)
- `editors/vscode/server/tsconfig.json` (skipLibCheck)
- `editors/vscode/package.json` (grammar 스크립트), `.vscodeignore`, `README.md`

신고된 화면의 모든 증상(함수 안 result 블록 전멸, `<-`·`val` 무색, 블록
이후 연쇄 오염)이 해소되고, 순수 TS 하이라이팅은 TS 문법과 바이트 단위로
동일(테스트로 고정). 남은 한계는 TextMate의 원리적 한계로, 정규식 판별이
컴파일러의 완전 파스와 다를 수 있는 지점(예: `result {` 형태의 순수 TS
식별자+블록, 식별자 `match(...)` 호출은 rl로 색칠될 수 있음, 여러 줄로
나뉜 `flow` 헤드는 미색칠)이다 — 이 잔여 오차의 해소는 엔진 기반 LSP
semantic tokens 후속 태스크의 몫.
