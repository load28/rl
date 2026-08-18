# TASK-063: `flow` — 함수 합성 (포인트프리 파이프라인)

- **상태**: 완료
- **시작일**: 2026-08-18
- **완료일**: 2026-08-18
- **커밋**: `a59d8c6`

## 목적

파이프라인 `|>`는 **값**을 함수들에 흘려보낸다. 여기에 짝이 되는 구문으로,
**값 없이 함수만 이어 붙여 새 함수를 만드는** `flow`를 추가한다
(Ramda/fp-ts의 `flow`). 파이프가 `x |> f |> g`라면 flow는 `flow |> f |> g` —
같은 체인을 값이 도착하기 전에 미리 조립해 두는 형태다.

## 범위

- 포함: `flow` 구문(파서·sema·codegen), 합성 헬퍼 `$rl_fl` 방출, 세 계층
  테스트, 레퍼런스/에러/AI/설계 문서 갱신, VSCode 문법 강조와 스니펫.
- 제외: 입력 타입 주석 문법(`flow<T> |> ...`), `?.` 스텝, 첫 스텝의 제네릭
  자동 구체화(TS의 고차 추론 한계 — 결정 4), 예제 파일 추가.

## 의사결정

### 결정 1: 합성 문법은 파이프라인 head 자리의 `flow` 키워드

- **상황**: 합성을 어떤 표면으로 낼지 정해야 했다. 통과 계약(모든 유효 TS는
  유효 rl)을 지키려면 **유효 TS에 존재할 수 없는 형태**이거나, 유효 TS일 수
  없는 문맥(= `|>`가 이미 있는 영역)에서만 클레임해야 한다.
- **검토한 대안**:
  - (A) head 없는 파이프라인 `|> f |> g`. 장점: 새 이름이 필요 없고 "값이
    빠진 파이프"라는 의미가 형태에 그대로 드러난다. 단점: head를 **빠뜨린
    오타**가 에러 없이 "값 파이프 → 함수 값"으로 의미를 바꾼다. 여러 줄
    파이프라인은 이미 `|>`로 줄을 시작하므로 눈으로도 구분이 어렵다.
  - (B) 라이브러리 함수 `flow(f, g, h)`. 장점: 컴파일러 변경 0. 단점:
    오버로드 arity 한계와 고차 제네릭 붕괴 — `|>`를 연산자로 만든 이유
    (설계 문서 §2)가 그대로 재현된다.
  - (C) 합성 연산자 `f >> g`. `>>`는 유효 TS의 시프트 연산자라 **통과 계약
    위반**. 검토 즉시 기각.
  - (D) head 자리의 `flow` 키워드 `flow |> f |> g`.
- **선택과 근거**: (D). 값이 놓일 자리에 이름이 놓이므로 파이프와 형태가
  1:1로 대응해 읽는 사람이 두 구문을 같은 규칙으로 이해한다. 파서는 "head
  토큰이 `flow` 하나인가"만 보면 되고(`pipes.rs::is_flow_head`), (A)와 달리
  head를 빠뜨리면 기존 "빈 head" 경로로 걸린다. 통과 계약도 무손상 —
  합성으로 클레임되는 코드에는 반드시 `|>`가 있고, `|>`가 있는 파일은 애초에
  유효한 TS가 아니다(`tests/passthrough.rs::flow_is_an_ordinary_identifier_in_typescript`
  가 fp-ts `flow` import·변수·타입 별칭·속성 접근이 그대로 통과함을 고정).

### 결정 2: `flow`는 예약어가 아니라 문맥 키워드

- **상황**: `flow`는 흔한 식별자다(fp-ts가 같은 이름의 함수를 export한다).
  예약어로 만들면 통과 계약이 깨진다.
- **검토한 대안**: ① 예약어 등재(§1.1 목록에 추가) — 계약 위반이므로 불가.
  ② head가 정확히 `flow` 식별자 **한 토큰**일 때만 합성.
