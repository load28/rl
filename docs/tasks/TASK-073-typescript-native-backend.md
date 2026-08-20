# TASK-073: TypeScript 네이티브 백엔드 — 단일 프로젝트 그래프

- **상태**: 완료
- **시작일**: 2026-08-19
- **완료일**: 2026-08-19
- **커밋**: 2d1dbc1

## 목적

rlc의 타입 계층을 TypeScript 5/6의 JS 컴파일러 API(`types_host.mjs`)에서
TypeScript 7 네이티브 컴파일러(typescript-go)의 API 서버로 옮긴다. 동시에
구조를 "파일 하나마다 타입 질문"에서 **하나의 실제 TypeScript 프로젝트
그래프**로 바꿔, `.ts`와 `.rl`이 같은 프로그램 안에서 서로를 본다.

철학은 계약 2의 연장이다:

> **rl은 구문과 rl 고유 의미를 소유하고, TypeScript는 TypeScript 의미를
> 소유한다.** rlc는 TypeScript 타입 시스템을 흉내 내지 않는다.

## 범위

- 포함: typescript-go 로컬 빌드 경로 확립, `src/typescript/` 백엔드 심(seam),
  가상 FS 기반 단일 프로젝트 로딩, TS 진단의 `.rl` 위치 매핑, 리터럴 match
  소진성과 `val` 메서드 판정을 새 백엔드로 이전, 기능 parity 확인 후
  `types_host.mjs` 제거.
- 제외: rl 파서/코드젠 재작성(별도), Language Service 전면 이전(별도 태스크).

## 사전 조사 (실측)

작업 전에 typescript@7.0.2 npm 패키지와 typescript-go HEAD를 직접 실측했다.
Codex가 제시한 설계 초안의 전제 중 몇 가지가 사실과 달라, 그 차이를 먼저
기록한다. 실측 스크립트는 `docs/tasks/TASK-073/`가 아니라 세션 스크래치에서
돌렸고, 결과만 여기 남긴다.

| 항목 | 실측 결과 |
|------|-----------|
| TS 7 API 존재 | `typescript/unstable/{sync,async,fs,ast,proto}` — `API → Snapshot → Project → {program, checker, emitter}` |
| Checker 표면 | `getTypeAtPosition`, `getSymbolAtPosition`, `getTypeOfSymbol`, `getResolvedSignature`, `getPropertyOfType`, `isTypeAssignableTo`, `getTypeArguments`, `typeToString` 등 |
| 가상 FS | `createVirtualFileSystem` + `fs` 콜백(`readFile`/`fileExists`/`directoryExists`/`getAccessibleEntries`/`realpath`) |
| narrowing | `if (state !== "idle")` 안의 **IIFE lowering 내부**에서도 `getTypeAtPosition`이 `"done" \| "loading"`을 반환 |
| `val` 판정 | `Map#set` → `lib.es2015.collection.d.ts`, `Store#set` → 사용자 파일, `any.set` → 심볼 없음 |
| 진단 | `{ fileName, pos, end, code, text }` — **UTF-16 code unit 오프셋** |
| 배치 | `getSymbolAtPosition(file, positions[])` 등 배열 오버로드 존재 |
| emit | 릴리스 7.0.2: `Emitter.printNode`뿐. **HEAD(7.1.0-dev)**: `Program.getDeclarationEmit` / `getJavaScriptEmit` / `emitToString` — 메모리로 받음 |
| 전송 | MessagePack 튜플 `[MessageType u8, method bin, payload bin]`, stdin/stdout. 소스 주석: *"The protocol is unversioned; both sides must be built from the same tree."* |

## 의사결정

### 결정 1: typescript-go를 로컬 빌드해 그 트리의 API 서버를 쓴다

- **상황**: 릴리스된 npm typescript@7.0.2로도 필요한 API가 대부분 존재한다.
  그럼에도 7.1에서 안정화 중인 API(Content Mapper 등)를 쓰려면 HEAD가 필요하다.
- **검토한 대안**: (A) 릴리스 npm 패키지만 사용 — 배포가 쉽고 Go 툴체인이
  필요 없지만, 개발 중인 API는 못 쓴다. (B) typescript-go HEAD를 빌드해 사용 —
  최신 API를 쓸 수 있지만 개발 환경에 Go 툴체인이 필요하다.
