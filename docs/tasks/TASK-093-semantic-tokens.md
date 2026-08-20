# TASK-093: 엔진 semantic tokens — 파서 소유 분류를 LSP 표준으로

- **상태**: 완료
- **시작일**: 2026-08-20
- **완료일**: 2026-08-20
- **커밋**: (후속 커밋에서 기록)

## 목적

TASK-092의 TextMate 완전 확장 문법은 정규식 근사라 원리적 한계가 남는다:
파서가 완전 파스로만 내리는 판별을 문법은 못 한다. 남은 오차는 두 방향 —

- **과잉**: 순수 TS의 `match(...)` 함수 호출이 rl 키워드로 색칠됨.
- **과소**: 첫 `|>`가 다음 줄에 있는 `flow` 헤드, 여러 줄로 나뉜
  `<-` 바인딩 등 같은-줄 lookahead가 못 보는 청구 구문이 무색.

이 잔여 오차를 우회가 아닌 정식 방법 — LSP `textDocument/semanticTokens`
(rust-analyzer가 쓰는 표준 오버레이) — 으로 해소한다. 분류의 단일 원천은
컴파일러의 구조 파스: 파서가 **청구한** 구문은 확정 토큰으로, **청구하지
않은** look-alike는 평범한 식별자로 보고해, 에디터의 색이 컴파일러의 판별과
구성적으로 일치하게 만든다.

## 범위

- 포함: 엔진의 파스 전용 semantic tokens 모듈(`src/engine/tokens.rs`),
  `rlc --server`의 `semanticTokens` 요청(무상태·text 기반), LSP 어댑터의
  표준 `semanticTokensProvider`(full), Rust 유닛 테스트 + 서버 E2E 테스트,
  설계 문서(`lsp-architecture.md`)·README 갱신.
- 제외: 언어 표면·컴파일러 파이프라인 변경 없음(`docs/reference/`·
  `docs/ai/rl.md` 무관). delta/range semantic tokens(full로 충분; 필요 시
  후속). TextMate 문법 변경 없음.

## 의사결정

### 결정 1: 보고 범위 — 모호한 표면만 (rust-analyzer의 경제성)

- **상황**: semantic tokens로 무엇을 보고할지 — 전 토큰 vs 일부.
- **검토한 대안**: 전 토큰 보고(테마 의존 폭발, 유지비, TextMate와 중복) vs
  문법이 판별 못 하는 것만(청구/비청구 판별 + 문법 lookahead의 사각).
- **선택과 근거**: 후자. rust-analyzer가 확립한 구성(보수적 문법 + tricky
  부분만 semantic)과 동일. `|>`·리터럴·`val`(문법과 파서가 유효 TS에서
  일치)·TS 소유 토큰은 보고하지 않는다 — semantic tokens가 꺼진
  테마/설정에서도 TASK-092의 문법이 그대로 기준선.

### 결정 2: 요청 형태 — `check`/`emitMap`처럼 무상태 text 기반

- **상황**: 처음에는 다른 semantic 요청처럼 `path` 기반(프로젝트 경유)으로
  붙였으나, `project_for` → `open_project` → `NativeBackend::new` →
  `Toolchain::resolve`가 **TypeScript 툴체인을 요구**함을 확인.
- **검토한 대안**: path 기반 유지(툴체인 없는 환경에서 실패 — 파스 전용
  기능이 타입 스택에 인질) vs text 기반 무상태 요청(어댑터가 버퍼 텍스트를
  보냄; 프로젝트·툴체인·canonicalize 불필요).
- **선택과 근거**: text 기반. 프로토콜의 기존 구분과 정확히 일치한다 —
  파스 전용 요청(check/emitMap)은 text를 받고, typed 요청은 path로
  프로젝트를 경유한다. 하이라이팅 정밀화는 문법과 같은 가용성(항상)을
  가져야 하므로 전자다. untitled 버퍼도 자연히 지원된다.

### 결정 3: 토큰 타입 — LSP 표준 이름만 사용

- **상황**: rl 고유 토큰 타입을 정의할지.
- **검토한 대안**: 커스텀 타입(테마가 모름 — 기본 매핑 없음) vs 표준 타입
  (`keyword`/`enum`/`enumMember`/`variable`/`property`/`function`/`operator`
  — 모든 semantic 지원 테마가 색을 가짐).
- **선택과 근거**: 표준 타입. 특히 부정(denial)의 핵심인 "함수 호출로
  되돌리기"는 `function`이 정확한 표준 분류다. 엔진의
  `SemanticTokenKind::as_str`가 표준 문자열을 그대로 내고 어댑터 legend가
  1:1 매핑.

### 결정 4: 비청구 look-alike 탐색 — 검색이 아니라 렉싱

- **상황**: verbatim(통과) 구간에서 `match(`/`result {`를 찾을 때 문자열·
  주석·정규식·템플릿 내부의 우발적 일치를 배제해야 한다.
- **검토한 대안**: 정규식/바이트 검색(문자열 내부 오탐) vs 기존 lexer 재사용
  (`lexer::lex`가 verbatim 구간을 유의 토큰으로 — 문자열/주석/정규식은
  토큰 스트림에 식별자로 나타나지 않음, 템플릿 보간은 pre-lexed 재귀).
