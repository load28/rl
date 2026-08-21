# TASK-118: 타입 에러 문안에서 구조적 타입을 선언 이름으로

- **상태**: 완료
- **시작일**: 2026-08-21
- **완료일**: 2026-08-21
- **커밋**: d4b5957

## 목적

`try`가 전파하는 `Err`가 반환 타입에 맞지 않을 때 rlc는 이렇게 말한다:

```
the `Err` this `try` propagates does not fit the enclosing function's return
type — ... (ts2322: Type 'Err<{ kind: "OutOfRange"; value: number; }>' is not
assignable to type 'Result<string, { kind: "NotANumber"; text: string; }>'.)
```

위치는 TASK-116이 정확히 잡았지만(그 `try` 위), **문안**은 tsc가 펼친 구조적
타입이라 읽기 어렵다. rl은 그 태그가 어느 enum의 케이스인지 안다 — Rust가
`` `?` couldn't convert the error to `ParseError` ``라고 말하는 수준으로
좁힐 여지가 있다(`Test.OutOfRange` / `ParseError`).

## 범위

- 포함: 진단 문안의 `{ kind: "X"; ... }`를 선언 표가 **유일하게** 지목할
  때만 `Enum.X`로 부른다.
- 포함: 한 enum의 케이스 전부가 유니언으로 나오면 그 enum 이름으로 줄인다
  (`ParseError`) — 목적의 예시가 요구하는 절반이 이것이다.
- 제외: 원문을 덮어쓰는 것. 지금 계약은 "옮긴 말 + 괄호 안의 원문"이고,
  원문은 사용자가 번역을 검증할 근거이므로 그대로 실려야 한다 — 이름은 옮긴
  말 쪽에 더한다.
- 제외: 번역되지 않는 진단(화이트리스트 밖, 그리고 사용자가 쓴 코드의 타입
  에러). 후자는 계약상 tsc가 말할 몫이고, 손대면 "원문 그대로"가 깨진다.

## 의사결정

### 1. 이름을 어디에 어떤 모양으로 싣는가

상황: 옮긴 말(rl 문장)과 원문(tsc 문장)이 이미 한 줄에 함께 실린다. 이름을
더할 자리는 셋이었다.

| 대안 | 장점 | 단점 |
|---|---|---|
| (가) 원문의 구조적 타입을 그 자리에서 치환 | 가장 짧다 | **원문이 원문이 아니게 된다** — 범위에서 제외한 바로 그것 |
| (나) 옮긴 말에 짧은 절을 덧붙임 (`... 는 `Test.OutOfRange`입니다`) | 짧다 | 어느 이름이 원문의 어느 타입인지 (ts 코드마다 문장 구조가 달라) 일반적으로 말할 수 없다 |
| (다) **원문을 rl 이름으로 다시 쓴 문장을 옮긴 말 쪽에 덧붙임** | 원문과 어순이 같아 1:1로 대조된다; 문장 구조를 해석할 필요가 없어 코드에 무관하게 일반적이다 | 한 줄이 길어진다 |

선택: **(다)**. 최종 모양은

```
<옮긴 말> (in rl's names: <이름으로 다시 쓴 원문>) (ts<코드>: <원문>)
```

(나)를 버린 이유가 결정적이다: `2322`는 `Type 'A' is not assignable to type
'B'.`, `2345`는 `Argument of type 'A' is not assignable to parameter of type
'B'.`이고, 화이트리스트가 커질수록 문장 구조는 더 갈라진다. 문장을 해석하지
않고 **타입 표기만** 바꾸면 어떤 코드가 와도 같은 규칙 하나로 처리된다.
길이 부담은 이름이 구조적 타입보다 짧다는 사실이 상당 부분 상쇄한다
(`{ kind: "OutOfRange"; value: number; }` → `Wire.OutOfRange`).

### 2. 언제 이름을 붙여도 되는가 (오인의 대가)

