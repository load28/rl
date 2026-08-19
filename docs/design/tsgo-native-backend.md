# TypeScript 7 native backend 전환 설계

이 문서는 `rlc`를 TypeScript 7.1 native compiler(`typescript-go`, 이하 tsgo)의
semantic/project backend에 연결하기 위한 전환 설계다. 기존 `types_host.mjs`를
단순히 다른 프로세스로 바꾸는 계획이 아니라, RL 컴파일러의 책임 경계를 다음
문장으로 재정렬한다.

> RL owns syntax and RL-only semantics. TypeScript owns TypeScript semantics.

즉 rlc는 RL 구문을 ordinary TypeScript로 낮추고, TypeScript 타입 의미는 하나의
실제 TypeScript project graph 안에서 tsgo가 소유한다.

## 배경

현재 `rlc --types`는 Rust 쪽이 RL 파일을 virtual `.ts`로 컴파일한 뒤,
embedded Node script인 `src/types_host.mjs`를 실행한다. 이 host는 TypeScript JS
Compiler API로 `Program`을 만들고 다음을 한꺼번에 수행한다.

- virtual RL module 등록
- hand-written `.ts` 파일을 같은 program에 포함
- `.rl` 상대 import와 `@rl/std` resolution
- TypeScript diagnostics
- declaration emit
- literal `match` typed exhaustiveness
- `val` method mutation typed query

이 구조는 "rlc가 TS 타입 시스템을 직접 구현하지 않는다"는 방향을 잘 지켰지만,
TypeScript 7 native compiler로 넘어가면 JS Compiler API 자체가 더 이상 최종
authority가 아니다. TypeScript 7.1 시점의 API는 IPC/server 기반이므로, RL은
backend 경계를 명시적으로 가져야 한다.

## 확인된 tsgo API

2026-08-19 기준 `microsoft/typescript-go` HEAD(`c6b013f...`)를 로컬에 clone하고
asdf Go `1.26.6`으로 빌드했다. `built/local/tsgo --version`은
`Version 7.1.0-dev`를 출력했다.

HEAD의 native-preview source API에서 확인한 핵심 surface:

- `tsgo --api --cwd <dir> --callbacks=...`: IPC API server entrypoint.
- `API.updateSnapshot({ openProject })`: project snapshot 생성.
- `Program.getSemanticDiagnostics()`, `getSyntacticDiagnostics()`,
  `getConfigFileParsingDiagnostics()`: diagnostics.
- `Checker.getTypeAtPosition()`, `getTypeAtLocation()`,
  `getTypeOfSymbolAtLocation()`: typed semantic query.
- `Checker.getSymbolAtPosition()`, `getSymbolAtLocation()`,
  `getResolvedSignature()`: symbol/signature query.
- `Program.emitToString(EmitOnly.OnlyDts)`, `getDeclarationEmit()`: declaration emit.
- `LanguageService.getCompletionsAtPosition()` 등 language service entrypoint.

`tools/tsgo-native-smoke.mjs`는 virtual FS 위에서 아래 세 사실을 확인한다.

- hand-written `user.ts`와 generated-style `state.ts`가 하나의 tsgo project에 들어간다.
- `state !== "idle"` 이후 type query가 `"idle"`을 제외한 narrowed literal union을 준다.
- `Map#set`은 TypeScript lib declaration으로, user `Store#set`은 source declaration으로 resolve된다.
- `.d.ts` emit이 `Program.emitToString(EmitOnly.OnlyDts)`로 가능하다.

따라서 literal match와 `val`의 typed half는 현재 tsgo API로 구현 가능한 범위에
들어와 있다.

## 비목표

- TypeScript type checker, module resolver, control-flow narrowing을 RL에서 구현하지 않는다.
- TS 타입 문자열을 파싱해 semantic verdict를 만들지 않는다. 문자열은 로그와
  테스트 가시화에만 쓴다.
- tsgo 내부 Go package를 직접 import하는 구조를 기본으로 삼지 않는다.
- tsgo 내부에 `checkRlMatch`, `checkRlValMutation` 같은 RL 전용 patch를 넣지 않는다.
- 기존 `types_host.mjs`를 parity 없이 먼저 제거하지 않는다.

## 목표 아키텍처

