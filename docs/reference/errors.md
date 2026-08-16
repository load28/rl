# rlc 에러 레퍼런스

rlc가 내는 모든 진단 메시지의 형식·원인·해결 방법입니다. 언어 규칙 자체는
[`language.md`](./language.md), CLI 동작은 [`cli.md`](./cli.md) 참조.

## 에러 형식

컴파일 에러는 stderr에 다음 형식으로 출력됩니다:

```
rlc: <파일>:<행>:<열>: <메시지>
```

- 행·열은 원본 `.rl` 소스 기준 1-기반이며, 열은 UTF-8 코드 포인트 단위입니다.
- 위치를 특정할 수 없는 에러(출력 검증 실패)는 `rlc: <파일>: <메시지>`로
  위치 없이 출력됩니다.
- Rust API에서는 같은 정보가 [`CompileError`](../../src/error.rs)로 반환되고,
  `Display` 구현이 위 형식(선두 `rlc: ` 제외)을 만듭니다.

에러 계층 원칙: 아래 rl 수준 에러는 전부 **rlc가 직접** 보고합니다. rlc가
방출한 코드가 tsc 에러를 만들지 않아야 하며, 통과 영역에 사용자가 쓴 TS
코드의 타입 에러는 tsc의 책임입니다.

파싱이 "실패"하는 것은 에러가 아닙니다 — rl 구문으로 완전히 파싱되지 않는
텍스트는 조용히 원문 통과합니다. 에러는 **rl 구문임이 확정된 뒤의 규칙
위반**에만 발생합니다.

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

### `enum <이름>: invalid type for field ` `` `<필드>` `` `: <swc 메시지>`

- **원인**: 필드의 타입 표기가 TypeScript 타입 문법으로 파싱되지 않음.
  swc 파서가 검출하며, 원래 swc 메시지가 뒤에 붙습니다.
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

- **원인**: `_` 암 뒤에 다른 암이 있음. `_`는 `switch`의 `default`가 되므로
  뒤의 암이 도달 불가능해지는 것을 막습니다.
- **위치**: 해당 `_` 패턴.
- **해결**: `_` 암을 마지막으로 옮깁니다.

### `match: duplicate arm "<태그>"`

- **원인**: 같은 태그의 암이 두 번 나옴 (두 번째 암은 도달 불가능).
- **위치**: 중복된(두 번째) 암의 패턴.
- **해결**: 암을 하나로 합치거나 태그를 확인합니다.

### `match on enum <이름> is not exhaustive: missing "<케이스>", ... (add the missing arms or a final `_` arm)`

- **원인**: `_` 없는 match의 암 태그들이 이 파일에 선언된 rl enum
  `<이름>`의 케이스들인데, 일부 케이스가 커버되지 않음. 후보 enum이 여럿이면
  빠진 케이스가 가장 적은 것을 기준으로 보고합니다.
- **위치**: `match` 키워드.
- **해결**: 빠진 케이스의 암을 추가하거나 마지막에 `_` 암을 둡니다.
- **참고**: 다른 파일에서 import한 enum이나 손으로 쓴 유니언에 대한 match는
  후보가 없으므로 이 검사를 받지 않습니다 — 런타임 가드만 남습니다
  ([language.md §3.6](./language.md#36-소진성-검사)).

```
$ rlc shapes.rl
rlc: shapes.rl:12:25: match on enum Shape is not exhaustive: missing "Rect"
     (add the missing arms or a final `_` arm)
```

---

## 출력 검증 에러

### `generated TypeScript failed to parse: <swc 메시지> (line <행>, col <열> of the generated output). This is either invalid TypeScript passed through from the source or an rlc bug; use --no-verify to bypass.`

- **원인**: 최종 생성된 TypeScript 전체를 swc로 파싱하는 자가 검사 실패.
  둘 중 하나입니다 — ① 통과 영역의 소스가 애초에 유효한 TS가 아니었거나
  (swc가 아직 모르는 최신 문법 포함), ② rlc가 잘못된 코드를 방출하는 버그.
- **위치**: 원본이 아닌 **생성물 기준** 행·열이 메시지 안에 표기되며, 에러
  자체는 위치 없이(`파일: 메시지`) 보고됩니다.
- **해결**: 소스의 해당 부분이 유효한 TS인지 확인합니다. 유효한데 swc가
  거부하는 최신 문법이라면 `--no-verify`로 우회합니다. 소스가 유효한데도
  발생하면 rlc 버그이므로 제보해 주세요.

---

## CLI 에러

컴파일 이전 단계의 에러들입니다. 전부 stderr로 출력되고 종료 코드 1입니다.

| 메시지 | 원인 / 해결 |
|--------|-------------|
| `rlc: --out-dir requires a value` | `-o`/`--out-dir` 뒤에 디렉터리가 없음. |
| `rlc: unknown option <옵션>` | 알 수 없는 `-` 시작 인자. `rlc -h`로 옵션 확인. |
| `rlc: no such file or directory: <경로>` | 입력 경로가 존재하지 않음. |
| `rlc: no .rl files found` | 입력 디렉터리에 `.rl` 파일이 하나도 없음. |
| `rlc: <경로>: <OS 에러>` | 파일 읽기/쓰기/디렉터리 생성 실패 (권한, 디스크 등). 해당 파일만 건너뛰고 계속 진행한 뒤 1로 종료. |

인자 에러와 존재하지 않는 경로는 즉시 종료하고, IO 에러는 파일 단위로
건너뛰며 계속 처리합니다 ([cli.md 종료 코드](./cli.md#종료-코드) 참조).