- **선택과 근거**: ②. 점 접근(`o.flow`), 호출(`flow()`), 괄호(`(flow)`)는
  토큰이 둘 이상이라 자동으로 값 head가 된다 — `flow` 변수를 파이프에
  흘리는 탈출구가 규범으로 존재한다(`(flow) |> f`, match 스크루티니 괄호와
  같은 성격). `tests/compile.rs::flow_is_a_contextual_keyword_only_at_a_pipeline_head`
  가 세 형태를 모두 고정한다.

### 결정 3: 방출은 이항 합성 헬퍼 `$rl_fl`의 중첩

- **상황**: 합성 결과가 순수 TS여야 하고(계약 2), rlc가 방출한 코드 때문에
  tsc 에러가 나면 안 된다.
- **검토한 대안** (셋 다 실제 tsc `--strict`로 검증, 스크립트는
  `probe.ts`/`probe2.ts`로 작성해 확인한 뒤 integration.rs로 이관):
  - (A) 화살표 방출 `($rl_v) => $rl_ap($rl_ap($rl_v, f), g)`. **탈락** —
    파라미터에 문맥 타입이 없어 `noImplicitAny`에서 TS7006. rlc가 방출한
    코드가 tsc 에러를 내므로 **에러 계층 계약 위반**이다. 파라미터 타입을
    `Parameters<typeof f>[0]`로 채우는 변형도 검토했으나 타입 트릭 금지에
    걸린다.
  - (B) 가변 인자 헬퍼 `$rl_flow(...fns)`. **탈락** — 오버로드 arity 한계와
    중간 단계 타입 소실이 라이브러리 `flow`과 동일하게 재현된다.
  - (C) 이항 헬퍼 중첩
    `function $rl_fl<A extends unknown[], B, C>(f: (...a: A) => B, g: (b: B) => C): (...a: A) => C`.
- **선택과 근거**: (C). 중첩이므로 **단계 수 제한이 없고**, 각 호출이 앞
  스텝의 반환 타입에서 구체적으로 추론되며, `A extends unknown[]` 덕분에
  **첫 스텝의 다인자 arity가 보존된다**(`flow |> add |> double`이
  `(a: number, b: number) => number`). 헬퍼는 `$rl_ap`과 같은 성격의
  생성물로 파일당 한 번, 파일 끝에 방출한다(함수 선언은 호이스팅되므로 원본
  행 위치가 유지된다).
  - 검증: `tests/integration.rs::flow_composition_infers_input_from_its_first_step`
    (커링 콤비네이터·메서드 스텝이 주석 없이 추론),
    `flow_composition_keeps_the_first_step_arity`,
    `flow_composition_runs_left_to_right_when_called`(호출 전에는 아무것도
    실행되지 않고, 호출 시 좌→우).

### 결정 4: 입력 타입은 첫 스텝이 정한다 — 한계를 규범으로 명시

- **상황**: 값이 없으므로 입력 타입을 정할 근거는 첫 스텝뿐이다. 첫 스텝이
  제네릭 함수나 커링 콤비네이터면 타입 인자가 `unknown`으로 붕괴한다
  (tsc 재현: `probe2.ts` → `TS18046: 'x' is of type 'unknown'`,
  `TS2345: '<T>(v: T) => T' is not assignable to '(v: unknown) => number'`).
- **검토한 대안**: ① 입력 타입 주석 문법 `flow<T> |> ...` 도입 — 파서가
  `<`를 제네릭 인자로 해석해야 하고(구조 파싱과 충돌), 화살표 첫 스텝으로
  이미 표현 가능하므로 과잉. ② 타입 트릭(`Parameters<...>`)으로 우회 —
  계약 위반. ③ 한계를 인정하고 탈출구를 문서화.
