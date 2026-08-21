# TASK-116: 진단을 정확한 구문 범위로 — 스팬 있는 에러와 위치 없는 에러 제거

- **상태**: 완료
- **시작일**: 2026-08-21
- **완료일**: 2026-08-21
- **커밋**: e4d2782

## 목적

에디터에서 rl 에러가 "어디서 났는지"를 Rust처럼 정확히 보여 주지 못했다.

보고된 증상: 아래 코드에서 `try inRange(n)`이 전파하는 `Err`(`Test`)가 함수의
반환 타입(`Result<string, ParseError>`)에 없어 타입 에러가 나는데, VS Code의
밑줄이 `const checked`의 첫 글자에 1글자로 붙었다.

```rl
export function readScore(text: string): Result<string, ParseError> {
  const n = try parseNum(text);
  const checked = try inRange(n);   // Err 타입이 ParseError가 아니다
  return Result.Ok(`점수 ${checked}`);
}
```

"이 케이스뿐 아니라 에러를 정확하게 표출하지 못하는 부분을 찾아 고쳐 달라"가
요청이었으므로, 먼저 진단 표면 전체를 실측하고(아래 조사) 그 결과를 범위로
해결한다.

## 범위

- 포함: rl 진단이 **범위(span)** 를 나르게 한다 — `RlError` → `CompileError`
  → 엔진 `Diagnostic` → `--server` JSON → VS Code 확장.
- 포함: `EmitAnchor`를 점에서 span으로 (`src..src_end`) — 글루에서 난 타입
  에러가 그 글루를 쓴 구문의 텍스트를 덮는다.
- 포함: `try` 앵커를 문의 시작이 아니라 `try` 키워드로 (Rust의 `?` 자리).
- 포함: 출력 자가 검사(verify) 실패를 `.rl` 위치로 되돌리고, 원인이 "거의 맞은
  rl 구문"이면 그 구문을 지목한다.
- 포함: sema·`val`의 개별 에러에 구문 범위 부여.
- 제외: 한 파일에서 rl 에러를 **여러 개** 보고하기 (`compile()`이 첫 에러에서
  멈추는 구조 자체를 바꿔야 한다) → TASK-117.
- 제외: 타입 에러 문안에서 구조적 타입(`{ kind: "OutOfRange"; ... }`)을 선언
  이름(`Test.OutOfRange`)으로 바꾸기 → TASK-118.
- 제외: 진단의 "note/label"(Rust의 보조 span) 도입. 현재 표면(LSP `Diagnostic`
  하나)에 담을 자리가 없다.

## 조사 — 정확하지 않던 지점들

실측(`rlc --check` / `--check-types` / `--server`의 `check`·`tsDiagnostics`)으로
확인한 것:

| # | 증상 | 원인 |
|---|------|------|
| 1 | `try`의 `Err` 타입 불일치가 `const`의 1글자에 붙음 | 앵커가 점이고 그 점이 `TryStmt::keyword_off`(문 시작) |
| 2 | verify 실패가 **위치 없이** 보고돼 에디터가 1행에 찍음 | swc가 생성물 좌표를 말하는데 매핑을 타고 돌아오지 않았음 |
| 3 | 소진성 에러가 `match` 다섯 글자만 밑줄 | 진단이 위치만 나르고, 에디터가 "그 위치의 단어"로 넓이를 추측 |
| 4 | 화이트리스트에 없는 글루 진단이 "가장 가까운 앞선 verbatim 바이트"에 1글자 | 앵커가 있는데도 fallback이 매핑만 봄 |

2번은 문서(`docs/ai/rl.md`)가 "TRAP"으로 부르는 가장 흔한 초보 실수의 표면이다
— `match s {`(괄호 누락), `try g()`(`;` 누락)는 계약대로 통과 영역으로 흘러가고,
그 결과가 위치 없는 자가 검사 실패였다.

## 의사결정

### 결정 1: 진단은 위치가 아니라 범위를 나른다

- **상황**: 에디터의 밑줄 넓이를 누가 정하는가. 지금은 확장이
  `analysis.wordAt`으로 추측한다 — `match`는 다섯 글자, `try` 문은 `const`
  한 단어.
- **검토한 대안**:
  (A) 확장이 rl 구문을 더 잘 추측하게 한다 — 확장이 파서를 흉내내야 하고,
  `analysis.ts`가 "진단 없는 구문 계층"이라는 규칙과 정면으로 어긋난다.
  (B) 컴파일러가 범위를 함께 보낸다 — 구문의 넓이를 아는 유일한 곳이 파서다.
