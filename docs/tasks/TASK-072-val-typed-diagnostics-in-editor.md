# TASK-072: 에디터에 타입 기반 `val` 진단 노출

- **상태**: 완료
- **시작일**: 2026-08-19
- **완료일**: 2026-08-19
- **커밋**: 미커밋

## 목적

TASK-071로 `val` 경로의 built-in 변경 메서드 판정이 `rlc --types`로 옮겨졌다.
에디터(LSP)는 rl 진단을 `rlc --check`로 받으므로(`editors/vscode/server/src/rlc.ts`),
`map.set("a", 1)` 같은 확실한 built-in 변경이 편집 중에는 표시되지 않는다.
타입 진단 경로(`tsproject.ts`)는 이미 같은 프로그램의 TypeChecker를 들고 있으므로
거기서 `rlc::val_method_calls`의 프로브를 답해 인라인 진단으로 띄울 수 있다.

## 범위

- 포함: `rlc --emit-map`/가상 문서 매핑으로 프로브 위치를 옮기고, `tsproject.ts`에서
  `types_host.mjs`와 **같은 판정**(심볼 선언이 기본 lib의
  `Array`/`Map`/`Set`/`WeakMap`/`WeakSet`/TypedArray 변경 메서드인지)을 수행.
- 포함: 에디터 서버 source 후보 수집기와 TypeChecker 판정 테스트를 추가한다.
- 제외: 판정 규칙 자체의 변경 (규범은 `language.md` §10.4).

## 의사결정

### 결정 1: 에디터 서버 내부에서 후보 수집과 TypeChecker 판정을 수행한다

- **상황**: CLI `rlc --types`는 Rust 쪽에서 `val_method_calls`를 수집하고
  `types_host.mjs`가 TypeChecker로 built-in mutator 여부를 판정한다. 에디터는 이미
  `rlc --emit-map` 결과를 TypeScript language service에 올려 두므로 같은 정보를
  가지고 있다.
- **검토한 대안**:
  - 저장 중/검증 중 `rlc --types`를 별도로 실행한다. 장점은 Rust 구현을 재사용한다.
    단점은 sidecar emit까지 포함하는 무거운 경로이고, 편집 중 진단 갱신마다 별도
    프로세스를 더 실행한다.
  - 에디터 서버가 source에서 후보만 수집하고, 최종 판정은 기존 `TsProject`의
    TypeChecker가 한다. 장점은 이미 준비된 가상 문서와 TypeScript program을 재사용한다.
    단점은 후보 수집기의 lexical scope 모델이 Rust 구현보다 단순해질 수 있다.
- **선택과 근거**: 두 번째를 선택한다. 사용자 정의 `set`/`push` 오탐을 막는 핵심은
  TypeChecker의 symbol declaration 판정이며, 후보 수집은 놓치면 진단이 빠지는 쪽으로
  보수적으로 설계한다.

### 결정 2: 후보 수집기는 별도 `valdiag.ts`로 둔다

- **상황**: `server.ts`는 이미 LSP routing과 진단 병합을 맡고 있고, `tsproject.ts`는
  TypeScript language service adapter다.
- **검토한 대안**: `server.ts`에 inline 구현하면 파일 수는 줄지만 검증하기 어렵다.
  `valdiag.ts`로 분리하면 source 후보 수집만 독립 테스트할 수 있다.
- **선택과 근거**: `valdiag.ts`를 추가한다. `tsproject.ts`에는 TypeChecker 판정만 추가해
  책임을 나눈다.

## 작업 내역

- 2026-08-19: TASK-072를 진행 중으로 전환했다.
- 2026-08-19: `types_host.mjs`의 built-in mutator 판정 로직과 에디터 가상 문서 진단
  병합 흐름을 검토했다.
- 2026-08-19: `editors/vscode/server/src/valdiag.ts`를 추가해 source text에서 `val`
  binding을 통한 built-in mutator 후보 호출을 수집하게 했다.
- 2026-08-19: `editors/vscode/server/src/tsproject.ts`에 `valMutationsFor()`를 추가해
  TypeChecker symbol declaration이 TypeScript default lib의 mutating built-in method인지
  판정하게 했다.
- 2026-08-19: `editors/vscode/server/src/server.ts`의 type diagnostic 병합 단계에서
  source 후보를 virtual document offset으로 옮기고, proven mutation을 source range의
  `rlc` 진단으로 추가했다.
- 2026-08-19: `valdiag`, `tsproject`, emit-map 기반 테스트를 추가했다.
- 2026-08-19: `npm install`로 VS Code extension 의존성을 설치한 뒤 `npm run compile`과
  `PATH="/Users/seominyeong/orca/workspaces/rl/tsgo-frontend-review/target/debug:$PATH"
  npm test`를 실행했다.
- 2026-08-19: Rust 필수 게이트(`cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test`)를 실행했다.

## 이슈 및 해결

### 이슈 1: VS Code extension 테스트가 PATH의 오래된 `rlc`를 사용함

- **증상**: `npm test`가 completion std signature와 새 val emit-map 테스트에서 실패했다.
  새 val 테스트는 `val const`가 지워진 현재 emit-map을 기대했지만, 테스트 프로세스는
  PATH의 `/Users/seominyeong/.local/bin/rlc`를 사용했다.
- **원인**: extension 테스트의 `COMPILER` 상수는 `rlc`이고, 기본 PATH가 현재 worktree의
  `target/debug/rlc`보다 사용자 로컬 설치본을 먼저 가리켰다.
- **해결**: `cargo build`로 현재 worktree의 compiler를 만든 뒤
  `PATH="/Users/seominyeong/orca/workspaces/rl/tsgo-frontend-review/target/debug:$PATH"
  npm test`로 테스트했다. 이 조건에서 서버 테스트 75개가 모두 통과했다.

## 검증

- [x] `npm run compile` (`editors/vscode`)
- [x] `PATH="/Users/seominyeong/orca/workspaces/rl/tsgo-frontend-review/target/debug:$PATH" npm test` (`editors/vscode`)
- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`

## 결과

에디터 type diagnostic 경로가 `val` binding을 통한 built-in mutator method call을
TypeScript checker로 판정해 inline 진단으로 표시한다. 사용자 정의 `set`/`push` 같은
동명 메서드는 default lib 선언이 아니므로 보고하지 않는다. 언어 규칙과 CLI 동작은 이미
TASK-071에서 문서화된 범위와 동일하므로 `docs/reference/`와 `docs/ai/rl.md` 갱신은
필요 없다.
