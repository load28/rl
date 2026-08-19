# TASK-070: `val` — 변경 금지 바인딩 수식자

- **상태**: 완료
- **시작일**: 2026-08-19
- **완료일**: 2026-08-19
- **커밋**: `2c7e101`

## 목적

TypeScript의 기본 mutable 의미는 그대로 두고, 사용자가 `val`을 명시한
바인딩에서만 mutation을 컴파일 시점에 금지한다. rl이 "런타임 없이 컴파일 타임
보장을 더한다"는 방향의 첫 비-구문(non-syntax) 기능이다.

## 범위

- 포함:
  - `val const` / `val let` / `val var` 선언 수식자.
  - 함수·화살표·메서드 매개변수, `catch` 바인딩, `for` 머리의 `val`.
  - `val` 바인딩에서 시작하는 접근 경로의 mutation 검사(대입·복합 대입·증감·
    `delete`·인덱스 대입·알려진 변경 메서드).
  - 같은 파일에서 이름으로 선언된 함수 호출 시의 변경 권한 검사.
  - 렉시컬 스코프·섀도잉 추적.
  - 방출물에서 `val` 완전 제거 (런타임 코드 0, 타입 변환 0).
  - 레퍼런스·에러·AI 가이드·README·VSCode 문법 갱신, 3계층 테스트.
- 제외 (의도적 비목표):
  - ownership / borrow checker / lifetime / move semantics.
  - 객체 자체의 deep immutability, `Object.freeze`·`Proxy` 런타임 강제.
  - 임의 외부 함수의 side effect 분석·전역 이펙트 추론.
  - 타입 수준 `readonly` 변환.
  - match 패턴 바인딩의 `val`(`Ok(val user)`) — 패턴 문법 확장이 필요해 후속.
  - `mut` 키워드 — 기본값이 mutable이므로 중복.

## 의사결정

### 결정 1: 검사를 AST(sema)가 아니라 토큰 스트림 위에서 한다

- **상황**: 기존 파이프라인은 parse(AST) → sema(AST 순회) → codegen이다.
  그런데 `val`이 다루는 대상 — 변수 선언, 매개변수, `x.a = 1` 같은 식 — 은
  전부 **통과 영역**이라 AST에서는 `Segment::Verbatim`(불투명한 바이트 범위)
  이다. AST에는 검사할 정보 자체가 없다.
- **검토한 대안**:
  - (A) 파서를 확장해 선언·식을 AST로 올린다 — rl이 TypeScript 파서가 된다는
    뜻이고, "완전 파싱된 rl 구문만 들어올린다"는 통과 계약의 구조를 무너뜨린다.
    비용도 가장 크다.
  - (B) swc로 통과 영역을 파싱해 검사한다 — verify.rs가 이미 swc를 쓰므로
    가능은 하다. 그러나 전체 파일 재파싱 비용이 붙고, swc AST의 위치를 원본
    바이트 오프셋으로 되돌리는 층이 하나 더 생긴다. 무엇보다 `val` 자체가
    swc가 모르는 문법이라 "제거 후 파싱 → 위치 보정"이 필요하다.
  - (C) 렉서가 만든 토큰 스트림 위에서 스코프·경로 분석을 직접 한다 — 파서가
    이미 만든 토큰을 재사용하므로 렉싱은 파일당 한 번 그대로, 위치는 처음부터
    원본 바이트 오프셋이다. 대신 "무엇이 선언인가"를 토큰 휴리스틱으로
    판정해야 한다.
- **선택과 근거**: (C). 비용(추가 렉싱 0, 파일당 선형 1패스)과 위치 정확도가
  가장 좋고, 통과 계약의 구조를 건드리지 않는다. 휴리스틱의 위험은 아래
  결정 3의 안전 방향 원칙으로 통제한다. `parser::lex_and_parse`가 토큰을 함께
  돌려주도록 해서 `compile()`에서 재사용한다.

### 결정 2: `val`을 파서가 들어올리는 세그먼트로 만든다 (`Segment::ValModifier`)

- **상황**: `val`은 방출물에 남으면 안 된다. 제거 방식을 정해야 했다.
- **검토한 대안**:
  - (A) codegen이 verbatim 구간을 복사할 때 `val` 바이트를 건너뛴다 — 방출이
    "바이트 그대로 복사"가 아니게 되고, 중첩 Program마다 제외 목록을 들고
    다녀야 한다.
  - (B) 파서가 `val` 키워드를 세그먼트로 들어올리고 codegen은 그 세그먼트에서
    아무것도 방출하지 않는다 — 기존 세그먼트 구조(enum/match/import 지정자와
    동일한 취급)에 그대로 들어맞는다.
