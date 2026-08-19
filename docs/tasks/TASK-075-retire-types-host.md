# TASK-075: `types_host.mjs` 제거 — 타입 경로 단일화

- **상태**: 완료
- **시작일**: 2026-08-19
- **완료일**: 2026-08-19
- **커밋**: `0dddbdd`

## 목적

TypeScript 5/6의 JS 컴파일러 API에 의존하는 `src/types_host.mjs`를 없애고,
타입 계층을 네이티브 백엔드 하나로 만든다. TS 7이 그 API를 더 제공하지
않으므로 이 경로는 유통기한이 있다 (TASK-051 참조).

## 범위

- 포함: `--types`가 하던 일을 네이티브 경로가 전부 할 수 있는지 최종 확인,
  CLI 정리와 마이그레이션 안내, `types_host.mjs`·관련 코드 제거,
  `docs/reference/` 갱신.
- 제외: 에디터 규약(TASK-074/077) — 먼저 끝났다.

## 선행 조건

1. TASK-074/077 완료 (사이드카 규약과 에디터). — 완료.
2. 기능 parity 확인 — **이 태스크의 절반이 여기에 들어갔다.** 아래 참조.

## 실측: parity는 "돌아간다"가 아니다 (2026-08-19)

레거시 `--types`와 네이티브 경로를 같은 픽스처에 나란히 돌렸다. 픽스처 하나에
타입 에러(`.rl`과 손으로 쓴 `.ts` 양쪽), 리터럴 소진성, enum 소진성, `val`
세 규칙, `@rl/std` import, `.rl` 간 import를 모두 넣었다. **네 군데가 어긋났고
넷 다 조용했다** — 없는 진단은 통과처럼 보인다.

| # | 증상 | 원인 | 고친 곳 |
|---|------|------|---------|
| 1 | `match (getShape())`의 소진성이 전혀 보고되지 않음 | 질문을 **스크루티니의 텍스트 위치**에서 했다. `getTypeAtPosition`은 그 위치의 노드를 답하므로 `getShape`(함수)의 타입이 온다 — `kind` 프로퍼티도, 리터럴 구성원도 없다 | `ScrutineeTemp` (아래 결정 1) |
| 2 | `val` **매개변수**가 통째로 안 보임 | 매개변수는 `declare()`를 거치지 않고 프레임에 직접 들어가는데, 프로브 수집이 `declare()`에만 있었다 | `val.rs`의 `param_scope` 지점에서도 수집 |
| 3 | 함수 경계 규칙(`cannot pass val binding ... to mutable parameter ...`)이 안 나옴 | `check_call`이 프로브 모드에서 즉시 `Ok`를 반환했다 | `ValPass` 프로브 — 짝짓기는 다른 규칙과 똑같이 체커가 |
| 4 | `tsconfig.json`이 없으면 손으로 쓴 `.ts`가 검사되지 않음 | 설정이 없을 때 프로젝트는 *열린 파일*로 추론되는데, 낮춘 모듈만 열었다 | `Query::sources` — 설정이 없을 때만 `.ts`를 함께 연다 |

1번은 측정으로 확정했다. 낮춘 코드의 여러 위치에 같은 질문을 던져 봤다:

```
silent    getShape (identifier)      ← 지금까지 물어보던 자리
silent    ( before getShape
silent    inside getShape
silent    ( of the call
silent    ) of the call
REPORTS ["Rect"]  $rl_m at its declaration
REPORTS ["Rect"]  $rl_m in the switch
```

**스크루티니 식(式)의 타입을 물을 수 있는 위치는 없다.** 물을 수 있는 것은
rlc가 만든 임시 변수뿐이고, 그것이 곧 스크루티니의 *값*이다.

## 의사결정

### 결정 1: 임시 변수의 위치를 emitter가 기록한다 (`ScrutineeTemp`)

문제 1을 고치는 방법이 셋이었다.

| 대안 | 장점 | 단점 |
|------|------|------|
| (A) 스크루티니 출력 위치에서 **거꾸로 세어** `$rl_m`을 찾는다 | 코드 변경 없음 | 생성물의 문자열 모양에 의존. 튜플/중첩이 늘면 조용히 깨진다 |
| (B) 방출 텍스트를 정규식으로 훑는다 | 간단 | 같은 문제, 더 심함 |
| (C) **emitter가 기록한다** | 문자열 가정 없음. `match` 키워드 오프셋으로 조인하므로 프로브 walk와 codegen walk를 순서로 맞출 필요도 없다 | `Rope`에 길이 0인 조각(`Piece::Mark`)과 공개 필드 하나 |

(C)로 갔다. `Rope::flatten`이 이제 `(code, mappings, marks)`를 돌려주고,
`MappedEmit::scrutinee_temps`가 `{ src: match 키워드, out: 임시 변수 이름 }`을
담는다. 마크는 텍스트를 만들지 않으므로 **바이트 동일 출력** 불변식이 그대로다
(스냅샷 테스트 214건이 그것을 지킨다). `trim()`은 마크를 잘라 내지 않고 건너뛴다.