상황: 태그만 보고 이름을 붙이면, 태그가 같은 남의 타입을 rl 케이스라고
우기게 된다. 이는 `translate` 자체가 화이트리스트인 이유(아닌 것을 아는 척
하지 않는다)와 같은 문제다.

선택한 규칙 — **둘 다** 만족할 때만 이름을 붙인다:

1. 그 태그를 선언한 enum이 선언 표에 **정확히 하나**. 둘이면 침묵한다
   (둘 중 찍는 것은 "어느 선언인가"에 대한 답이 아니다).
2. 그 객체 타입의 필드 이름 집합이 그 케이스의 페이로드와 **정확히 일치**.
   타입 텍스트까지는 보지 않는다 — 제네릭 인스턴스화 때문에 선언 텍스트와
   tsc의 표시가 다를 수 있고(TASK-098), 필드 이름만으로도 오인은 충분히
   걸러진다.

대안으로 "태그가 겹치면 필드로 가린다"도 검토했지만, 그러면 A의 케이스와
필드가 우연히 같은 B의 케이스를 A라고 부를 수 있다. 겹치면 침묵이 안전하다.

### 3. 유니언 전체를 enum 이름으로 줄일 것인가

목적의 예시(`Result<string, { kind: "NotANumber"; text: string; }>` →
`Result<string, ParseError>`)가 요구하므로 줄인다. 조건은 **한 enum의 케이스
전부가, 각각 한 번씩** 나올 때. 일부만 나오면 `E.A | E.B`로 쓴다 — 그게 사실
그대로다. (제네릭 enum이면 타입 인자가 빠진 이름이 되지만, 이 문장은 읽기
보조이고 원문이 함께 실리므로 감수한다.)

### 4. 선언 표를 누가, 언제 만드는가

`translate`는 (구문, 코드, 문안)만 받는 순수 함수였다. 선언 표를 넣으려면
누군가 파일과 그 임포트를 파싱해야 한다(`pattern_analyses` + `externs_of`).

- 대안 A: `translate` 안에서 만든다 → 시그니처는 그대로지만 진단마다 파싱.
- 대안 B: **호출자가 만들어 넘긴다**(선택). 배치 경로(`semantics::report`)는
  파일별로 한 번 만들어 `HashMap`에 담고, **글루 위 진단이 하나라도 있는
  파일에서만** 만든다(`glue_anchor`로 먼저 걸러 낸다). 에디터 경로
  (`service_diagnostics`)는 그 pass에서 첫 번역이 일어날 때 한 번 만든다.
  번역이 없는 pass는 비용이 0이다.

임포트한 enum까지 표에 넣은 것은 의도적이다 — 에러 enum은 보통 다른 모듈에
산다. 이름은 `externs_of`가 주는 **임포트가 붙인 이름**(별칭/네임스페이스)을
쓰므로, `import { Wire as W }`면 `W.OutOfRange`라고 부른다. 이 규칙은 소진성·
호버가 쓰는 표와 같은 표라서 따로 만든 규칙이 아니다.

## 작업 내역

1. `src/engine/semantics.rs`
   - `translate(kind, code, message)` → `translate(kind, code, message,
     declarations)`. 문안 조립은 `name_types`의 결과가 있을 때만
     `(in rl's names: ...)` 절을 끼운다.
   - `name_types` + 보조 함수 추가: `case_members`(따옴표를 아는 스캔으로
     문안 안의 객체 타입 수집, 인식 못 한 객체 안으로는 내려간다),
     `union_runs`(`" | "`로 이어진 것끼리 묶는다), `collapsed`(한 enum을
     전부 덮으면 enum 이름), `recognize`(위 §2의 두 규칙), `object_fields`,
     `split_top`, `object_end`, `string_end`.
   - `report`가 파일별 선언 표를 지연 생성(`HashMap`)하고, 스냅샷의 텍스트를
     먼저 읽는 `externs_of` 헬퍼를 소진성 경로와 공유하도록 정리.
   - 단위 테스트 7개 추가(문안 규칙 전체: 이름 붙는 경우/부분 유니언/태그
     중복/필드 불일치/구조적 타입 없음/중첩/태그 유니언).
