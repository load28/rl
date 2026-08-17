# rlc 에러 레퍼런스

rlc가 내는 모든 진단의 형식과 해결 방법입니다. 언어 규칙은
[`language.md`](./language.md), CLI 동작은 [`cli.md`](./cli.md)를 보세요.

```
rlc: <파일>:<행>:<열>: <메시지>
```

행·열은 원본 `.rl` 기준 1-기반입니다. 위치를 특정할 수 없는 에러(출력 검증)는
`rlc: <파일>: <메시지>`로 나옵니다.

rl 구문으로 완전히 파싱되지 않는 텍스트는 에러가 아니라 조용히 통과합니다 —
에러는 **rl 구문임이 확정된 뒤의 규칙 위반**에만 발생합니다. 통과 영역의 타입
에러는 tsc의 몫입니다.

## enum

| 메시지 | 원인과 해결 |
|--------|-------------|
| `enum <이름>: duplicate case "<태그>"` | 한 enum에 같은 태그가 두 번. 위치는 두 번째 태그. 합치거나 이름을 바꿉니다 |
| `enum <이름>: invalid type for field ``<필드>``: <상세>` | 필드 타입이 TypeScript 타입으로 파싱되지 않음. 위치는 타입 시작 지점. 표기를 고치거나 `--no-verify`로 우회(이 경우 tsc 단계에서 드러납니다) |

```rl
enum Shape { Circle(r: number), Circle(d: number) }
// rlc: file.rl:1:33: enum Shape: duplicate case "Circle"

enum E { A(x: number]) }
// rlc: file.rl:1:15: enum E: invalid type for field `x`: Expected ',', got ']'
```

## match

| 메시지 | 원인과 해결 |
|--------|-------------|
| ``match: the wildcard arm `_` must be the last arm`` | `_` 뒤의 암은 도달 불가능. `_`를 마지막으로 옮깁니다 |
| `match: duplicate arm "<태그>"` | **무가드 암이 이미 덮은 태그**가 다시 나옴 — `A \| A`, `A \| B => .., B => ..`, `A => .., A if c => ..`. 가드 암끼리의 반복은 에러가 아닙니다. 위치는 두 번째 태그 |
| `match: or-pattern alternatives must bind the same fields` | 대안들이 하나의 구조 분해를 공유하므로 같은 (필드, 이름) 집합을 바인딩해야 합니다. `A(x) \| B(x)`·`A(x, y) \| B(y, x)`는 되고 `A(x) \| B(y)`·`A \| B(x)`는 안 됩니다 |
| `match: nested patterns cannot be combined with or-patterns` | 중첩 패턴(`Tag(field: Inner(...))`)은 대안별 경로 조건이 필요해 공유 구조 분해와 양립하지 않습니다. 암을 나눕니다 |
| `` match: binding `<이름>` is used more than once in this pattern (rename one with `field: alias`) `` | 한 패턴(중첩 포함)이 같은 이름을 두 번 바인딩 — 한 스코프에 두 번 선언됩니다. 별칭으로 바꿉니다 |

### 소진성

```
$ rlc shapes.rl
rlc: shapes.rl:12:25: match on enum Shape is not exhaustive: missing "Rect"
     (add the missing arms or a final `_` arm)
```

`_` 없는 match가 알려진 enum의 케이스를 다 덮지 않았습니다. 위치는 `match`
키워드이고(import된 enum이어도 컴파일 중인 파일 기준), 메시지가 enum의 출처를
표시합니다.

| 출처 | 메시지 |
|------|--------|
| 같은 파일의 rl enum | `match on enum <이름> is not exhaustive: ...` |
| import한 exported enum | `match on enum <이름> (imported from "<지정자>") is not exhaustive: ...` |
| 내장 `Option`/`Result` | `match on built-in enum <이름> is not exhaustive: ...` |

