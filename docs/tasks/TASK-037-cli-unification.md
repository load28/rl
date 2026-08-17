# TASK-037: CLI 통일 — 기본 build 모드와 `--types` 파이프라인

- **상태**: 완료
- **시작일**: 2026-08-17
- **완료일**: 2026-08-17
- **커밋**: —

## 목적

[TASK-036](./TASK-036-unified-type-build-plan.md)의 통일 모델에 맞게 rlc CLI를
재편한다: 단독(tsc)이든 번들러 플러그인이든 **타입과 빌드가 같은 방식으로**
나오도록 사용자용 표면을 build/`--types`/`--check` 셋으로 좁히고, 통일 모델에서
불필요해진 기능은 제거한다 (사용자 지시: "불필요한 기능들은 없어도 됨").

## 범위

- 포함 (사용자용 표면):
  - 기본 모드 = **build**: `rlc [-o dir] [-w] <inputs>`가 `.rl` 컴파일에 더해
    손으로 쓴 `.ts`를 통과(+`.rl` 지정자 재작성)시켜 **완결된 TS 트리**를
    방출한다. 입력 파일을 덮어쓰게 되는 경우는 에러로 거부한다.
  - **`--types`**: "캐시 트리 컴파일 → tsc `--emitDeclarationOnly` → 사이드카"
    체인을 rlc 한 명령으로 내재화한다 (`--tsc <bin>` 옵션, `-w` 조합).
- 포함 (제거):
  - `--rewrite-imports bare` — 번들러 경로는 플러그인(`off`)이 대체했다.
  - `--emit-std <file>`의 파일 쓰기 — 자동 방출(TASK-035)이 대체했다.
    stdout 전용 `--emit-std`(무인자)로 축소하고 플러그인을 맞춘다.
- 포함 (도구용으로 유지): `-p`, `--symbols`, `--sidecar`,
  `--rewrite-imports <js|ts|off>`, `--no-banner`, `--no-verify`.
- 제외: vite 플러그인의 `types` 옵션(빌드 시 사이드카 자동 갱신 — 다음 단계),
  VSCode 확장 정렬, watch에서의 증분 선언 방출(전체 재실행으로 시작).

## 의사결정

### 결정 1: build를 별도 서브커맨드가 아니라 기본 모드로

- **상황**: TASK-036 결정 4는 호환성을 위해 기본 `rlc <dir>` 수집 규칙을
  유지하고 build를 별도 모드로 두기로 했었다.
- **검토한 대안**: 계획대로 별도 모드 / 기본 모드를 build로 교체.
- **선택과 근거**: 기본 모드 교체. 사용자가 CLI 최적화와 불필요 기능 제거를
  명시적으로 지시해 호환성 제약이 풀렸고, "rlc = 소스 트리를 완결된 TS 트리로"
  라는 한 문장이 두 모드 공용 정신 모델이 된다. 계획 변경으로 기록한다.

### 결정 2: `.ts` 통과 수집은 build/`--check`/`--types`에서만

- **상황**: 디렉터리 수집에 `.ts`를 포함하면 `--symbols`/`--sidecar` 같은
  도구 모드도 `.ts`를 집게 된다.
- **검토한 대안**: 모든 모드에서 포함 / 컴파일 계열 모드에서만 포함.
- **선택과 근거**: 컴파일 계열에서만. 사이드카는 `.rl` 전용 산출물이고,
  심볼 출력에 통과-전용 `.ts`가 끼는 것은 소음이다.

### 결정 3: in-place에서 입력을 덮어쓰게 되면 에러

- **상황**: `.ts` 통과를 수집에 넣으면 `-o` 없는 실행이 손으로 쓴 `.ts`를
  자기 자신 위에(지정자가 재작성된 채로) 덮어쓴다 — 소스 파괴다.
- **검토한 대안**: `.ts`가 있으면 조용히 건너뜀 / `-o` 필수화 / 출력 경로가
  입력과 같으면 에러.
- **선택과 근거**: 같으면 에러(`pass -o <dir>` 안내). 건너뛰면 트리가
  불완전해지는데 조용히 성공한 것처럼 보이고, `-o` 전면 필수화는 `.rl`만 있는
  in-place 사용(파일 이름이 달라 안전)을 불필요하게 막는다.

