# 구조 리뷰 — "케이스별 코드"가 남아 있는 곳

이 문서는 rlc를 **"문제가 생겼을 때 그 케이스만 막는 코드"가 있는가**라는
관점 하나로 훑은 결과다. 판단 기준은 하나다: *어떤 규칙이 한 곳에서 한 번
정의되고 모든 소비자가 그것을 읽는가, 아니면 필요할 때마다 다시 쓰였는가.*

주장은 전부 재현 가능한 근거를 붙였다. 근거 없는 인상은 적지 않았다.

## 0. 총평

프런트엔드의 핵심 판단은 이미 구조로 옮겨져 있다. 확인한 것들:

- **소진성**은 Maranget usefulness 알고리즘(`analysis/usefulness.rs`)이다.
  태그를 세던 규칙이 중첩 패턴을 "아무것도 커버하지 않음"으로 취급하던
  문제를, 그 케이스를 예외 처리하는 대신 알고리즘 교체로 해결했다
  (TASK-103). rustc 패턴 분석과 같은 계보다.
- **내장 `Option`/`Result`**는 문자열 특례가 아니라 resolver에 들어가는
  선언이다(`resolve/mod.rs::builtin_enums`). 지역 > import > 내장 섀도잉이
  일반 규칙 하나로 처리된다.
- **선언 표**는 resolver의 결과에서 한 번 파생된다
  (`Table::from_resolution`). 소진성·에디터·진단이 같은 표를 읽는다.
- **codegen**은 Core IR만 소비한다(TASK-150/151). parser AST가 방출
  경계에 없다.
- 통과 계약 자체는 실제로 견고하다. 아래 코퍼스 1068개 파일에서
  **변환 결과 바이트 불일치 0건**이다.

즉 "케이스별로 짜였을까"라는 걱정에 대한 답은 **대체로 아니오**다. 어려운
판단들은 이미 규칙 하나로 접혀 있다.

문제는 다른 데 있었다. 케이스별 코드가 아니라, **층 사이의 약속이 한쪽에만
적혀 있는 자리**다: "이 판단은 다른 층이 해 줄 것"이라고 가정해 놓고 그
층이 대답하지 않는 경우를 짝지어 두지 않은 곳. 발견 1(체커가 없으면
소진성이 사라짐)이 그렇고, **절대 불변 원칙 #2가 그것 때문에 깨져 있다.**

절대 불변 원칙 #1(통과 계약)은 **깨져 있지 않다.** 초판은 깨졌다고 적었고
그것은 틀렸다 — 발견 2에 철회 근거를 남겼다. 변환은 실제 TypeScript
1068개에서 바이트 동일하고, conformance 178건 차등 테스트에서 swc와 ts7의
불일치는 0건이었다.

리뷰 중 확인된 사실 하나: 이 저장소의 `cargo test`는 TypeScript 7 툴체인이
없으면 **빨간불이다.** 내 변경 이전, `HEAD`(e11a170)에서 그렇다.
typescript-go를 클론·빌드해 붙이면 673건 전부 초록으로 바뀐다 — 즉
테스트가 잘못된 게 아니라, 툴체인 없는 구성에서 실제로 깨지는 것이 있다.
발견 1이 그것이다.

아래 발견은 심각도 순이다.

---

## 발견 1 (심각) — 엔진 경로에서 소진성 검사가 조용히 사라진다

### 증상

TypeScript 7 툴체인이 없으면 `cargo test`가 실패한다. 내 변경 이전,
`HEAD`(e11a170)에서 그렇다:

```
test result: FAILED. 2 passed; 1 failed
    engine_cache::an_error_node_keeps_its_file_and_other_files_checkable
assertion `left == right` failed   left: 1   right: 2
```

테스트는 파일 두 개(파싱 실패 1 + 소진되지 않은 `match` 1)에서 진단 2건을
기대하는데 1건만 온다. 빠진 쪽은 소진성이다.

