# TASK-069: match 리터럴 패턴 — 문자열/숫자/불리언

- **상태**: 완료
- **시작일**: 2026-08-18
- **완료일**: 2026-08-18
- **커밋**: —

## 목적

`docs/design/match-literal-patterns.md`의 설계를 구현한다. 기존 태그 match의
패턴 공간을 문자열/숫자/불리언 리터럴로 확장해서, TypeScript에 흔한 리터럴
유니언(`"north" | "south"`, `200 | 404`)을 rl `match`로 자연스럽게 다룰 수
있게 한다. 리터럴 유니언 소진성은 rlc가 직접 추론하지 않고 이미 있는
TypeScript 연동 경로(`--types`)에 붙인다.

## 범위

- 포함: 리터럴 패턴 파싱(문자열/숫자/불리언 + or-패턴), 의미 검사(태그·리터럴
  혼합 금지, 중복 리터럴, or-패턴 종류 일치, 와일드카드 위치), codegen(값
  switch / if-체인, 런타임 가드, 스크루티니 1회 평가), `--types` 경로의 리터럴
  유니언 소진성 검사와 `.rl` 위치 매핑, 테스트와 레퍼런스/AI 문서 갱신.
- 제외: 튜플 패턴 안의 리터럴(설계 §18), 리터럴 이외의 패턴(객체/배열/범위),
  기본(타입 없는) 컴파일 경로에서의 소진성 추론, 리터럴 바인딩.

## 의사결정

### 결정 1: 숫자 리터럴을 렉서 토큰으로 만들지 않고 바이트로 스캔한다

- **상황**: `src/lexer.rs`는 식별자·문자열·템플릿·정규식과 다섯 개의 융합
  연산자만 토큰으로 만들고, 나머지 유의 바이트는 1바이트 `Punct`다. 따라서
  `200`은 `Punct('2') Punct('0') Punct('0')`, `1_000`은 `Punct('1')`
  뒤에 `Ident("_000")`으로 들어온다. 리터럴 패턴을 파싱하려면 숫자를 하나의
  단위로 봐야 한다.
- **검토한 대안**:
  - **A. 렉서에 `TokenKind::Num`을 추가한다.** 파서가 깔끔해진다. 대신
    정규식-vs-나눗셈 휴리스틱(`prev_sig`)과 기존 모든 스캐너(괄호 매칭,
    `expr_body_end`, `guard_end`, 타입 스캔)의 토큰 경계가 바뀐다.
    `1_000` → 지금은 `Punct + Ident`, 바꾸면 한 토큰 — 전 구문에 영향이
    번지는 변경이고, 얻는 것은 한 구문의 편의뿐이다.
  - **B. 매치 암 파서에서 소스 바이트로 숫자를 스캔하고, 그 바이트 범위를
    덮는 토큰들을 커서에서 건너뛴다.** 변경이 `parser/literals.rs` 한
    파일에 갇힌다. 스캔 끝이 토큰 경계와 정확히 일치하지 않으면
    (`1e5x` 같은 것) 파스를 실패시켜 통과로 돌린다.
