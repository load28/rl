# TASK-073: TypeScript 7.1 native backend 전환 검토

- **상태**: 완료
- **시작일**: 2026-08-19
- **완료일**: 2026-08-19
- **커밋**: `8231619`

## 목적

`rlc`의 현재 TypeScript 연동 경로를 감사하고, TypeScript 7.1 native compiler
(`typescript-go`) API를 semantic engine으로 쓰는 전환 제안이 현재 코드 구조와
어떻게 맞물리는지 검토한다. rl은 TypeScript 7과 시기를 맞춰 출시하는 방향이므로,
API 안정화를 기다리기보다 현재 HEAD와 nightly API에 실제 연동 smoke test를 붙인다.
전달받은 원 설계 문서를 RL의 현재 아키텍처에 맞게 다듬어 공식 전환 설계와 단계별
실행 계획으로 만든다.

## 범위

- 포함: 현재 `types_host.mjs`/`probe.rs`/`val.rs`/`sema.rs`/`sidecar.rs`의 역할
  분리, 전환 위험도, 단계별 작업 순서 검토.
- 포함: TypeScript 7.1 / `typescript-go` API 안정화 상황의 1차 확인.
- 포함: `typescript-go` HEAD clone/build, asdf 기반 Go 설치 기록, 최소 API smoke test.
- 포함: refined architecture/phase plan 문서화.
- 제외: production native backend 구현, 기존 JS host 제거.

## 의사결정

### 결정 1: 구현 전용 태스크가 아니라 검토 태스크로 시작한다

- **상황**: 전달받은 목표는 `rlc`의 TypeScript semantic backend를 전면 교체하는
  대형 작업이다. 현재 저장소에는 이미 `types_host.mjs`를 통한 JS Compiler API
  연동, emit mapping, sidecar, literal match typed exhaustiveness, `val` typed
  mutation query가 결합되어 있다.
- **검토한 대안**:
  - 바로 `src/typescript/` 계층을 만들고 구현을 시작한다. 장점은 빠르게 형태를
    잡을 수 있다는 점이고, 단점은 현재 host가 담당하는 기능 parity를 놓치기 쉽다.
  - 별도 워크트리/브랜치에서 먼저 역할 목록과 migration boundary를 고정한다.
    장점은 기존 계약을 보존한 채 작은 구현 태스크로 쪼갤 수 있다는 점이고, 단점은
    첫 산출물이 코드가 아니라 검토 문서라는 점이다.
- **선택과 근거**: 검토 태스크를 먼저 둔다. `types_host.mjs`가 선언 방출과 typed
  semantic probe를 동시에 수행하므로, replacement 작업은 API adapter 교체가
  아니라 project graph ownership 재설계다. 기능 목록을 고정하지 않으면
  sidecar/diagnostic mapping/literal match/val 중 하나가 쉽게 퇴행한다.

### 결정 2: backend seam을 먼저 만들고 legacy host는 parity 이후 제거한다

- **상황**: 제안은 `types_host.mjs`를 단순히 TS7 API 호출로 바꾸지 말라고 한다.
  현재 코드도 이 판단과 맞다. `src/types_host.mjs`는 TypeScript Program 생성,
  virtual `.rl` module, `.rl` import 및 `@rl/std` resolution, declaration emit,
  diagnostics, literal match, `val` mutation 판정을 한 프로세스에서 처리한다.
- **검토한 대안**:
  - JS host를 즉시 제거한다. 장점은 구조가 빨리 단순해진다. 단점은 `--types`
    기능과 editor sidecar가 한 번에 깨질 수 있다.
  - `TypeScriptBackend` 계층을 추가하고 JS host를 `LegacyJsBackend`, tsgo를
    `NativeTsBackend`로 나란히 둔다. 장점은 동일 fixture를 두 backend에 걸 수
    있다. 단점은 전환 기간 동안 adapter가 하나 늘어난다.
- **선택과 근거**: 후자를 선택한다. 특히 `src/main.rs`의 `--types` pipeline은
  host 실패 exit code까지 사용자 진단으로 번역하므로, native backend는 먼저
  같은 결과 shape를 제공해야 한다.

### 결정 3: `sema.rs`는 "TS semantic 제거"가 아니라 "RL-only rule 명확화"가 우선이다