### 결정 4: `--types`의 tsc는 서브프로세스, 탐색은 `--tsc` → `node_modules/.bin/tsc` → PATH

- **상황**: 선언 본문은 타입 추론이 필요해 tsc 없이 만들 수 없다 (TASK-036
  결정 3에서 스폰으로 확정, 여기서는 탐색 순서와 실패 처리를 정한다).
- **검토한 대안**: PATH만 / 프로젝트 로컬 우선.
- **선택과 근거**: 프로젝트 로컬(`node_modules/.bin/tsc`) 우선 — 프로젝트가
  고정한 TypeScript 버전이 전역보다 정확하다. 어느 후보도 없으면
  `rlc: tsc not found …` 에러로 설치/`--tsc`를 안내하고, `--types` 외의 모드는
  tsc 없이도 전부 동작한다.

### 결정 5: 캐시 트리는 `.rl-build/` 고정, 지정자는 **소스 그대로**(`off`)

- **상황**: 선언 방출용 중간 트리의 위치와 지정자 형태를 정해야 했다.
  지정자에는 상충이 있다: tsc가 캐시 안에서 **해석**할 수 있어야 하고
  (`ts`/`js` 재작성이 유리), 방출된 선언이 소비 측에서 **그대로 유효**해야
  한다 (tsc는 선언 방출 시 지정자를 보존하므로 소스 지정자
  `"./x.rl"`/`"@rl/std"`가 유리 — 소비 측 해석은 사이드카 이름 붙임과
  `paths`가 담당한다).
- **검토한 대안**:
  - 캐시를 `ts` 재작성으로 컴파일: 첫 구현. 사이드카에 `"./rl.ts"` 같은
    캐시 지정자가 새어 나와 소비 측에서 해석 불가 (아래 이슈 1).
  - `js` 재작성 + 선언 텍스트를 사후 재작성해 원복: 텍스트 먼징이 취약하다.
  - `off`(소스 그대로) + 캐시 안 해석을 tsconfig로 공급:
    `allowArbitraryExtensions`가 `x.rl` import를 `x.d.rl.ts` 선언으로
    해석하므로, 컴파일된 모듈마다 `export * from "./x.ts"` 심을 두고
    `@rl/std`는 `paths`로 실체화된 `rl.ts`에 매핑한다.
  - 위치는 임시 디렉터리(`$TMP`) / 프로젝트의 `.rl-build/` 고정.
- **선택과 근거**: `off` + 심/`paths`. 선언 방출이 지정자를 보존한다는
  성질을 그대로 이용하므로 사후 가공이 없고, 캐시 트리가 통과 계약의
  형태(소스와 같은 지정자)를 유지한다 — 플러그인이 서빙하는 모듈과도 같은
  모양이다. 위치는 `.rl-build/` 고정(TASK-033이 도입한 관례, gitignore
  대상) — 실패 시 들여다볼 수 있다. 합성 tsconfig는 `rootDir`를 캐시
  트리로 고정해 TASK-033 이슈 2(공통 루트 승격)의 재발을 막는다.

## 작업 내역

- 2026-08-17: 현황 조사 — `main.rs` 전체, `--emit-std`/`bare` 사용처,
  VSCode 확장의 선언 방출 옵션(`DECLARATION_OPTIONS`), 테스트 구조 확인.
- 2026-08-17: `ImportRewrite::Bare` 제거 (`lib.rs`, `codegen/mod.rs`,
  CLI 파싱, 관련 테스트 2개 삭제).
- 2026-08-17: 수집·빌드 재편 (`main.rs`) — `collect_rl_files` →
  `collect_sources(include_ts)` (`.ts`/`.mts`/`.cts` 동반 수집, 닷 디렉터리·
  `node_modules` 스킵), `build_jobs`가 통과 `.ts`의 파일 이름을 유지,
  `compile_jobs`에 입력 덮어쓰기 가드(`same_file`) + std 충돌 가드.
  `--emit-std`를 stdout 전용 무인자 모드로 축소.