- **선택과 근거**: (B). 확장이 아니라 컴파일러가 아는 사실이고, CLI·에디터·
  다른 소비자가 같은 답을 보게 된다. 끝 위치는 **선택적**이다(0 = 없음):
  넓이를 모르는 진단까지 지어내지 않기 위해, 그 경우 종전의 단어 추측이 남는다.

### 결정 2: `EmitAnchor`는 점이 아니라 span

- **상황**: 글루에서 난 타입 에러의 위치는 TASK-104가 앵커로 되찾았지만, 앵커는
  `src` 한 점이라 밑줄이 1글자였다.
- **검토한 대안**: (A) 소비자가 앵커 위치에서 다시 텍스트를 스캔해 넓이를
  구한다 — 파서가 아는 것을 소비자가 다시 추측하는 일. (B) 앵커에 `src_end`를
  더한다.
- **선택과 근거**: (B). `docs/design/rust-parity-analysis.md` §10.2가 애초에
  "글루 span → **그 구성물의 소스 span**"이라고 적어 둔 대로다 — 구현이 점에
  머물러 있었을 뿐이다. 방출 바이트는 그대로이므로 계약 영향 없음.

### 결정 3: `try`의 앵커는 `try` 키워드부터 식 끝까지

- **상황**: `const checked = try inRange(n);`에서 진단이 가리킬 곳.
- **검토한 대안**: (A) 문 전체(`const ... ;`) — 선언은 잘못이 없다.
  (B) `try` 키워드 한 점 — 다시 1글자. (C) `try <식>`.
- **선택과 근거**: (C). Rust가 `?`를 가리키는 것과 같은 자리이고, 실제로 타입이
  맞지 않는 것은 그 식이 만든 `Err`다. `TryStmt::keyword_off`(문 시작)는
  그대로 두고 `span`을 따로 뒀다 — 이미 그 오프셋을 쓰는 곳(파서·기존 보고)의
  의미를 바꾸지 않기 위해서다.

### 결정 4: verify 실패는 매핑을 타고 원본으로 돌아온다

- **상황**: 자가 검사는 **생성물**을 읽는데 사용자가 여는 파일은 `.rl`이다.
  기존 메시지는 `(line 12, col 16 of the generated output)`을 문안에 담고
  에러 자체는 위치가 없었다(에디터는 1행에 표시).
- **검토한 대안**:
  (A) 그대로 둔다 — 가장 흔한 실수의 피드백이 가장 나쁘다.
  (B) 위치만 되돌린다 — 낫지만 "왜 실패했는지"는 여전히 swc의 말이다.
  (C) 위치를 되돌리고, 되돌아온 자리에 rl 키워드가 서 있으면 그 구문을 지목한다.
- **선택과 근거**: (C). 되돌아온 위치가 verbatim 구간이고 그 줄에 홀로 선
  `match`/`try`/`result`/`flow`가 있으면, 그것은 계약상 "완전히 파싱되지 않아
  통과된 rl 구문"이다 — 유효한 TS에서는 그 자리에 올 수 없다. 판정은
  보수적으로 한다(점 뒤·식별자 일부는 제외; 실패한 줄만 본다). 못 찾으면 원래
  문장 그대로다.
- **확인**: `match s { ... }` → `file.rl:3:10`, `const x = try g()` →
  `file.rl:5:13`. 둘 다 종전에는 위치 없음.

### 결정 5: 화이트리스트 밖의 글루 진단도 구문 범위를 쓴다

- **상황**: 번역표에 없는 코드는 원문 + `(in code rlc generated for this
  construct)`로 전달되는데, 위치는 "가장 가까운 앞선 verbatim 바이트"였다.
- **선택과 근거**: 앵커가 있으면 앵커의 span을 쓴다. 문안이 "이 구문을 위해
  생성한 코드에서 났다"고 말하는데 위치가 그 구문이 아닌 것은 앞뒤가 맞지
  않는다. 앵커가 없을 때만 종전 fallback.

### 결정 6: `val`의 범위는 경로 전체가 아니라 뿌리 이름

- **상황**: `cfg.a.b = 2`에서 무엇을 덮을 것인가.
- **선택과 근거**: 뿌리 식별자(`cfg`). 판정의 근거가 "이 바인딩이 `val`이다"
  이므로 그 이름이 진단의 주어다. 경로 전체를 덮으려면 `val.rs`가 경로 끝을
  기록해야 하는데, 얻는 것에 비해 분석 자료를 늘리는 값이 크다.

