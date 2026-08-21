# TASK-117: 한 파일의 rl 에러를 여러 개 보고한다

- **상태**: 대기
- **시작일**: —
- **완료일**: —
- **커밋**: —

> 이 문서는 **문제 기록**이다. TASK-116(진단 범위) 작업 중 실측으로 확인한
> 격차를 나중에 처음 보는 사람이 재현·판단할 수 있도록 남긴다. 구현은 하지
> 않았다.

## 목적

`compile()`은 rl 수준 에러를 **하나만** 보고한다. 첫 에러에서 멈추므로 그
파일의 나머지는 검사되지 않는다. 그 결과 세 가지가 따라온다 — 하나는
불편이고, 하나는 일관성 문제이고, **하나는 진단이 사라지는 버그**다.

## 증상 (실측)

재현 파일 `many.rl` — 소진되지 않은 `match`가 세 개(4·8·12행):

```rl
enum Shape { Circle(r: number), Square(s: number), Tri(a: number) }

export function f(x: Shape): number {
  return match (x) { Circle(r) => r };
}

export function g(x: Shape): number {
  return match (x) { Square(s) => s };
}

export function h(x: Shape): number {
  return match (x) { Tri(a) => a };
}
```

### 증상 1 — 기본 경로는 하나만 (두더지 잡기)

```
$ rlc --check many.rl
rlc: many.rl:4:10: match on enum Shape is not exhaustive: missing "Square", "Tri" ...
```

4행을 고치면 8행이, 8행을 고치면 12행이 새로 나타난다. 파일에 문제가 몇 개
남았는지 끝까지 알 수 없고, 빌드는 매번 한 개씩만 알려 준다. tsc·rustc는
전부 모아서 보고한다.

### 증상 2 — typed 경로는 이미 여러 개를 낸다 (경로별 비대칭)

같은 파일을 typed 경로로 검사하면 셋 다 나온다:

```
$ rlc --check-types many.rl
rlc: many.rl:4:10: match is not exhaustive: missing "Square", "Tri" ...
rlc: many.rl:8:10: match is not exhaustive: missing "Circle", "Tri" ...
rlc: many.rl:12:10: match is not exhaustive: missing "Circle", "Square" ...
```

**개수만 다른 게 아니라 문안도 다르다** — 기본 경로는 `match on enum Shape
is not exhaustive`, typed 경로는 `match is not exhaustive`(enum 이름 없음).
에디터는 두 경로를 합쳐 보여 주므로(확장 `server.ts::mergeTyped`) 한 파일
안에서 같은 종류의 에러가 서로 다른 문안으로 섞인다. LSP로 실측한 결과:

```
발행 1회차 (--check): 1건  → 4행
발행 2회차 (typed 합류): 3건 → 4행(기본 경로 문안), 8·12행(typed 문안)
```

### 증상 3 — rl 에러 하나가 그 파일의 typed 진단을 통째로 가린다 ★

증상 2가 성립한 것은 **소진성이 typed 경로에서 다시 계산되기 때문**이다
(`defer_to_checker`가 켜지면 sema가 소진성을 건너뛰므로 projection이
성공한다). 소진성이 아닌 rl 에러가 하나라도 있으면 그 파일의 projection
자체가 실패하고(`Blocked`), **그 파일의 typed 진단이 하나도 오지 않는다.**

`mixed.rl` — 4행에 중복 암, 8·12행에 소진성 에러(총 3건):

```rl
export function f(x: Shape): number {
  return match (x) { Circle(r) => r, Circle(r) => 0, Square(s) => s, Tri(a) => a };
}
export function g(x: Shape): number { return match (x) { Square(s) => s }; }
export function h(x: Shape): number { return match (x) { Tri(a) => a }; }
```

```
$ rlc --check mixed.rl
rlc: mixed.rl:4:38: match: duplicate arm "Circle"

에디터(LSP publishDiagnostics 실측): 2건
  4행: match: duplicate arm "Circle"
  4행: unreachable arm: an earlier arm already matches every value
```

**8행과 12행의 소진성 에러가 사라졌다.** 타입 에러와 `val` 위반도 마찬가지로
전부 막힌다 — 4행의 중복 암 하나가 그 파일의 typed 검사 전체를 막기 때문이다.
사용자가 보기에는 "이 파일에 문제는 4행 하나"로 읽힌다.

## 원인

### 1. 공개 시그니처가 에러 하나를 담는다

```rust
pub fn compile(source: &str, options: &Options) -> Result<String, CompileError>
```

`Err`는 값 하나다. 그래서 검사 단계는 첫 에러에서 `?`로 즉시 빠져나온다.
조기 리턴 지점은 `src/sema.rs` 26곳, `src/val.rs` 2곳이다
(`grep -c "return Err(RlError::"`).

### 2. 분석은 이미 전부 계산해 두었고, **보고**가 하나로 좁힌다

이게 중요하다 — 계산이 부족한 게 아니다.

- `src/sema.rs:91` — `analyses.unresolved.first()`: 해석 실패한 이름은
  **목록**으로 와 있는데 첫 개만 보고한다.
- `report_coverage()` — `uncovered` 벡터를 만들어 놓고
  `.find(|(_, c)| c.positions.len() == 1)`로 첫 match만 보고한다.
- `program.stray_pipes` / `stray_if_lets` / `stray_results` /
  `result_missing_kw` — 전부 `Vec<usize>`인데 `.first()`만 쓴다.

