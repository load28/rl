# TASK-071: `val` 변경 메서드 판정을 타입 기반으로 — 이름 기준 오탐 제거

- **상태**: 완료
- **시작일**: 2026-08-19
- **완료일**: 2026-08-19
- **커밋**: `221453b`

## 목적

`val`의 변경 메서드 검사가 **메서드 이름만** 보고 판정하기 때문에(TASK-070 결정 6),
`set`/`add`/`push` 같은 이름을 가진 **사용자 정의 immutable API**가 오탐으로
막힌다. `val`은 opt-in 안전 기능이므로 오탐 비용이 미탐 비용보다 크다. 이름 기준
판정을 걷어내고, 타입 정보를 실제로 얻을 수 있는 `rlc --types` 경로에서만
확실하게 식별된 built-in 수신자에 한해 변경 메서드를 잡는다.

## 범위

- 포함:
  - `src/val.rs`에서 이름 기준 **최종 판정** 제거 (문법적 mutation 검사는 그대로).
  - `val` 경로 위의 메서드 호출을 **프로브(질문)** 로 수집하는 공개 API
    (`rlc::val_method_calls`) 추가 — 리터럴 match의 `rlc::literal_matches`와 대칭.
  - `rlc --types` 경로에서 TypeScript TypeChecker로 수신자의 메서드 심볼을
    해석해 `Array`/`Map`/`Set`/`WeakMap`/`WeakSet`/TypedArray의 알려진 변경
    메서드만 에러로 보고.
  - 회귀 테스트(단위 + `--types` CLI), 레퍼런스/AI 문서 갱신.
  - 작업 중 드러난 별개의 `val` 오탐 하나(선언자 스캔이 `Map<string, number>`의
    쉼표를 선언자 구분자로 읽던 문제) 수정 — 같은 "오탐 제거" 목적이고 새 회귀
    테스트를 막고 있었다 (아래 이슈 2).
- 제외:
  - 자체 타입 시스템, 메서드 본문 분석, 이펙트 추론, 반환 타입 기반 추론,
    라이브러리 mutable API 데이터베이스.
  - 기본 경로에서 초기화식(`= []`, `= new Map()`)으로 수신자 타입을 문법적으로
    확정하는 보완 검사 → 후속 태스크로 분리 (아래 결정 4).
  - LSP(`editors/vscode`)에 타입 기반 `val` 진단 배선 → 후속 태스크로 분리.

## 의사결정

### 결정 1: 이름 기준 목록은 "판정"에서 "질문 후보 필터"로 격하한다

- **상황**: TASK-070 결정 6은 타입이 없다는 이유로 이름 목록(`push`/`set`/...)을
  최종 판정에 썼다. 이 목록을 완전히 지우면 `--types` 경로에서 모든 `val` 경로
  메서드 호출을 host로 보내야 하고, 지우지 않으면 오탐이 남는다.
- **검토한 대안**: (A) 목록을 완전히 삭제하고 모든 메서드 호출을 프로브로 보낸다.
  (B) 목록을 남기되 **프로브 후보 축소**에만 쓰고 판정은 심볼로 한다.
- **선택과 근거**: (B). 목록 밖 이름은 어차피 host가 mutation으로 판정하지 않으므로
  결과가 동일하고, JSON 페이로드와 host의 노드 탐색 횟수만 줄어든다. 오탐은
  "이름이 목록에 있다 → 에러"라는 마지막 고리를 끊는 것으로 사라지므로 (B)로도
  목적이 완전히 달성된다. 대신 Rust 쪽 목록은 host가 판정하는 이름의 **상위집합**
  이어야 한다는 계약을 두 파일 주석에 명시했다.

### 결정 2: 프로브는 `probe.rs`가 아니라 `val.rs`에서 만든다

- **상황**: 리터럴 match 프로브는 AST 워커(`probe.rs`)가 만든다. `val` 프로브도
  같은 자리에 두는 것이 대칭적으로 보인다.
- **검토한 대안**: (A) `probe.rs`에 AST 기반 수집기를 추가한다. (B) `val.rs`의
  스코프 워커를 재사용해 수집 모드를 추가한다.
- **선택과 근거**: (B). `val` 바인딩과 그 접근 경로는 전부 **통과 영역의 TS**라
  AST에는 불투명한 바이트 범위로만 존재한다(`ast::Segment::Verbatim`). 어떤
  식별자가 `val` 바인딩인지는 `val.rs`의 스코프/섀도잉 추적 없이는 알 수 없으므로
  (A)는 그 로직을 통째로 복제해야 한다. `Checker`에 수집 싱크(`calls`)를 달아
  같은 워크를 두 모드로 쓰는 것이 최소 변경이다.

### 결정 3: built-in 판정은 타입 이름이 아니라 **메서드 심볼의 선언 위치**로 한다

- **상황**: "수신자가 `Map`인가"를 host에서 어떻게 확인할 것인가.
- **검토한 대안**: (A) `checker.typeToString(type)`이 `Map<...>`로 시작하는지 본다.
  (B) 프로퍼티 심볼의 선언이 TypeScript 기본 lib 파일에 있고, 그 선언을 감싼
  인터페이스 이름이 `Map`이며 메서드 이름이 그 인터페이스의 변경 메서드인지 본다.
