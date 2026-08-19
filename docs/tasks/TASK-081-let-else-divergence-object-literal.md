# TASK-081: let-else 발산 판정 — 객체 리터럴 반환 오탐 수정

- **상태**: 완료
- **시작일**: 2026-08-19
- **완료일**: 2026-08-19
- **커밋**: `50420b9`

## 목적

`else` 블록이 객체 리터럴을 반환하면 발산 판정이 실패한다:

```rl
const Some(value: v) = boxed() else { return { kind: "Err", error: "no" }; };
// rlc: let-else: the `else` block must end with a `return`, `throw`, ...
```

`Result`를 반환하는 함수에서 `Err`를 리터럴로 전파하는 것이 가장 자연스러운
형태인데 그게 막힌다. TASK-080 작업 중 발견했다.

## 범위

- 포함: `src/parser/lets.rs`의 `block_diverges`가 문장 경계를 판정할 때
  **식의 중괄호**(객체 리터럴, 화살표 함수 본문)를 블록 문과 구분하도록
  한다. 레퍼런스(`language.md` §6.4, `errors.md`)와 AI 문서(`docs/ai/rl.md`)에
  판정 규칙을 명시한다.
- 제외: 발산 판정을 **의미 분석**으로 바꾸지 않는다 — 구문 검사라는 성격은
  그대로다(`if (c) return a; else return b;`는 여전히 거부). 세미콜론 없는
  (ASI) 스타일도 이번 범위 밖이다(아래 "남은 한계").

## 의사결정

### 결정 1: 문장 경계에서 식의 중괄호를 제외한다 (판정 자체를 바꾸지 않고)

- **상황**: `block_diverges`는 "마지막 최상위 문장의 첫 토큰"을 찾기 위해
  최상위 `;`와 최상위 `}`를 문장 경계로 삼는다. `return { … };`에서는
  객체 리터럴의 `}`가 경계로 잡혀 "마지막 문장"이 뒤따르는 `;` 하나가 되고,
  `;`는 식별자가 아니므로 발산이 아니라고 판정된다.
- **검토한 대안**:
  - **대안 A — 마지막 `;`/`}` 대신 뒤에서부터 훑어 마지막 문장을 찾는다.**
    방향만 바꿀 뿐 같은 모호성(블록 문의 `}`인가 객체 리터럴의 `}`인가)을
    그대로 만난다. 기각.
  - **대안 B — 발산을 의미 분석으로 판정한다** (모든 경로가 반환하는지).
    정확하지만 rlc는 타입·제어 흐름 분석을 하지 않는다는 설계 전제와
    충돌하고, 레퍼런스가 "구문 검사"라고 규범으로 못박고 있다. 기각.
  - **대안 C — 최상위 `{`가 문장을 여는지 그때그때 판정하고, 여는 경우에만
    그 `}`를 문장 경계로 쓴다.** 판정 로직 자체(마지막 문장의 첫 토큰이
    네 키워드 중 하나인가)는 손대지 않는다.
- **선택과 근거**: 대안 C. 고칠 것은 "무엇이 문장을 끝내는가" 하나이고,
  검사의 성격·에러 메시지·문서화된 한계는 전부 그대로 유지된다.

### 결정 2: 여는 `{`의 종류는 "문장 시작 키워드 + 직전 토큰"으로 판정한다

- **상황**: 최상위 `{`가 블록 문의 본문인지 식의 중괄호인지 구분해야 한다.
  JS/TS의 고전적 모호성이라 완전한 판정은 파서를 새로 쓰는 일이다.
- **검토한 대안**:
  - **대안 A — 직전 토큰만 본다.** `return {`(식) vs `) {`(블록)은 갈리지만
    `return match (v) { … };`는 직전이 `)`라 블록으로 오판한다.
  - **대안 B — 현재 문장의 첫 토큰만 본다.** `if (c) return { k: 1 };`에서
    문장이 `if`로 시작하므로 객체 리터럴을 `if`의 블록으로 오판한다.
  - **대안 C — 둘 다 본다**: ① 현재 문장이 블록 본문을 가질 수 있는 키워드
    (`BLOCK_STMT_WORDS`)로 시작하고, ② 직전 토큰이 헤드의 `)`이거나
    식을 여는 키워드가 **아닌** 식별자일 때만 블록이다. `{`가 문장의 첫
    토큰이면(맨 블록) 무조건 블록이다.
