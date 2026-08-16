# TASK-018: VSCode 언어 서비스 (LSP 확장)

- **상태**: 완료
- **시작일**: 2026-08-16
- **완료일**: 2026-08-16
- **커밋**: fa0c492

## 목적

rl을 VSCode에서 실제 언어처럼 쓸 수 있게 한다: 문법 하이라이팅, 컴파일 에러
진단, 자동완성, 호버, 정의로 이동, 문서 심볼, 빠른 수정. Microsoft가
공식적으로 권장하는 LSP(Language Server Protocol) 확장 패턴을 따른다.

## 범위

- 포함:
  - `editors/vscode/` — VSCode 확장 (공식 lsp-sample 구조: 루트 매니페스트 +
    `client/`(vscode-languageclient) + `server/`(vscode-languageserver)).
  - TextMate 문법(`source.rl`) — rl 구문 하이라이팅 + `source.ts` 폴백.
  - 진단: 저장/편집 시 `rlc --check`를 실행해 에러를 에디터에 표시.
  - 자동완성: match 암 위치의 케이스 태그, `Enum.` 멤버 생성자, 케이스 필드
    바인딩, enum/match/try/let-else 스니펫.
  - 호버·정의로 이동·문서 심볼: 파일 내 rl enum / 케이스 태그 +
    내장 `Option`/`Result`.
  - 빠른 수정(Code Action): 소진되지 않은 match에 빠진 암 / `_` 암 추가.
  - 서버 분석 로직 단위 테스트 (`node --test`).
- 제외:
  - 마켓플레이스 배포(vsce publish), 아이콘/브랜딩.
  - 프로젝트(다중 파일) 단위 심볼 해석 — 소진성 검사와 동일하게 파일 단위.
  - 시맨틱 토큰, 리네임, 포매터, 시그니처 헬프 (후속 태스크 후보).
  - Rust 컴파일러 코드 변경 (진단은 기존 `rlc --check` CLI 계약만 사용).

## 의사결정

### 결정 1: 진단은 자체 재구현이 아니라 `rlc --check` 실행으로

- **상황**: 에디터에 표시할 컴파일 에러를 어디서 얻을지 결정 필요.
- **검토한 대안**:
  - A. 서버(TypeScript)에 rl 검사 로직 재구현 — 프로세스 실행이 없어 빠르지만
    sema의 모든 규칙(중복/소진성/or-패턴 바인딩/발산 검사...)을 이중 유지해야
    하고, 컴파일러와 어긋나는 순간 잘못된 진단이 된다.
  - B. rlc를 wasm으로 빌드해 서버에 내장 — 단일 진실 소스는 유지되지만
    빌드 파이프라인(wasm-bindgen, 배포 크기)이 커지고 저장소 구조가 복잡해짐.
  - C. 설치된 `rlc` 바이너리를 `--check`로 실행하고 stderr의
    `파일:행:열: 메시지`(errors.md의 공개 계약)를 파싱.
- **선택과 근거**: C. 진단이 항상 실제 컴파일러와 바이트 단위로 일치하고
  (단일 진실 소스), Rust 쪽 변경이 전혀 필요 없다. `rlc`가 없으면 진단만
  조용히 꺼지고 나머지 기능(하이라이팅/완성/호버)은 그대로 동작한다.
  탐색 순서는 `rl.compilerPath` 설정 → 워크스페이스의
  `target/{release,debug}/rlc` → PATH. wasm 내장(B)은 후속 태스크 후보.

### 결정 2: 확장 구조는 공식 lsp-sample 패턴 (client/server 분리)

- **상황**: 요청이 "공식 패턴 적용". VSCode 언어 기능 구현 방식 선택 필요.
- **검토한 대안**:
  - A. `vscode.languages.*` 직접 등록(단일 확장, LSP 없음) — 간단하지만
    에디터 종속적이고 공식 가이드가 언어 서비스에 권장하는 방식이 아님.
  - B. Microsoft lsp-sample 구조 — 루트 매니페스트 + `client/`
    (vscode-languageclient) + `server/`(vscode-languageserver, Node IPC).
    LSP라 추후 다른 에디터(Neovim 등)에서도 서버 재사용 가능.
- **선택과 근거**: B. VSCode 공식 문서(Language Server Extension Guide)가
  제시하는 표준 구조 그대로: `tsc -b` 프로젝트 레퍼런스, postinstall로
  client/server 의존성 설치, `onLanguage:rl` 활성화.

### 결정 3: 완성/호버용 구문 분석은 서버에 경량 파서로 (컴파일러 규칙의 부분집합)

- **상황**: 케이스 태그 완성·호버·정의 이동에는 파일 안의 rl enum 목록과
  match 컨텍스트가 필요하다. 진단(결정 1)은 위치 정보를 주지만 구조는 주지
  않는다.
- **검토한 대안**:
  - A. rlc에 `--dump-ast` JSON 모드 추가 — 정확하지만 키 입력마다 프로세스
    실행이 필요하고 CLI 표면이 늘어난다.
  - B. 서버에 경량 스캐너/파서(TypeScript) — scanner.rs와 같은 원칙(문자열·
    주석·템플릿·정규식을 마스킹한 뒤 구조 파싱)으로 enum 선언과 match
    블록만 읽는다. 컴파일러의 판별 규칙(페이로드 괄호 또는 제네릭이 있어야
    rl enum, 예약어 배제, 내장 Option/Result와 섀도잉)을 그대로 미러링.