- **선택과 근거**: **B**. 설계 원칙 10("구현 편의를 위해 현재
  AST/lexer/parser 구조를 불필요하게 재설계하지 않는다")과 태스크 지시
  ("negative number가 unary expression 형태라면 기존 lexer 구조를 억지로
  변경하지 말고 현재 구조에 가장 자연스러운 방법으로 처리한다")를 그대로
  따른다. 확인: `cargo test`의 passthrough 50개·compile 191개가 렉서 변경
  없이 모두 통과.

### 결정 2: 음수는 `-`를 리터럴 패턴의 일부로 받는다

- **상황**: `-1`은 렉서에서 `Punct('-')` + 숫자다. Rust의 `match`는 `-1`을
  리터럴 패턴으로 받고, JS `switch`도 `case -1:`을 허용한다.
- **검토한 대안**: ① 지원하지 않는다(HTTP 코드·상태값에는 음수가 흔하지
  않지만 `-1`은 "없음" 관용구로 매우 흔하다) ② `-` 뒤에 숫자가 오면
  하나의 리터럴로 본다.
- **선택과 근거**: ②. 스캔 시작점만 `-`로 옮기면 되고, 방출은 소스 텍스트
  복사라 `case -1:`이 그대로 나온다. 중복 판정은 부호를 반영한 값으로 한다.

### 결정 3: 리터럴 동치는 표기가 아니라 **값**으로 판정한다

- **상황**: `switch`는 `===`로 비교하므로 `200`과 `0xc8`, `"a"`와 `'\x61'`은
  같은 케이스다. 중복 검사를 표기 문자열로 하면 이 중복을 놓친다.
- **검토한 대안**: ① 소스 텍스트를 키로 쓴다(단순하지만 오탐 아닌 미탐이
  생긴다 — 도달 불가능한 암이 조용히 남는다) ② 값으로 정규화한다.
- **선택과 근거**: ②. `LiteralValue`를 `Str(디코드된 문자열)` /
  `Num(f64, -0은 0으로)` / `BigInt(십진 문자열)` / `Bool`로 두고 `PartialEq`로
  비교한다. `1n`은 `1n === 1`이 거짓이므로 `Num`과 다른 변종으로 둔다.
  확인: `literal_duplicate_compares_values_not_spellings` 테스트.
  **방출은 정규화된 값이 아니라 소스 표기 그대로**다 — `0xff`를 `255`로
  바꿔 내보내면 사용자가 쓴 코드가 아닌 것이 되기 때문.

### 결정 4: `case` 라벨을 소스에서 복사된 **매핑된 조각**으로 방출한다

- **상황**: 태그 match의 `case "Circle"`은 컴파일러가 지어내는 글루라
  매핑이 없다. 리터럴은 사용자가 쓴 텍스트다.
- **검토한 대안**: ① `Rope::push_lit`으로 문자열을 만들어 넣는다 ②
  `Rope::push_src`로 원본 바이트를 복사한다.
- **선택과 근거**: ②. 표기 보존(결정 3)이 자동으로 따라오고, 유니언에 없는
  리터럴을 썼을 때 tsc가 내는 `Type '"c"' is not comparable to ...` 진단이
  기존 방출 매핑을 타고 **사용자가 쓴 리터럴 위치**로 정확히 돌아온다.
  확인: `types_maps_a_bad_case_literal_back_to_the_rl_source` 테스트가
  `src/main.rl:1:61`을 기대한다.

### 결정 5: 리터럴 패턴에도 가드를 허용한다

- **상황**: 설계 §17은 "duplicate/exhaustiveness 의미를 명확히 설계할 수
  없으면 v1에서는 금지"라고 열어 두었다.
- **검토한 대안**:
  - **A. 금지.** 파서에서 `allow_guard`를 태그로 제한. 범위가 작아진다.
  - **B. 허용, 태그 가드와 완전히 같은 규칙.** 가드 암은 아무것도 커버하지
    못하므로 ① 가드 암끼리는 같은 리터럴을 반복할 수 있고 ② 소진성(타입
    경로 포함)에서 커버로 인정되지 않는다.
- **선택과 근거**: **B**. 의미가 애매한 게 아니라 이미 확립된 규칙을 그대로
  물려받는 경우다. 구현 비용도 사실상 0이었다 — 가드가 있는 match는
  기존 if-체인 경로로 가고, 그 경로는 `Pattern`을 전수 매치하므로
  `Literals` 팔을 어차피 써야 했다. 확인:
  `literal_duplicate_is_allowed_between_guarded_arms`,
  `runtime_literal_match_with_guard`,
  `types_does_not_count_a_guarded_arm_as_covering`.

### 결정 6: 종류 혼합 금지는 **or-패턴 안에서만** 건다

- **상황**: 설계 §4는 `"a" | 1`을 semantic error로 하라고 한다. 그럼 암을
  가로지르는 혼합(`"a" => .., 1 => ..`)도 막을 것인가?
- **검토한 대안**: ① match 전체에서 한 종류만 허용 — 규칙이 단순하지만
  `type T = "auto" | 0` 같은 **유효한 TS 리터럴 유니언**을 rl에서 다룰 수
  없게 된다 ② or-패턴 안에서만 금지.
- **선택과 근거**: ②. 설계가 요구한 최소 규칙이고, 타입 정합성 판단은
  tsc의 몫이라는 프로젝트 원칙(에러 계층)과 맞는다. 혼합 유니언에 대해서도
  중복 검사와 타입 소진성은 종류+값 키로 정상 동작한다.

### 결정 7: 소진성은 `--types`의 TypeScript 체커에게 묻고, 진단은 rlc가 만든다

- **상황**: `_` 없는 리터럴 match가 소진인지 알려면 스크루티니의 TS 타입이
  필요하다. rlc 안에 타입 추론기를 만들지 않는 것이 설계 계약이다.
- **검토한 대안**:
  - **A. 호스트(`types_host.mjs`)가 진단 문자열까지 만들어 일반
    `diagnostics` 배열로 돌려준다.** 기존 `TypeDiagnostic::render`가
    방출 매핑을 거꾸로 타 `.rl` 위치로 옮겨 준다. 다만 위치는 스크루티니가
    되고, 기존 rl 소진성 에러는 `match` 키워드를 가리킨다 — 일관성이 깨진다.
  - **B. rlc가 "질문"(스크루티니의 방출 위치 + 덮은 리터럴)을 보내고,
    호스트는 "빠진 리터럴"만 돌려준다. 문장과 위치는 rlc가 만든다.**
- **선택과 근거**: **B**. 진단 위치가 기존 rl 소진성 에러와 같은 `match`
  키워드가 되고, 메시지 형식이 rlc 한곳에 남는다(rl 에러는 rlc가 낸다는
  에러 계층과도 맞다). 호스트는 타입 조회만 한다.
- **구현**: `rlc::literal_matches(source)`가 `_` 없는 리터럴 match마다
  `(match 키워드 오프셋, 스크루티니 바이트 범위, 덮은 리터럴)`을 낸다.
  `--types`는 스크루티니 오프셋을 방출 매핑으로 출력 오프셋 → UTF-16
  위치로 옮겨 호스트에 넘기고, 호스트는 그 범위에 **완전히 들어가는 가장
  넓은 노드**(= 스크루티니 식 전체)를 찾아 `getTypeAtLocation`을 부른다.

### 결정 8: 유한 리터럴 집합으로 확정될 때만 검사하고, `boolean`은 확정으로 본다

- **상황**: 설계 §14/태스크 §14는 `string`·`number`·`boolean`·`unknown`·
  `any`·`T`·`"a" | string` 등을 "검사하지 않음"으로 나열하는데, §12/§22는
  `true | false`를 검사 대상으로 두고 `function bool(flag: boolean)`을 완료
  조건에 넣었다. TypeScript에서 `true | false`는 **정규화되어 `boolean`과
  같은 타입 객체**라 둘을 구분할 방법이 없다.
- **검토한 대안**: ① `boolean`을 배제한다 — 목록에는 충실하지만 `true |
  false`도 함께 배제된다 ② `boolean`을 `{true, false}`로 본다.
- **선택과 근거**: ②. 상위 원칙은 "체커 결과를 **완전한 유한 리터럴 집합**으로
  바꿀 수 있을 때만 검사한다"이고 `boolean`은 정확히 그 조건을 만족한다.
  오탐이 생길 수 없는 검사이므로 "잘못된 진단보다 검사하지 않는 편이 낫다"는
  단서에도 걸리지 않는다. `string`/`number`는 리터럴 집합이 무한하므로 그대로
  배제된다. 확인: `types_checks_boolean_and_number_unions`,
  `types_does_not_guess_when_the_scrutinee_type_is_open`.
- TS `enum` 멤버 타입도 배제한다 — 멤버는 `E.A`로 쓰지 리터럴 패턴으로 쓰지
  않으므로 "빠졌다"는 보고가 곧 오탐이다.

### 결정 9: 튜플 패턴에는 리터럴을 넣지 않는다

- **상황**: `TuplePattern::Elems`는 `Vec<Pattern>`이라 `Pattern`에
  `Literals`를 더하면 구조적으로는 튜플 안에도 들어갈 수 있다.
- **선택과 근거**: 설계 §18("부분적으로 parse만 되는 상태를 만들지 않는다")에
  따라 `parse_tuple_elems`는 손대지 않았다 — 튜플 원소는 여전히 태그 패턴이나
  `_`뿐이다. sema/codegen의 튜플 경로에는 도달 불가능한 `Literals` 팔을
  "아무 조합도 커버하지 않음"으로 보수적으로 두어 패닉 없이 안전하게 만들었다.
  확인: `tuple_patterns_do_not_accept_literals`.

## 작업 내역

- 2026-08-18: 설계 문서(`docs/design/match-literal-patterns.md`)와 현재 코드
  대조 — `src/ast.rs`의 `Pattern`, `src/parser/matches.rs::parse_arms`,
  `src/sema.rs::check_match`/`check_exhaustiveness`,
  `src/codegen/matches.rs::emit_switch`/`emit_if_chain`, `src/lexer.rs`(숫자
  토큰 없음 확인), `--types` 파이프라인(`src/main.rs::types_once`,
  `src/types_host.mjs`, 방출 매핑 `EmitMapping`) 확인. 설계와 코드가 어긋나는
  부분은 없었고, AST 확장 형태도 설계 제안과 같은 모양으로 갈 수 있었다.
- 2026-08-18: **AST** — `src/ast.rs`에 `Pattern::Literals(Vec<LiteralPattern>)`,
  `LiteralPattern { span, value }`, `LiteralValue { Str, Num, BigInt, Bool }`와
  `kind()`/`render()` 추가. 설계의 `off: usize` 대신 `span: Span`을 쓴 것은
  codegen이 **소스 바이트를 그대로 복사**해야 해서다 (결정 4).
- 2026-08-18: **파서** — `src/parser/literals.rs` 신설(숫자 바이트 스캐너,
  문자열 이스케이프 디코더, or-대안 파싱), `src/parser/mod.rs`에 모듈 등록,
  `src/parser/matches.rs::parse_arms`에 리터럴 분기 추가 + `allow_guard`를
  "`_`가 아닌 모든 패턴"으로 확대. `parse_tuple_elems`는 미변경.
- 2026-08-18: **sema** — `src/sema.rs::check_match`에 ① 태그·리터럴 혼합 금지
  ② or-대안 종류 일치 ③ 값 기준 중복 리터럴(무가드 암 기준) 추가.
  `check_exhaustiveness`가 태그 없는 match를 enum 후보와 대조하지 않도록
  `MatchCheck`를 `tags`가 비어 있지 않을 때만 등록하도록 수정(그대로 두면
  리터럴 match가 내장 `Option`/`Result`에 붙어 오진단이 났다 — 이슈 1).
- 2026-08-18: **codegen** — `src/codegen/matches.rs`에 `is_literal_match`,
  `literal_text`(매핑된 소스 조각), `unexpected(literal)` 추가.
  `emit_switch`는 `switch ($rl_m)`로, `emit_if_chain`은 `$rl_m === <리터럴>`
  or-체인으로 방출. `src/codegen/mod.rs::Emitter::src_slice`를 `pub(super)`로.
- 2026-08-18: **타입 소진성** — `src/probe.rs` 신설(`literal_matches`,
  `LiteralMatch`, `Literal`; 프로그램 전체를 재귀 순회), `src/lib.rs`에서 공개.
  `src/main.rs`에 `LiteralCheck`/`literal_checks`/`output_offset`/
  `utf16_position`을 더해 `--types` 잡에 `literalChecks`를 실어 보내고
  `literalMissing` 응답을 `.rl` 위치로 출력. `src/types_host.mjs`에
  `checkLiteralMatches`/`widestNodeIn`/`finiteLiterals` 추가.
- 2026-08-18: **테스트** — `tests/compile.rs`에 리터럴 섹션 20개,
  `tests/passthrough.rs`에 6개, `tests/integration.rs`에 tsc/node 실행 8개,
  `tests/cli.rs`에 `--types` 타입 소진성 7개.
  `cargo test`(전체 366개) 통과.
- 2026-08-18: **문서** — `docs/reference/language.md` §3.1/3.2/3.4/3.6/3.7 갱신
  + §3.8(리터럴 패턴)·§3.9(타입 소진성) 신설, `docs/reference/errors.md`에
  리터럴 패턴 에러 표, `docs/reference/cli.md` `--types` 절, `docs/ai/rl.md`
  match 절과 체크리스트, `README.md`·`src/lib.rs` 소개문.

## 이슈 및 해결

### 이슈 1: 리터럴 match가 내장 `Option` 소진성 검사에 걸렸다

- **증상**: `const v = match (x) { "Some" => 1, "None" => 2 };`가
  `match on built-in enum Option is not exhaustive: missing ...`로 실패.
- **원인**: `check_match`는 `_` 없는 match마다 `MatchCheck`를 등록하고,
  `check_exhaustiveness`는 후보 enum마다 `check.tags.iter().all(...)`로
  적합성을 본다. 리터럴 match는 태그가 하나도 없어 이 조건이 **공허하게
  참**이 되고, 후보 표의 첫 enum(내장 `Option`)이 그대로 채택됐다.
- **해결**: `tags`가 비어 있으면 `MatchCheck`를 아예 등록하지 않는다.
  회귀 방지로 `literal_match_is_not_checked_against_enums` 테스트 추가.

### 이슈 2: `getTypeAtLocation`이 스크루티니가 아니라 그 첫 토큰의 타입을 줬다

- **증상**: 초안에서는 스크루티니의 **시작 위치**에 있는 가장 깊은 노드를
  찾아 타입을 물었다. `match (obj.field)`에서는 `obj`의 타입이 돌아온다.
- **원인**: 위치 하나로는 식의 범위를 표현할 수 없다.
- **해결**: `LiteralMatch`에 `scrutinee_end`를 더해 방출 범위 `[start, end)`를
  넘기고, 호스트는 그 범위에 **완전히 포함되는 가장 넓은 노드**를 고른다.
  방출 형태가 `const $rl_m = (<스크루티니>);`라 이 범위에는 스크루티니 식
  하나만 통째로 들어간다.

### 이슈 3: 통과(passthrough) 테스트 두 개를 잘못 세웠다

- **증상**: `match (x) { 1 }`과 `match (a, b) { ("x", 1) => 1, _ => 0 }`이
  "그대로 통과"할 것으로 기대했는데 `generated TypeScript failed to parse`.
- **원인**: 둘 다 애초에 **유효한 TypeScript가 아니다** — `match(x)` 뒤에
  같은 줄의 `{`는 ASI가 세미콜론을 넣지 않아 구문 에러이고,
  `("x", 1) => 1`도 유효한 식이 아니다. 파서는 정상적으로 구문을 claim하지
  않았고(통과), 통과된 원문이 출력 자가 검사에서 걸린 것 — 리터럴 패턴
  도입 이전과 같은 기존 동작이다.
- **해결**: 테스트의 전제를 고쳤다. 첫 번째는 ASI가 적용되는 줄바꿈 형태
  (`match (x)\n{ 1 }`)로 바꿔 진짜 유효한 TS를 확인하고, 두 번째는
  `verify: false`로 "바이트 그대로 통과"만 확인한다.

### 이슈 4: 문자열 이스케이프의 서로게이트 쌍

- **증상**: `"😀"` 같은 서로게이트 쌍을 한 글자씩 디코드하면
  `char::from_u32`가 실패한다(고아 서로게이트는 `char`가 아니다).
- **원인**: JS 문자열은 UTF-16 코드 유닛 열이고 Rust `char`는 코드 포인트다.
- **해결**: `\uD800..\uDBFF` 뒤에 `\uDC00..\uDFFF`가 오면 JS와 같은 규칙으로
  하나의 코드 포인트로 합친다. 그래도 남는 고아 서로게이트는 파스 실패로
  두어 통과시킨다 — 리터럴 패턴에 쓰일 일이 없는 병리적 입력이다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` — 366개 통과 (compile 191 / integration 70 / passthrough 50 /
      cli 16 / doc 13 / emit_map 9 / sidecar 8 / stdlib 3 / 기타 6)

## 결과

| 파일 | 변경 |
|------|------|
| `src/ast.rs` | `Pattern::Literals`, `LiteralPattern`, `LiteralValue`(+`kind`/`render`) |
| `src/parser/literals.rs` | **신규** — 숫자 바이트 스캔, 문자열 디코드, 리터럴 or-대안 파싱 |
| `src/parser/mod.rs` | 모듈 등록 |
| `src/parser/matches.rs` | 암 패턴에 리터럴 분기, 가드 허용 범위 확대 |
| `src/sema.rs` | 혼합 금지·or-대안 종류 일치·값 기준 중복 검사, 태그 없는 match를 enum 소진성에서 제외 |
| `src/codegen/matches.rs` | `switch ($rl_m)` / `$rl_m === ...` 방출, 리터럴 런타임 가드, 매핑된 `case` 라벨 |
| `src/codegen/mod.rs` | `src_slice` 가시성 |
| `src/probe.rs` | **신규** — `literal_matches` 공개 API (타입 소진성 질문 수집) |
| `src/lib.rs` | `Literal`/`LiteralMatch`/`literal_matches` 공개 |
| `src/main.rs` | `--types`가 질문을 보내고 답을 `.rl` 위치로 보고 |
| `src/types_host.mjs` | 체커로 스크루티니 타입 조회 → 유한 리터럴 유니언일 때만 누락 보고 |
| `tests/{compile,passthrough,integration,cli}.rs` | 리터럴 패턴 테스트 41개 |
| `docs/reference/{language,errors,cli}.md`, `docs/ai/rl.md`, `README.md` | 레퍼런스·AI 문서 갱신 |

기존 태그/튜플/중첩/가드 match 동작은 그대로다 — 관련 테스트 전부 통과.

후속 후보(별도 태스크로 등록 필요): 튜플 패턴 원소의 리터럴,
`rlc --check --typed` 같은 명시적 타입 검사 모드(설계 §최종 의견 4번).