**가드 암과 중첩 패턴 암은 케이스를 커버하지 못합니다** — 그 태그를 덮으려면
무가드·무중첩 암이 따로 필요합니다. 손으로 쓴 유니언이나 해석되지 않는
import의 enum은 이 검사를 받지 않고 런타임 가드만 남습니다
([`language.md` §3.6](./language.md#36-소진성-검사)).

튜플 match는 곱집합으로 검사하고 빠진 **조합**을 보고합니다 (5개 이상이면
앞의 셋과 총 개수만):

```
rlc: nav.rl:4:15: match on (Conn, Mode) is not exhaustive: missing (Offline, Manual)
     (add the missing arms or a final `_` arm)
```

### 튜플 match

| 메시지 | 원인과 해결 |
|--------|-------------|
| `match: tuple pattern has <n> elements but the match has <m> scrutinees` | 튜플 패턴의 원소 수가 스크루티니 수와 다름. 위치는 패턴 시작 |
| `` match: binding `<이름>` is used more than once in this tuple pattern (rename one with `field: alias`) `` | 한 튜플 패턴의 두 원소가 같은 이름을 바인딩 — 한 스코프에 두 번 선언됩니다. 별칭으로 바꿉니다: `(Some(value), Some(value: other))` |

## try / let-else

| 메시지 | 원인과 해결 |
|--------|-------------|
| `` `try` cannot be used inside a match expression, a template interpolation, or another `try` — ... `` | 그 자리의 `return`은 둘러싼 함수가 아니라 match의 IIFE에서 반환됩니다. 로직을 별도 함수로 추출하세요 ([§5.4](./language.md#54-사용-위치-제약)). 위치는 `try`(선언 형태면 `const`/`let`/`var`) |
| `let-else cannot be used inside a match expression, a template interpolation, or a `try` — ...` | `try`와 같은 위치 제약 ([§6.4](./language.md#64-사용-위치와-발산-제약)). 위치는 선언 키워드 |
| ``let-else: the `else` block must end with a `return`, `throw`, `break`, or `continue` statement`` | `else`가 발산하지 않으면 뒤의 구조 분해가 케이스 미보장 상태로 실행됩니다. 검사는 구문 수준이라 `if (c) return a; else return b;`로 끝나는 블록도 거부됩니다. 위치는 `else` |

```rl
function f(): number {
  const Some(v) = find() else { log(); };
  return v;
}
// rlc: file.rl:2:26: let-else: the `else` block must end with a `return`, ...
```

모듈 최상위의 `try`/let-else는 이 검사로 잡히지 않고, 최상위 `return`이 유효한
TS가 아니어서 아래 출력 검증 에러로 드러납니다.

## 파이프라인

| 메시지 | 원인과 해결 |
|--------|-------------|
| `` pipeline: `|>` could not be parsed here (steps must be expressions; parenthesize ternaries and arrow functions) `` | `\|>`가 파이프라인으로 완전히 파싱되지 않았습니다. `\|>`는 유효한 TS에 존재할 수 없어 통과시킬 수 없으므로 위치와 함께 에러입니다. 흔한 원인: head/step 최상위의 삼항이나 괄호 없는 화살표(괄호로 감쌉니다 — `(c ? a : b) \|> f`, `x \|> (n => n + 1)`), 빈 스텝(`x \|>;`), `?.` 시작 스텝. 위치는 `\|>` |

head/step 내부의 `try` 문은 위의 try 위치 제약 에러로 보고됩니다
([`language.md` §7.4](./language.md#74-구조-규칙)).

```rl
const a = c ? x : y |> f;
// rlc: file.rl:1:21: pipeline: `|>` could not be parsed here (steps must be
//      expressions; parenthesize ternaries and arrow functions)
```

## 출력 검증

```
generated TypeScript failed to parse: <상세> (line <행>, col <열> of the
generated output). This is either invalid TypeScript passed through from the
source or an rlc bug; use --no-verify to bypass.
```

생성물 자가 검사 실패입니다 — ① 통과 영역의 소스가 애초에 유효한 TS가 아니었거나
(검증기가 아직 모르는 최신 문법 포함) ② rlc의 버그. 행·열은 **생성물 기준**으로
메시지 안에 표기되고, 에러 자체는 위치 없이 보고됩니다. 소스가 유효한데도
발생하면 rlc 버그이므로 제보해 주세요.

## CLI

컴파일 이전 단계의 에러입니다. 전부 stderr로 나가고 종료 코드 1입니다.

| 메시지 | 원인 / 해결 |
|--------|-------------|
| `--out-dir requires a value` | `-o` 뒤에 디렉터리가 없음 |
| `--node requires a path to the node binary` | `--node` 뒤에 경로가 없음 |
| `--rewrite-imports requires a value (js, ts, or off)` / `... expects js, ts, or off (got <값>)` | 값이 없거나 셋 중 하나가 아님 |
| `--sidecar requires a directory of tsc-emitted .d.ts files` | `--sidecar` 뒤에 디렉터리가 없음 |
| `--emit-std takes no inputs (the build materializes @rl/std itself)` | stdout 전용 단독 모드 — 빌드에서는 자동 방출이 대신합니다 |
| `--types does not combine with -p, --check, --symbols, or --sidecar` | `--types`는 자체 파이프라인 (`-w`는 조합 가능) |
| `unknown option <옵션>` | 알 수 없는 `-` 시작 인자. `rlc -h` 참조 |
| `no such file or directory: <경로>` | 입력 경로가 없음 |
| `no sources found` | 입력에서 컴파일할 파일을 찾지 못함 |
| `<경로>: output would overwrite the input — pass -o <dir>` | 통과 `.ts`를 제자리 컴파일하면 소스를 덮어씀. `-o`로 출력 트리를 분리 |
| `<a.rl> would shadow <a.ts> — rename one of them` | `--types`가 `a.rl`을 올릴 가상 모듈 경로에 같은 이름의 실제 `a.ts`가 이미 있음. 한쪽 이름을 바꿉니다 |
| `node not found — install Node.js or pass --node <path> (--types needs it)` | `--types`가 node를 찾지 못함 |
| `typescript not found — install it (npm i -D typescript)` | 프로젝트에서 TypeScript를 해석하지 못함 |
| `declaration emit failed: <상세>` | 선언 방출 호스트가 비정상 종료 |
| `no declarations emitted for <모듈>` | 방출 결과에 그 모듈의 선언이 없음 (rlc 버그) |
| `<경로>: <OS 에러>` | 파일 IO 실패. 해당 파일만 건너뛰고 계속 진행한 뒤 1로 종료 |

인자 에러와 없는 경로는 즉시 종료하고, IO 에러는 파일 단위로 건너뛰며 계속
처리합니다.