- **선택과 근거**: (B). (A)는 사용자가 `class Map`을 정의하면 다시 오탐이 되고,
  제네릭/유니언/`this` 타입 표기에 흔들린다. (B)는 `program.isSourceFileDefaultLibrary()`
  로 "TypeScript 자신이 선언한 것"임을 확인하므로 사용자 정의 타입과 절대 섞이지
  않고, 유니언 수신자처럼 선언이 여러 개인 경우 "전부 built-in 변경 메서드일 때만"
  이라는 보수적 조건으로 자연히 판단 불가 → 허용이 된다. 반환 타입은 아예 보지
  않으므로 "`this`를 반환하니 mutation" 같은 추론도 구조적으로 불가능하다.

### 결정 4: 진단 위치는 경로의 루트, 문구에는 수신자를 넣는다

- **상황**: 새 에러 메시지를 어디에 어떤 문구로 낼 것인가.
- **검토한 대안**: (A) 기존 문구 유지(`cannot call mutating method \`set\` through
  val binding \`m\``). (B) 수신자를 넣는다(`... method \`set\` of built-in \`Map\` ...`).
- **선택과 근거**: (B). 이번 변경의 핵심은 "이름이 아니라 수신자가 근거"라는
  것이고, 메시지가 그 근거를 말하면 사용자 정의 `set`이 통과하는 이유가 바로
  읽힌다. 위치는 기존과 같이 경로의 루트 식별자다 — `s.u.p.tags.push(...)`에서
  문제의 주체는 `s`다.

### 결정 5: 기본 `rlc` 경로에서는 메서드 호출을 아예 판정하지 않는다

- **상황**: 이름 기준을 없애면 `rlc`/`rlc --check`(에디터 진단 경로)에서
  `items.push(1)`이 통과한다. 사용자 체감 보장이 줄어든다.
- **검토한 대안**: (A) 제안대로 typed 경로에만 둔다. (B) `val const xs: T[] = []`
  처럼 선언에서 수신자 타입이 문법적으로 확정되는 경우는 기본 경로에서도 막는다.
- **선택과 근거**: (A). (B)는 "이름 추측"이 아니라 "문법적 증명"이라 원칙과
  충돌하지는 않지만, `Map`이 사용자 클래스로 섀도잉된 경우 배제 등 별도 판정
  로직이 필요해 이번 태스크 안에서 "무엇이 증명이고 무엇이 추측인가"의 경계를
  흐린다. 보장 축소는 문서(`language.md` §10.3/§10.7)에 명시하고, (B)는 후속
  검토 대상으로 남긴다.

## 작업 내역

- 2026-08-19: 현재 구현 분석 — 이름 기준 판정은 `val.rs`의 `is_mutating_method()`와
  `check_mutation()` 후반부 두 곳뿐임을 확인. `--types` 파이프라인
  (`probe.rs` → `main.rs::literal_checks` → `types_job` → `types_host.mjs`
  → `literalMissing` 보고)이 그대로 재사용 가능한 형태임을 확인.
- 2026-08-19: `src/val.rs` — `is_mutating_method` → `is_builtin_mutator_name`
  (프로브 후보 필터로 격하, 주석에 상위집합 계약 명시). `Path`에
  `last_prop_tok`(메서드 이름 토큰) 추가. `check()`/`method_calls()`가 공유하는
  `run(src, tokens, calls)`로 워크를 일반화 — `calls`가 있으면 프로브 모드로,
  아무것도 보고하지 않고 `ValMethodCall`을 모은다. `check_mutation`의 이름 기준
  에러 분기를 프로브 수집으로 교체.
- 2026-08-19: `src/lib.rs` — `pub fn val_method_calls(source) -> Vec<ValMethodCall>`
  공개 (doctest 2개: 수집되는 것과 수집되지 않는 것).
- 2026-08-19: `src/main.rs` — `ValCheck`/`val_checks()`(방출 매핑으로 메서드 이름을
  가상 모듈의 UTF-16 범위로 변환) → `types_job`의 `valChecks` → 호스트 응답의
  `valMutations` → `.rl` 원본 위치로 보고.
- 2026-08-19: `src/types_host.mjs` — `BUILTIN_MUTATORS` 표와 `checkValMutations()`/
  `builtinMutator()` 추가. 판정은 `checker.getSymbolAtLocation(메서드 이름)` →
  모든 선언이 `program.isSourceFileDefaultLibrary()`인 인터페이스 선언이고 그
  인터페이스가 표에 있으며 메서드가 그 인터페이스의 변경 메서드일 때만.
- 2026-08-19: 수동 e2e 확인 — 임시 프로젝트에서
  `rlc --types src`가 `Map#set`/`Set#add`/`Array#push`/`Uint8Array#fill`/
  `WeakMap#set`/`val` 매개변수의 `Array#sort`/`Map | Set`의 `delete`를 보고하고,
  `Query#set`·`Collection#add`·`any` 수신자·`readonly number[]`의 읽기·
  `items.map().filter()`는 보고하지 않음을 확인.