### 결정 2: 플래그 이름 — `--native-*`는 과도기 표식이었다

레거시가 사라지면 "native"는 아무 것도 구분하지 않는다.

| 이전 | 이후 | 뜻 |
|------|------|-----|
| `--check` | `--check` | rl 수준만. TypeScript도 node도 필요 없음 |
| `--native-check` | `--check-types` | 거기에 타입 검사까지 |
| `--native-sidecar` / `--types` | `--types` | `--check-types` + 사이드카 |

별칭은 두지 않았다. 1.0 이전이고, `--native-*`를 부르는 곳은 이 저장소의
확장 프로그램뿐이라 같은 커밋에서 고쳤다.

### 결정 3: 진단은 stderr, rlc 자신의 형식으로

`--native-check`는 stdout에 `파일:행:열: rl: 메시지`를 찍었다. 그런데
`cli.md`는 "stdout은 `-p`·`--emit-std`·`--symbols`·`--emit-map`·`help` 전용"
이라고 규정한다 — 규범 문서와 어긋나 있었다.

이제 전부 stderr에 `rlc: 파일:행:열: 메시지`다. **rl 수준 에러는 `--check`와
글자까지 같다** (`rl:` 표식을 뗐다 — 그것은 rlc 자신의 에러이지 층을 구분할
대상이 아니다). 타입 에러만 `ts(코드):`를 단다. 회귀는 테스트로 막았다:
`check()` 헬퍼가 stdout이 비었음을 단언한다.

### 결정 4: 메시지 문구 — 증명한 것만 말한다

레거시가 내던 문구 중 둘이 재현 불가능했다.

- **enum 소진성**: 레거시 `--types`는 sema가 답했으므로 `match on enum Shape
  is not exhaustive`라고 이름을 댔다. 네이티브는 *타입*에서 답하므로 이름이
  없다 → `match is not exhaustive`. 대신 좁혀진 타입을 쓰므로 더 정확하다.
  `--check`는 그대로 이름을 대므로, 두 모드의 문구 차이를 문서에 적었다.
- **built-in 변경 메서드**: 레거시는 `of built-in \`Map\``까지 댔다. TS 7
  API로 수신자 타입의 심볼 이름을 얻으려면 질의 종류를 하나 더 만들어야 하고,
  그것은 이 재설계가 경계하는 "타입을 이름으로 다루기"에 가깝다. 컴파일러가
  답한 것은 "이 메서드는 TypeScript 자신의 것"이므로, 그만큼만 말한다:
  ``cannot call mutating method `set` through val binding `map` (...)``.
  괄호 안 설명은 대입 규칙과 **같은 문장**을 쓴다.

리터럴 소진성 문구(`match on literal union is not exhaustive`)는 레거시
`--types`에만 있던 것이라 **그대로 유지**했다 — 이 문구를 보던 사용자에게는
아무 변화가 없다.

### 결정 5: 종료 코드 셋

`--native-sidecar`는 "썼는가"를 종료 코드로 답했다(타입 에러가 있어도 0).
`--types`는 "검사가 통과했는가"를 답했다(1). 둘 다 필요한 답이라 셋으로 갈랐다.

| 코드 | 의미 |
|------|------|
| 0 | 보고된 것 없음 |
| 1 | 보고됨. `--types`라면 사이드카는 **갱신된 상태** |
| 2 | 검사를 시작할 수 없었음 (rl 수준 에러로 낮출 것이 없음) — 아무것도 쓰지 않았다 |

확장 프로그램은 이제 0/1을 "썼다", 2를 "이전 사이드카를 지켜라"로 읽는다.
파일 mtime을 보는 휴리스틱도 검토했지만, 컴파일러가 이미 아는 사실을
파일 시각으로 되짚는 것이라 버렸다.

### 결정 6: `--types`의 기본 출력 디렉터리는 `.rl-types` 유지

네이티브 경로는 `-o` 없이 소스 옆에 썼고, 레거시 `--types`는 `.rl-types`에
썼다. 사용자의 tsconfig `paths`·`.gitignore`가 후자를 가리키고 있으므로
`--types`는 `.rl-types`를 유지한다. 소스 옆에 두는 배치(에디터 기본)는
`-o`로 명시하며, 확장 프로그램이 그렇게 부르도록 고쳤다.

### 결정 7: `-j, --jobs`는 이 모드에 적용하지 않는다

프로그램은 하나이고 시간은 대부분 체커가 쓴다. 낮추기를 병렬화해도 측정
가능한 이득이 없어 문서에 "영향 없음"이라고 적는 쪽을 택했다.

## 작업 내역

- 2026-08-19: parity 픽스처 작성 후 두 경로 비교 → 위 4건 발견.
- 2026-08-19: `codegen/rope.rs`에 `Piece::Mark`/`push_mark`, `flatten`이 마크를
  반환. `codegen/matches.rs`가 `$rl_m` 앞에 마크를 남긴다. `MappedEmit`에
  `scrutinee_temps` 공개 필드 + 독테스트.