- **선택과 근거**: (B). "파서가 구문 여부를 구조적으로 판정하고, codegen은
  AST만 보고 방출한다"는 단계 계약이 유지된다. `--emit-map`의 원본↔출력 매핑도
  자동으로 맞는다(`tests/emit_map.rs`의 불변식 검사로 확인). 스팬에 키워드
  뒤의 공백·탭까지 포함시켜 `val const x` → `const x`가 되게 했다(주석은 보존).

### 결정 3: 수식자 인식 규칙은 "유효한 TS에 존재할 수 없는 두 형태"로 한정

- **상황**: 통과 계약(모든 유효한 TS는 그대로 유효한 `.rl`)을 지키려면
  `val`을 어디서 수식자로 볼지의 규칙이 계약 증명 가능해야 한다.
- **검토한 대안**:
  - (A) `val`을 예약어로 만든다 — `const val = 1;` 같은 기존 코드가 깨진다.
    즉시 탈락.
  - (B) `val` 뒤에 식별자가 오면 수식자 — `(val as User)`, `for (val of xs)`,
    `x = val\nconst y = 1;`(ASI) 같은 유효한 TS가 깨진다.
  - (C) 두 형태로 한정: ① `val` + 같은 줄의 `const|let|var`,
    ② 매개변수 목록 항목 맨 앞(`(`/`,` 직후, TS 매개변수 수식자 뒤)의
    `val` + 같은 줄의 바인딩(식별자·`{`·`[`), 단 뒤 식별자가 연산자 단어
    (`as`/`satisfies`/`is`/`in`/`of`/`instanceof`/`keyof`/예약어)면 제외.
- **선택과 근거**: (C). `IDENT IDENT` 시퀀스는 그 두 자리에서 유효한 TS일 수
  없고(선언 키워드 앞, 매개변수 항목 앞), 유일한 반례인 연산자 단어와 ASI
  줄바꿈을 명시적으로 배제했다. `tests/passthrough.rs`에 `val`을 평범한
  식별자로 쓰는 12가지 형태를 계약 테스트로 고정했다.

### 결정 4: 휴리스틱의 실패는 항상 "덜 잡는 쪽"으로 넘어가게 만든다

- **상황**: 토큰 위에서 "이 `(`가 매개변수 목록인가", "이 선언이 무슨 이름을
  묶는가"를 판정하다 보면 실패 케이스가 반드시 생긴다.
- **검토한 대안**: 실패 시 (A) 에러를 낸다, (B) `val`로 간주한다,
  (C) `val`이 아닌 것으로 간주한다.
- **선택과 근거**: (C). 오탐(false positive)은 **정상 코드를 거부**하므로
  치명적이고, 미탐(false negative)은 검사 하나를 놓칠 뿐이다. 구체적으로:
  - 매개변수 목록 판정 실패 → 그 함수의 `val` 매개변수는 등록되지 않는다
    (본문에서 검사 안 함). 반대로 잘못 판정해 등록되는 이름은 항상
    **비-val**이라 바깥 `val`을 가릴 뿐이다.
  - `val` 스코프는 반드시 정확히 닫히는 범위(대응 `}` 또는 화살표 식의 끝)로만
    만들어, 함수 밖으로 `val`이 새지 않게 했다.
  - 반환 타입이 객체 리터럴인 함수(`function f(val x): { a: number } { ... }`)
    처럼 본문 `{`를 잘못 짚을 수 있는 자리는 타입 브레이스를 건너뛰도록 처리
    (`body_after_params`).

### 결정 5: 호출 시 권한 검사는 "같은 파일 + 이름 선언 + 경로 인자"로 한정

- **상황**: `val` 인자를 일반 매개변수로 넘기는 것을 막으려면 시그니처가
  필요하다. 어디까지 볼 것인가.
- **검토한 대안**: (A) 임의 호출을 전부 막는다 — 외부 함수에 `val` 값을 못
  넘기게 되어 실용성이 사라진다. (B) 타입 정보(`--types` 경로)를 끌어온다 —
  컴파일 단계가 tsc에 의존하게 되어 에러 계층 계약이 무너진다.
  (C) 같은 파일에서 이름으로 선언된 함수(`function f`, `const f = (...) =>`,
  `const f = function`)만 시그니처를 모아 검사한다.
- **선택과 근거**: (C). 스펙이 요구한 예제(`read`/`update`/`process`)를 전부
  커버하면서 외부 함수 호출은 그대로 허용한다. 같은 이름이 서로 다른
  시그니처로 두 번 선언되면 추측하지 않고 검사 대상에서 제외한다. 인자는
  `x`·`x.y.z` 형태의 접근 경로일 때만 판정한다 — 계산된 인자는 권한을 말할 수
  없기 때문이다. 한계는 `language.md` §10.7에 표로 명시했다.