typescript-go를 클론해서 빌드하고(`go build -o built/local/tsgo ./cmd/tsgo`,
`npm ci && npx tsc -b _packages/native-preview`) `RLC_TSGO_ROOT`로 붙이면
**673 passed / 0 failed**로 초록이 된다. 그 상태에서 백엔드만 떼고 같은
입력을 돌리면 차이가 그대로 드러난다:

```sh
# 백엔드 없음 — 소진성이 사라진다
$ rlc --check-types ps
rlc: ps/a-blocked.rl:1:18: pipeline: `|>` could not be parsed here ...
rlc: no TypeScript compiler found — ...
rlc: the TypeScript layer did not run — only rl-level diagnostics are shown

# 백엔드 있음 — 둘 다 나온다
$ RLC_TSGO_ROOT=../typescript-go rlc --check-types ps
rlc: ps/a-blocked.rl:1:18: pipeline: `|>` could not be parsed here ...
rlc: ps/b-valid.rl:2:15: match is not exhaustive: missing "B" ...
```

(형제 경로 `../typescript-go`가 자동 탐색되므로, 백엔드 없는 쪽은 그
경로가 보이지 않는 디렉터리에서 돌려야 한다 — 처음 측정했을 때 이것
때문에 A/B가 오염됐다.)

CLI 배치 경로(`--check`)는 백엔드와 무관하게 항상 둘 다 보고한다. 즉
**같은 규칙에 대해 CLI 경로와 엔진 경로의 답이 다르고, 엔진 쪽은 외부
툴체인 유무에 달려 있다.**

경고 문구가 상황을 더 나쁘게 만든다. `main.rs`는 이렇게 말한다:

```
rlc: the TypeScript layer did not run — only rl-level diagnostics are shown
```

바로 위 주석은 한술 더 뜬다 — *"the rl diagnostics above are **complete**,
the typed layer is missing"*. 소진성은 rl-level 진단이고, 지금 빠져 있다.
사용자에게 "rl 진단은 다 보여줬다"고 말하면서 그중 하나를 빠뜨리고 있다.

### 원인

두 곳에서 따로 내린 결정이 만나지 않는다.

`engine/projection.rs`는 문서를 투영할 때 무조건 이렇게 연다:

```rust
// Exhaustiveness and `val`'s pairing are the checker's answers here
defer_to_checker: true,
```

`defer_to_checker`는 `sema::check_all`에서 **rlc 자신의 선언 기반 소진성
보고를 끈다**(`sema.rs`: `if !defer_to_checker { report_coverage(...) }`).
그리고 `engine/semantics.rs`가 체커의 답으로 다시 계산한다:

```rust
for members in &answers.tag_members { ... }   // 알파벳 수집
for file in files {
    let Some(asked) = by_file.get(&file.source_path) else { continue };  // ← 여기
    ...
}
```

백엔드가 없으면 `Project::check`가 `answers`를 `Default::default()`로
둔다. `tag_members`가 비고, `by_file`이 비고, 모든 파일이 `continue`된다.
**소진성 검사가 어디에서도 실행되지 않는다.**

투영 시점(항상 위임)과 체크 시점(백엔드 유무 판정)이 서로를 모른다.
"위임한다"는 결정에 "위임 대상이 없으면 되돌린다"는 짝이 없다.

### 왜 심각한가

세 개의 규범 진술을 동시에 어긴다.

- `CLAUDE.md` 절대 불변 원칙 #2 — "rl 수준 에러(중복 케이스, **소진되지
  않은 match**, 잘못된 필드 타입)는 전부 rlc가 직접 보고한다."
- `docs/design/compiler-core.md` §7 — "backend가 실패해도 rl semantic pass
  전체를 중단하지 않는다: 해당 typed fact를 `Unknown`으로 두고 **독립적으로
  판정 가능한 rl 오류는 계속 보고한다.**" 소진성은 독립적으로 판정
  가능하다 — CLI가 그렇게 하고 있다.
