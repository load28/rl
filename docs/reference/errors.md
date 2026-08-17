# rlc 에러 레퍼런스

rlc가 내는 모든 진단 메시지의 형식·원인·해결 방법입니다. 언어 규칙 자체는
[`language.md`](./language.md), CLI 동작은 [`cli.md`](./cli.md) 참조.

## 에러 형식

컴파일 에러는 stderr에 다음 형식으로 출력됩니다:

```
rlc: <파일>:<행>:<열>: <메시지>
```

- 행·열은 원본 `.rl` 소스 기준 1-기반입니다.
- 위치를 특정할 수 없는 에러(출력 검증 실패)는 `rlc: <파일>: <메시지>`로
  위치 없이 출력됩니다.

rl 구문으로 완전히 파싱되지 않는 텍스트는 에러가 아니라 조용히 원문
통과합니다. 에러는 **rl 구문임이 확정된 뒤의 규칙 위반**에만 발생합니다.
통과 영역에 쓴 TypeScript 코드의 타입 에러는 rlc가 아니라 tsc가 보고합니다.

---

## enum 에러

### `enum <이름>: duplicate case "<태그>"`

- **원인**: 한 rl enum 안에 같은 태그의 케이스가 두 번 선언됨.
- **위치**: 중복된(두 번째) 태그.
- **해결**: 케이스를 하나로 합치거나 태그 이름을 바꿉니다.

```rl
enum Shape { Circle(r: number), Circle(d: number) }
// rlc: file.rl:1:33: enum Shape: duplicate case "Circle"
```

### `enum <이름>: invalid type for field ` `` `<필드>` `` `: <상세>`

- **원인**: 필드의 타입 표기가 TypeScript 타입 문법으로 파싱되지 않음.
  파서가 알려준 상세 메시지가 뒤에 붙습니다.
- **위치**: 해당 필드의 타입 시작 지점.
- **해결**: 타입 표기를 고칩니다. 검사를 끄려면 `--no-verify`
  (라이브러리에서는 `Options { verify: false }`) — 이 경우 잘못된 타입이
  생성물에 그대로 전파되어 tsc 단계에서 드러납니다.

```rl
enum E { A(x: number]) }
// rlc: file.rl:1:15: enum E: invalid type for field `x`: Expected ',', got ']'
```

---

## match 에러

### `match: the wildcard arm `_` must be the last arm`

- **원인**: `_` 암 뒤에 다른 암이 있음. `_` 뒤의 암은 도달 불가능합니다.
- **위치**: 해당 `_` 패턴.
- **해결**: `_` 암을 마지막으로 옮깁니다.

### `match: duplicate arm "<태그>"`

- **원인**: **무가드 암이 이미 덮은 태그**가 다시 나옴 (두 번째 암은 도달
  불가능). or-패턴의 대안도 각각 하나의 태그로 셉니다 — `A | A => ..`,
  `A | B => .., B => ..`, `A => .., A if c => ..` 모두 해당. 가드 암끼리는
  같은 태그를 반복할 수 있으므로 에러가 아닙니다.
- **위치**: 중복된(두 번째) 태그.
- **해결**: 암을 하나로 합치거나 태그를 확인합니다.

### `match: or-pattern alternatives must bind the same fields`

- **원인**: or-패턴의 대안들이 서로 다른 (필드, 바인딩 이름) 집합을 바인딩함.
  대안들은 하나의 본문(과 하나의 구조 분해)을 공유하므로 같은 집합을
  바인딩해야 합니다 — `A(x) | B(y)`, `A(v) | B(v: w)`, `A | B(x)`는 안 되고
  `A(x) | B(x)`, `A(x, y) | B(y, x)`는 됩니다.
- **위치**: 첫 대안과 바인딩이 다른 대안의 태그.
- **해결**: 대안들의 바인딩을 같은 집합으로 맞추거나 암을 분리합니다.

### `match on enum <이름> is not exhaustive: missing "<케이스>", ... (add the missing arms or a final `_` arm)`

