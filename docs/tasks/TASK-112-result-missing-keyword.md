# TASK-112: 키워드를 빠뜨린 `result` 바인딩 진단

- **상태**: 완료
- **시작일**: 2026-08-20
- **완료일**: 2026-08-20
- **커밋**: `b9ab5fd`

## 목적

[TASK-101](./TASK-101-rust-parity-review.md) GAP-6의 "미청구(missed-claim)
진단" 항목. `result` 블록에서 선언 키워드를 빠뜨린 바인딩

```rl
const out = result {
  const a <- readNum();
  b <- readNum();      // ← `const` 없음
  a + b
};
```

의 대접이 두 가지로 갈려 있었고, 둘 다 나빴다:

1. **블록이 이미 판별된 경우**(다른 문장에 진짜 바인딩이 있음): 그 줄은
   평범한 TS로 통과해 `b < -readNum();` **비교**로 방출된다. 컴파일 성공,
   종료 코드 0, 아무 말도 없음 — 조용한 오역이다.
2. **그 줄이 유일한 바인딩 후보인 경우**: 블록이 판별되지 않아 통째로 통과한
   뒤 verify가 `generated TypeScript failed to parse: Expected a semicolon
   (line 1, col 20 of the generated output)`이라며 **생성물 좌표**를 들이민다.

`|>`·`if let`은 같은 상황에서 rl 좌표의 에러를 준다. 그 대접을 맞춘다.

## 범위

- 포함: 파서의 키워드 없는 바인딩 인식, 보고 조건, sema 에러, 계약 테스트,
  레퍼런스·AI 문서·CHANGELOG.
- 제외: 구조 분해 바인딩(`{ a, b } <- f();`)의 키워드 누락 — 아래 결정 3.
  블록 밖의 `<-`(그건 `result` 블록 문제가 아니다).

## 의사결정

### 결정 1: 보고 조건은 "그 텍스트가 TypeScript일 수 없음"이 확정된 곳

- **상황**: `b <- readNum();`는 **그 자체로 유효한 TypeScript**다 —
  `b < -readNum();`, 즉 비교식 문장. 무조건 에러로 만들면 통과 계약(모든 유효한
  TS는 그대로 유효한 `.rl`)이 깨진다.
- **검토한 대안**:
  - (a) 언제나 에러 — 계약 위반. 실제로 `function f(): result { a <- b; }`
    같은 유효한 TS가 깨진다(아래 확인 참조).
  - (b) 경고 — rl에는 경고 계층이 없다(`errors.md`는 에러만 규정한다).
  - (c) **텍스트가 TypeScript일 수 없다는 것이 이미 확정된 곳에서만** 에러.
- **선택과 근거**: (c). 이것이 `|>`·`if let`·`val`이 이미 쓰는 판단 방식이고
  (`language.md` §8.4의 "판별하는 것은 바인딩"), 계약을 건드리지 않는다.

### 결정 2: 확정되는 두 자리

- **① 이미 판별된 블록 안.** 블록이 rl `result` 블록으로 청구되려면 최소 하나의
  진짜 바인딩(`const x <- f();`)이 있어야 하고, 선언 키워드 뒤의 `<-`는 유효한
  TypeScript일 수 없다(선언자에 초기화 `=`가 필요하다). 그러므로 **판별된 블록이
  있는 파일은 유효한 TypeScript가 아니다** — 그 안에서 무엇을 보고하든 계약과
  무관하다. `results.rs` 헤더가 이미 같은 논증으로 "바인딩이 있는데 파싱에
  실패한 블록은 통과시킬 수 없다"고 적어 뒀다.
- **② 식이 시작하는 자리의 같은 줄 `result {`.** 판별된 바인딩이 하나도 없는
  경우(위 2번)를 잡으려면 다른 근거가 필요하다. 두 조건을 **모두** 요구한다:
  - `{`가 `result`와 **같은 줄**일 것. 줄바꿈이 있으면 ASI로
    `result;` + 블록 문이 되어 유효한 TS다 — `type X = result` + 다음 줄 블록도
    마찬가지다.
  - `result` 앞 토큰이 **식이 시작하는** 것일 것(`=` 대입, `(`, `[`, `,`, `=>`,
    `return`). 이것이 `function f(): result { ... }`(반환 타입 주석),
    `class result { }`, `interface result { }` 같은 "식별자 다음에 같은 줄
    `{`가 합법인" 형태를 전부 배제한다.
  - 두 조건이 각각 필요하다: 같은 줄만 요구하면 `function f(): result {`가,
    앞 토큰만 요구하면 `type X = result` + 다음 줄 블록이 새어 든다. 둘 다
    passthrough 테스트로 고정했다.

### 결정 3: 인식하는 모양을 좁게 유지한다