- **상황**: 제안은 `sema.rs`에서 TypeScript semantics를 제거하라고 한다. 현재
  `sema.rs`는 enum 중복, wildcard 위치, RL pattern rule, tag enum exhaustiveness,
  `try`/let-else placement처럼 RL lowering의 유효성에 가까운 규칙을 담당한다.
- **검토한 대안**:
  - `sema.rs`를 전면 삭제 또는 TS query로 대체한다.
  - tag enum exhaustiveness와 literal exhaustiveness의 소유권을 구분하고,
    TS가 알 수 없는 RL enum/pattern 구조 검사는 유지한다.
- **선택과 근거**: 유지하되 audit한다. RL enum은 lowering 전 문법/의미 규칙을
  갖고 있고, TypeScript가 원본 RL pattern validity를 알 수 없다. 반면 literal
  match처럼 scrutinee 타입이 본질인 기능은 이미 `probe.rs` + host로 분리되어
  있어 native backend로 옮기는 대상이다.

### 결정 4: TypeScript 7 출시 전이라도 tsgo HEAD 연동 테스트를 시작한다

- **상황**: rl은 아직 출시 전이고 TypeScript 7과 시기를 맞춰 나가는 것이 목표다.
  따라서 안정 API만 기다리면 전환 위험을 늦게 발견한다.
- **검토한 대안**:
  - TypeScript 7.1 stable 이후에만 native backend를 붙인다. 장점은 API churn이
    적다. 단점은 핵심 설계 결함을 늦게 발견한다.
  - 현재 `typescript-go` HEAD와 `typescript@next` API로 smoke/integration test를
    먼저 만든다. 장점은 RL에 필요한 semantic primitive의 실제 존재 여부를 지금
    확인할 수 있다. 단점은 API 이름과 package layout이 바뀔 수 있다.
- **선택과 근거**: 후자를 선택한다. `typescript@next` 7.1 nightly tarball에는
  `dist/api/sync`/`dist/api/async`가 이미 들어 있고, HEAD의 source API도
  `updateSnapshot`, `getTypeAtPosition`, `getSymbolAtPosition`, `emitToString`을
  제공한다. 전환 코드는 adapter 뒤에 숨기면 API churn 비용을 제한할 수 있다.

### 결정 5: Go는 asdf로만 설치하고 `typescript-go` 폴더에 로컬 고정한다

- **상황**: 로컬에는 Node/npm은 있었지만 Go가 없었다. `typescript-go` 공식 빌드
  문서는 Go 1.26 이상을 요구한다.
- **검토한 대안**:
  - 시스템 전역 Go를 설치한다. 장점은 단순하다. 단점은 사용자 환경을 넓게 건드린다.
  - `asdf`의 golang 플러그인으로 Go 1.26 계열을 설치하고 `typescript-go` 폴더에만
    고정한다. 장점은 재현성과 격리가 좋다.
- **선택과 근거**: asdf를 사용한다. `asdf` 0.19는 `asdf local` 대신 `asdf set`을
  쓰므로, `/Users/seominyeong/orca/typescript-go/.tool-versions`에 `golang 1.26.6`을
  기록했다.

### 결정 6: refined 설계는 별도 design 문서로 승격한다

- **상황**: 전달받은 문서는 방향과 금지사항이 풍부하지만, 현재 rl 코드베이스의
  실제 모듈/테스트/전환 순서로 재구성되어 있지는 않다.
- **검토한 대안**:
  - TASK-073 문서 안에만 계획을 계속 확장한다. 장점은 파일이 하나다. 단점은
    태스크 기록과 장기 아키텍처 문서가 섞인다.
  - `docs/design/tsgo-native-backend.md`를 새 규범 설계 문서로 두고, TASK-073은
    조사/결정/검증 기록으로 둔다.
- **선택과 근거**: 별도 design 문서로 승격한다. 이 전환은 여러 태스크에 걸칠
  장기 작업이므로, 후속 TASK들이 같은 설계 문서를 참조해야 한다.

### 결정 7: 첫 opt-in 구현은 기존 JSON job/result shape를 그대로 재사용한다