- `engine/project.rs`의 `backend` 필드 doc — "A project without one still
  opens and **still answers the rl layer**; only the typed facts degrade to
  unknown."

실사용 영향: TypeScript 7 툴체인이 없는 환경에서 에디터(`--server`,
VS Code 확장)를 쓰면 **소진성 경고가 나오지 않는다.** 에러가 아니라
침묵이라 사용자가 알아챌 방법이 없다.

### 왜 CI가 못 잡았나

`.github/workflows/ci.yml`이 `typescript@7`을 설치하고 `RLC_TSGO_API`를
설정한다. 백엔드가 있으면 `tag_members`가 차서 테스트가 통과한다. 즉
**게이트가 백엔드 있는 구성 하나만 검증한다.** 백엔드 없는 구성 —
기여자 노트북의 기본 상태이자 이 테스트가 문서화하려던 바로 그 계약 —
은 CI에서 한 번도 실행되지 않는다.

이 테스트는 잘못되지 않았다. 실패가 옳다. 게이트가 그걸 볼 수 없는 곳에
서 있을 뿐이다.

**툴체인을 붙이는 것으로는 이 발견이 닫히지 않는다.** 붙이면 이 저장소의
테스트는 초록이 되고 타입 계층을 실제로 검증할 수 있게 되지만(그래서
붙일 가치가 있다), rl을 쓰는 사용자 쪽 사정은 그대로다: TypeScript 7이
없는 환경의 에디터에서는 여전히 소진성 경고가 조용히 사라진다. 환경
문제와 구조 문제가 같은 증상을 냈을 뿐이고, 고쳐야 할 것은 폴백 규칙의
부재다.

### 구조적 해법

`defer_to_checker`를 투영 시점의 무조건 상수에서 빼고, "체커에게 물어본
답이 있으면 그것을, 없으면 rlc 자신의 답을"이라는 **하나의 폴백 규칙**으로
만든다. `Coverage`가 이미 단일 원천이므로 두 번째 구현이 필요하지는 않고,
엔진이 `checked_coverage` 결과가 빈 파일에 대해 선언 기반 `Coverage`로
떨어지면 된다.

게이트 쪽은 CI 잡을 둘로 나눈다 — 백엔드 있음/없음. 위 테스트가 그
차이를 이미 정확히 짚고 있으므로 매트릭스만 있으면 된다.

---

## 발견 2 — **철회됨.** `verify`의 swc 사용은 문제가 아니었다

초판은 "`verify_output`이 통과 영역까지 검사해서 유효한 TypeScript를
거부한다 — 실제 `.d.ts` 1068개 중 22개(2.1%)"라고 적었다. **이 주장은
틀렸다.** 아래에 근거와 함께 남긴다. 지운 것보다 남긴 것이 다음 사람에게
유용하다.

### 무엇이 틀렸나

거부된 코드가 유효한 TypeScript인지를 확인하지 않았다. `.d.ts`(선언 파일)를
`.rl`로 넣으면 rlc는 그것을 `.ts`로 방출한다. 그런데 `.d.ts`의 앰비언트
문법은 `.ts` **소스**에서는 유효하지 않다. 즉 입력 자체가 잘못됐다.

tsgo(ts7 7.1.0-dev)에 같은 코드를 `.ts`로 물어보면 swc와 **같은 판정**이
나온다:

```
amb.ts(1,14):  error TS1155: 'const' declarations must be initialized.
args.ts(4,11): error TS1215: Invalid use of 'arguments'. Modules are
                             automatically in strict mode.
```

앞은 `export const version: string;`, 뒤는 `(...arguments: T) => R`.
초판이 "swc가 거부하는 유효한 TypeScript"라고 제시한 바로 그 두 예시다.
ts7도 거부한다. swc가 옳았다.

`.tsx` 6건도 마찬가지로 무효한 근거였다. `.tsx`는 애초에 rlc의 입력
확장자가 아니다(`engine/project.rs`: `TS_EXTENSIONS = ["ts", "mts",
"cts"]`), 문서 어디도 JSX 지원을 주장하지 않는다. `.tsx`를 `.rl`로
바꿔 넣은 것은 계약 밖의 입력을 만든 것이다.