- **선택과 근거**: lexer 재사용. 컴파일러가 이미 소유한 판별을 그대로 쓰며,
  멤버 접근(`value.match(...)`)은 직전 토큰(`.`/`?.`) 검사로 제외한다 —
  문법도 lookbehind로 안 칠하는 자리라 보고할 것이 없다.

## 작업 내역

- 2026-08-20: `src/engine/tokens.rs` 신설 — `scan`(구조 파스 → 바이트 오프셋
  분류)과 `semantic_tokens`(UTF-16 좌표 변환, 공개 API). AST 워커: match/
  tuple match 키워드·패턴 태그(`tag_off`)·바인딩(`name_span`/`alias_span`)·
  중첩 패턴, enum 이름·케이스, `flow` 헤드(`head_span`, head 없음 = 합성),
  result 키워드·단순명 바인딩·`<-`(binding_span 뒤 공백 스킵으로 위치 복원),
  let-else 태그(선언 키워드 뒤 공백 스킵, best-effort)·if let 패턴, bare
  `try`. verbatim 구간은 lexer로 재렉싱해 비청구 `match(`→`function`,
  `result {`→`variable` 부정 토큰. 유닛 테스트 7개(청구/부정/문자열·주석
  침묵/여러 줄 flow·`<-`/UTF-16 좌표).
- 2026-08-20: `src/server.rs`에 `semanticTokens` 요청 추가(text 기반 무상태,
  모듈 프로토콜 문서 갱신), `engine/mod.rs` re-export.
- 2026-08-20: 어댑터 — `engine.ts`에 `semanticTokens(compiler, text)`,
  `server.ts`에 `semanticTokensProvider` capability(legend=표준 7종,
  full)와 핸들러(`SemanticTokensBuilder`, 정렬 후 delta 인코딩; 엔진
  불가 시 빈 응답 = 문법 색 단독 — 다른 엔진 기능과 같은 강등).
- 2026-08-20: `server.test.ts`에 E2E 테스트 — LSP
  `textDocument/semanticTokens/full`을 실제 서버에 요청해 delta 디코딩 후
  ① `match(1)` 호출의 부정(`function`) ② 다음 줄 `|>`를 가진 `flow`의 청구
  (`keyword`) ③ 실제 match 키워드·태그·바인딩을 단언. rlc만 있으면 돌고
  tsgo 불필요.
- 2026-08-20: 문서 — `docs/design/lsp-architecture.md` 백엔드 표·구조도에
  semantic tokens 추가, 확장 README 기능표에 행 추가.

## 이슈 및 해결

### 이슈 1: path 기반 첫 구현이 툴체인 없는 환경에서 실패

- **증상**: `semanticTokens`를 기존 `semantic()` 라우팅(path → 프로젝트)으로
  붙이면, 프로젝트 최초 열기가 `Toolchain::resolve` 실패로 에러.
- **원인**: `open_project`가 `NativeBackend::new`에서 TypeScript 툴체인을
  즉시 요구 — 파스 전용 기능에는 불필요한 의존.
- **해결**: 결정 2 — text 기반 무상태 요청으로 재설계. 프로젝트를 아예
  경유하지 않는다.

### 이슈 2: let-else 태그와 `<-`의 오프셋이 AST에 없음

- **증상**: `LetElseStmt`는 `tag` 문자열만, `ResultBind`는 바인딩 span만
  기록 — 토큰 위치가 없다.
- **원인**: AST 오프셋은 에러 보고용으로만 수집돼 왔다.
- **해결**: AST를 바꾸지 않고 소스에서 복원 — 선언 키워드/바인딩 span 끝
  뒤 ASCII 공백만 건너뛴 지점이 기대 단어와 일치할 때만 보고(best-effort;
  사이에 주석이 오는 희귀 케이스는 그 토큰만 조용히 생략, 오배치 없음).
  AST에 오프셋 필드를 늘리는 대안은 codegen/sema에 무관한 침습이라 기각.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` (tokens 유닛 7개 포함 전 스위트 통과)
- [x] `editors/vscode`: `npx tsc -b` 클린; 확장 스위트 83개 중 신규
      semantic tokens E2E 포함 41 pass / 4 fail — 실패 4개는 tsgo 부재
      환경의 기존 completion 실패(TASK-092에서 HEAD 동일 재현 확인), 문법
      테스트 5/5 유지
- [x] `rlc --server` 스모크: `semanticTokens` 요청이 부정(`function`)과
      여러 줄 `flow` 청구(`keyword`)를 정확한 좌표로 반환

## 결과

- `src/engine/tokens.rs` (신규 — 파스 전용 분류, 유닛 테스트 포함)
- `src/engine/mod.rs` (re-export), `src/server.rs` (`semanticTokens` 요청)
- `editors/vscode/server/src/engine.ts`·`server.ts` (LSP semanticTokens
  capability + 핸들러), `server.test.ts` (E2E)
- `docs/design/lsp-architecture.md`, `editors/vscode/README.md`

TASK-092가 남긴 잔여 오차가 정식 경로로 해소됐다: 에디터 색상 =
TextMate 완전 확장 문법(기준선, 항상) + 파서 소유 semantic tokens(정밀화,
엔진 가용 시) — rust-analyzer와 같은 2층 구성. 남은 선택지(필요해지면
후속 태스크): range/delta 토큰, enum 페이로드 필드 오프셋의 AST 기록.