- **상황**: `rlc --types`의 Rust 경로는 이미 JS host와 고정된 JSON job/result
  shape로 declaration emit, diagnostics, literal match, `val` typed probe를 주고받는다.
- **검토한 대안**:
  - tsgo용 Rust data model과 parser를 즉시 새로 만든다. 장점은 장기 구조에 더
    가깝지만, 첫 검증에서 기존 기능과의 차이를 새 protocol 문제와 구분하기 어렵다.
  - `RLC_TS_BACKEND=tsgo`일 때만 host script를 바꾸고, 입출력 shape는 유지한다.
    장점은 현재 `--types` pipeline의 후단을 그대로 검증할 수 있다. 단점은 임시
    Node source API host가 남는다.
- **선택과 근거**: 후자를 선택한다. 이번 태스크의 목표는 production backend가 아니라
  current HEAD API로 RL에 필요한 semantic primitive와 declaration emit을 실제
  `rlc --types` 경로에 연결해보는 것이다.

### 결정 8: tsgo source API host는 Node strip-types 제약을 피한다

- **상황**: Node 26의 `--experimental-strip-types`는 TypeScript enum을 strip-only
  모드에서 처리하지 못한다. tsgo의 generated `TypeFlags` enum을 직접 import하면
  host가 시작하지 못한다.
- **검토한 대안**:
  - tsgo source tree를 별도로 transpile해 import한다. 장점은 enum 값을 이름으로
    쓸 수 있다. 단점은 smoke test가 외부 빌드 산출물에 더 의존한다.
  - 필요한 enum-like 비트 값만 host 상수로 둔다. 장점은 현재 source API import
    방식과 동일하게 실행된다. 단점은 해당 비트 값이 API churn 대상일 수 있다.
- **선택과 근거**: 상수를 둔다. 이 파일은 opt-in experimental host이고, 값은
  `typescript-go/_packages/native-preview/src/enums/typeFlags.enum.ts`에서 확인한
  `Enum | EnumLiteral` 비트다. production adapter 단계에서는 source API churn을
  protocol boundary에서 다시 흡수해야 한다.

## 작업 내역

- 2026-08-19: 원본 작업트리 `/Users/seominyeong/orca/rl`이 `main`이고
  `origin/main`보다 뒤처져 있음을 확인한 뒤 `git fetch origin`으로 최신
  `origin/main`을 가져왔다.
- 2026-08-19: 새 검토용 워크트리를
  `/Users/seominyeong/orca/rl/.codex-worktrees/tsgo-frontend-review`에 만들고
  브랜치 `codex/tsgo-frontend-review`를 `origin/main`에서 생성했다.
- 2026-08-19: 전달받은 TypeScript 7.1 native backend 전환 제안 전문을 읽고,
  현재 저장소의 `src/types_host.mjs`, `src/probe.rs`, `src/val.rs`, `src/sema.rs`,
  `src/sidecar.rs`를 1차로 확인했다.
- 2026-08-19: upstream 상황을 1차 확인했다. `microsoft/TypeScript#63703`은
  TypeScript 7.1 stable 목표일이 2026-11-10이고, 7.1 항목에 Content Mapper API,
  Emit API, Language Service API 안정화를 명시한다. `microsoft/typescript-go`
  Discussion #455는 API 방향을 IPC/message-passing 기반의 curated API로 설명한다.
  `microsoft/typescript-go#4804`는 `checker.getTypeAtLocation` API가 7.1 milestone
  아래에서 아직 panic regression을 가질 수 있음을 보여준다.
- 2026-08-19: `microsoft/typescript-go#3610`은 checker API가 아직 JS TypeChecker
  전체가 아니라 subset이며, transpiler consumer에게 필요한 API gap이 계속
  논의되고 있음을 보여준다. 따라서 native backend implementation은 API 이름을
  고정 가정하지 않고 조사 태스크와 adapter boundary를 먼저 둬야 한다.
- 2026-08-19: `/Users/seominyeong/orca/typescript-go`에
  `microsoft/typescript-go`를 `--recurse-submodules`로 clone했다. HEAD는
  `c6b013f5706d58582f566df778cc0df2683b58f5`, TypeScript submodule은
  `5848bc5157b22ff7f4e3369f4645a514a433b15f`.