### 제대로 된 차등 테스트 결과

swc와 ts7이 **같은 파일에 대해 실제로 갈리는지**를 봐야 했다.
typescript-go의 conformance 스위트에서 단일 파일 케이스 178개를 뽑아
양쪽 판정을 비교했다.

| | 결과 |
|---|---|
| swc가 거부 | 16건 |
| ts7이 문법 오류(TS1xxx)로 거부 | 같은 16건 |
| **swc만 거부한 유효 코드** | **0건** |

swc만 거부한 것처럼 보인 10건은 배치 실행의 축소 보고 때문이었고, 개별로
물으니 ts7이 동일한 코드를 낸다 — `TS1047`(rest 파라미터 optional),
`TS1049`(set 접근자 파라미터 개수), `TS1253`(abstract 멤버 위치). 애초에
전부 TypeScript 자신의 **negative conformance 테스트**, 즉 일부러 무효하게
쓴 파일이다.

**재현 가능한 swc/ts7 불일치를 하나도 찾지 못했다.** 초판의 "swc는 tsc가
아니므로 어긋나는 지점이 곧 계약 위반"이라는 논증은 실측 없이 세운
것이었고, 실측하니 어긋나는 지점이 없었다.

### 살아남는 사실 하나

변환 자체는 실제로 바이트 동일하다 — 코퍼스 1068개 전부. 이건 여전히
참이고, 통과 계약이 실전에서 견고하다는 좋은 증거다. 다만 그것은
`verify`에 대한 고발이 아니라 파서에 대한 칭찬이다.

### 남은 진짜 과제 (별건)

`verify`를 손대야 할 이유가 지금은 없다. 다만 두 가지는 확인해 둘 만하다.

- 에러 문구의 "or an rlc bug"는 사용자에게 판단을 떠넘긴다. 실패 위치를
  `mappings`로 역매핑하면 생성분인지 통과분인지 rlc가 말할 수 있다. 이건
  **정확성 문제가 아니라 문구 문제**이므로 우선순위는 낮다.
- swc 45.0.0은 crates.io 최신이고, `TsSyntax`에는 rlc가 쓰지 않는
  `dts`·`no_early_errors` 플래그가 있다. 지금 필요하지 않지만, 나중에
  선언 파일을 입력으로 받게 되면 그때 `dts: true`가 답이다.

## 발견 3 — 게이트가 한 가지 구성만 지킨다

`tests/passthrough.rs`는 절대 불변 원칙 #1 — 프로젝트의 존재 이유 — 을
`#[test]` 56개로 지킨다. 전부 사람이 하나씩 떠올린 예시다
(`string_prototype_match`, `class_method_named_match`, ...). 즉 계약 검증
자체가 **케이스별로 짜여 있다.**

fuzz 타깃도, property 테스트도, 외부 코퍼스도 없다(`Cargo.toml`에
dev-dependencies 자체가 없다).

정직하게 적자면, **내가 돌린 코퍼스 테스트는 버그를 하나도 찾지 못했다.**
1068개 전부 바이트 동일이었고, conformance 178건 차등도 불일치 0건이었다.
초판은 이 게이트가 발견 2를 놓쳤다고 적었지만 발견 2 자체가 틀렸으므로,
"손으로 고른 56개라서 버그를 놓치고 있다"는 주장에는 지금 근거가 없다.
코퍼스 테스트의 값어치는 버그 발견이 아니라 **회귀 방지망**이다 —
"1068개 바이트 동일"은 지켜야 할 좋은 성질이고, 지금은 아무도 그것을
지키고 있지 않다.

같은 문제가 타입 계층에도 있다. 툴체인 없이 이 컨테이너에서
`tests/native.rs` 38건은 **0.00초에 전부 초록**이었다 — 조용히 스킵된
것이다. typescript-go를 붙이고 다시 돌리니 같은 38건이 **13.51초**에
실제로 실행됐다. 즉 그 초록은 38건분의 검증이 아니라 0건분의 침묵이었다.