### 결정 6: 변경 메서드는 이름 기준 목록으로 검사한다

- **상황**: `items.push(1)`을 막으려면 수신자의 타입을 알아야 하는데 rlc에는
  타입이 없다.
- **검토한 대안**: (A) 검사하지 않는다(스펙이 허용한 후속 분리),
  (B) 표준 built-in의 대표 변경 메서드 이름 목록으로 검사한다.
- **선택과 근거**: (B). 스펙이 명시적으로 예제(`items.push(1)`,
  `map.set("a", 1)`)를 요구했고, 목록에 있는 이름(`push`/`set`/`add`/...)은
  이를 가진 표준 built-in에서 전부 변경 메서드다. 같은 이름의 사용자 메서드는
  오탐이 될 수 있으나 **`val`을 쓴 코드에서만** 발생하므로 TS 호환 계약과는
  무관하고, 문서에 명시했다.

### 결정 7: `x = v`(바인딩 교체)는 `val`의 검사 대상이 아니다

- **상황**: `val const x`에서 `x = v`를 누가 막는가.
- **선택과 근거**: `const`/`let` 축이 담당한다(스펙의 표와 동일). rlc가
  중복으로 막으면 `val let x`의 재할당까지 막게 되고, `const` 재할당은 tsc가
  이미 정확한 위치로 보고한다. 구현상으로도 "경로 스텝이 0개면 검사하지
  않는다"는 한 줄 규칙으로 떨어진다.

## 작업 내역

- 2026-08-19: 저장소 구조 분석 — `lexer.rs`(토큰), `parser/mod.rs`(토큰 루프),
  `sema.rs`(AST 검사), `codegen/mod.rs`(세그먼트 방출), `lib.rs`(파이프라인)를
  읽고 `val`이 들어갈 자리를 결정(위 의사결정 1·2).
- `src/val.rs` 신설:
  - `modifier_at` — 파서와 검사기가 **공유하는** 구조 판정(결정 3).
  - `modifier_end` — 제거할 스팬(키워드 + 뒤 공백/탭).
  - `check` — `uses_val` 빠른 경로 → `collect_signatures`(시그니처 테이블) →
    `Checker::walk`(스코프 스택 + 변경 경로 + 호출 검사).
  - 경로 파싱(`parse_path`)은 `.p`/`?.p`/`[i]`/`!`를 스텝으로 보고,
    대입 연산자 판정(`assignment_op_at`)은 토큰 **인접성**으로 `==`/`>=`/`!=`
    같은 비교와 `+=`/`**=`/`>>>=`/`??=`를 구분한다.
- `src/ast.rs`: `Segment::ValModifier(Span)` 추가.
- `src/parser/mod.rs`: 토큰 루프에 `val` 분기 추가, `lex_and_parse` 공개,
  `cursor::{dotted_at, find_close_at}`을 `pub(crate)`로 재수출.
- `src/codegen/mod.rs`: `ValModifier`는 아무것도 방출하지 않음.
- `src/sema.rs`·`src/probe.rs`: 새 세그먼트를 무시 목록에 추가.
- `src/lib.rs`: `mod val;`, `compile_mapped`에서 `sema::check` 다음에
  `val::check(source, &tokens)` 실행.
- `src/main.rs`: `rlc help val` 주제 추가.
- 테스트: `tests/compile.rs` +21(방출·에러·스코프·호출·한계),
  `tests/passthrough.rs` +5(식별자 `val`의 통과 계약),
  `tests/emit_map.rs` +1(매핑 불변식), `tests/integration.rs` +2(tsc 타입체크,
  node 실행 결과가 `val` 없는 TS와 동일).
- 문서: `docs/reference/language.md` §10 신설(§10 제한사항 → §11 재번호,
  참조 3곳 갱신), `docs/reference/errors.md` `## val`,
  `docs/reference/cli.md` 주제 표(누락돼 있던 `result`도 함께 수정),
  `docs/ai/rl.md` `## val` + Errors/Checklist, `README.md`, `CLAUDE.md`,
  `docs/design/compiler-architecture.md` §4-1,
  `editors/vscode/syntaxes/rl.tmLanguage.json`(`storage.modifier.val.rl`),
  `editors/vscode/README.md`.