- **선택과 근거**: ③. TS가 고차 위치의 제네릭을 추론하지 못하는 한계라
  라이브러리 `flow`도 동일하다. 탈출구는 타입 인자 명시
  (`flow |> wrap<number> |> .length` — tsc로 통과 확인)와 주석 화살표
  (`flow |> ((s: string) => s.trim()) |> ...`). `language.md` §7.5와 §9
  제한사항, `docs/ai/rl.md`에 명시했다.

### 결정 5: 첫 스텝은 메서드 스텝이 될 수 없다 (sema 에러)

- **상황**: `flow |> .trim()`을 어떻게 처리할지. 메서드 스텝은 방출 시
  `(($rl_v) => ($rl_v).trim())` 화살표가 되는데, 첫 스텝에는 `$rl_v`를
  문맥으로 타이핑해 줄 앞 스텝이 없다.
- **검토한 대안**: ① 허용하고 `<A>($rl_v: A) => ...`로 방출 — `A`에 멤버가
  없어 tsc 에러가 rlc 방출 코드에서 난다(계약 위반). ② 구조 파싱 실패로
  떨어뜨려 일반 `|>` 에러로 수렴 — 이유를 알려주지 못한다. ③ 클레임하되
  sema가 전용 메시지로 보고.
- **선택과 근거**: ③. 위치(첫 스텝의 `.`)와 해법을 함께 준다. 두 번째
  스텝부터는 헬퍼의 인자 위치가 문맥을 주므로 메서드 스텝이 정상 동작한다.

## 작업 내역

- 2026-08-18: 기존 파이프라인 구현(`parser/pipes.rs`, `codegen/mod.rs::emit_pipe`,
  `sema.rs`)과 설계 문서 `docs/design/pipeline-operator.md`를 읽고 확장 지점을
  확인했다.
- 2026-08-18: 방출 형태 검증을 먼저 했다 — 스크래치에 `probe.ts`/`probe2.ts`/
  `probe3.ts`를 만들어 `tsc --strict --noEmit`으로 (A)(B)(C) 후보와 한계
  케이스(제네릭 첫 스텝, 커링 첫 스텝, 타입 인자 명시, 중첩 합성)를 돌렸다.
  결과는 결정 3·4에 기록.
- 2026-08-18: 구현.
  - `src/ast.rs`: `PipeExpr::head`를 `Option<Program>`으로 (합성이면 `None`).
  - `src/parser/pipes.rs`: `is_flow_head()` — head 토큰이 정확히 하나이고
    식별자 `flow`일 때 합성으로 판정, 그 경우 head를 파싱하지 않는다.
  - `src/sema.rs`: 합성의 첫 스텝이 메서드 스텝이면 위치와 함께 에러,
    head가 있을 때만 head를 순회.
  - `src/codegen/mod.rs`: `emit_flow()` — 첫 스텝을 시작으로 이후 스텝마다
    `$rl_fl(acc, step)` 중첩, 메서드 스텝은 `(($rl_v) => ($rl_v)<체인>)`.
    `used_flow` 플래그로 파일당 한 번 헬퍼 방출. 스텝이 하나면 그 스텝
    자체이므로 헬퍼를 쓰지 않는다.
  - `src/main.rs`: `rlc help` 토픽 `pipe`의 별칭에 `flow` 추가.
- 2026-08-18: 테스트 추가 — `tests/compile.rs` 9개(중첩 방출, 메서드 스텝,
  단일 스텝, 헬퍼 1회, 헬퍼 미방출, 문맥 키워드 3형태, 표현식 위치, 괄호
  화살표 스텝, 첫 스텝 메서드 에러, 빈 스텝 에러), `tests/passthrough.rs` 1개,
  `tests/integration.rs` 4개(추론·arity·입력 타입 불일치·실행 순서).
- 2026-08-18: 문서 — `docs/reference/language.md` §7.1 문법·§7.5 신설·§9
  제한사항, `docs/reference/errors.md` 파이프라인 표,
  `docs/design/pipeline-operator.md` §11(대안 비교표 포함),
  `docs/ai/rl.md` `## |>` 절, `CHANGELOG.md`, `README.md`, `CLAUDE.md`
  (개요 문장과 아키텍처 맵의 `pipes.rs` 설명).