- **상황**: 어디까지를 "키워드를 빠뜨린 바인딩"으로 볼 것인가.
- **선택과 근거**: **맨 이름 하나** + 붙여 쓴 `<-` + 식 + `;`만. 그 이유:
  - `obj.x <- f();`, `{ a, b } <- f();` 같은 모양은 실수 빈도가 낮은 반면
    비교식으로 읽힐 여지는 그대로다. 키워드 경로는 키워드가 닻이 돼 주지만
    여기서는 그것이 없다.
  - 키워드 경로가 이미 배제하는 두 꼬리(`<-` 뒤에 짝 없는 `>`가 남는 제네릭
    타입 인자, `;` 누락으로 다음 문장까지 흘러간 run)를 같은 규칙으로 배제한다.
  - 비교를 쓰려던 사람에게는 **공백**이라는 탈출구가 있다: `b < -f();`.
    이것은 이미 언어의 규칙이다(`<-`는 붙여 쓴 두 바이트).

### 결정 4: 보고는 sema에서, 파서는 무오류로

기존 `stray_pipes`/`stray_if_lets`/`stray_results`와 같은 모양을 따른다:
파서는 `Program.result_missing_kw`에 바이트 오프셋만 모으고, `sema::check`가
문안을 만든다. 파이프라인 단계 분리(CLAUDE.md)를 그대로 지킨다.

## 작업 내역

1. `src/parser/results.rs`
   - `BindRun::NoKeyword { at }`, `Attempt::MissingKeyword(at)` 추가.
   - `scan_bind`가 선언 키워드가 아닐 때 `scan_no_keyword`로 넘긴다(전에는
     즉시 `NotBind`).
   - `scan_no_keyword` — 결정 3의 좁은 모양만 인식.
   - `no_keyword_is_certain(cur, open)` — 결정 2의 ②. `{` 토큰 인덱스에서
     `result` 토큰과 그 앞 토큰을 보고 판정한다.
   - `parse_result_block`이 후보를 `missing`에 모아 두고, 블록을 다 읽은 뒤
     `saw_bind || no_keyword_is_certain(...)`일 때만 `MissingKeyword`를 낸다.
     값 자리(마지막 run)의 키워드 없는 run은 `;`가 없으므로 애초에 인식되지
     않는다.
2. `src/ast.rs` — `Program.result_missing_kw`.
3. `src/parser/mod.rs` — 새 variant 처리.
4. `src/sema.rs` — 에러 문안:
   `` `result` binding is missing its declaration keyword (write `const <binding> <- <expression>;`, or `let`/`var`) ``
5. `tests/compile.rs` — `a_binding_without_a_declaration_keyword_is_a_located_error`
   (두 경우 모두, 행·열까지 확인).
   `tests/passthrough.rs` — `a_keyword_less_binding_shape_is_only_claimed_where_typescript_cannot_reach`
   (반환 타입 `result`, `type X = result` + 다음 줄 블록, ASI 형태, `class result`).
6. 문서: `errors.md`(표 + 예시), `language.md` §8.4(판별 표 + 설명),
   `docs/ai/rl.md`, `CHANGELOG.md`, `rust-parity-analysis.md` GAP-6·P5 표시.

## 이슈 및 해결

- **증상**: 처음 설계에서는 값 자리(마지막 run)의 검사
  `if run_start < close && !matches!(scan_bind(...), NotBind) { return Malformed }`
  가 `NoKeyword`도 Malformed로 만들어, `result\n{ a <- b }`(유효한 TS)를
  에러로 만들 뻔했다. **원인**: 그 검사는 `saw_bind` 판정보다 **먼저** 온다.
  **해결**: 그 자리에서는 `NoKeyword`를 `NotBind`처럼 무시하도록 명시적
  match로 바꿨다. 실제로는 값 자리 run에 `;`가 없어 `scan_no_keyword`가
  애초에 `NotBind`를 주지만, 순서 의존을 코드에 남기지 않았다.
- **증상**: 앞 토큰만 보는 판정으로는 `type X = result` + 다음 줄 블록이
  에러가 됐다(`=`가 대입으로 읽힌다). **해결**: 같은 줄 조건을 함께 요구
  (결정 2). 두 조건 각각의 반례를 passthrough 테스트로 고정했다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` (tsgo 있음)

손 확인:

```sh
rlc r.rl   # rlc: r.rl:3:3: `result` binding is missing its declaration keyword ...
```

## 결과

- 조용한 오역(`b < -readNum()`)이 사라졌다.
- 생성물 좌표를 들이미는 verify 에러 대신 rl 좌표의 에러가 나온다.
- GAP-6의 남은 항목: 중첩 패턴 내부 소진성 v2, let-else·`if let`의 or-패턴,
  도달 불가 arm(다음 태스크).