```
.rl / .ts project
      │
      ▼
RL frontend
  lexer/parser
  RL-only validation
  val structural analysis
      │
      ▼
Lowering
  ordinary TypeScript virtual files
  source maps / offset maps
      │
      ▼
TypeScriptBackend
  project lifecycle
  virtual file updates
  diagnostics
  semantic query batches
  emit
      │
      ▼
NativeTsBackend
  tsgo API server / IPC
  one TypeScript project graph
```

### 책임 분리

| 책임 | 소유자 |
|------|--------|
| RL 구문 판별과 passthrough 계약 | `lexer`, `parser` |
| RL-only validation | `sema`, `val` structural half |
| ordinary TypeScript lowering | `codegen` |
| source ↔ generated offset map | `EmitMapping`, 후속 Content Mapper adapter |
| TypeScript diagnostics | `TypeScriptBackend` |
| literal match finite union 판단 | `TypeScriptBackend` query |
| `val` built-in mutator 판단 | `TypeScriptBackend` query |
| declaration emit | `TypeScriptBackend` emit |
| hover/completion/definition/references | `TypeScriptBackend` language service |

## 모듈 배치

새 backend 계층은 `src/typescript/`에 둔다.

```
src/typescript/
  mod.rs
  backend.rs      trait와 backend-neutral data model
  project.rs      RL/TS input collection, virtual file set, tsconfig model
  mapper.rs       RL source ↔ generated TS position mapping
  semantic.rs     literal/val query request and response types
  emit.rs         declaration/JS emit response types
  native.rs       tsgo backend orchestration
  protocol.rs     tsgo host protocol serialization/parsing
```

초기에는 Node source API host를 subprocess로 둔다.

```
src/tsgo_host.mjs
```

이 host는 Rust가 넘긴 backend-neutral job을 받아 tsgo API를 호출한다. 장기적으로
Rust가 직접 IPC protocol을 말할 수 있게 되면 `native.rs`에서 Node host를 우회할
수 있지만, RL 쪽 semantic logic은 그 전환과 무관해야 한다.

## Backend trait

개념적 trait는 아래 능력을 제공한다. 실제 Rust signature는 구현하면서 조정한다.

```rust
trait TypeScriptBackend {
    fn open_project(&mut self, project: TsProjectInput) -> Result<TsProjectHandle, TsBackendError>;
    fn diagnostics(&mut self, project: TsProjectHandle) -> Result<Vec<TsDiagnostic>, TsBackendError>;
    fn query_semantics(
        &mut self,
        project: TsProjectHandle,
        queries: SemanticQueryBatch,
    ) -> Result<SemanticQueryResults, TsBackendError>;
    fn emit(&mut self, project: TsProjectHandle, request: EmitRequest)
        -> Result<EmitResult, TsBackendError>;
}
```

중요한 점은 `sema.rs`, `val.rs`, `probe.rs`가 tsgo protocol을 알면 안 된다는
것이다. 이들은 계속 "질문"을 만들고, backend가 답한다.

## Project graph 모델

TypeScript 쪽은 한 project graph만 본다.

```
src/user.ts
src/state.rl
      │
      ▼
virtual src/state.ts

tsgo project:
  src/user.ts
  virtual src/state.ts
  virtual __rl_std__.ts (필요 시)
```

현재 JS host가 custom module resolution으로 처리하던 두 가지는 Native backend의
핵심 이슈다.

1. `@rl/std`
   - tsgo project config의 `paths` 또는 VFS overlay로 해결한다.
   - smoke 이후 첫 구현 대상이다.

2. relative `.rl` import
   - 현재 `--types`는 declaration specifier 보존을 위해 lowering 시
     `rewrite_imports: Off`를 쓴다.
   - JS host는 `resolveModuleNames` hook으로 `"./x.rl"`을 virtual `"./x.ts"`로
     매핑한다.
   - tsgo source API의 현재 public surface에는 JS Compiler API 같은
     `resolveModuleNames` hook이 없다. 대신 VFS overlay와
     `allowArbitraryExtensions`를 사용하고, 각 virtual RL module에 대해
     `x.d.rl.ts` shim을 제공한다.
   - shim은 `export * from "./x"` 형태로 generated virtual `.ts` module을 다시
     노출한다. generated text에 default export가 있으면 `export { default } from
     "./x"`도 함께 추가한다. 이러면 TypeScript resolver는 `"./x.rl"`을 찾을 수 있고,
     declaration emit은 사용자가 쓴 source specifier `"./x.rl"`을 그대로 보존한다.
   - 이 방식은 named/default export 중심의 현재 RL module graph parity를 제공한다.
     package boundary 사례는 별도 fixture로 확장해야 한다.