- 2026-08-19: 테스트 — `tests/compile.rs`의
  `val_reaches_mutating_built_in_methods`를 `val_never_calls_a_method_a_mutation_from_its_name`
  (통과 계약)으로 뒤집고, `val_method_calls_are_collected_for_the_typed_pass`,
  `a_type_argument_list_does_not_declare_a_val_binding` 추가.
  `val_const_forbids_mutation_at_any_depth`의 push 단언은 깊은 복합 대입으로 교체.
  `tests/cli.rs`에 `--types` 회귀 5개 추가(built-in 3종/사용자 정의 허용/판단 불가
  허용/문법 mutation 유지).
- 2026-08-19: 문서 — `language.md` §10.3 개정 + §10.4 신설(§10.5~§10.8 재번호),
  §10.8 한계 표에 2행 추가, `errors.md`의 메서드 에러 행 교체, `cli.md`의
  `--types` 절에 항목 추가, `docs/ai/rl.md`의 `val`·에러 절 갱신,
  `docs/design/compiler-architecture.md`에 프로브 모드 설명 추가.

## 이슈 및 해결

### 이슈 1: 호스트가 `ReferenceError: Cannot access 'BUILTIN_MUTATORS' before initialization`으로 죽음

- **증상**: `rlc --types`가 `rlc: declaration emit failed: ... ReferenceError:
  Cannot access 'BUILTIN_MUTATORS' before initialization`로 실패.
- **원인**: `types_host.mjs`는 모듈 최상위에서 곧바로 검사를 실행하는데
  (`const valMutations = checkValMutations(...)`), 표를 파일 아래쪽에 `const`로
  선언해 뒀다. 함수 선언과 달리 `const`는 호이스팅되지 않아 TDZ에 걸린다.
- **해결**: `BUILTIN_MUTATORS` 선언을 최상위 실행부(`const job = JSON.parse(...)`)
  앞으로 옮겼다. 주석에 이유를 남겼다.

### 이슈 2: `val const items: number[] = [];`가 `number`를 val 바인딩으로 등록 (기존 버그)

- **증상**: 새 회귀 테스트용 파일(`val const m = new Map<string, number>();`와
  `val const items: number[] = [];`가 같은 파일에 있는 경우)에서
  `rlc: ...:2:18: cannot mutate through val binding \`number\``. 이번 변경 이전
  커밋(`git stash`)에서도 재현되는 **기존 오탐**이었다.
- **원인**: `collect_decl_names`는 선언자 목록을 쉼표로 나누는데, `<...>`는
  스캐너가 짝을 맞추는 괄호가 아니라 `new Map<string, number>()`의 쉼표가
  선언자 구분자로 보인다. 그래서 `number`가 `val` 바인딩이 되고, 이후
  `number[] = []`(다른 선언의 타입 표기)가 "인덱스 접근 + 대입" 경로로 읽혔다.
- **해결**: 쉼표 뒤 후보에 `declarator_at()` 가드를 추가했다 — 진짜 선언자는
  `=`/`:`/`,`/`;`가 뒤따르거나 구조 분해 패턴(`{`/`[`)이고, 타입 인자는
  `>`/`[`/`|` 등이 뒤따른다. 첫 선언자는 기존대로 무조건 수집한다.
  회귀 테스트: `a_type_argument_list_does_not_declare_a_val_binding`
  (`val let a, b, c;`·`val const p = 1, q = {...};`의 다중 선언자도 함께 고정).
  남은 한계: `f<A, B, C>()`처럼 타입 인자가 셋 이상이면 가운데 인자가 `,`를
  뒤에 두므로 여전히 선언자로 보인다 — 그 이름이 나중에 변경 경로의 루트로
  쓰일 때만 문제가 되어 실사용 위험이 낮다고 판단해 남겼다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` — 유닛/통합/doctest 전부 통과 (compile 214, cli 21, integration 72,
      passthrough 55, stdlib 8, emit_map 3, sidecar 9, doctest 15). node/tsc가 있는
      환경이라 `--types` 통합·CLI 테스트도 실제로 실행됐다.

## 결과

- `src/val.rs` — 이름 기준 판정 제거, 프로브 수집 모드, 선언자 스캔 가드.
- `src/lib.rs` — `val_method_calls` 공개 API + `ValMethodCall`.
- `src/main.rs` — `ValCheck` 프로브 전달과 `.rl` 위치 보고.
- `src/types_host.mjs` — 심볼 기반 built-in 변경 메서드 판정.
- `tests/compile.rs`, `tests/cli.rs` — 회귀 테스트.
- `docs/reference/language.md`, `docs/reference/errors.md`, `docs/reference/cli.md`,
  `docs/ai/rl.md`, `docs/design/compiler-architecture.md` — 규범 문서 갱신.

후속: [TASK-072](./TASK-072-val-typed-diagnostics-in-editor.md) — 에디터(LSP)에
타입 기반 `val` 진단 노출. 결정 5의 대안 (B)(선언에서 수신자가 문법적으로
확정되는 경우 기본 경로 검사)는 채택하지 않았고, 필요해지면 별도 태스크로
다룬다.
