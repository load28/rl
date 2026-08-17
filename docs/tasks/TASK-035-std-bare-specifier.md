# TASK-035: `@rl/std` — 표준 라이브러리를 rlc가 알아서 놓는다

- **상태**: 완료
- **시작일**: 2026-08-17
- **완료일**: 2026-08-17
- **커밋**: —

## 목적

표준 라이브러리를 쓰려면 `rlc --emit-std src/rl.ts`로 파일을 만들고 소스에서
`"./rl"`로 가리켜야 했다. 생성물이 소스 트리에 들어오고, 그 파일이 없는
배치에서는 소스가 **존재하지 않는 형제 파일**을 가리키게 된다(에디터에서
`Option`/`Result`의 타입도 정의 이동도 없었다). 지정자를 bare로 바꾸고
rlc가 모듈을 알아서 놓게 한다.

## 범위

- 포함: `@rl/std` 지정자 인식, 컴파일 시 자동 방출과 지정자 재작성,
  `Options::std_import`, `rlc::imports_std`, `--emit-std -`(stdout),
  Vite 플러그인의 가상 모듈, 문서, `rl-calc` 전환.
- 제외: 사용된 멤버만 방출하는 가지치기(트리셰이킹) — 별도 태스크.

## 의사결정

### 결정 1: 상대 경로가 아니라 bare 지정자로 한다

- **상황**: 소스가 생성물을 가리켜야 하는데, 그 위치는 소비자마다 다르다
  (rlc 출력 트리 / 번들러의 가상 모듈 / 에디터).
- **검토한 대안**:
  - 상대 경로 `"./rl"` 유지: 세 소비자 중 어느 하나에서도 어긋난다. 특히
    TypeScript의 `paths`는 **상대 경로에 적용되지 않아** 에디터가 해석할
    방법이 없다 (TASK-031에서 확인).
  - npm 패키지(`@rl/std`를 실제로 배포): 가장 관례적이지만 npm에 묶인다.
  - bare 지정자 + 소비자별 해석: `paths`(에디터), 자동 방출·재작성(rlc),
    가상 모듈(번들러) 셋이 모두 성립한다.
- **선택과 근거**: 세 번째. 이름은 나중에 실제 패키지를 낼 때 그대로 쓸 수
  있도록 `@rl/std`로 정했다.

### 결정 2: 출력 트리에 자동으로 쓰고, 위치는 `-o`(없으면 공통 상위)

- **상황**: 자동 방출이면 어디에 쓸지 정해야 한다.
- **선택과 근거**: `-o` 디렉터리, 없으면 출력들의 공통 상위 디렉터리에
  `rl.ts`로 쓴다. 파일마다 복사하지 않고 프로젝트에 하나만 둔다. 각 출력의
  지정자는 자기 위치에서의 상대 경로로 계산한다 —
  `build/main.ts`는 `./rl.js`, `build/deep/nested.ts`는 `../rl.js`.

### 결정 3: 기본값은 "재작성하지 않음"이 아니라 `--rewrite-imports`를 따른다

- **상황**: 지정자 형태(`.js`/`.ts`/확장자 없음)를 별도 옵션으로 둘지.
- **선택과 근거**: `.rl` 지정자와 같은 규칙을 쓴다. 한 프로젝트 안에서 두
  종류의 지정자가 다른 형태로 나오면 혼란스럽다. `off`는 재작성을 끄므로
  `@rl/std`가 그대로 남고, 그것이 번들러 플러그인이 원하는 상태다.

### 결정 4: 라이브러리 API는 위치를 모른다

- **상황**: `compile()`이 자동 방출까지 하게 할지.
- **선택과 근거**: 하지 않는다. rl은 IO를 라이브러리 밖에 둔다(TASK-022의
  결정과 같은 선). `Options::std_import`로 "이 출력은 std를 어디로
  가리켜야 하는가"만 받고, 파일을 쓰는 것은 CLI의 일이다.
  `rlc::imports_std`는 빌드 도구가 그 판단을 할 수 있게 하는 최소 정보다.

## 작업 내역

- 2026-08-17: `ast::RlSpecifier`(Relative/Std)를 추가하고 파서가 `@rl/std`를
  인식하게 했다(`stdlib::STD_SPECIFIER`). codegen은 `Std` 지정자를
  `Options::std_import`로 바꾸고, 없으면 그대로 둔다.
- 2026-08-17: `rl_imports`는 `Std`를 제외한다 — 따라갈 파일이 없으므로
  모듈 그래프의 간선이 아니다. 대신 `imports_std`를 추가했다.