- 2026-08-19: `typescript/project.rs`의 프로브가 `scrutinee_position`(임시
  변수)으로 질문하도록 교체.
- 2026-08-19: `val.rs` — `ValPass` 신설, `check_call`이 프로브 모드에서 수집,
  `param_scope`에서 `val` 매개변수를 바인딩으로 수집. `typescript/check.rs`가
  함수 경계 위반을 보고.
- 2026-08-19: `Query::sources` + `host.mjs`의 `openFiles`에 합류 (설정 없는
  프로젝트의 `.ts` 검사).
- 2026-08-19: `--types`가 표준 라이브러리 선언(`rl.d.ts`)도 쓴다 — 레거시가
  쓰던 파일이고 소비 측 `paths`가 가리킨다.
- 2026-08-19: 플래그 개명, 진단을 stderr로, 종료 코드 3분할, 경로를 cwd 기준
  상대로 (`shown()`).
- 2026-08-19: `src/types_host.mjs` 삭제 + `main.rs`에서 레거시 928줄 제거
  (`types_once`/`types_watch`/`run_types_host`/JSON 파서/`TypeDiagnostic`/…).
- 2026-08-19: 테스트 — `tests/cli.rs`의 guard를 "rlc가 TypeScript를 해석하는가"
  로, `tests/integration.rs`의 guard를 "선언 emit까지 되는가"로 교체
  (`link_typescript`는 확장이 TypeScript를 더 이상 벤더링하지 않으므로 삭제).
  `tests/native.rs`에 회귀 2건 추가.
- 2026-08-19: `docs/reference/{cli,errors,language}.md`, `docs/ai/rl.md`, CI 갱신.

## 이슈 및 해결

### 이슈 1: 첫 픽스처가 `val xs = [...]`였다

- **증상**: `--check-types`가 "generated TypeScript failed to parse"로 실패.
- **원인**: 픽스처가 틀렸다. `val`은 `const`/`let`/`var` **앞**에만 온다
  (`language.md` §10.1). `val xs`의 `val`은 평범한 식별자라 그대로 통과했고,
  그 결과가 유효한 TypeScript가 아니었다 — 통과 계약이 정확히 동작한 것이다.
- **해결**: 픽스처를 `val const xs`로 고쳤다. 컴파일러 쪽 변경 없음.

### 이슈 2: `Rope::trim`이 마크를 잘라 낼 뻔했다

- **증상**: (사전 발견) 마크의 `text()`가 빈 문자열이라, 앞뒤 공백을 걷어내는
  루프가 "전부 공백인 조각"으로 보고 제거한다.
- **해결**: `trim`이 마크를 건너뛰도록 인덱스 기반으로 바꿨다. 현재 `emit_match`의
  마크는 가장자리에 있지 않지만, 잠재 버그를 남기지 않았다.

### 이슈 3: 확장 프로그램의 사이드카 판정

- **증상**: 종료 코드의 의미를 바꾸자 "더 이상 컴파일되지 않는 파일은 마지막
  사이드카를 지킨다" 테스트가 깨졌다.
- **원인**: 확장이 "종료 코드 0 = 썼다"로 읽고 있었는데, 이제 0은 "검사가
  통과했다"다.
- **해결**: 종료 코드 2(결정 5)로 정확히 구분. 중간에 mtime 비교 휴리스틱을
  넣어 봤지만 1초 granularity 여유값 때문에 오판이 생겨(같은 테스트가 잡았다)
  버렸다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` — 전 스위트 통과 (`RLC_TSGO_ROOT` 지정, native 19건 포함)
- [x] 확장 프로그램 `node --test` — 70건, skipped 0 (6회 연속 통과)

## 결과

타입 경로가 하나다. `src/types_host.mjs`는 사라졌고 TypeScript의 JS 컴파일러
API에 의존하는 코드는 저장소에 없다. `--check`는 여전히 TypeScript 없이
돌고, 타입이 필요한 모든 질문은 `--check-types`/`--types`가 진짜 컴파일러에게
묻는다.

parity 작업이 이 태스크의 실질이었다. 옮기는 것 자체는 플래그 이름과 삭제였고,
**조용히 사라진 검사 넷을 찾아 되살린 것**이 내용이다.

변경 파일:

- 삭제: `src/types_host.mjs`, `main.rs`의 레거시 타입 경로(928줄)
- 수정: `src/{lib,val}.rs`, `src/codegen/{rope,matches,mod}.rs`,
  `src/typescript/{check,project,backend,native,host.mjs}`, `src/main.rs`,
  `tests/{cli,integration,native}.rs`,
  `editors/vscode/server/src/{sidecar.ts,test/sidecar.test.ts}`,
  `docs/reference/{cli,errors,language}.md`, `docs/ai/rl.md`,
  `.github/workflows/ci.yml`

### 남은 것

- enum 소진성 메시지가 `--check`와 다르다 (결정 4). rlc의 선언 표와 체커의
  답을 합쳐 이름을 되살릴 수는 있으나, 태그 집합이 같은 enum 둘을 구별할 수
  없어 추측이 된다. 필요해지면 별도 태스크로.
