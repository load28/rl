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
적혀 있는 자리**다. "이 판단은 다른 층이 해 줄 것"이라고 가정해 놓고 그
층이 대답하지 않는 경우를 짝지어 두지 않은 곳 — 발견 1(체커가 없으면
소진성이 사라짐)과 발견 2(자가 검사가 남의 코드를 검사함)가 그렇다.
그 결과 **두 절대 불변 원칙이 지금 둘 다 깨져 있다.**

리뷰 중 확인된 사실 하나: 이 저장소의 `cargo test`는 **지금 빨간불이다.**
내 변경 이전, `HEAD`(e11a170)에서 그렇다. 발견 1이 그 원인이다.

아래 발견은 심각도 순이다.

---

## 발견 1 (심각) — 엔진 경로에서 소진성 검사가 조용히 사라진다

### 증상

이 컨테이너에서 `cargo test`가 **빨간불이다.** 내 변경 이전, `HEAD`
(e11a170)에서 그렇다:

```
test result: FAILED. 2 passed; 1 failed
    engine_cache::an_error_node_keeps_its_file_and_other_files_checkable
assertion `left == right` failed   left: 1   right: 2
```

테스트는 파일 두 개(파싱 실패 1 + 소진되지 않은 `match` 1)에서 진단 2건을
기대하는데 1건만 온다. 빠진 쪽은 소진성이다. 같은 두 파일을 CLI로 돌리면
둘 다 보고된다:

```sh
$ rlc --check ps
rlc: ps/a-blocked.rl:1:18: pipeline: `|>` could not be parsed here ...
rlc: ps/b-valid.rl:2:15: match on enum E is not exhaustive: missing "B" ...
```

즉 **같은 규칙에 대해 CLI 배치 경로와 엔진 경로의 답이 다르다.**

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

### 구조적 해법

`defer_to_checker`를 투영 시점의 무조건 상수에서 빼고, "체커에게 물어본
답이 있으면 그것을, 없으면 rlc 자신의 답을"이라는 **하나의 폴백 규칙**으로
만든다. `Coverage`가 이미 단일 원천이므로 두 번째 구현이 필요하지는 않고,
엔진이 `checked_coverage` 결과가 빈 파일에 대해 선언 기반 `Coverage`로
떨어지면 된다.

게이트 쪽은 CI 잡을 둘로 나눈다 — 백엔드 있음/없음. 위 테스트가 그
차이를 이미 정확히 짚고 있으므로 매트릭스만 있으면 된다.

---

## 발견 2 (심각) — `verify`가 통과 영역까지 검사한다: 불변 원칙 #1 위반

### 증상

실제 npm 패키지의 `.d.ts` 1068개를 `.rl`로 두고 컴파일하면 **22개(2.1%)가
거부된다.** 전부 유효한 TypeScript다.

```
rlc: f293.rl:1:14: generated TypeScript failed to parse:
     'const' declarations must be initialized.
rlc: f371.rl:35:17: generated TypeScript failed to parse:
     'eval' and 'arguments' cannot be used as a binding identifier in strict mode.
```

해당 소스 줄:

```ts
export const version: string;                      // f293:1  — 앰비언트 선언
fn: (...arguments: ArgumentsType) => ReturnType,   // f371:35 — 타입 위치의 arguments
```

둘 다 tsc가 받는 코드다. 그리고 이 파일들에는 rl 구문이 **하나도 없다** —
`--no-verify`로 컴파일하면 출력이 입력과 바이트 단위로 같다. 변환은
옳고, 자가 검사만 틀렸다.

동일 원인으로 `.tsx` 6개도 거부된다(`verify.rs`가 `tsx: false`).

### 재현

```sh
find / -path '*node_modules*' -name '*.d.ts' | \
  awk '{printf "cp %s corp/f%d.rl\n", $0, NR}' | sh
rlc -o out corp            # 22건 실패
rlc --no-verify -o out corp && # 1068/1068 바이트 동일
```

### 왜 구조 문제인가

`verify_output`은 **rlc가 생성한 코드**가 아니라 **출력 전체**를 swc로
파싱한다. 모듈 doc이 그 의도를 명시한다: "and that passthrough code was
valid TS to begin with".

그 순간 rlc는 "내가 만든 코드가 올바른가"를 검사하는 도구가 아니라
**tsc가 아닌 파서로 만든 TypeScript 문법 검사기**가 된다. swc와 tsc가
어긋나는 모든 지점이 곧 불변 원칙 #1 위반이다. 앰비언트 문맥, strict-mode
바인딩 규칙, JSX — 셋 다 이미 어긋났고, swc를 올릴 때마다 새로 생길 수 있다.