- **원인**: `_` 없는 match가 알려진 enum `<이름>`의 케이스를 전부 커버하지
  않음. 알려진 enum은 세 출처가 있고 메시지가 출처를 표시합니다
  (같은 이름이면 로컬 > 임포트 > 내장 순으로 섀도잉):
  - 같은 파일에 선언된 rl enum — `match on enum <이름> is not exhaustive: ...`
  - 상대 경로 `.rl` import로 가져온 exported rl enum(CLI가 자동 수집,
    [language.md §7.3](./language.md#73-선언-수집과-프로젝트-단위-소진성)) —
    `match on enum <이름> (imported from "<지정자>") is not exhaustive: ...`
  - 내장 enum(`Option`/`Result`) —
    `match on built-in enum <이름> is not exhaustive: ...`

  **가드 암은 케이스를 커버하지 못합니다** — 조건이 거짓일 수 있으므로, 그
  태그를 커버하려면 무가드 암이 따로 필요합니다.
- **위치**: `match` 키워드 (import된 enum이어도 컴파일 중인 파일 기준).
- **해결**: 빠진 케이스의 암을 추가하거나 마지막에 `_` 암을 둡니다.
- **참고**: 손으로 쓴 유니언, 해석되지 않는 import(존재하지 않는 파일,
  re-export 체인 등)에 대한 match는 이 검사를 받지 않습니다 — 런타임 가드만
  남습니다 ([language.md §3.6](./language.md#36-소진성-검사)).

```
$ rlc shapes.rl
rlc: shapes.rl:12:25: match on enum Shape is not exhaustive: missing "Rect"
     (add the missing arms or a final `_` arm)
```

---

## try 에러

### `` `try` cannot be used inside a match expression, a template interpolation, or another `try` — it compiles to a `return` from the enclosing function``

- **원인**: `try` 문이 match 표현식(스크루티니·암 본문), 템플릿 보간, 또는
  다른 try의 식 내부에서 사용됨. 그 위치의 `return`은 둘러싼 함수가 아니라
  match의 switch IIFE 등에서 반환되어 Rust와 의미가 달라집니다.
- **위치**: 해당 `try` 문의 시작 (선언 형태면 `const`/`let`/`var`).
- **해결**: try를 쓰는 로직을 별도 함수로 추출한 뒤 match 암에서는 그 함수를
  호출합니다 ([language.md §5.4](./language.md#54-사용-위치-제약)).
- **참고**: 모듈 최상위(함수 밖)의 try는 이 검사로 잡히지 않고, 생성물의
  최상위 `return`이 모듈에서 유효하지 않아 출력 검증 에러(아래)로
  드러납니다.

---

## let-else 에러

### `let-else cannot be used inside a match expression, a template interpolation, or a `try` — it compiles to statements in the enclosing function`

- **원인**: let-else 문이 match 표현식(스크루티니·가드·암 본문), 템플릿 보간,
  또는 try의 식 내부에서 사용됨. try와 같은 위치 제약입니다.
- **위치**: 해당 문의 선언 키워드(`const`/`let`/`var`).
- **해결**: 로직을 별도 함수로 추출합니다
  ([language.md §6.4](./language.md#64-사용-위치와-발산-제약)).

### ``let-else: the `else` block must end with a `return`, `throw`, `break`, or `continue` statement``

- **원인**: `else` 블록이 발산하지 않음. 블록이 발산하지 않으면 블록 뒤의
  구조 분해가 케이스 미보장 상태로 실행됩니다. 검사는 구문 수준이므로
  마지막 최상위 문장이 네 키워드 중 하나로 시작해야 합니다 —
  `if (c) return a; else return b;`로 끝나는 블록도 거부됩니다.
- **위치**: `else` 키워드.
- **해결**: 블록의 마지막 문장을 `return`/`throw`/`break`/`continue`로
  끝나게 재구성합니다.

```rl
function f(): number {
  const Some(v) = find() else { log(); };
  return v;
}
// rlc: file.rl:2:26: let-else: the `else` block must end with a `return`, ...
```

---

## 출력 검증 에러

### `generated TypeScript failed to parse: <상세> (line <행>, col <열> of the generated output). This is either invalid TypeScript passed through from the source or an rlc bug; use --no-verify to bypass.`

- **원인**: 최종 생성된 TypeScript를 검증하는 자가 검사 실패. 둘 중
  하나입니다 — ① 통과 영역의 소스가 애초에 유효한 TS가 아니었거나
  (검증기가 아직 모르는 최신 문법 포함), ② rlc의 버그.
- **위치**: 원본이 아닌 **생성물 기준** 행·열이 메시지 안에 표기되며, 에러
  자체는 위치 없이(`파일: 메시지`) 보고됩니다.
- **해결**: 소스의 해당 부분이 유효한 TS인지 확인합니다. 유효한데 거부되는
  최신 문법이라면 `--no-verify`로 우회합니다. 소스가 유효한데도 발생하면
  rlc 버그이므로 제보해 주세요.

---

## CLI 에러

컴파일 이전 단계의 에러들입니다. 전부 stderr로 출력되고 종료 코드 1입니다.

| 메시지 | 원인 / 해결 |
|--------|-------------|
| `rlc: --out-dir requires a value` | `-o`/`--out-dir` 뒤에 디렉터리가 없음. |
| `rlc: --tsc requires a path to the tsc binary` | `--tsc` 뒤에 경로가 없음. |
| `rlc: --emit-std takes no inputs (...)` | `--emit-std`는 stdout 전용 단독 모드 — 빌드에서는 `@rl/std` 자동 방출이 대신합니다. |
| `rlc: --types does not combine with -p, --check, --symbols, or --sidecar` | `--types`는 자체 파이프라인 — 다른 모드와 조합 불가 (`-w`는 가능). |
| `rlc: unknown option <옵션>` | 알 수 없는 `-` 시작 인자. `rlc -h`로 옵션 확인. |
| `rlc: no such file or directory: <경로>` | 입력 경로가 존재하지 않음. |
| `rlc: no sources found` | 입력에서 컴파일할 파일을 하나도 찾지 못함. |
| `rlc: <경로>: output would overwrite the input — pass -o <dir>` | 통과 `.ts`를 제자리 컴파일하면 소스를 덮어쓰게 되어 거부. `-o`로 출력 트리를 분리. |
| `rlc: tsc not found — install TypeScript (npm i -D typescript) or pass --tsc <path>` | `--types`가 실행할 tsc를 찾지 못함. |
| `rlc: <경로>: <OS 에러>` | 파일 읽기/쓰기/디렉터리 생성 실패 (권한, 디스크 등). 해당 파일만 건너뛰고 계속 진행한 뒤 1로 종료. |

인자 에러와 존재하지 않는 경로는 즉시 종료하고, IO 에러는 파일 단위로
건너뛰며 계속 처리합니다 ([cli.md 종료 코드](./cli.md#종료-코드) 참조).