- 2026-08-19: `asdf`에 `golang` 플러그인을 추가하고 Go `1.26.6`을 설치했다.
  `asdf exec go version`이 `go version go1.26.6 darwin/arm64`를 출력했다.
- 2026-08-19: `typescript-go`에서 `npm ci` 후 `asdf exec npm run build`를 실행해
  `built/local/tsgo`를 빌드했다. `./built/local/tsgo --version`은
  `Version 7.1.0-dev`를 출력했다.
- 2026-08-19: HEAD source API를 조사했다. `cmd/tsgo/api.go`는 `--api`, `--cwd`,
  `--callbacks`, `--async`, `--timing`을 받는 API server entrypoint를 제공한다.
  `_packages/native-preview/src/api/sync/api.ts`는 `API.updateSnapshot()`,
  `Program.getSemanticDiagnostics()`, `Program.emitToString()`,
  `Checker.getTypeAtPosition()`, `Checker.getSymbolAtPosition()`,
  `Checker.getResolvedSignature()`를 제공한다.
- 2026-08-19: `tools/tsgo-native-smoke.mjs`를 추가했다. 이 스크립트는
  `typescript-go` source API와 virtual file system을 사용해 cross-file literal
  narrowing, `Map#set` vs user `Store#set` symbol declaration identity,
  declaration emit을 한 번에 확인한다.
- 2026-08-19: smoke script를 실행했다.
  `node --experimental-strip-types --no-warnings --conditions @typescript/source
  tools/tsgo-native-smoke.mjs` 결과:
  - project root가 `/src/user.ts`, `/src/state.ts` 두 파일을 하나의
    `/tsconfig.json` project로 열었다.
  - `state !== "idle"` 이후 `state`의 타입은 `"done" | "loading"`으로 나와
    `"idle"`이 제거된 실제 narrowed type을 확인했다.
  - `map.set`의 declaration path는
    `built/local/lib.es2015.collection.d.ts`, `store.set`은 `/src/state.ts`로 나와
    built-in mutator와 사용자 메서드가 symbol/declaration identity로 구분 가능함을
    확인했다.
  - `Program.emitToString(EmitOnly.OnlyDts)`가 `/src/state.d.ts`,
    `/src/user.d.ts`를 반환했다.
- 2026-08-19: 전달받은 설계를 현재 rl 구조에 맞춰 다듬어
  `docs/design/tsgo-native-backend.md`를 추가했다. 이 문서는 목표 아키텍처,
  backend trait, project graph 모델, source mapping, literal/val semantic query,
  CLI migration, Phase 1~9 실행 계획, risk register, 바로 다음 작업을 정의한다.
- 2026-08-19: `src/tsgo_host.mjs`를 `rlc --types` host result shape에 맞게 추가했다.
  이 host는 `RLC_TSGO_ROOT`의 `typescript-go` source API를 import하고, virtual
  overlay file system으로 generated `.ts`, hand-written `.ts`, `@rl/std` module을
  하나의 tsgo project에 올린다.
- 2026-08-19: `src/main.rs`의 `run_types_host()`에 `RLC_TS_BACKEND=tsgo` opt-in
  switch를 추가했다. 기본값은 기존 `types_host.mjs`이며, tsgo 경로에서만 Node에
  `--experimental-strip-types --no-warnings --conditions @typescript/source`를 붙인다.
- 2026-08-19: tsgo host exit code 2가 legacy TypeScript 미설치 메시지로 먼저
  잡히던 분기를 고쳐, tsgo 경로에서는 `typescript-go` checkout/build 안내가 나오게 했다.
- 2026-08-19: `/private/tmp/rlc-tsgo-input/shapes.rl`로 복사한 예제를
  `RLC_TS_BACKEND=tsgo cargo run -- --types -o /private/tmp/rlc-tsgo-types ...`로
  실행해 `shapes.rl.d.ts`와 std declaration sidecar가 생성됨을 확인했다.
- 2026-08-19: clippy 게이트가 기존 `src/val.rs`의 `uses_val()` match guard에 대해
  `collapsible_match`를 오류로 올려, 동작 변화 없이 guard 형태만 정리했다.

## 이슈 및 해결

### 이슈 1: Orca runtime 연결 실패

