# TASK-080: 구문이 도입하는 이름의 emit-map — `try`·패턴 바인딩

- **상태**: 완료
- **시작일**: 2026-08-19
- **완료일**: 2026-08-19
- **커밋**: `62aa7f4`

## 목적

에디터에서 `try` 선언형의 바인딩과 let-else·`if let`·match의 패턴 바인딩에
호버해도 타입이 나오지 않는다. 이 이름들은 codegen이 원본에서 복사하지 않고
문자열로 다시 조립해 방출하므로 emit-map에 구간이 생기지 않고, 언어 서버의
`toServiceOffset`이 "컴파일러 글루"로 판정해 질의 자체를 포기하기 때문이다.
TASK-078이 `result` 블록의 `<-` 바인딩에 대해 한 일을, 이름을 도입하는 나머지
구문 전부에 적용한다.

## 범위

- 포함:
  - `TryStmt.decl`의 바인딩 텍스트를 스팬으로 바꾸고 codegen이 원본에서
    복사해 방출한다.
  - `Binding`이 필드 이름·별칭의 스팬을 들고, match 암·let-else·`if let`·
    튜플 match·중첩 패턴의 구조 분해를 원본 바이트로 방출한다.
  - or-패턴의 바인딩은 의도적으로 매핑하지 않는다(결정 2).
  - 에디터: 이름 변경이 TypeScript의 구조 분해 shorthand 확장을 보존하도록
    한다(결정 3).
  - 레퍼런스(`docs/reference/cli.md`)의 `--emit-map` 매핑 목록 갱신.
- 제외: 방출 바이트는 한 바이트도 바뀌지 않는다. 언어 표면·에러·CLI 동작도
  그대로다. let-else의 `else` 블록 발산 판정 버그(아래 "후속")는 별개다.

## 의사결정

### 결정 1: `Binding`에 스팬 필드를 **추가**한다 (TASK-078처럼 대체하지 않고)

- **상황**: TASK-078은 `ResultBind.binding: String`을 `binding_span: Span`으로
  **대체**했다. `Binding`도 같은 방식으로 갈 수 있는지 정해야 했다.
- **검토한 대안**:
  - **대안 A — `name: String`/`alias: Option<String>`을 스팬으로 대체한다.**
    AST가 원본 좌표만 들고 다닌다는 점에서 일관적이다. 그러나 sema가 이
    문자열들을 중복 바인딩 검사·소진성 커버 계산(`src/sema.rs:131`,
    `:153`)에서 직접 비교하므로, 대체하면 sema 전체에 원본 슬라이스를
    넘겨야 한다. 이 태스크의 목적과 무관한 파급이다.
  - **대안 B — 스팬 필드를 추가한다.** 두 표현이 공존한다는 단점이 있지만,
    파서가 같은 토큰에서 한 번에 만들어 넣으므로 어긋날 수 없다.
- **선택과 근거**: 대안 B. TASK-078이 대안 B를 기각한 이유는 "`String`은
  아무도 쓰지 않게 된다"였는데, 여기서는 정반대로 sema가 계속 쓴다.
  확인: `git grep -n "b.name\|b.alias" src/sema.rs`.

### 결정 2: or-패턴의 바인딩은 매핑하지 않는다

- **상황**: `A(x) | B(x) => …`는 구조 분해를 **하나만** 방출하고 sema가 모든
  대안이 같은 집합을 바인딩하도록 보장한다. 방출된 `x`를 어느 대안의 `x`로
  매핑할지 정해야 했다.
- **검토한 대안**:
  - **대안 A — 첫 대안의 바이트로 매핑한다.** 호버는 되지만, 이름 변경이
    첫 대안만 고치고 나머지 대안의 `x`는 그대로 둔다(방출되지 않으니 편집이
    생기지 않는다). 결과 파일은 "or-패턴 대안들이 같은 집합을 바인딩해야
    한다"는 sema 에러가 되고, 사용자는 자기가 하지 않은 편집을 되돌려야
    한다. 정의 이동·참조 찾기도 임의의 한 대안을 가리킨다.
  - **대안 B — 매핑하지 않는다(현행 유지).** or-패턴 바인딩 호버만 못 하고,
    나머지는 전부 얻는다.