## 작업 내역

- 2026-08-21: 증상 재현. `/tmp` 프로젝트에 보고된 `score.rl`을 그대로 두고
  `npm i -D typescript@7` 후 `rlc --check-types score.rl` → `22:3`(`const`),
  `rlc --server`의 `tsDiagnostics` → 범위 `(21,2)-(21,3)` (1글자). 진단 표면
  전체를 훑는 배터리(`a_exhaust`/`b_dup`/`c_unknown`/`d_trypos`/`h_badsyntax`/
  `i_trynosemi`/`g_valerr`)로 위 조사표를 만들었다.
- 2026-08-21: `ast.rs` — `TryStmt::span`, `IfLetStmt::head_span`,
  `ResultBind::expr_span` 추가. 파서(`tries.rs`/`iflets.rs`/`results.rs`)가
  채운다. `try`의 span은 선언 형태에서도 `try` 키워드에서 시작한다.
- 2026-08-21: `lib.rs` — `EmitAnchor::src_end`. `codegen/rope.rs`의
  `Piece::Open`과 `anchored()`가 span을 나르고, `codegen/mod.rs`가 구문마다
  끝을 정한다(match는 스크루티니 닫는 괄호, let-else는 `else` 앞, 파이프라인은
  마지막 스텝).
- 2026-08-21: `error.rs` — `RlError::end`와 `RlError::span()`,
  `CompileError::end_line`/`end_col`. `lib.rs::compile_mapped`가 둘을 잇는다.
- 2026-08-21: `verify.rs` — `verify_output`이 문자열 대신 `Failure`를 돌려주고,
  새 `at_source()`가 생성물 좌표 → 바이트 → 매핑/앵커 → `.rl` 범위로 옮긴다.
  `rl_construct_at()`이 실패한 줄의 홀로 선 rl 키워드를 찾는다.
- 2026-08-21: `LetElseStmt::head_span` 추가 — sema가 소스 텍스트를 갖지 않아
  `keyword_off..else_off`를 직접 트림할 수 없었다. 넓이를 아는 곳(파서)이
  기록한다.
- 2026-08-21: `sema.rs`·`val.rs`의 보고 지점 대부분을 `RlError::span`으로.
  소진성은 `MatchAnalysis::head_end`(신설)를 써 `match (스크루티니)`를 덮는다.
- 2026-08-21: 엔진 — `SourceAnchor::end`, `Diagnostic::end`,
  `translate_on_glue`가 앵커를 통째로 돌려주고, `language.rs`의
  `service_diagnostics`가 글루 진단(번역된 것과 아닌 것 모두)에 앵커 span을
  쓴다.
- 2026-08-21: `server.rs`가 `endLine`/`endCol`을 실어 보내고, 확장의
  `rlc.ts`가 그것을 통과시키고 `server.ts::toDiagnostic`이 밑줄로 쓴다
  (없으면 종전 `wordAt`).
- 2026-08-21: 테스트 — `tests/compile.rs`에 범위 계약 7건(신설),
  `src/verify.rs`에 `byte_of`/`rl_construct_at` 단위 테스트 4건,
  `editors/vscode`의 `server.test.ts`에 LSP 발행 진단의 범위 2건(테스트
  클라이언트가 알림을 기다릴 수 있게 `waitFor` 추가). 기존 테스트 5건은
  좋아진 위치·문안으로 갱신.
- 2026-08-21: 문서 — `errors.md`(진단의 범위 절 신설, 출력 검증 절 재작성),
  `cli.md`(서버 프로토콜 필드), `ai/rl.md`(계약 세 줄), 설계 문서 §10.2 주석,
  `CHANGELOG.md`.

## 이슈 및 해결

### 이슈 1: `checked_coverage`의 키가 `match` 키워드 오프셋

- **증상**: 엔진의 typed 소진성에 끝 위치를 주려고 `MatchAlphabets` 튜플을
  `((start, end), ...)`로 바꿨더니 `analysis::checked_coverage`의 시그니처와
  어긋났다(`expected &[(usize, Vec<Vec<String>>)]`).
- **원인**: 그 오프셋은 단순한 위치가 아니라 **match의 identity**다(probe·
  scrutinee 임시·분석이 모두 그것으로 짝을 맞춘다). 튜플에 끝을 끼워 넣는 것은
  identity를 바꾸는 일이었다.