- **선택과 근거**: B. 완성·호버는 근사여도 되는 보조 기능이고(정답은 항상
  진단이 준다), 프로세스 실행 없이 키 입력 지연 없이 동작해야 한다.
  규칙 미러링이 어긋나면 잘못된 완성 후보가 뜰 뿐 컴파일 결과에는 영향이
  없다. 미러링한 규칙은 단위 테스트로 고정했다.

### 결정 4: 하이라이팅은 TextMate 문법 + `source.ts` 폴백

- **상황**: .rl은 TS 상위집합이므로 TS 하이라이팅을 최대한 재사용해야 한다.
- **검토한 대안**: A. TS 문법 복제 후 수정(유지보수 불가) /
  B. 시맨틱 토큰만(LSP) — 테마 호환성·기본 하이라이팅 부재 /
  C. `source.rl` 문법에서 rl 전용 패턴(match 키워드, 암 태그, enum 케이스)을
  먼저 매칭하고 나머지는 `"include": "source.ts"`로 VSCode 내장 TS 문법에
  위임.
- **선택과 근거**: C. 통과 영역(순수 TS)은 내장 문법이 그대로 칠하고,
  rl 전용 구문만 위에 얹는다 — 언어 설계(통과 원칙)와 정확히 같은 구조.

## 작업 내역

- 2026-08-16: 저장소 구조·언어 레퍼런스·CLI/에러 계약 조사. 태스크 등록.
- 2026-08-16: `editors/vscode/` 확장 골격 작성 (매니페스트, tsconfig 프로젝트
  레퍼런스, language-configuration, TextMate 문법).
- 2026-08-16: 서버 구현 — `analysis.ts`(마스킹 스캐너 + enum/match 구조 파싱),
  `rlc.ts`(컴파일러 탐색 + `--check` 실행 + stderr 파싱), `server.ts`
  (진단/완성/호버/정의/심볼/코드액션 와이어링). 클라이언트 `extension.ts`.
- 2026-08-16: 서버 분석 로직 단위 테스트 23개 작성, `npm test` 통과 확인
  (`npm run compile` 클린 빌드 포함).
- 2026-08-16: 엔드투엔드 스모크 테스트 — ① `cargo build`로 rlc 빌드 후
  컴파일된 `server/out/rlc.js`의 `findCompiler`/`runCheck`를 직접 호출:
  소진성 에러가 `2:11` 위치로, 중복 케이스가 `1:29`로 파싱되고, 정상 파일은
  진단 0개, 없는 컴파일러 경로는 `not-found`로 분류됨을 확인. ② 서버를
  `--stdio`로 띄워 LSP `initialize` 핸드셰이크에 completion/hover/definition/
  documentSymbol/codeAction capability가 응답되는 것을 확인. ③ 문법·언어
  설정 JSON 파싱 검증. 검증 게이트(cargo fmt/clippy/test) 통과 확인.
  README에 편집기 지원 섹션 추가.

## 이슈 및 해결

### 이슈 1: `armTags`가 가드 암의 태그를 놓치고 `_`를 태그로 수집

- **증상**: 단위 테스트
  `armTags collects or-pattern alternatives and skips _` 실패 —
  `match (v) { ..., D if f > 0 => 3, _ => 0 }`에서 기대값 `[A,B,C,D]` 대신
  `[A,B,C,_]`가 나옴.
- **원인**: 초기 구현이 or-대안 문자열 전체를 trim해 식별자 검사에 넘겼다.
  가드 암 `D if f > 0`은 전체 문자열이 식별자가 아니어서 탈락했고,
  `_`는 `[A-Za-z_$]...` 정규식과 예약어 검사를 both 통과해 태그로 수집됐다.
- **해결**: 대안에서 **선행 식별자만** 추출(`/^\s*([A-Za-z_$][\w$]*)/`)하고
  `_`를 명시적으로 제외하도록 수정. 회귀는 해당 단위 테스트가 고정.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`
- [x] `cd editors/vscode && npm install && npm run compile && npm test`

## 결과

`editors/vscode/`에 VSCode 확장 신설 (Rust 소스 무변경):

- `package.json`/`tsconfig.json`/`language-configuration.json` — 확장 매니페스트
  (언어 등록, 설정 `rl.compilerPath`·`rl.verify`·`rl.trace.server`).
- `syntaxes/rl.tmLanguage.json` — rl 전용 스코프 + `source.ts` 폴백.
- `client/src/extension.ts` — LSP 클라이언트 (Node IPC).
- `server/src/analysis.ts` — 마스킹 스캐너, enum/match 구조 파싱, 내장
  `Option`/`Result`(섀도잉 포함), 컨텍스트 판정(암 위치/멤버 접근/필드 바인딩).
- `server/src/rlc.ts` — rlc 탐색·실행·진단 파싱.
- `server/src/server.ts` — 진단(디바운스), 완성, 호버, 정의, 심볼, 코드액션.
- `server/src/test/analysis.test.ts` — 분석 로직 단위 테스트 23개.
- `editors/vscode/README.md` — 설치/개발/기능 문서. 루트 `README.md`에 편집기
  지원 섹션 추가.

후속 후보: 시그니처 헬프, 시맨틱 토큰, wasm 내장 진단, 프로젝트 단위 심볼.