- **선택과 근거**: 대안 B. 방출된 구조 분해는 여러 패턴을 **대표**하므로 어느
  하나의 바이트라고 주장하는 것 자체가 거짓이고, 매핑의 계약("매핑된 조각은
  원본과 출력에서 같은 것을 가리킨다")을 깬다. 잘못된 답보다 답 없음이
  낫다는 것은 `mapper.rs`가 이미 세운 원칙이다. 계약 테스트
  `or_pattern_bindings_are_left_unmapped`로 고정했다.

### 결정 3: 이름 변경은 TypeScript의 shorthand 확장을 보존한다

- **상황**: 매핑이 생기면서 패턴 바인딩이 이름 변경의 대상이 됐다. rl의
  `Some(value)`는 `const { value } = $rl_m;`으로 방출되는데, TypeScript는
  구조 분해 shorthand를 새 이름으로 **대체**하지 못하고 `value: <새이름>`으로
  확장한다. 기존 서버 코드는 LSP 편집의 `newText`를 버리고 범위만 써서
  `params.newName`으로 치환했으므로, 그대로 두면 `Some(value)` →
  `Some(newName)`이 되어 **다른 필드를 바인딩**하게 된다.
- **검토한 대안**:
  - **대안 A — `newText`가 자리표시자 그대로가 아니면 이름 변경을
    거부한다.** 안전하지만 가장 흔한 패턴 형태에서 이름 변경을 못 하게 된다.
  - **대안 B — `newText`를 그대로 살려 자리표시자만 새 이름으로 바꾼다.**
    `value: newName`이 되고, rl의 패턴 문법에서 `Some(value: newName)`은
    정확히 같은 뜻의 별칭 바인딩이다 — 우연이 아니라 방출 형태가 곧 rl의
    별칭 문법이기 때문이다.
- **선택과 근거**: 대안 B. 단, `newText`에 자리표시자가 아예 없는 예상 밖의
  형태는 대안 A처럼 거부한다(이름 변경 전체 취소). 확인:
  `renaming a destructuring shorthand keeps TypeScript's expansion` 테스트가
  실제 tsgo로 `value: rlRenamePlaceholder`(선언)와 `rlRenamePlaceholder`(사용)
  두 형태를 모두 받는 것을 고정한다.

## 작업 내역

- 2026-08-19: 증상 재현. `rlc --emit-map`으로 `const n = try load();`,
  `const Some(value: v) = … else …;`, `if let Some(value: w) = …`,
  `Some(value: v) => …`의 바인딩 오프셋을 매핑에 넣어 보고 전부 `None`임을
  확인했다. 원인은 codegen이 이 이름들을 `push_lit`으로 방출하는 것
  (`src/codegen/mod.rs`의 `emit_try`/`emit_let_else`, `src/codegen/matches.rs`의
  `bind_str`/`collect_conds_binds`).
- 2026-08-19: `src/ast.rs` — `TryStmt.decl: Option<(String, String)>` →
  `Option<(String, Span)>`, `Binding`에 `name_span`/`alias_span` 추가.
- 2026-08-19: `src/parser/tries.rs` — 스캔 시작점과 `trim()` 길이로 바인딩
  스팬을 계산(TASK-078의 `scan_bind`와 같은 산술)하고 텍스트 복사를 없앴다.
- 2026-08-19: `src/parser/matches.rs` — `parse_bindings`가 `eat_ident`가
  이미 돌려주던 스팬을 버리지 않고 `Binding`에 싣도록 했다.
- 2026-08-19: `src/codegen/matches.rs` — `bind_str`/`bind_str_from`을
  `Rope`를 돌려주는 `bind_rope`/`bind_rope_from`으로, `pattern_conds_binds`의
  두 번째 반환값도 `Rope`로 바꿨다. 이름 목록 방출은 새 `binding_list`
  (원본 복사)와 `binding_list_lit`(or-패턴용, 글루)로 갈린다. 호출부는
  `format!("… {{ {bind}")`를 `push_lit("… { ") + append(bind)`로 쪼갰다 —
  바이트는 동일하다.
- 2026-08-19: `src/codegen/mod.rs` — `emit_try`가 바인딩을 `src_slice` +
  `push_src`로, `emit_let_else`가 `matches::binding_list`로, `emit_if_let`이
  로프가 된 `binds`를 `append`로 방출한다.
- 2026-08-19: `cargo test`로 방출 바이트 불변을 확인(기존 스냅샷 214건 포함
  전부 통과). `rlc --emit-map`을 다시 돌려 네 바인딩이 모두 매핑됨을 확인.
- 2026-08-19: `tests/emit_map.rs`에 계약 테스트 3건 추가
  (`try_declaration_bindings_are_mapped_to_emitted_declarations`,
  `pattern_bindings_are_mapped_to_their_destructurings`,
  `or_pattern_bindings_are_left_unmapped`)과 헬퍼 2개.
- 2026-08-19: 에디터 — `tstypes.ts`에 `RENAME_PLACEHOLDER`와
  `TsReference.newText`를 추가하고, `tsgo.ts`가 LSP 편집의 `newText`를
  버리지 않도록, `server.ts`의 이름 변경이 자리표시자를 치환하도록 했다.
- 2026-08-19: 에디터 테스트 — `emitmap.test.ts`에 실제 tsgo로 `try`·let-else·
  `if let`·match 바인딩을 호버하는 테스트 2건, `tsgo.test.ts`에 shorthand
  확장을 고정하는 테스트 1건 추가. `npm test`(rlc·tsgo 모두 있는 상태에서
  68 통과 / 8 skip — skip은 전부 선언 방출 관련 사이드카 테스트)로 확인.
- 2026-08-19: `docs/reference/cli.md`의 `--emit-map` 매핑 목록에 도입 이름과
  or-패턴 예외를 기록했다.

## 이슈 및 해결

### 이슈 1: 매핑을 붙이자 이름 변경이 패턴 바인딩을 망가뜨릴 수 있게 됐다

- **증상**: 코드 변경 자체는 아니지만, 매핑이 생기면 `Some(value)`의 `value`에
  대한 이름 변경이 활성화된다. 서버가 LSP 편집의 `newText`를 버리고 범위만
  쓰기 때문에 `Some(value)` → `Some(x)`가 되고, 이는 rl에서 "필드 `x`를
  바인딩"이라는 **다른 의미**다. 이전에는 매핑이 없어 이름 변경이 통째로
  거부됐으므로 드러나지 않던 문제다.
- **원인**: `server.ts`의 `onRenameRequest`가 모든 위치를 `params.newName`으로
  치환한다. TypeScript는 구조 분해 shorthand를 `value: <새이름>`으로 확장해서
  돌려주는데 그 확장이 버려진다.
- **해결**: 결정 3. `newText`를 끝까지 나르고 자리표시자만 치환한다. 확장이
  예상 밖 형태면 이름 변경 전체를 거부한다.

### 이슈 2: 테스트 컨텍스트 문자열이 엉뚱한 글자를 짚었다

- **증상**: `try_declaration_bindings_are_mapped_to_emitted_declarations`가
  "expected a mapping for \"n\" in \"const n = try\""로 실패했다.
- **원인**: 헬퍼가 컨텍스트 안에서 `needle`을 `find`하는데,
  `"const n = try"` 안의 첫 `n`은 `const`의 `n`이다.
- **해결**: 컨텍스트를 `"n = try load"`로 바꿔 바인딩이 첫 글자가 되게 했다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` — 438 통과, 실패 0 (통합 테스트 포함)
- [x] `npm test` (editors/vscode, rlc·tsgo 있는 상태) — 68 통과, 실패 0,
      8 skip(선언 방출 사이드카 — 이 태스크와 무관)

## 결과

`try` 선언형의 바인딩과 패턴 바인딩(match 암·let-else·`if let`·튜플 match·
중첩 패턴)의 필드 이름·별칭이 원본에서 복사돼 방출되고, `--emit-map`에
구간으로 실린다. 에디터는 이 이름들 위에서 호버·정의 이동·참조 찾기·이름
변경을 얻는다 — `const total = try load();`의 `total`은 이제 `number`로,
`if let Some(value: shown)`의 `shown`은 `string`으로 뜬다. 방출 바이트는
한 바이트도 바뀌지 않았다(기존 스냅샷 테스트 전부 통과).

변경 파일:

- `src/ast.rs` — `TryStmt.decl`의 텍스트 → 스팬, `Binding`에 스팬 두 개
- `src/parser/tries.rs` — 바인딩 스팬 계산
- `src/parser/matches.rs` — `parse_bindings`가 스팬을 싣는다
- `src/codegen/matches.rs` — 바인딩 목록 방출이 로프로, or-패턴은 글루로
- `src/codegen/mod.rs` — `try`/let-else/`if let` 방출이 원본을 복사
- `tests/emit_map.rs` — 계약 테스트 3건
- `editors/vscode/server/src/tstypes.ts` / `tsgo.ts` / `server.ts` —
  이름 변경이 shorthand 확장을 보존
- `editors/vscode/server/src/test/emitmap.test.ts` / `tsgo.test.ts` —
  종단 테스트 3건
- `docs/reference/cli.md` — `--emit-map` 매핑 목록

후속(별개 태스크로 등록할 것): let-else의 `else` 블록 발산 판정이
`else { return { kind: "Err", error: e }; }`처럼 **객체 리터럴을 반환**하는
경우를 거부한다 — 마지막 최상위 문장을 찾을 때 `{ … }`를 블록 문으로 읽는
것으로 보인다. 이번 변경과 무관하지만 작업 중 발견했다.