- **해결**: identity는 그대로 두고 `(파일, 키워드 오프셋) → 끝`을 별도 맵으로
  들고 보고 시점에 붙였다.

### 이슈 2: npm의 `typescript@7`로는 에디터 테스트가 무더기로 실패

- **증상**: 검증 환경을 만들려고 `npm i -D typescript@7`을 깔고
  `editors/vscode`에서 `npm test`를 돌리자 completion/hover 계열 29건이
  실패했다(`hover has an answer`가 `undefined`). `git stash`로 이번 변경을
  걷어내도 **동일하게** 실패해 변경과 무관함은 확인됐지만, 그 상태로는
  에디터 경로를 실제로 검증할 수 없었다.
- **원인**: 그 케이스들은 tsgo의 **language service**(`tsgo --lsp`)가 답해야
  하는데, npm 패키지로 해석된 실행 파일은 이 환경에서 그 표면을 답하지
  못했다. 즉 "환경에 없는 기능"이 아니라 **잘못된 툴체인**이었다.
- **해결**: typescript-go를 클론해 직접 빌드하고
  (`go build -o built/local/tsgo ./cmd/tsgo`, API 클라이언트는 방금 만든
  tsgo 자신으로 `./built/local/tsgo -b _packages/native-preview --force`)
  `RLC_TSGO_ROOT`로 가리켰다. 그 뒤 `npm test`는 **82건 전부 통과, skip 0**.
  npm으로 깔았던 `typescript@7`은 되돌렸다(`editors/vscode/package.json`·
  lockfile 원복).
- **남은 부채**: 없음. 이 저장소의 `scripts/setup --tsgo-root <checkout>`가
  같은 체크아웃 모드를 이미 지원한다.

### 이슈 3: 기존 스냅샷 테스트가 옛 위치를 고정하고 있었다

- **증상**: `try_inside_match_arm_is_an_error`가 `(2, 18)`(= `const`)를,
  세 건이 "generated TypeScript failed to parse" 문안을 기대해 실패.
- **원인**: 이번 변경이 의도적으로 바꾼 바로 그 동작.
- **해결**: 새 동작(= 개선된 위치·문안)으로 갱신하고, 끝 위치까지 함께
  고정했다.

## 검증

툴체인은 직접 빌드한 typescript-go 체크아웃이다(npm 패키지 아님):
`go build -o built/local/tsgo ./cmd/tsgo` + `./built/local/tsgo -b
_packages/native-preview --force`, `RLC_TSGO_ROOT`로 지정.

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` — 11개 타깃 전부 통과(신규 13건 포함)
- [x] `RLC_REQUIRE_TSGO=1 cargo test --test native` — 30건 통과(skip 없음)
- [x] `editors/vscode`: `npm test` — **82건 전부 통과, skip 0**
      (신규 진단 범위 2건 포함)
- [x] 보고된 시나리오 수동 확인:
      `rlc --check-types score.rl` → `score.rl:22:19`(= `try`),
      `tsDiagnostics` 범위 `(21,18)-(21,32)` = `try inRange(n)`

## 결과

진단이 시작 위치와 함께 **끝**을 나른다. 에디터의 밑줄은 이제 그 에러가 말하는
구문 — `try inRange(n)`, `match (shape)`, 중복된 태그, `val` 바인딩 — 을 덮고,
위치 없는 rl 에러는 남지 않았다(자가 검사 실패도 `.rl`의 행·열을 갖는다).

변경 파일:

- 컴파일러: `src/ast.rs`, `src/error.rs`, `src/lib.rs`, `src/verify.rs`,
  `src/sema.rs`, `src/val.rs`, `src/server.rs`,
  `src/parser/{tries,iflets,results}.rs`, `src/codegen/{mod,rope}.rs`,
  `src/analysis/mod.rs`,
  `src/engine/{projection,semantics,language,project}.rs`
- 에디터: `editors/vscode/server/src/{rlc,server}.ts`,
  `editors/vscode/server/src/test/server.test.ts`
- 테스트: `tests/compile.rs`, `tests/emit_map.rs`
- 문서: `docs/reference/{errors,cli}.md`, `docs/ai/rl.md`,
  `docs/design/rust-parity-analysis.md`, `CHANGELOG.md`

후속: [TASK-117](./TASK-117-multiple-rl-diagnostics.md),
[TASK-118](./TASK-118-named-error-types-in-messages.md).