## Source mapping

현재 `EmitMapping`은 source byte offset ↔ generated byte offset을 갖는다. 기존
`--types`는 TypeScript diagnostic의 generated position을 다시 `.rl` position으로
되돌릴 때 이 mapping을 사용한다.

Native backend도 처음에는 이 mapping을 그대로 사용한다.

```
RL byte offset
  → generated TS byte offset
  → generated TS UTF-16 offset
  → tsgo query/diagnostic
  → generated TS UTF-16 line/column
  → generated TS byte offset
  → RL byte offset
```

Content Mapper API가 안정화되면 `mapper.rs`가 tsgo Content Mapper 입력을
생성하도록 확장한다. 이때도 `EmitMapping`은 폐기하지 않고 source of truth로
재사용한다.

## Semantic query 설계

### Literal match

입력:

- generated module path
- scrutinee generated UTF-16 range or representative position
- covered literal values
- RL diagnostic source location

Backend:

- tsgo checker의 actual narrowed type을 조회한다.
- finite literal set인지 타입 객체/constituent로 판단한다.
- string/number/boolean literal로 확정될 때만 missing을 반환한다.
- `any`, `unknown`, `string`, `number`, type parameter, `"a" | string` 등은
  diagnostic 없음.

출력:

- missing literal list
- no verdict

문자열 `typeToString`은 테스트 출력에만 사용하고 verdict에는 쓰지 않는다.

### `val` method mutation

입력:

- generated module path
- method identifier position/range
- method name, binding name
- RL diagnostic source location

Backend:

- method identifier symbol을 조회한다.
- declaration owner가 TS default library의 known mutable built-in인지 확인한다.
- user-defined `set`, `push`, `add`는 허용한다.
- `any`, unresolved, union 일부만 mutating 등 확실하지 않은 경우 허용한다.

출력:

- built-in receiver name when provable mutation
- no verdict

## CLI 모드 재정리

장기적으로는 다음 구분으로 간다.

| 명령 | 의미 |
|------|------|
| `rlc compile` 또는 현재 build 경로 | RL → ordinary TypeScript tree |
| `rlc check` | RL lowering + native TS project diagnostics |
| `rlc build` | RL lowering + native TS emit |
| `rlc --types` | 호환 alias. native declaration sidecar pipeline로 migration |

당장 CLI 표면을 크게 바꾸지는 않는다. 우선은 `--types` 내부 backend만 바꿀 수 있게
한다.

초기 선택 방식:

```sh
RLC_TS_BACKEND=legacy-js rlc --types src
RLC_TS_BACKEND=tsgo RLC_TSGO_ROOT=../typescript-go rlc --types src
```

기본값은 parity 확보 전까지 `legacy-js`다.

## 단계별 실행 계획

### Phase 1 — Native backend spike

목표: 현재 tsgo HEAD API로 RL이 필요한 primitive가 실제로 가능한지 고정한다.

- asdf Go 설치와 `typescript-go` HEAD build 기록.
- `tools/tsgo-native-smoke.mjs` 유지.
- `src/tsgo_host.mjs` 추가: 기존 `types_host.mjs` job subset을 받아 tsgo API 호출.
- `RLC_TS_BACKEND=tsgo`로 `--types`에서 선택 가능하게 연결.
- 테스트:
  - cross-file narrowed literal union.
  - `Map#set` error.
  - user `Store#set` no error.
  - declaration emit.

완료 기준: native backend opt-in tests가 통과하고, known limitation이 문서화된다.

### Phase 2 — Backend seam

목표: JS host와 tsgo host를 `TypeScriptBackend` abstraction 뒤로 이동한다.

- `src/typescript/backend.rs` trait 추가.
- 기존 `run_types_host` job/result shape를 backend-neutral data로 이름 변경.
- `LegacyJsBackend`와 `NativeTsBackend` adapter 추가.
- `probe.rs`/`val.rs`는 계속 query 생성만 담당.

완료 기준: legacy-js tests는 그대로 통과하고, tsgo opt-in tests는 같은 fixture를
공유한다.

