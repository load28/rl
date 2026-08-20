# TASK-094: 마크다운 코드 펜스 rl 하이라이팅 — injection 문법

- **상태**: 완료
- **시작일**: 2026-08-20
- **완료일**: 2026-08-20
- **커밋**: 이 커밋

## 목적

VS Code에서 마크다운 문서의 ` ```rl ` 코드 펜스가 하이라이팅되지 않는다.
확장이 `.rl` 파일용 `source.rl` 문법만 contribute하고, 마크다운 문법
(`text.html.markdown`)에 주입되는 injection 문법이 없기 때문이다 — VS Code는
각 언어 확장이 injection 문법을 제공해야만 펜스 안을 그 언어로 칠한다.

## 범위

- 포함: 마크다운 펜스용 injection 문법(`syntaxes/rl.markdown.tmLanguage.json`)
  추가, `package.json` grammars 등록(`injectTo` + `embeddedLanguages`),
  grammar 테스트, README 갱신.
- 제외: `source.rl` 문법 자체의 변경. 다른 호스트 언어(예: MDX)에의 주입.

## 의사결정

### 결정 1: injection 문법은 손으로 쓴 정적 파일로 둔다

- **상황**: `syntaxes/rl.tmLanguage.json`은 `build.mjs`가 vendored TS 문법에서
  생성하는 생성물이라, 새 문법 파일도 생성 파이프라인에 넣을지 정해야 했다.
- **검토한 대안**:
  - A. `build.mjs`가 함께 생성 — 소스/생성물 규약이 일관되지만, 이 파일은
    TS 문법과 아무 관계가 없어 생성할 입력이 없다. 생성기는 순수 복사가 된다.
  - B. 정적 파일로 직접 커밋 — 30줄짜리 독립 문법이고 upstream 갱신의 영향을
    받지 않는다.
- **선택과 근거**: B. 이 문법은 VS Code 내장 마크다운 문법의
  `fenced_code_block_*` 패턴을 rl로 특수화한 것일 뿐 TS 문법과 무관하므로,
  생성 파이프라인에 넣으면 유지비만 는다. "생성물 손대지 말 것" 규약은
  `rl.tmLanguage.json`에만 해당한다.

### 결정 2: 펜스 매칭 정규식은 VS Code 내장 마크다운 문법의 형태를 그대로 따른다

- **상황**: 펜스 시작/끝을 어떤 정규식으로 잡을지. 자체 발명하면 내장 문법과
  미묘하게 다른 경계(들여쓰기, `~~~`, 언어 뒤 속성)가 생긴다.
- **검토한 대안**: 단순 ` ``` ` 만 잡는 자체 규칙 / 내장
  `fenced_code_block_js` 패턴의 언어 부분만 `rl`로 바꾼 사본.
- **선택과 근거**: 내장 패턴 사본. 내장 문법과 같은 경계 규칙(3+ 백틱/틸드,
  들여쓰기 백레퍼런스 `\2`·`\3`, `rl` 뒤 속성 허용, 대소문자 무시)을 가져야
  다른 언어 펜스와 동작이 일치한다. 스코프 이름도 내장과 같은
  `markup.fenced_code.block.markdown` / `fenced_code.block.language.markdown`
  을 써서 테마 호환을 지킨다.

### 결정 3: 테스트는 injection 문법을 독립 문법으로 직접 토크나이즈한다

- **상황**: vscode-textmate로 주입을 온전히 재현하려면 호스트인 내장 마크다운
  문법을 vendoring해야 한다.
- **검토한 대안**: 마크다운 문법 vendoring 후 `getInjections`로 주입 재현 /
  injection 문법 자체를 루트 문법으로 로드해 펜스 픽스처를 토크나이즈.
- **선택과 근거**: 후자. 이 문법의 계약(펜스 인식, `source.rl` 임베드, 펜스
  종료, 다른 언어 펜스 불간섭)은 루트 로드만으로 전부 검증된다. 내장 마크다운
  문법 vendoring은 upstream 추적 부채를 새로 만든다.

## 작업 내역

- 2026-08-20: 원인 확인 — `editors/vscode/package.json`의 `contributes.grammars`
  에 `source.rl` 하나뿐, injection 없음.
- 2026-08-20: `syntaxes/rl.markdown.tmLanguage.json` 작성 —
  `injectionSelector: "L:text.html.markdown"`, ` ```rl `/`~~~rl` 펜스를 잡아
  내용을 `meta.embedded.block.rl`로 감싸고 `source.rl`을 include.
- 2026-08-20: `package.json` grammars에 등록 — `injectTo:
  ["text.html.markdown"]`, `embeddedLanguages: { "meta.embedded.block.rl":
  "rl" }` (토글 주석 `editor.action.commentLine` 등이 임베드 언어를 따르도록).
- 2026-08-20: `server/src/test/grammar.test.ts`에 마크다운 펜스 테스트 추가,
  registry가 `markdown.rl.codeblock`을 로드하도록 확장.
- 2026-08-20: `editors/vscode/README.md` 기능 표에 마크다운 펜스 행 추가.
- 2026-08-20: 검증 — `npm run compile && node --test
  server/out/test/grammar.test.js`, 저장소 게이트(fmt/clippy/test).

## 이슈 및 해결

없음.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` (전 스위트 통과, tsc/node 통합 테스트 포함)
- [x] `editors/vscode`: `npm test` — 84 테스트 중 39 통과, 45 skip
  (rlc 미설치 환경의 toolchain 의존 테스트 — 설계된 skip), 실패 0.
  grammar 테스트 6개(신규 마크다운 펜스 테스트 포함) 전부 통과.

## 결과

- `editors/vscode/syntaxes/rl.markdown.tmLanguage.json` (신규): 마크다운
  펜스 injection 문법 — ` ```rl `/`~~~rl` 펜스 내용을
  `meta.embedded.block.rl`로 감싸 `source.rl`을 임베드.
- `editors/vscode/package.json`: grammars에 `markdown.rl.codeblock` 등록 —
  `injectTo: ["text.html.markdown"]` + `embeddedLanguages`.
- `editors/vscode/server/src/test/grammar.test.ts`: registry가 injection
  문법을 로드하도록 확장, 펜스 임베드/종료/타 언어 불간섭 테스트 추가.
- `editors/vscode/README.md`: 기능 표에 마크다운 코드 펜스 행 추가.
- `docs/tasks/INDEX.md`, 이 문서.

언어 표면·CLI·stdlib 변경 없음 — `docs/reference/`와 `docs/ai/rl.md`는
해당 없음. 확장은 마켓플레이스 배포 시점에만 버전을 올리므로 버전 불변.