에러 문구가 이 설계 결함을 자백하고 있다:

> This is either invalid TypeScript passed through from the source **or an
> rlc bug**

컴파일러가 둘을 구분하지 못한다고 사용자에게 말하고 있다. 그런데 rlc는
구분할 수 있다 — `MappedEmit::mappings`와 `anchors`가 어떤 바이트가 원문
복사이고 어떤 바이트가 생성분인지 이미 알고 있다.

### 구조적 해법

`verify`의 책임을 **rlc가 생성한 바이트**로 한정한다. 실패 위치를
`mappings`로 역매핑해서 verbatim 구간에 떨어지면 rlc 에러가 아니다 —
그 파일은 원래 tsc의 몫이다. 부수 효과로 에러 문구에서 "or an rlc bug"가
사라지고, verify 실패는 항상 진짜 컴파일러 버그를 뜻하게 된다.

케이스별 대응(앰비언트 플래그 켜기, tsx 켜기, strict 끄기)은 하지 말아야
한다. 그건 다음 swc 릴리스에서 같은 종류의 버그를 다시 만든다.

---

## 발견 3 (심각) — 계약 검증이 손으로 고른 56개 케이스다

`tests/passthrough.rs`는 절대 불변 원칙 #1 — 프로젝트의 존재 이유 — 을
`#[test]` 56개로 지킨다. 전부 사람이 하나씩 떠올린 예시다
(`string_prototype_match`, `class_method_named_match`, ...). 즉 계약 검증
자체가 **케이스별로 짜여 있다.**

fuzz 타깃도, property 테스트도, 외부 코퍼스도 없다(`Cargo.toml`에
dev-dependencies 자체가 없다). 발견 2가 지금까지 잡히지 않은 이유가
정확히 이것이다.

같은 문제가 타입 계층에도 있다. 이 컨테이너에서 `tests/native.rs` 38건은
**0.00초에 전부 초록**이다 — TypeScript 7 툴체인이 없어 조용히 스킵된다.
CI는 `RLC_REQUIRE_TSGO`로 이 스킵을 실패로 바꾸지만, 그건 툴체인이
*있는* 구성 하나만 지킨다. 툴체인이 없는 구성에서 실제로 무엇이 깨지는지를
아는 테스트는 딱 하나 있고(`engine_cache`의 그 테스트), 그건 지금 빨간불이다.

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
| 1 | 9 — 규범 문서 동기화 | 나머지를 재생산하는 원인. 코드 변경 없음 |
| 2 | 1 — 엔진 소진성 폴백 | `cargo test`가 지금 빨간불. 불변 원칙 #2 위반 |
| 3 | 2 — verify 책임 축소 | 불변 원칙 #1을 실제로 깨고 있음 |
| 4 | 3 — 코퍼스 테스트 | 2를 잡았어야 했던 게이트 |
| 5 | 4 — `validate_semantic` | 릴리스 O(n²), 에디터 지연 |
| 6 | 5 — `in_function_body` | 릴리스 O(n²) |
| 7 | 6 — Claim 통일 | 구문별 곁가지의 원천 |
| 8 | 7 — Arena 전환 | 측정된 상수 비용 |
| 9 | 8 — 두 번째 바인더 | 의식적 결정, 지금은 유지 가능 |

발견 9를 먼저 고쳐야 1과 2의 수정이 올바른 계층에 들어가고, 3이 있어야
2의 수정이 회귀하지 않는다. 1과 2는 성격이 같다 — 둘 다 "이 판단은 다른
층이 해 줄 것"이라고 가정했는데 그 층이 대답하지 않는 경우를 짝지어 두지
않은 것이다.

## 부록 — 이 리뷰가 쓴 측정 방법

세 가지 다 저장소에 없는 것들이고, 셋 다 게이트로 만들 가치가 있다.

1. **외부 코퍼스 통과 테스트** — 실제 TypeScript 트리를 `.rl`로 컴파일해
   바이트 동일을 단언. 발견 2를 잡는다.
2. **스케일링 벤치** — 구문 n개짜리 파일을 n=1000/2000/4000으로 컴파일해
   순수 통과 대비 기울기를 본다. 발견 4·5를 잡는다.
3. **백엔드 없는 구성의 CI 잡** — 발견 1을 잡는다.