CI는 `RLC_REQUIRE_TSGO`로 이 스킵을 실패로 바꾸지만, 그건 툴체인이
*있는* 구성 하나만 지킨다. 툴체인이 없는 구성에서 실제로 무엇이 깨지는지를
아는 테스트는 딱 하나 있고(`engine_cache`의 그 테스트), 그건 그 구성에서
빨간불이다.

정리하면 게이트가 지키는 범위는 이렇다.

| 구성 | `cargo test` | `native.rs` 38건 | 소진성 계약 |
|------|--------------|------------------|-------------|
| 툴체인 있음 (CI) | 673 passed | 실제 실행 (13.5s) | 지켜짐 |
| 툴체인 없음 (기본) | 1 failed | 조용히 스킵 (0.0s) | **깨짐** |

두 줄 중 CI가 도는 건 위 한 줄뿐이다.

### 구조적 해법

게이트에 코퍼스 테스트를 넣는다. 위 재현 절차가 그대로 테스트다: 실제
TypeScript 트리를 `.rl`로 컴파일해 **바이트 동일**을 단언한다. 56개
케이스는 회귀 테스트로 남기되, 계약의 근거는 코퍼스가 진다.

---

## 발견 4 — 릴리스 빌드에서 도는 O(n²) 내부 검사

`analysis/mod.rs::validate_semantic`:

```rust
for analysis in &file.patterns.matches {
    assert!(file.hir.sites.iter().any(|(_, site)| { ... node_span(site.node) ... }));
}
```

match마다 파일 전체 site를 선형 탐색하고, 안쪽에서 `node_span`
(HashMap 조회)을 부른다. `debug_assert!`가 아니라 `assert!`라 **릴리스에서도
돈다.** callgrind 프로파일 상위가 전부 SipHash인 이유다.

측정 (`--check`, release, 3회 최소값):

| 파일 | n=1000 | n=2000 | n=4000 |
|------|--------|--------|--------|
| 순수 TS 통과 | 5 ms | 7 ms | 9 ms |
| `match` n개 | 47 ms | 105 ms | **270 ms** |

통과는 선형인데 match는 배가할 때마다 2.5배씩 는다. 이 루프만 사전
인덱스로 바꿔 실험하니 4000에서 **270 → 196 ms**로 떨어졌다.

에디터 경로도 같은 코드를 탄다. semantic 캐시 키가 파일 전체 내용
해시(`engine/project.rs`)라 **키 입력 한 번마다 그 파일 전체가 다시
검사된다.** 큰 파일에서 타이핑 지연이 제곱으로 는다.

### 구조적 해법