즉 `analysis`·파서 층은 이미 여러 개를 들고 있고, sema의 보고 함수가 하나로
줄인다. 이 부류는 **`Vec`을 돌려주게 바꾸는 것으로 끝난다.**

### 3. 반면 `Checker::visit_*`는 진짜 조기 리턴이다

`check_try` / `check_let_else` / `check_if_let` / 각종 arm 검사는 방문 도중
`return Err(...)`로 나간다. 이쪽은 "에러를 담고 계속 방문한다"로 바꿔야 하고,
**어디까지 계속할지**의 판단이 필요하다(아래 설계 쟁점 1).

### 4. typed 경로는 이미 `Vec`이다

`src/engine/semantics.rs::report()`는 `Vec<Diagnostic>`을 만든다. 그래서
증상 2의 비대칭이 생긴다. 목표 상태는 "기본 경로도 같은 모양"이다.

## 설계 쟁점 (착수 전에 정할 것)

### 쟁점 1: 어디서 멈추는가 (에러 복구 경계)

에러를 담고 계속 진행하는 것이 **항상** 옳지는 않다.

- enum 선언 자체가 깨졌으면(`invalid type for field`) 그 enum을 쓰는 match의
  소진성은 헛소리가 된다 — 원인 위에 결과를 쌓는 꼴.
- 이미 같은 성격의 규칙이 있다: **오타로 판정된 이름이 있으면 그 match의
  소진성 에러를 억제한다**(`report_resolution`이 `report_coverage`보다 먼저
  돌고, 하나라도 있으면 거기서 끝난다 — "해석되지 않는 패턴에는 물어볼
  소진성 질문이 없다"). 여러 개를 모으게 되면 이 억제를 **match 단위**로
  다시 표현해야 한다(지금은 파일 단위로 자연히 성립하던 것).

### 쟁점 2: 공개 API를 어떻게 두는가

- `compile()`의 시그니처는 유지하는 쪽이 낫다(첫 에러 반환). 소비자가 이미
  있고, "코드를 내놓거나 실패하거나"라는 계약 자체는 옳다.
- 여러 개가 필요한 소비자는 엔진·서버·CLI 진단 출력이다. `check_all()` 같은
  별도 진입점을 두고 `compile()`이 그것의 첫 항목을 쓰는 형태가 자연스럽다.
- `RlError` → `CompileError` 변환은 이미 `compile_mapped`의 클로저
  하나(`to_compile_error`)이므로 `Vec`으로 확장하기 쉽다.

### 쟁점 3: 순서와 상한

- 보고 순서는 **소스 순서**가 기본. 지금 순서(해석 → 소진성 → 단일 match →
  튜플 match)는 "한 개만 낸다"는 전제에서 정해진 것이라 재검토가 필요하다.
- 한 파일이 에러 100개를 내면 그대로 다 낼 것인가. tsc는 다 낸다.
- 기존 테스트는 대부분 `err(src)`로 **첫 에러 하나**를 단언한다
  (`tests/compile.rs`). 순서가 바뀌면 그 단언들이 흔들린다.

### 쟁점 4: `Blocked`를 완화할 것인가 (증상 3의 근본)

증상 3은 "에러가 하나"라서가 아니라 **"rl 에러가 있으면 projection을 포기"**
해서 생긴다. 별도로 고칠 수도 있다:

- 지금: `ProjectedDocument::project()`가 `compile()`을 부르고 실패하면 그
  파일은 스냅샷에 못 들어간다 → 그 파일의 typed 진단 전부 소실.
- 대안: rl 에러가 있어도 **방출은 가능하다**(codegen은 무오류다). 방출된
  코드로 typed 검사를 진행하고 rl 에러를 함께 보고하면 증상 3이 사라진다.
- 위험: rl 에러가 있는 코드의 방출물은 의미가 어긋날 수 있어 **엉뚱한 타입
  에러**를 부를 수 있다. 어떤 rl 에러가 "방출해도 되는" 것인지 분류가 필요하다
  (예: 소진성·중복 암은 방출해도 안전, `try` 위치 제약은 방출 자체가 위험).

이 쟁점은 TASK-117과 분리해 따로 다룰 수도 있다 — 증상 3만 놓고 보면 이쪽이
더 직접적인 해법이다.

## 범위 (착수 시)

- 포함(예정): sema·val이 에러를 모아 돌려준다. `compile()`의 공개 계약은
  유지하고, 여러 개를 얻는 경로를 연다. 기본 경로와 typed 경로의 소진성
  **문안 통일**.
- 제외(예정): 파서의 무오류 계약 변경. 파서는 지금처럼 에러를 내지 않는다.
- 미정: 쟁점 4(`Blocked` 완화)를 여기 포함할지 별도 태스크로 낼지.

## 재현 방법

```sh
# 증상 1·2
rlc --check many.rl          # 1건
rlc --check-types many.rl    # 3건 (문안도 다름)

# 증상 3 — 에디터에 실제로 무엇이 발행되는지
#   LSP 서버를 --stdio로 띄우고 didOpen 후 publishDiagnostics를 수집한다.
#   TASK-116에서 editors/vscode/server/src/test/server.test.ts에 추가한
#   client.waitFor("textDocument/publishDiagnostics", ...)가 그 도구다.
```

## 의사결정

(착수 시 기록)

## 작업 내역

(착수 시 기록)

## 이슈 및 해결

(착수 시 기록)

## 검증

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo test`

## 결과

(착수 시 기록)