- **선택과 근거**: 대안 C. 검토한 형태를 전부 만족한다 —
  `return { … };` / `return match (v) { … };` / `const x = match (v) { … };` /
  `const f = () => { … };`는 식으로, `if (c) { … }` / `try { … } catch (e) { … }` /
  `for (…) { … }` / `function g() { … }` / `class A { … }` / 맨 블록은 문장으로
  판정된다. 확인: `tests/compile.rs`의 세 테스트가 각 형태를 고정한다
  (`let_else_diverges_when_the_return_value_is_an_object_literal`,
  `let_else_divergence_still_sees_block_statements`,
  `let_else_non_diverging_else_ending_in_a_brace_is_still_an_error`).

## 작업 내역

- 2026-08-19: 최소 재현으로 원인을 좁혔다. `else { return "x"; }`는 통과,
  `else { return { k: "x" }; }`는 거부. 세미콜론을 지우면
  (`else { return { k: "x" } }`) 통과하는 것으로, 원인이 "객체 리터럴의 `}`가
  문장 경계로 잡혀 마지막 문장이 `;` 하나가 된다"임이 확정됐다.
- 2026-08-19: `src/parser/lets.rs` — `BLOCK_STMT_WORDS`/`EXPR_BRACE_WORDS`
  상수와 `brace_opens_statement`를 추가하고, `block_diverges`가 최상위 `{`를
  만날 때 종류를 기록해 두었다가 대응하는 `}`가 블록 문의 것일 때만 문장
  경계로 쓰도록 바꿨다.
- 2026-08-19: 임시 파일로 통과/거부 형태를 훑어 확인했다 — 통과: 객체 리터럴
  반환, 앞에 문장이 있는 경우, `return match (…) { … };`, 선언 초기화의
  객체 리터럴, `try/catch` 뒤 `return`, 화살표 본문 뒤 `return`, `class` 선언
  뒤 `return`. 거부(그대로): `log("a");`, `const x = { n: 1 };`,
  `if (c) { return 1; }`, `for (…) { … }`, 맨 블록, 빈 블록.
- 2026-08-19: `tests/compile.rs`에 단위 테스트 3건,
  `tests/integration.rs`에 `Err` 리터럴을 전파하는 런타임 테스트 1건 추가.
- 2026-08-19: `docs/reference/language.md` §6.4에 문장 경계 규칙과 예제를,
  `docs/reference/errors.md`의 해당 에러 행에 같은 규칙을,
  `docs/ai/rl.md`의 let-else 항목에 한 줄로 반영했다.
- 2026-08-19: `cargo fmt --check` / `cargo clippy --all-targets -- -D warnings`
  / `cargo test`(442 통과, 실패 0) / `npm test`(editors/vscode, 68 통과) 통과.

## 이슈 및 해결

### 이슈 1: 문장 경계를 줄이면 세미콜론 없는 코드가 더 나빠질 수 있다

- **증상**: 경계 후보를 줄이는 변경이라, 예컨대 `x = { a: 1 }` 다음 줄에
  `return 1`이 오는(세미콜론 없는) 코드는 이전엔 객체 리터럴의 `}`가 경계로
  잡혀 우연히 `return`을 찾았지만 이제는 못 찾는다.
- **원인**: 이 검사는 애초에 개행을 문장 경계로 보지 않는다(ASI 미지원).
  세미콜론 없는 스타일에서는 `log(1)` 다음 줄의 `return 2`도 이전부터
  못 찾았다 — 즉 우연히 맞던 경우가 하나 줄어드는 것이지 지원되던 스타일이
  깨지는 게 아니다.
- **해결**: 범위를 넓히지 않고 한계로 남겼다. 남은 부채: ASI(개행) 경계
  지원. 필요해지면 별도 태스크로 다룬다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` — 442 통과, 실패 0 (통합 테스트 포함)
- [x] `npm test` (editors/vscode) — 68 통과, 실패 0, 8 skip(선언 방출 사이드카)

## 결과

`else` 블록이 객체 리터럴(또는 화살표 함수)을 값으로 쓰는 문장으로 끝나도
발산으로 인정된다. 판정의 성격(구문 검사)과 에러 메시지·위치는 그대로고,
발산하지 않는 블록은 여전히 같은 에러로 거부된다.

변경 파일:

- `src/parser/lets.rs` — `brace_opens_statement`와 두 단어 목록 추가,
  `block_diverges`가 블록 문의 `}`만 문장 경계로 사용
- `tests/compile.rs` — 단위 테스트 3건
- `tests/integration.rs` — 런타임 테스트 1건
- `docs/reference/language.md` / `docs/reference/errors.md` / `docs/ai/rl.md`
  — 문장 경계 규칙 명시

남은 한계: 세미콜론 없는(ASI) 스타일은 이 검사가 문장을 구분하지 못한다
(이번 변경 이전부터). 필요해지면 별도 태스크.