cross-phase 무결성 검사는 `debug_assert!` + 개발 빌드로 옮기고,
남길 것은 사전 인덱스(keyword_off → site)로 O(n)화한다. 근본적으로는
"match와 HIR site를 keyword_off로 다시 맞춰본다"는 것 자체가 ID로
이어져 있어야 할 관계를 오프셋으로 재결합하는 것이다 — HIR이 명시적으로
금지한 방식(`hir/mod.rs`: "nothing in analysis is tempted to use [a span]
as identity")을 검증 코드가 하고 있다.

---

## 발견 5 — `in_function_body`가 구문마다 토큰 0부터 다시 스캔한다

`parser/mod.rs`에서 네 번 반복되는 같은 줄:

```rust
stmt.in_function = crate::flow::in_function_body(self.src, tokens, i);
```

`flow::in_function_body`는 매번 `tokens[0..i]`를 처음부터 훑으며 중괄호
스택을 다시 쌓는다. 그런데 **파서는 이미 그 스트림을 선형으로 걷고
있다.** 알고 있는 상태를 버리고 매번 재계산한다.

측정:

| 파일 | n=1000 | n=2000 | n=4000 |
|------|--------|--------|--------|
| 순수 TS 통과 | 5 ms | 7 ms | 9 ms |
| `try` n개 | 19 ms | 55 ms | **238 ms** |

4000개에서 26배다. 전형적인 "지금 필요한 답만 그 자리에서 구하기".

### 구조적 해법

파서 루프가 함수 본문 깊이를 상태로 들고 다니고, 구문은 그 상태를
읽는다. 네 곳의 중복 호출도 자연히 사라진다.

---

## 발견 6 — 파서의 실패 프로토콜이 세 종류다

TASK-139이 `Claim<T>` (`Parsed` / `NotRl` / `Malformed{error, recovery}`)
커밋 모델을 도입했다. 그런데 실제로 쓰는 건 8개 구문 중 2개다.

| 구문 | 반환 타입 |
|------|-----------|
| `enum`, `match` | `Claim<T>` |
| `try`, let-else, `if let`, pipeline, import | `Option<T>` |
| `result` | 전용 `Attempt<'t>` (4개 변형) |

`Option`을 쓰는 구문들은 "실패 = 통과"만 표현할 수 있어서, "커밋했는데
망가짐"을 호출부(`parser/mod.rs`)가 각자 손으로 판정한다 — `stray_if_lets`,
`recovery_statement_span` 같은 구문별 곁가지가 그 결과다. 진단 카탈로그에도
그 흔적이 남아 있다: `MalformedEnum`/`MalformedMatch`(Claim 계열) vs
`StrayPipe`/`StrayIfLet`/`StrayResult`(Option 계열).

새 모델이 도입될 때 **문제가 났던 두 구문만** 옮겨졌다. 정확히 이 리뷰가
찾는 패턴이다.

### 구조적 해법

나머지 6개를 `Claim`으로 옮기고 `Attempt`를 흡수한다. `Attempt`의
`MissingKeyword`는 `Malformed`의 특수형으로 표현된다.

---

## 발견 7 — dense한 ID를 HashMap으로 조회한다

`hir/ids.rs`는 `Arena<I, T>` — 인덱스 = 정체성인 typed vector — 를 정의해
둔다. 그런데 소스 맵은 그걸 쓰지 않는다:

```rust
pub struct HirSourceMap {
    node_spans: HashMap<NodeId, Span>,
    def_spans: HashMap<DefId, Span>,
    pattern_spans: HashMap<PatternId, Span>,
    ast_origins: HashMap<NodeId, AstOrigin>,
}
```

`resolve::Resolution::uses`/`sites`, `core_ir::lower::temp_ordinals`도
같다(총 8곳). `NodeId(u32)`는 아레나의 조밀한 인덱스라 배열 첨자면
충분한데, 기본 SipHash를 태운다. 발견 4의 프로파일에서 상위 항목이 전부
`hash_one`/`sip::Hasher::write`였던 직접적 원인이다.

### 구조적 해법

이미 있는 `Arena`(또는 `IndexVec<I, Option<T>>`)로 바꾼다. rustc가
`IndexVec`를 쓰는 이유와 같다.

---

## 발견 8 — 토큰 스트림 위에 바인더가 하나 더 있다

`val.rs` 1369줄은 스코프 프레임, 함수 선언 수집, 파라미터 파싱, 구조분해
이름 수집, 접근 경로 해석 — 요컨대 **작은 TypeScript 바인더**를 토큰
스트림 위에 다시 구현한다. `flow/mod.rs`(731줄)도 마찬가지로 HIR이 아니라
토큰 위에서 CFG를 만든다.

HIR·resolver·scope(`ScopeId`, `LocalId`가 이미 선언되어 있다)가 생긴
지금, 의미 분석이 두 층으로 갈라져 있는 셈이다.

공정하게 적자면 이건 **의식적 결정**이고 문서화되어 있다: `val`이 다루는
바인딩과 변경은 통과 TypeScript 영역에 살고, AST는 그걸 일부러 불투명한
바이트 범위로 둔다. 게다가 근사의 한계에는 구조적 탈출구가 있다 —
`probes`가 짝짓기를 하지 않고 넘기면 `--check-types`가 심볼 정체성으로
짝을 짓는다. 직접 찔러 본 섀도잉 케이스(파라미터, catch, 클래스 필드,
템플릿 내부 IIFE, `switch`/라벨/`for-of` 블록)는 전부 옳게 통과했고,
오답은 `with` 블록 하나뿐이었다(모듈에서 금지된 구문).

부수적으로 드러난 것: 렉서가 융합하는 다중 바이트 연산자는 rl 자신의
문법이 필요로 한 것뿐이다(`=>`, `||`, `?.`, `??`, `|>`). 나머지는
`Punct(u8)`이라 `val.rs`가 `+=`, `**=`, `>>>=`, `&&=`를 인접성으로
70여 줄에 걸쳐 다시 조립한다. 토큰 집합이 설계가 아니라 수요를 따라
자란 흔적이다.

### 구조적 해법

당장 급하지는 않다. 다만 `val`이 커지면 두 번째 바인더를 키우는 대신
resolver의 `ScopeId`/`LocalId` 위로 옮기는 것이 방향이다. 렉서의
연산자 융합은 그와 별개로 렉서에서 끝내는 편이 맞다.

---

## 발견 9 — 규범 문서가 두 리팩토링 뒤처져 있다

이게 앞의 일곱 개를 다시 만들어 낼 자리다.

`docs/design/compiler-architecture.md`는 스스로를 규범이라 선언한다:

> 모듈 배치가 이 문서와 어긋나면 **버그로 취급한다.**

그런데 이 문서의 파이프라인은 여전히
`lexer → parser → sema/val → codegen → verify`다. 실제 파이프라인은
`lowered-ir.md`(TASK-150)에 있는 `AST → HIR → resolution → SemanticFile
→ Core IR → TypeScript IR → printer`다. **규범 문서 두 개가 서로
모순되고, 오래된 쪽이 "어긋나면 버그"라고 선언하고 있다.**

`CLAUDE.md`의 아키텍처 맵도 같다:

- 지도에 없는 소스 파일 14개: `hir/{mod,lower,ids}.rs`, `resolve/mod.rs`,
  `flow/mod.rs`, `core_ir/{mod,lower}.rs`, `diagnostics.rs`, `probe.rs`,
  `sidecar.rs`, `analysis/usefulness.rs`, `codegen/{core,rope}.rs`,
  `parser/literals.rs`, `engine/{names,tokens,completions,declarations,hints}.rs`
- 지도에 있는데 없는 파일: `codegen/enums.rs`, `codegen/matches.rs`
  (Core IR 전환 때 사라짐)
- 파이프라인 설명이 `compile()` 실제 코드와 다름
- HIR / resolve / flow / Core IR — **네 개 계층이 통째로 빠져 있음**

작은 것 하나 더: `val.rs`의 doc이 `method_calls`를 "legacy candidate
collector"로 참조하는데 그 함수는 존재하지 않는다.

### 왜 심각한가

CLAUDE.md는 "새 기능은 해당 단계에만 손댄다: 새 구문 = ast + parser,
새 검사 = sema"라고 지시한다. 이 지도를 믿고 작업하면 **HIR과 resolver를
건너뛰고 AST와 sema에 직접 붙이게 된다.** 발견 6(파서 프로토콜 분열)과
발견 8(두 번째 바인더)이 바로 그 모양이다. 잘못된 지도는 잘못된 코드를
계속 생산한다.

### 구조적 해법

`compiler-architecture.md`를 `lowered-ir.md`에 맞춰 갱신하거나, 규범
선언을 넘기고 역사 문서로 강등한다(`rust-rewrite.md`가 이미 받은 처분).
CLAUDE.md 지도와 파이프라인 문단은 실제 계층으로 다시 쓴다. 그리고 지도
동기화를 CI 게이트로 만들 수 있다 — `src/**/*.rs` 목록과 지도의 차집합이
비어 있는지 확인하는 테스트면 충분하다.

---

## 우선순위 제안

| 순위 | 발견 | 이유 |
|------|------|------|
| 1 | 1 — 엔진 소진성 폴백 | 불변 원칙 #2 위반. 툴체인 없으면 `cargo test` 빨간불 |
| 2 | 9 — 규범 문서 동기화 | 나머지를 재생산하는 원인. 코드 변경 없음 |
| 3 | 4 — `validate_semantic` | 릴리스 O(n²), 에디터 지연 |
| 4 | 5 — `in_function_body` | 릴리스 O(n²) |
| 5 | 3 — 게이트 매트릭스 | 백엔드 없는 구성이 CI에서 한 번도 안 돈다 |
| 6 | 6 — Claim 통일 | 구문별 곁가지의 원천 |
| 7 | 7 — Arena 전환 | 측정된 상수 비용 |
| 8 | 8 — 두 번째 바인더 | 의식적 결정, 지금은 유지 가능 |
| — | 2 — verify | **철회.** 실측 결과 문제 없음 |

발견 1이 1순위인 이유는 유일하게 확인된 계약 위반이기 때문이다. 발견 9는
코드를 바꾸지 않으면서 나머지의 재발을 막으므로 그 다음이다. 4·5는 측정된
성능 결함이라 판단이 필요 없다.

## 부록 — 이 리뷰가 쓴 측정 방법

세 가지 다 저장소에 없는 것들이고, 셋 다 게이트로 만들 가치가 있다.

1. **백엔드 없는 구성의 CI 잡** — 발견 1을 잡는다. 셋 중 유일하게 실제
   버그를 잡은 방법이다.
2. **스케일링 벤치** — 구문 n개짜리 파일을 n=1000/2000/4000으로 컴파일해
   순수 통과 대비 기울기를 본다. 발견 4·5를 잡는다.
3. **외부 코퍼스 통과 테스트** — 실제 TypeScript 트리를 `.rl`로 컴파일해
   바이트 동일을 단언. 버그는 못 찾았지만 회귀 방지망으로 값어치가 있다.
   **다만 코퍼스를 계약 안의 입력으로 제한해야 한다** — `.d.ts`나 `.tsx`를
   넣으면 계약 밖을 시험하게 되고, 그게 발견 2를 잘못 세운 원인이었다.

## 부록 B — 로컬에 tsgo 붙이기

이 리뷰의 A/B 측정과 673건 초록 확인에 쓴 절차다. `rlc`는 형제 경로
`../typescript-go`를 자동 탐색하므로 저장소 옆에 두면 환경 변수도 필요 없다
(`typescript/native.rs::Toolchain::resolve`).

```sh
cd ..                                   # rl 저장소의 부모
git clone --depth 1 https://github.com/microsoft/typescript-go.git
cd typescript-go
go build -o built/local/tsgo ./cmd/tsgo     # Go 1.24+, 몇 분 걸린다
npm ci && npx tsc -b _packages/native-preview
built/local/tsgo --version                  # Version 7.1.0-dev
```

두 산출물이 **한 빌드에서 나와야** 한다(`Toolchain::check`가 강제한다):

- `built/local/tsgo` — API 서버로 실행되는 실행 파일
- `_packages/native-preview/dist/api/sync/api.js` — host가 import하는 JS
  클라이언트. `host.mjs`는 그 옆의 `dist/ast/index.js`도 함께 쓴다.

명시적으로 지정하려면 `RLC_TSGO_ROOT=<체크아웃>`, 또는 두 경로를 따로
`RLC_TSGO_API` / `RLC_TSGO_BIN`으로 준다.

측정할 때 주의: 자동 탐색 때문에 "백엔드 없음" 쪽은 `../typescript-go`가
보이지 않는 디렉터리에서 돌려야 한다. 환경 변수만 지우면 형제 경로로
그대로 찾아간다.