- **선택과 근거**: (B). 사용자 지시. 단 **전송 프로토콜이 버전 협상 없이
  "같은 트리"를 요구**하므로, 바이너리와 JS 클라이언트를 **반드시 같은 트리에서**
  가져온다. 배포 시점의 경로는 별도로 정한다(후속 태스크).

### 결정 2: 경로는 하드코딩하지 않고 환경 변수로 해석한다

`RLC_TSGO_ROOT`(typescript-go 체크아웃) / `RLC_TSGO_BIN`(빌드된 tsgo 바이너리)
순으로 해석하고, 둘 다 없으면 `../typescript-go`를 시도한 뒤 실패 시 명확한
진단을 낸다.

### 결정 3: 전송 계층은 트리의 공식 JS 클라이언트를 쓰는 얇은 node 호스트

- **상황**: rlc(Rust)가 tsgo API 서버에 닿는 방법이 필요하다.
- **검토한 대안**: (A) Rust에서 MessagePack 채널을 직접 구현 — 프로세스가
  하나 줄고 node 의존이 사라지지만, 프로토콜에 버전 협상이 없고
  (`syncChannel.ts`: *"The protocol is unversioned; both sides must be built
  from the same tree."*) 클라이언트 코드가 Go 소스에서 **생성**된다
  (`_packages/native-preview/src/api/node/{protocol,node,encoder}.generated.ts`).
  즉 tsgo를 올릴 때마다 Rust 클라이언트를 다시 만들어야 한다. 게다가 서버가
  가상 FS를 **콜백으로 역호출**하므로 콜백 채널까지 구현해야 한다.
  (B) 같은 트리의 JS 클라이언트를 쓰는 node 호스트 — 프로토콜 변화가 트리
  안에서 흡수된다.
- **선택과 근거**: (B). 단 [`backend::TypeScriptBackend`] 뒤에 두어 (A)로
  교체할 수 있게 한다. 이것은 설계 불변 조건 12(TS API 불안정성을 한 계층에
  격리)를 지키는 방향이기도 하다 — 지금 Rust로 내리면 그 불안정성이 rlc
  본체로 올라온다.

### 결정 4: `.rl`은 TypeScript에게 "같은 자리의 `.ts`"로 보인다

가상 FS는 lowering 결과를 `src/x.rl` → `src/x.ts`로 얹고, 디렉터리 열거
(`getAccessibleEntries`)에서도 `.ts`를 보이고 `.rl`은 감춘다. 그래야 사용자의
`tsconfig.json`이 디렉터리를 include할 때 손대지 않은 채로 lowering 결과가
프로그램에 들어온다 — `paths`도, shim 파일도, 합성 tsconfig도 필요 없다.

### 결정 5: rl enum 소진성도 TypeScript에 위임한다

- **상황**: 리터럴 match는 원래 위임하고 있었지만, enum 소진성은 rlc가 자체
  선언 레지스트리(+ TASK-022의 크로스파일 수집)로 판정했다.
- **검토한 대안**: (A) 유지 — tsgo 없이도 동작하고 기존 테스트가 그대로.
  단 narrowing을 반영 못 한다. (B) 위임 — lowering 결과가 discriminated
  union이므로 scrutinee 타입의 `kind` 유니언 구성원을 물으면 된다.
- **선택과 근거**: (B). 실측 차이가 결정적이다 —
  `if (s.kind !== "Point")` 안의 `match (s) { Circle(r) => r }`에 대해
  `rlc --check`는 `missing "Square", "Point"`, `rlc --native-check`는
  `missing "Square"`. 전자는 도달할 수 없는 케이스를 요구하는 오탐이다.
  또 import한 enum은 선언 수집 없이 타입만으로 풀린다.
  기존 경로는 그대로 두고 `Options::defer_to_checker`로 갈랐다.

### 결정 7: `@rl/std`는 프로젝트의 모듈 하나로 들어간다

- **상황**: 레거시 `--types` 호스트는 `@rl/std`를 위해 **모듈 해석 훅**을
  구현했다(`resolveModuleNames`에서 bare 지정자를 가로챈다). TS 7 API는
  파일시스템만 열어주고 해석 훅은 없다.
- **검토한 대안**: (A) 사용자 tsconfig에 `paths`를 주입 — 사용자 설정을
  건드린다. (B) 표준 라이브러리를 프로젝트 루트의 모듈
  (`__rl_std__.ts`)로 얹고, lowering이 `@rl/std`를 그 **상대 경로**로
  재작성한다(`Options::std_import`가 원래 그 용도다).
- **선택과 근거**: (B). 해석 훅이 필요 없어지고, 방출된 `.d.ts`도
  `../__rl_std__.ts`를 가리켜 사이드카 트리가 그 자체로 일관된다. 실측:
  사이드카 트리에 소비자 `.ts`를 놓고 tsgo를 돌려 종료 코드 0 확인.

### 결정 6: `val`의 binding resolution도 심볼 동일성으로

- **상황**: `val.rs`는 토큰 스트림 위의 자체 스코프 모델로 "이 경로가 어느
  바인딩에 뿌리내렸는가"를 판정했다. 섀도잉·재선언·구조분해가 어긋나기 쉬운
  지점이고, 어긋나도 눈에 안 띈다.
- **검토한 대안**: (A) 구문으로 확정 가능한 것은 rlc가 계속 판정. (B) 무엇이
  mutation인지는 rl이 정하고(구문), 어느 바인딩인지는 심볼 id로 판정.
- **선택과 근거**: (B). API 실측에서 `Symbol.id`가 snapshot 전역으로 안정임을
  확인했다(모듈을 넘어도 같은 바인딩이면 같은 id). `val_probes`가 바인딩과
  mutation을 **짝짓지 않은 채** 내놓고, 백엔드가 심볼로 짝짓는다. 백엔드
  질의는 `ValQuery` 대신 범용 `SymbolQuery`로 일반화했다 — 백엔드는
  프리미티브만 답하고 해석은 rl이 한다.

## 작업 내역

- 2026-08-19: 실측 (위 표). `typescript@7.0.2` 설치 후 가상 FS 프로젝트를 띄워
  진단·심볼·타입·narrowing·좌표 단위를 확인.
- 2026-08-19: `microsoft/typescript-go` HEAD를 `/home/user/typescript-go`에
  shallow clone (`c6b013f5`, 2026-08-19). `go build -o built/local/tsgo
  ./cmd/tsgo` — go.mod이 Go 1.26을 요구해 툴체인이 자동 내려받아졌고 3분
  소요. `built/local/tsgo --version` → `Version 7.1.0-dev`. lib 파일은
  `internal/bundled/libs`(108개)로 바이너리에 embed되므로 TypeScript
  서브모듈은 필요 없었다.
- 2026-08-19: 같은 트리의 JS API 패키지 빌드 — `npm ci` 후
  `_packages/native-preview`에서 `npx tsc -b` → `dist/api/sync/api.js`.
- 2026-08-19: `src/typescript/host.mjs` 작성 — 계층형 가상 FS(결정 4),
  단일 프로젝트 열기, 진단 수집, `getTypeAtPosition` 기반 리터럴 소진성,
  `getSymbolAtPosition` 기반 `val` 메서드 판정.
- 2026-08-19: 혼합 프로젝트 스파이크(`.ts` + `.rl` 2개, `include: ["src"]`)로
  인수 조건 2건 실측 통과 (아래 "스파이크 결과").

### 스파이크 결과 (typescript-go HEAD, 7.1.0-dev)

`src/user.ts`가 `State`와 `class Store`를 선언하고, `src/state.rl`과
`src/mutate.rl`이 그것을 import하는 **하나의** 프로젝트:

```
diagnostics: []                                   ← 크로스 파일 해석 성립
literalMissing: [{ index: 0, missing: ["done"] }] ← narrowing 반영
valMutations:
  map.set   → bundled:///libs/lib.es2015.collection.d.ts   ← 내장 = 변경
  store.set → /home/user/rlspike/src/user.ts               ← 사용자 정의 = 허용
```

- `if (state !== "idle")` 안의 `match (state) { "loading" => ... }`가
  `"done"`만 누락으로 보고 `"idle"`은 요구하지 않는다 — lowering이 만드는
  IIFE 안에서도 TypeScript의 narrowing이 유지된다.
- `val` 판정 근거는 이름이 아니라 **선언 위치**다. 내장은
  `bundled:///libs/...`로 식별된다.

- 2026-08-19: Rust 쪽 백엔드 배선 — `src/typescript/`
  (`backend.rs` 심 / `native.rs` 툴체인 해석·호스트 구동 / `mapper.rs` 좌표
  변환 / `project.rs` 프로젝트 조립 / `check.rs` CLI 경로). `--native-check`
  와 `--project`를 추가하고 `serde_json`을 의존성에 넣었다.
- 2026-08-19: 실측 — 혼합 프로젝트에서 세 종류의 진단이 모두 `.rl` 위치로:

  ```
  src/state.rl:5:12: rl: match is not exhaustive: missing "done"
  src/mutate.rl:5:3:  rl: `map` is a val binding: `set` mutates it
  src/mutate.rl:11:9: ts(2322): Type 'number' is not assignable to type 'string'.
  ```

  세 번째 줄은 `const 한글: string = 1;`이다 — UTF-16↔바이트 변환이 맞아야
  나오는 위치다.
- 2026-08-19: enum 소진성 위임 — `rlc::tag_matches` 프로브,
  `Options::defer_to_checker`, host의 `tagChecks`(`getPropertyOfType("kind")`
  → 리터럴 값). 실측: `rlc --check`가 `missing "Square", "Point"`인 자리에서
  `rlc --native-check`는 `missing "Square"`.
- 2026-08-19: `val` binding resolution 위임 — `rlc::val_probes`(바인딩/뮤테이션
  비짝지음), 백엔드 질의를 범용 `SymbolQuery`로, 짝짓기는 `Symbol.id` 비교.
  `defer_to_checker`가 `val::check`도 건너뛴다.
- 2026-08-19: 선언 emit — host의 `emitDeclarations`, `-o <dir>`로 기록.
  `.rl`의 enum이 컴파일러가 emit한 `.d.ts`에서 유니언 타입 + 생성자 const로
  나오는 것을 확인.
- 2026-08-19: `@rl/std` — lowering이 상대 경로로 재작성하고, 프로젝트 그래프에
  `__rl_std__.ts` 모듈을 얹는다(결정 7). 선언 emit에도 포함돼
  `out/__rl_std__.d.ts`가 함께 나온다.
- 2026-08-19: `tests/native.rs` 5건 추가 (단일 프로젝트 그래프 / narrowed
  소진성 / `val` 심볼 판정 / `any` 무판정 / 타입 에러 위치 매핑). tsgo 트리가
  빌드돼 있지 않으면 조용히 skip한다 — 가드는 컴파일러의 해석 규칙을
  그대로 미러링한다.

### parity 실측 (레거시 `--types` vs `--native-check`)

같은 입력(`examples/shapes.rl`)에 두 경로를 돌려 비교했다.

| 항목 | 레거시 | 네이티브 | 판정 |
|------|--------|----------|------|
| 선언 본문 | — | — | **바이트 동일** |
| 파일 이름 | `shapes.rl.d.ts` | `src/shapes.d.ts` | 다름 |
| 레이아웃 | 입력 디렉터리 기준 평탄화 | 프로젝트 루트 미러 | 다름 |
| `@rl/std` 지정자 | `@rl/std` (에디터가 해석) | `../__rl_std__.ts` | 다름 |
| `.d.ts.map` | 있음 (`sources`가 `.rl`) | **없음** | 갭 |
| 배너 | `// @generated ... --sidecar` | 없음 | 다름 |

네이티브 트리는 **그 자체로 해석된다** — 사이드카 트리에 소비자 `.ts`를 놓고
tsgo를 돌려 종료 코드 0을 확인했다. 레거시 트리는 에디터가 `@rl/std`를
해석해 줘야 성립한다. 두 규약은 양립하지 않으므로(파일 이름이 다르면
트리 내부 상대 해석이 깨진다) 통일은 확장 프로그램 변경과 함께 해야 한다 —
Phase 7의 일이다. `.d.ts.map`은 `rlc::build_sidecar`를 재사용해 붙일 수
있지만 그 함수가 `<name>.rl.d.ts` 이름 규약을 전제하므로 같은 결정에 묶인다.

## 이슈 및 해결

### 이슈 1: `DocumentIdentifier`가 `{ fileName }`이 아니다

- **증상**: 스파이크 초기에 `snapshot.getDefaultProjectForFile({ fileName })`이
  계속 `undefined`를 반환하고 `getProjects()`는 `/dev/null/inferred`만 보였다.
- **원인**: `proto.d.ts`의 `DocumentIdentifier`는 `string | { uri }`다.
  `{ fileName }` 객체는 어느 쪽도 아니어서 조용히 빗나갔다.
- **해결**: 경로 문자열을 그대로 넘기고, 프로젝트는 `openProjects`로 연 뒤
  `getProject(tsconfig)`로 집는다.

### 이슈 2: emit API가 없다고 잘못 판단했다

- **증상**: "TS7에는 선언 emit API가 없으니 sidecar를 대체할 수 없다"고 보고했다.
- **원인**: 릴리스된 npm typescript@7.0.2의 `dist/api/sync/api.d.ts`만 보고
  `Emitter`에 `printNode`밖에 없는 것을 확인한 뒤, `Program` 쪽을 확인하지
  않았다. 실제로는 HEAD의 `Program`에 `getDeclarationEmit` /
  `getJavaScriptEmit` / `emitToString`이 있다 (`internal/api/proto.go`의
  `MethodEmit`/`MethodEmitToString` 등).
- **해결**: HEAD 소스에서 확인 후 `emitDeclarations`를 프로토콜에 추가.
  `rlc --native-check -o <dir>`이 컴파일러가 emit한 `.d.ts`를 그대로 쓴다 —
  rlc는 선언 구문을 스스로 만들지 않는다.

### 이슈 3: TypeScript enum에 `match`를 쓰면 생성 코드가 `.kind`를 참조한다

- **증상**: `enum Plain { A, B }`(괄호 없는 케이스 = TS enum)에 `match`를 쓰면
  `Property 'kind' does not exist on type 'Plain'`.
- **원인**: 판별 규칙상 이것은 rl enum이 아니므로 통과되지만, `match` 방출은
  `$rl_m.kind` switch다. **기존 경로도 동일**하다 (`rlc --types`가 같은 진단을
  낸다) — 이번 작업의 회귀가 아니다.
- **해결**: 이번에는 보고 방식만 맞췄다. 글루 코드에 떨어진 진단은 위치 없이
  "rlc bug"라고 하던 것을, 가장 가까운 앞선 verbatim 바이트의 `.rl` 위치로
  보고하고 `(in code rlc generated for this construct)`를 붙인다. 근본 해결
  (`match` scrutinee가 rl enum이 아님을 rl 진단으로 보고)은 이제 타입을 물을
  수 있으므로 가능해졌다 — 후속 태스크.

### 이슈 4: `include`가 디렉터리를 훑으면 가상 모듈이 안 보인다

- **증상**: `tsconfig.json`이 `"include": ["src"]`면 lowering 결과가 프로그램에
  들어오지 않는다.
- **원인**: 파일 목록이 디렉터리 열거로 만들어지는데, 가상 FS가 `readFile`만
  덮어쓰면 열거 결과에는 `.rl`밖에 없다.
- **해결**: `getAccessibleEntries`를 실제 디스크 위에 겹쳐, 각 `.rl` 자리에
  `.ts`가 보이게 하고 `.rl`은 감춘다 (결정 4).

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` (기존 전량 + `tests/native.rs` 13건)

## 결과

핵심 결과: `rlc`의 타입 계층이 TypeScript 7 네이티브 컴파일러의 API 서버
위에서 돌고, `.ts`와 `.rl`이 **하나의 실제 프로젝트 그래프** 안에서 서로를
본다 (`2d1dbc1`). 변경 파일: `src/typescript/{mod,backend,native,mapper}.rs`,
`src/typescript/host.mjs`, `src/main.rs`, `src/lib.rs`, `tests/native.rs`(신규).

범위에 있던 마지막 항목 — "기능 parity 확인 후 `types_host.mjs` 제거" — 는
TASK-075(`0dddbdd`)에서 완료됐고, 에디터/언어 서비스 이전은 예정대로
TASK-074·TASK-076·TASK-077이 이어받아 완료했다. 즉 이 태스크의 범위는
전부 전달된 상태이며, 그 사이 상태 값만 `진행 중`으로 남아 있던 것을
TASK-099에서 정정했다 (전환 완료 사실은 TASK-079 문서에도 기록돼 있다).

후속 태스크: [TASK-100](./TASK-100-ts-enum-match-diagnostic.md) — 위 "이슈 3"의
근본 해결(TS enum을 scrutinee로 쓴 `match`를 rl 진단으로 보고).