- 2026-08-18: 에디터 — `editors/vscode/syntaxes/rl.tmLanguage.json`에
  `|>` 앞의 `flow`만 키워드로 칠하는 규칙, `server/src/server.ts`에 `flow`
  스니펫.
- 2026-08-18: 게이트 — `cargo fmt --check`, `cargo clippy --all-targets --
  -D warnings`, `cargo test`(tsc·node 있으므로 통합 테스트 포함) 모두 통과.

## 이슈 및 해결

### 이슈 1: 첫 방출 후보(화살표)가 rlc 코드에서 tsc 에러를 냈다

- **증상**: `($rl_v) => ...` 형태로 방출하면 `--strict`에서
  `TS7006: Parameter '$rl_v' implicitly has an 'any' type`.
- **원인**: 합성에는 입력 값이 없어 파라미터에 문맥 타입이 붙지 않는다.
  타입을 채우려면 `Parameters<typeof f>[0]` 같은 타입 트릭이 필요한데
  설계 계약이 금지한다.
- **해결**: 이항 헬퍼 `$rl_fl` 중첩으로 바꿨다(결정 3). 파라미터를 rlc가
  만들지 않으므로 문제가 사라지고, 메서드 스텝의 화살표는 헬퍼의 인자
  위치라 문맥 타입을 받는다.

### 이슈 2: 헬퍼 문자열에 공백이 끼어 들어갔다

- **증상**: 방출된 `$rl_fl` 선언이
  `... (b: B) => C):              (...a: A) => C ...`처럼 공백 덩어리를
  포함했다.
- **원인**: 편집 스크립트(Python heredoc) 안에서 줄을 이은 백슬래시가
  Python 단계에서 소비돼, Rust 소스에는 `\`+개행이 아니라 들여쓰기 공백이
  그대로 담긴 문자열 리터럴이 들어갔다.
- **해결**: Rust 소스의 리터럴을 한 줄로 고쳐 다시 방출을 확인했다
  (`cargo run -- -p --no-banner`). 회귀는 `tests/compile.rs::
  flow_emits_nested_composition_helper_calls`가 헬퍼 선언 전문을 검사한다.

### 이슈 3: 실행 순서 테스트의 기대값이 어긋났다

- **증상**: `flow_composition_runs_left_to_right_when_called`가
  `left: [" | 10 | s1,s2"] right: ["| 10 | s1,s2"]`로 실패.
- **원인**: 합성 시점에는 아무 스텝도 실행되지 않으므로 첫 `order.join(",")`
  이 빈 문자열이고, `console.log`의 구분 공백이 앞에 남는다 — 즉 테스트가
  검증하려던 성질이 그대로 나타난 것이고 기대 문자열이 틀렸다.
- **해결**: 기대값을 `" | 10 | s1,s2"`로 고치고 "호출 전에는 아무것도
  실행되지 않는다"는 주석을 달았다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` — 281 passed / 0 failed (tsc·node 존재, 통합 테스트 포함)

## 결과

- 컴파일러: `src/ast.rs`, `src/parser/pipes.rs`, `src/sema.rs`,
  `src/codegen/mod.rs`, `src/main.rs`.
- 테스트: `tests/compile.rs`(+9), `tests/passthrough.rs`(+1),
  `tests/integration.rs`(+4).
- 문서: `docs/reference/language.md`, `docs/reference/errors.md`,
  `docs/design/pipeline-operator.md`, `docs/ai/rl.md`, `CHANGELOG.md`,
  `README.md`, `CLAUDE.md`.
- 에디터: `editors/vscode/syntaxes/rl.tmLanguage.json`,
  `editors/vscode/server/src/server.ts`.

후속 후보(등록하지 않음): 입력 타입 주석 문법(`flow<T>`), `?.` 시작 스텝,
std에 합성 친화 콤비네이터 추가.