- 2026-08-17: CLI — `std_placement`/`common_ancestor`/`std_specifier`로
  위치와 지정자를 계산하고, 컴파일 전에 모듈을 한 번 쓴다.
  `--emit-std -`는 stdout으로 출력한다.
- 2026-08-17: Vite 플러그인이 `@rl/std`를 가상 모듈로 해석하고
  `rlc --emit-std -`의 출력을 돌려준다.
- 2026-08-17: 확인.
  ```
  $ rlc -o build src/
  rlc: std → build/rl.ts
  build/main.ts         → import ... from "./rl.js"
  build/deep/nested.ts  → import ... from "../rl.js"
  $ rlc --rewrite-imports ts -o out2 src/   → "../rl.ts"
  $ rlc --rewrite-imports off -p src/main.rl → "@rl/std" 그대로
  ```
  Vite 경로도 확인 — `vite build` 성공, `node dist/main.js` 정상, 번들에
  파일이 하나도 추가되지 않았다.
- 2026-08-17: `rl-calc`을 `"./rl"` → `"@rl/std"`로 옮기고 `std` 스크립트를
  없앴다. 빌드·출력은 이전과 동일하다.
- 2026-08-17: `std.md` 개요를 지정자 중심으로 다시 쓰고, `cli.md`에
  "표준 라이브러리 자동 방출" 절과 `--emit-std -`를 추가했다.

## 이슈 및 해결

### 이슈 1: 출력 디렉터리가 없어 상대 경로가 깨졌다

- **증상**: 중첩 파일의 지정자가
  `"../..///private/tmp/rlstd/build/rl.js"`로 나왔다.
- **원인**: `relative_path`가 두 경로 중 **하나만** canonicalize했다. 아직
  만들어지지 않은 출력 디렉터리는 실패해 상대 경로로 남고, 다른 쪽은
  절대 경로가 되어 공통 접두사가 0이 됐다.
- **해결**: 둘 다 성공할 때만 canonicalize하고, 아니면 둘 다 원본 경로를
  쓴다. 사이드카 경로 계산도 같은 함수를 쓰므로 함께 고쳐졌다.

### 이슈 2: `\0` 가상 id는 호스트의 TypeScript 패스를 건너뛴다

- **증상**: 플러그인이 std를 `\0`-접두 가상 모듈로 돌려주자
  `Expected '{', got 'type'`으로 Rollup 파싱이 실패했다.
- **원인**: Vite는 `\0`으로 시작하는 id에 esbuild 변환을 적용하지 않는다.
  rlc가 내놓는 것은 TypeScript라 변환이 반드시 필요하다.
- **해결**: 경로 모양의 id(`<cwd>/__rl_std__.ts`)로 바꿨다. 플러그인이
  먼저 해석·로드하므로 디스크를 읽지 않고, 확장자가 `.ts`라 변환은 걸린다.

### 이슈 3: 테스트가 옛 `rlc`를 불러 빈 std를 받았다

- **증상**: 플러그인이 길이 0인 모듈을 돌려줬다.
- **원인**: PATH의 `rlc`가 `--emit-std -`를 모르는 이전 빌드였다
  (TASK-032의 이슈 1과 같은 함정).
- **해결**: `cargo install --path . --force` 후 정상. 외부 바이너리에
  의존하는 검증의 특성이다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` — 93 + 21 + 35 + 8 + 2 + 8 전부 통과 (신규 3)
- [x] CLI 실측 — `-o` 있음/없음, `js`/`ts`/`bare`/`off` 네 모드, 중첩 디렉터리
- [x] Vite 플러그인 실측 — 가상 모듈로 빌드·실행
- [x] `rl-calc` 전환 후 빌드·출력 동일

## 결과

- 수정: `src/ast.rs`, `src/parser/imports.rs`, `src/codegen/mod.rs`,
  `src/lib.rs`, `src/main.rs`, `src/stdlib.rs`, `tests/compile.rs`,
  `integrations/vite/index.js`, `docs/reference/std.md`,
  `docs/reference/cli.md`, `docs/tasks/INDEX.md`
- 추가: `docs/tasks/TASK-035-std-bare-specifier.md`

후속: 사용된 멤버만 방출하는 가지치기. 측정해 두면 — `rl-calc`의 std는
7,682바이트로 번들(22,929바이트)의 3분의 1이고, 실제로 쓰는 멤버는 약 30개
중 9개다. 콤비네이터가 객체 프로퍼티라 번들러는 손대지 못하므로(모듈 단위로
`Result` 전체가 빠지는 것까지가 한계), rlc가 생성 시점에 고르는 수밖에
없다. `stdlib.rs`를 "선택 가능한 멤버 표"로 재구성해야 한다.