- 검증: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test`(단위 212 / 통과 55 / 통합 72 / emit-map 8 / CLI 16 / sidecar 9 /
  stdlib 3 / doctest 13) 전부 통과.

## 이슈 및 해결

### 이슈 1: `delete x.a`와 `++x.a`가 검사에서 빠졌다

- **증상**: `val const x = ...; delete x.a;`와 `++x.a;`가 통과했다.
- **원인**: 변경 판정을 "경로 **뒤**에 오는 연산자"로만 했는데, 이 둘은
  연산자가 경로 **앞**에 있다.
- **해결**: `check_mutation`에 `mutates` 인자를 추가해, `delete`와 전위
  `++`/`--`를 만난 자리에서 "이미 변경임이 확정된 경로"로 호출하게 했다.
  (`tests/compile.rs::val_forbids_every_mutating_operator`가 22가지 형태를
  한 번에 고정한다.)

### 이슈 2: `match (s) { ... }`가 매개변수 목록으로 오인됐다

- **증상**: match 암 본문에서 스크루티니 `val` 바인딩을 변경해도 에러가 나지
  않았다.
- **원인**: 매개변수 목록 판정이 "`( ... )` 다음에 `{`가 오면 함수 본문"이라
  `match (s) {`가 매개변수 목록으로 잡혔고, 스크루티니 이름 `s`가 **비-val**
  바인딩으로 등록되어 바깥 `val`을 가렸다.
- **원인 파악**: `match (s) { Circle(r) => { s.kind = "Point"; ... } }`가 통과하는
  것을 보고 스코프 등록 지점을 역추적.
- **해결**: `(` 앞 단어가 예약어면(단 `function`/`async`/`catch` 제외) 또는
  `match`면 매개변수 목록이 아니라고 판정. `if`/`while`/`for`/`switch` 등
  제어 머리도 같은 규칙으로 한 번에 배제된다.

### 이슈 3: `val const Some(value) = o else { ... }`가 태그를 바인딩으로 등록했다

- **증상**: let-else 형태에 `val`을 붙이면 바인딩 `value`가 아니라 태그 `Some`이
  `val`로 등록됐다.
- **원인**: 바인딩 이름 수집기가 `Ident` 다음에 오는 `( ... )`를 몰랐다.
- **해결**: `collect_pattern_names`에 "식별자 뒤에 `(`가 오면 괄호 안의 항목이
  실제 바인딩(`name` 또는 `name: alias`)"이라는 분기를 추가. 이 자리(선언 타깃)
  에서 `Ident(`는 rl let-else 패턴뿐이라 다른 형태와 충돌하지 않는다.

### 이슈 4: match 패턴의 `val`은 위치 있는 에러가 되지 못한다

- **증상**: `match (r) { Ok(val user) => ... }`는 패턴이 파싱되지 않아 match가
  통과되고, 방출물에 남은 `match` 텍스트 때문에 **위치 없는** 출력 검증
  에러가 난다.
- **원인**: 패턴 안의 `Ok(val user)`는 구조적으로 화살표 함수 매개변수
  목록(`(val user) => ...`)과 구분되지 않아, 수식자 판정만으로는 "여기는
  패턴이다"를 알 수 없다.
- **해결**: 이번 범위에서는 우회 — 패턴 바인딩의 `val`은 미지원임을
  `language.md` §10.7과 `errors.md`에 명시했다. 남은 부채: match 패턴 문법에
  `val`을 넣는 후속 작업에서 파서가 직접 위치 있는 에러를 내게 한다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`

## 결과

`val`은 컴파일 시점 전용 바인딩 수식자로 동작한다 — 방출물에는 키워드도
런타임도 타입 변환도 남지 않고, 기존 TypeScript의 mutable 의미는 그대로다.

변경 파일: `src/val.rs`(신규), `src/ast.rs`, `src/parser/mod.rs`,
`src/parser/cursor.rs`, `src/codegen/mod.rs`, `src/sema.rs`, `src/probe.rs`,
`src/lib.rs`, `src/main.rs`, `tests/compile.rs`, `tests/passthrough.rs`,
`tests/emit_map.rs`, `tests/integration.rs`, `docs/reference/language.md`,
`docs/reference/errors.md`, `docs/reference/cli.md`, `docs/ai/rl.md`,
`docs/design/compiler-architecture.md`, `docs/design/project-front-end.md`,
`README.md`, `CLAUDE.md`, `editors/vscode/syntaxes/rl.tmLanguage.json`,
`editors/vscode/README.md`.

후속 후보(별도 태스크로 등록 필요):

- match 패턴 바인딩의 `val` (`Ok(val user)`) — 패턴 문법 확장 + 위치 있는 에러.
- 별칭 우회(`const alias = valBinding;`)를 어디까지 따라갈지 — 지금은 비목표.