2. `src/engine/projection.rs` — `translate_on_glue`가 선언 표를 받아 넘긴다.
   기존 테스트 4곳은 `pattern_analyses`로 표를 만들어 넘기도록 갱신.
3. `src/engine/language.rs` — `service_diagnostics`가 첫 번역에서 표를 만들어
   같은 `translate`에 넘긴다(두 표면이 갈라질 수 없게 유지).
4. `tests/native.rs` — `a_restated_diagnostic_calls_a_case_by_its_declared_name`:
   좁혀진 케이스를 `Result.Err`로 감싸 전파하는 실제 프로젝트를 tsgo로 검사해
   `Err<Wire.OutOfRange>` / `Result<number, ParseError>`와 원문이 함께 나오는지
   확인(배치 typed 경로 = API 서버).
5. `editors/vscode/server/src/test/emitmap.test.ts` —
   "a restated diagnostic names the case it is about": 같은 시나리오를
   **에디터 경로**(`rlc --server`의 `tsDiagnostics` = language service)로
   확인. 범위가 `try inner(w)`인 것까지 함께 잠근다.
6. 문서: `docs/reference/errors.md`에 규범(붙이는 조건·모양·예시) 추가,
   `docs/ai/rl.md` 한 줄 갱신, `docs/design/rust-parity-analysis.md` §10.4에
   TASK-118 주석.

검증 명령(CI의 `native` 잡과 같은 구성 — 핀 박힌 typescript-go를 직접 빌드해
두 경로를 모두 돌렸다):

```sh
git clone https://github.com/microsoft/typescript-go.git ../typescript-go
git -C ../typescript-go checkout c6b013f5706d58582f566df778cc0df2683b58f5
(cd ../typescript-go && go build -o built/local/tsgo ./cmd/tsgo \
  && npm ci && npx tsc -b _packages/native-preview)

cargo fmt --check
cargo clippy --all-targets -- -D warnings
RLC_TSGO_ROOT=$PWD/../typescript-go RLC_REQUIRE_TSGO=1 cargo test

cd editors/vscode && npm ci && npx tsc -b
PATH="$PWD/../../target/debug:$PATH" RLC_TSGO_ROOT=/home/user/typescript-go \
  node --test "server/out/test/*.test.js"
```

API 서버(배치)만으로는 에디터 경로를 확인할 수 없었다: npm의 `typescript@7`은
API 클라이언트는 주지만 이 환경에서 language server 실행 파일
(`@typescript/typescript-<platform>/lib/tsc`)이 설치되지 않아
`rlc --server`의 `tsDiagnostics`가 "no tsgo language server found"로 답했다.
typescript-go를 직접 빌드하니 두 경로가 모두 열렸다.

## 이슈 및 해결

1. **tsc의 홑따옴표를 문자열로 읽어 타입 전체를 건너뛰었다.**
   증상: 스캐너가 `'Err<{ kind: "OutOfRange"; ... }>'`를 통째로 문자열로 보고
   지나가 아무것도 인식하지 못함. 원인: 처음에 `"`와 `'` 둘 다 문자열
   구분자로 취급했는데, tsc 문안은 **타입 표기를 홑따옴표로 감싼다**. 해결:
   문안 스캔에서 문자열은 `"`만으로 판정한다(tsc가 문자열 리터럴 타입을
   쌍따옴표로 찍으므로 안쪽도 이 규칙으로 맞는다).

2. **구조적으로 펼쳐진 케이스가 언제 나오는지 몰라 e2e 시나리오를 못 잡았다.**
   증상: `function inner(w: Wire): Result<number, Wire>`처럼 이름 있는 타입을
   쓰면 tsc가 `Err<Wire>`로 찍어 이름 붙일 것이 없다. 조사: 별칭이 사라지는
   자리는 **좁히기**였다 — `if (w.kind === "OutOfRange")` 안에서 `w`는 별칭을
   잃고 멤버 타입 그대로가 된다. 해결: 그 모양을 그대로 e2e 테스트로 굳혔고
   (`tests/native.rs`), 목적에 적힌 원래 증상과 같은 문안이 재현되는 것을
   확인했다.