- 2026-08-17: `--types` 파이프라인 구현 — `types_once`(캐시 재구축 →
  `off` 컴파일 → `<stem>.d.rl.ts` 심 → tsconfig 합성 → `run_tsc`(탐색:
  `--tsc` → `node_modules/.bin/tsc` → PATH) → 사이드카 + `rl.d.ts` 산출),
  `types_watch`(변경 시 전체 재실행). tsc가 타입 에러를 보고해도 선언은
  나오므로 사이드카는 갱신하고 종료 코드로만 실패를 알린다.
- 2026-08-17: 스모크 테스트로 왕복 검증 — 혼합 트리(`.rl` 상호 import +
  `@rl/std` + 손으로 쓴 `.ts`)를 `rlc -o build src` → `tsc`(nodenext) →
  `node` 실행, `rlc --types src` → 소비 tsconfig(`rootDirs`/`paths`)로
  `tsc --noEmit` 통과 확인.
- 2026-08-17: 통합 테스트 3개 추가 (`integration.rs`):
  `cli_build_emits_a_complete_tree_that_runs`,
  `cli_refuses_to_overwrite_a_pass_through_input`,
  `cli_types_sidecars_typecheck_the_source_tree` (사이드카가 소스 지정자를
  보존하는지 + 소비 측 타입검사 왕복까지).
- 2026-08-17: 정합 — vite 플러그인 `--emit-std` 호출 형태 갱신, 플러그인
  README "타입은 같은 명령으로" 재작성, `cli.md` 재편(사용자용/도구용 옵션
  분리, "타입 생성" 절 신설, "두 모드, 한 파이프라인" 예시),
  `language.md` §4.1(`@rl/std`)·§7.2(bare 제거), `errors.md` CLI 표 갱신,
  README(빠른 시작·std 절), `std.md`, CHANGELOG, `.gitignore`
  (`.rl-build/`·`.rl-types/`), VSCode 서버 호버 문구.

## 이슈 및 해결

### 이슈 1: 사이드카 선언에 캐시 트리 지정자가 새어 나옴

- **증상**: 첫 구현(캐시를 `ts` 재작성으로 컴파일)에서
  `.rl-types/notice.rl.d.ts`가 `import { Option } from "./rl.ts"`를 담아,
  소비 측(`rootDirs` 병합 뷰)에서 해석되지 않았다 — 그 위치에 `rl.ts`가
  없고 `.ts` 확장자 import는 `allowImportingTsExtensions` 없이는 에러다.
- **원인**: tsc의 선언 방출은 모듈 지정자를 **보존**한다. 캐시 트리에서
  재작성된 지정자가 그대로 선언에 남는다.
- **해결**: 보존을 역이용 — 캐시를 `off`로 컴파일해 선언이 소스 지정자
  (`"./x.rl"`, `"@rl/std"`)를 담게 하고, 캐시 안의 해석은
  `allowArbitraryExtensions` + 모듈별 `<stem>.d.rl.ts` 심 + `paths`로
  공급했다 (결정 5). 통합 테스트가 사이드카의 지정자 보존과 소비 측
  타입검사 왕복을 고정한다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` — 168개 전체 통과 (compile 92, integration 23{신규 3
  포함, tsc·node 실행}, passthrough 35, sidecar 8, stdlib 2, doctest 8)

## 결과

- 수정: `src/main.rs`(수집·가드·`--types` 파이프라인·usage), `src/lib.rs`·
  `src/codegen/mod.rs`(Bare 제거), `tests/integration.rs`(신규 3, bare 1
  삭제), `tests/compile.rs`(bare 1 삭제), `integrations/vite/{index.js,
  README.md}`, `editors/vscode/server/src/server.ts`(호버 문구),
  `docs/reference/{cli,language,errors,std}.md`, `README.md`,
  `CHANGELOG.md`, `.gitignore`
- 사용자용 CLI 표면: `rlc [-o] [-w]`(build) / `--types [--tsc]` /
  `--check` — 두 모드에서 소스와 타입 명령이 동일해졌다.
- 후속: vite 플러그인의 `types: true` 옵션(빌드 시 `rlc --types` 자동
  실행), VSCode 확장이 통일 파이프라인과 산출 동일성을 보장하는 정렬,
  watch에서의 증분 선언 방출.