### Phase 3 — Project graph parity

목표: `.rl` imports, `@rl/std`, hand-written `.ts`, generated virtual TS가 하나의
native TS project graph에 들어간다.

- `@rl/std` resolution parity.
- relative `.rl` import resolution은 VFS overlay의 `x.d.rl.ts` shim으로 1차 확정.
  default export fixture도 통과했다. package boundary 사례는 추가 fixture로 확장한다.
- tsconfig loading / project references / paths / package resolution parity fixture 추가.
- module resolution을 RL에서 재구현하지 않고 tsgo API boundary로 처리한다.

완료 기준: `.rl + .ts mixed project`가 native backend에서 JS host와 같은 diagnostics
및 declaration emit을 낸다.

### Phase 4 — Mapping and diagnostics

목표: 모든 TS diagnostic을 `.rl` source position으로 돌려보낸다.

- 현재 `EmitMapping` 기반 reverse mapping을 `mapper.rs`로 이동.
- tsgo diagnostic shape parser 추가.
- Content Mapper API가 사용 가능하면 adapter 추가.
- source mapping regression tests 추가.

완료 기준: generated TS path가 사용자 diagnostic에 노출되지 않는다.

### Phase 5 — Literal match migration

목표: literal match typed exhaustiveness를 native semantic query로 옮긴다.

- actual narrowed type query 사용.
- finite literal set 판정은 type flags/constituents 기반.
- type string parsing 금지.
- uncertainty → no diagnostic 정책 고정.

완료 기준: narrowed literal match, cross-file literal union, generic/open type false
positive 방지 테스트 통과.

### Phase 6 — `val` migration

목표: built-in mutating method 판정을 native symbol/declaration identity로 옮긴다.

- method symbol declaration path/source metadata 사용.
- default library owner + known mutator table 판정.
- user-defined same-name methods 허용.
- `any`/unknown/unresolved 허용.

완료 기준: `Map#set`, `Array#push`는 error, user `set`/`push`는 no error.

### Phase 7 — Emit migration

목표: declaration emit과 가능하면 JS/source map emit을 tsgo API로 맡긴다.

- `Program.emitToString(EmitOnly.OnlyDts)` 또는 selected declaration emit 사용.
- sidecar generation은 mapping quality가 충분할 때만 단순화.
- 기존 `sidecar.rs`는 parity 전까지 유지.

완료 기준: `.rl` export declarations, std declarations, declaration maps parity.

### Phase 8 — Language service

목표: editor sidecar 중심 구조를 native language service 중심 구조로 점진 전환한다.

- hover, completion, definition, references, rename, signature help query를 adapter로 제공.
- `.rl` source ↔ virtual TS position mapping 적용.
- VSCode sidecar path는 fallback으로 유지.

완료 기준: editor 기능이 하나의 native TS project graph를 공유한다.

### Phase 9 — Legacy 제거

목표: native backend parity 확보 뒤 legacy JS Compiler API 의존을 제거한다.

- `types_host.mjs` 제거.
- TypeScript 5/6 JS API 안내 제거.
- docs/reference/cli.md와 docs/ai/rl.md 갱신.

완료 기준: native backend가 기본값이고 전체 테스트가 통과한다.

## Risk register

| 위험 | 대응 |
|------|------|
| tsgo API churn | `NativeTsBackend`/host에 격리하고 smoke test를 CI 후보로 유지 |
| relative `.rl` module resolution hook 부재 | 공식 API 조사 후 gap 문서화, 필요 시 generic upstream API 요청 |
| IPC chattiness | literal/val query batch 유지 |
| source mapping drift | `EmitMapping`을 source of truth로 유지, Content Mapper는 adapter |
| false positive diagnostics | certainty required 정책을 테스트로 고정 |
| declaration emit 차이 | legacy-js/native output fixture 비교 후 migration |

## 바로 다음 작업

1. `RLC_TS_BACKEND=tsgo` opt-in path를 `--types`에 연결한다.
2. `src/tsgo_host.mjs`를 추가해 현재 job protocol subset을 처리한다.
3. opt-in CLI tests를 추가한다. tsgo가 없으면 skip한다.
4. `.rl` relative import parity gap을 작은 fixture로 재현하고 upstream API 조사를 이어간다.
