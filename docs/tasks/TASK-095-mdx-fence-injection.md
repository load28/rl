# TASK-095: 펜스 injection을 MDX로 확장 — Svelte 확장과 동등하게

- **상태**: 완료
- **시작일**: 2026-08-20
- **완료일**: 2026-08-20
- **커밋**: 이 커밋

## 목적

TASK-094의 마크다운 펜스 injection을 실제 서드파티 언어 확장의 기준
구현(Svelte `svelte-vscode`)과 나란히 검증한 결과, 남은 차이는 하나였다 —
Svelte는 마크다운(`text.html.markdown`)뿐 아니라 MDX(`source.mdx`)에도
주입해서 `.mdx` 문서의 펜스도 하이라이팅한다. rl도 같은 수준으로 맞춘다.

## 범위

- 포함: injection 문법의 `injectionSelector`와 `package.json` `injectTo`에
  `source.mdx` 추가, 두 곳이 같은 호스트 집합을 가리키는지 검사하는 테스트.
- 제외: MDX 확장 vendoring이나 MDX 전용 규칙. GitHub 등 다른 렌더러의
  하이라이팅(linguist 등록 — 별도 사안, 아래 참고).

## 의사결정

### 결정 1: 주입 대상은 Svelte 확장과 동일한 두 호스트로 한다

- **상황**: TASK-094는 MDX를 범위에서 제외했었다. 사용자 요청("Svelte처럼")
  으로 기준을 명시적으로 Svelte 확장에 맞추게 됐다.
- **검토한 대안**: 마크다운만 유지 / 마크다운 + MDX(`source.mdx`) /
  더 넓게(예: reStructuredText 등 다른 호스트).
- **선택과 근거**: 마크다운 + MDX. Svelte 확장(`markdown.svelte.codeblock`)의
  `injectionSelector: "L:text.html.markdown, L:source.mdx"`가 검증된 실전
  기준이고, `source.mdx`는 공식 MDX 확장(unifiedjs `vscode-mdx`)의 루트
  스코프라 대상이 명확하다. 그 이상의 호스트는 실수요가 없다.
  (Svelte의 나머지 injection 두 개 — `markdown.svelte.codeblock.script`/
  `.style` — 는 Svelte 컴포넌트의 `<script>`/`<style>` 블록 전용이라 rl에는
  해당 개념이 없다.)

### 결정 2: selector·injectTo 동기화를 테스트로 계약화한다

- **상황**: 주입 대상이 `rl.markdown.tmLanguage.json`(injectionSelector)과
  `package.json`(injectTo) 두 곳에 나뉘어 있다. 한쪽만 고치면 에디터에서
  주입이 조용히 빠진다(에러 없이 하이라이팅만 안 됨).
- **검토한 대안**: 문서 주석으로만 남김 / 테스트로 두 파일의 호스트 집합
  일치를 검사.
- **선택과 근거**: 테스트. grammar.test.ts가 이미 두 파일을 읽을 수 있는
  자리이고, 조용한 회귀는 주석으로 못 막는다. `embeddedLanguages` 매핑도
  같은 테스트에서 고정한다.

## 작업 내역

- 2026-08-20: 기준 확인 — VS Code 내장 `markdown.tmLanguage.json`의
  `fenced_code_block_tsx`(내장 언어는 마크다운 문법에 하드코딩)와 Svelte
  확장의 `markdown-svelte.json`(서드파티는 injection, 마크다운 + MDX 주입)을
  원본에서 가져와 TASK-094 구현과 비교. 남은 차이는 MDX 주입뿐.
- 2026-08-20: `syntaxes/rl.markdown.tmLanguage.json` —
  `injectionSelector`를 `"L:text.html.markdown, L:source.mdx"`로 확장,
  머리말 주석 갱신.
- 2026-08-20: `package.json` — 해당 grammar의 `injectTo`에 `source.mdx` 추가.
- 2026-08-20: `server/src/test/grammar.test.ts` — selector/injectTo/
  embeddedLanguages 동기화 계약 테스트 추가.
- 2026-08-20: `README.md` 기능 표의 마크다운 펜스 행에 MDX 명시.
- 2026-08-20: 검증 — grammar 테스트, 저장소 게이트(fmt/clippy/test).

## 이슈 및 해결

없음.

### 참고: GitHub README는 이 방식으로 해결되지 않는다

GitHub은 linguist에 등록된 언어만 펜스를 칠하고 저장소별 확장점이 없다.
linguist 레지스트리 확인 결과 펜스 라벨 `rl`에 매핑된 언어는 없고(평문
처리), `.rl` 확장자는 Ragel이 선점(`tm_scope: none`). 등록에는 수백 개
저장소 실사용 요건이 있어 현 시점에는 불가 — README에서는 ` ```ts ` 근사가
현실적 대안이다. 에디터 밖 문서 사이트는 Shiki가 `rl.tmLanguage.json`을
커스텀 언어로 그대로 소비할 수 있다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` (전 스위트 통과)
- [x] `editors/vscode`: grammar 테스트 7개(신규 동기화 계약 테스트 포함) 통과

## 결과

- `editors/vscode/syntaxes/rl.markdown.tmLanguage.json`: `injectionSelector`
  가 `L:text.html.markdown, L:source.mdx` — Svelte 확장과 동일한 호스트.
- `editors/vscode/package.json`: `injectTo`에 `source.mdx` 추가.
- `editors/vscode/server/src/test/grammar.test.ts`: selector·injectTo·
  embeddedLanguages 동기화 계약 테스트.
- `editors/vscode/README.md`: 펜스 행에 MDX 명시.
- `docs/tasks/INDEX.md`(TASK-094 상태 완료 반영 포함), 이 문서.

언어 표면·CLI·stdlib 변경 없음 — `docs/reference/`·`docs/ai/rl.md` 해당 없음.