3. **에디터 경로를 검증할 language server가 없었다.**
   증상: `rlc --server`의 `tsDiagnostics`가
   `no tsgo language server found — install TypeScript 7 ...`. 원인: npm의
   `typescript@7`이 API 클라이언트(`dist/api/sync/api.js`)는 깔아 주지만
   플랫폼 실행 파일 패키지(`@typescript/typescript-linux-x64`)가 이 환경에
   설치되지 않았고, LSP 표면은 그 실행 파일을 쓴다(`typescript/service.rs`).
   해결: CI가 핀으로 박아 둔 커밋(`c6b013f5`)으로 typescript-go를 클론해
   `go build ./cmd/tsgo` + `npx tsc -b _packages/native-preview`로 두 반쪽을
   한 빌드에서 만들고 `RLC_TSGO_ROOT`로 물렸다. 그 뒤 에디터 경로도 실측했고
   (아래 결과), 확장 테스트 83개가 skip 0으로 통과한다.

4. **선언 표를 만드는 비용이 번역과 무관한 파일에도 붙었다.**
   증상(설계 단계에서 발견): `report`가 진단마다 표를 요구하면, 글루와 아무
   상관 없는 평범한 타입 에러가 있는 파일도 파싱하게 된다. 해결:
   `glue_anchor(...).is_some()`으로 먼저 거른 뒤에만 표를 만든다. 남은 부채는
   없다(표는 파일당 한 번, pass 안에서 재사용).

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` — 전체 통과. `RLC_TSGO_ROOT`(직접 빌드한 typescript-go) +
      `RLC_REQUIRE_TSGO=1`이라 typed 테스트가 하나도 skip되지 않았다
      (단위 77 + 통합 73 + native 31 + 나머지).
- [x] VS Code 확장 테스트 — `node --test server/out/test/*.test.js`,
      **83 pass / 0 skip**(추가한 에디터 경로 테스트 포함).

## 결과

글루 위 진단의 옮긴 말이 rl 이름으로 다시 쓴 문장을 함께 싣는다.

배치 typed 경로(`rlc --check-types`, API 서버):

```
rlc: src/a.rl:12:13: the `Err` this `try` propagates does not fit the enclosing
function's return type — rl has no automatic conversion, so widen the return
type or convert the error (in rl's names: Type 'Err<Wire.OutOfRange>' is not
assignable to type 'Result<number, ParseError>'.) (ts2322: Type 'Err<{ kind:
"OutOfRange"; value: number; }>' is not assignable to type 'Result<number,
{ kind: "NotANumber"; text: string; }>'.)
```

에디터 경로(`rlc --server`의 `tsDiagnostics`, language service). 이쪽은 tsc가
설명 사슬(elaboration)까지 붙이는데, 규칙이 문장 구조와 무관하므로 사슬의 각
줄도 그대로 이름이 붙는다 — (다)를 고른 이유가 여기서 드러난다:

```
the `Err` this `try` propagates does not fit ...
(in rl's names: Type 'Err<Wire.OutOfRange>' is not assignable to type
 'Result<number, ParseError>'.
   Type 'Err<Wire.OutOfRange>' is not assignable to type 'Err<ParseError>'.
     Property 'text' is missing in type 'Wire.OutOfRange' but required in
     type 'ParseError'.)
(ts2322: ... 원문 그대로 ...)
```

범위는 `try inner(w)`(TASK-116) 그대로다.

변경 파일: `src/engine/semantics.rs`, `src/engine/projection.rs`,
`src/engine/language.rs`, `tests/native.rs`,
`editors/vscode/server/src/test/emitmap.test.ts`,
`docs/reference/errors.md`, `docs/ai/rl.md`,
`docs/design/rust-parity-analysis.md`,
`docs/tasks/TASK-118-named-error-types-in-messages.md`,
`docs/tasks/INDEX.md`.