- **증상**: `orca repo list --json`이 `runtime_unavailable`을 반환했다.
- **원인**: Orca app runtime이 꺼져 있거나 CLI bootstrap 상태가 stale이었다.
- **해결**: `orca open --json`으로 앱을 기동했지만 연결이 다시 끊겨, 이번 단계는
  Git worktree로 직접 생성했다. Orca UI 등록이 필요하면 후속으로 앱 상태를 다시
  확인한다.

### 이슈 2: 샌드박스가 Git 메타데이터 쓰기를 막음

- **증상**: `git fetch origin`은 `.git/FETCH_HEAD`를 열 수 없었고,
  `git worktree add`는 `refs/heads/codex/tsgo-frontend-review` 생성에 실패했다.
- **원인**: 현재 실행 환경의 파일 시스템 제한이 Git 내부 메타데이터 쓰기를 막았다.
- **해결**: 동일 작업을 승인 경로로 재실행해 원격 갱신과 새 worktree/branch 생성을
  완료했다.

### 이슈 3: tsgo host 초기화 순서 오류

- **증상**: `RLC_TS_BACKEND=tsgo cargo run -- --types ...`가
  `ReferenceError: Cannot access 'cwd' before initialization`로 실패했다.
- **원인**: `resolveTsgoRoot()`가 fallback candidate로 `cwd`를 참조하는데, `cwd`를
  그 뒤에서 초기화하고 있었다.
- **해결**: job parsing 직후 `cwd`를 먼저 계산하도록 순서를 바꿨다.

### 이슈 4: Node strip-only 모드가 tsgo generated enum import를 거부함

- **증상**: tsgo host가
  `TypeScript enum is not supported in strip-only mode`로 실패했다.
- **원인**: `src/tsgo_host.mjs`가 tsgo의 generated `TypeFlags` TypeScript enum 파일을
  직접 import했다.
- **해결**: `TypeFlags` import를 제거하고, literal match에서 필요한
  `Enum | EnumLiteral` 비트만 `TYPE_FLAG_ENUM_LIKE` 상수로 두었다.

### 이슈 5: 예제 폴더의 `.rl`/`.ts` shadow 방지 규칙

- **증상**: `examples/shapes.rl`을 직접 `--types` 입력으로 주면
  `would shadow examples/shapes.ts`로 실패했다.
- **원인**: 예제 폴더에는 같은 stem의 pass-through TypeScript 파일이 있어,
  compiler가 출력 충돌을 막았다.
- **해결**: smoke용으로 같은 `.rl` 파일을 `/private/tmp/rlc-tsgo-input/`에 복사해
  충돌 없는 단일 입력으로 CLI 경로를 확인했다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`
- [x] `node --experimental-strip-types --no-warnings --conditions @typescript/source tools/tsgo-native-smoke.mjs`
- [x] `RLC_TS_BACKEND=tsgo cargo run -- --types -o /private/tmp/rlc-tsgo-types /private/tmp/rlc-tsgo-input/shapes.rl`

`cargo test`는 unit/CLI/compile/emit_map/integration/passthrough/sidecar/stdlib/doc
tests를 모두 통과했다. tsgo backend CLI smoke는 `shapes.rl.d.ts`와 std declaration
sidecar 생성을 확인했다.

## 결과

검토용 브랜치와 워크트리를 만들고, TypeScript 7.1 native backend 전환이 현재
HEAD API로 실험 가능함을 확인했다. 특히 RL에 필요한 첫 semantic primitive인
cross-file narrowed literal type, declaration identity 기반 `val` method 판정,
declaration emit이 모두 하나의 tsgo project graph 위에서 동작한다. refined design은
`docs/design/tsgo-native-backend.md`에 기록했다.

추가로 Phase 1의 최소 구현인 `RLC_TS_BACKEND=tsgo` opt-in path와
`src/tsgo_host.mjs`를 붙였고, 단일 `.rl` 입력의 `--types` sidecar 생성을 확인했다.
아직 production parity는 아니다. 남은 핵심 후속 작업은 relative `.rl` import
resolution, mixed project fixture, tsgo semantic query parity test를 TASK-074 이후
별도 구현 태스크로 쪼개는 것이다.
