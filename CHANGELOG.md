# Changelog

이 프로젝트의 주목할 만한 변경 사항을 기록합니다.
형식은 [Keep a Changelog](https://keepachangelog.com/ko/1.1.0/)를 따르고,
버전은 [Semantic Versioning](https://semver.org/lang/ko/)을 따릅니다.

## [Unreleased]

### Added

- npm 패키징: `npm install --save-dev rl-lang`으로 rlc가 프리빌트
  바이너리로 설치된다 (bin `rlc`, esbuild/swc 방식의 플랫폼 패키지
  optionalDependencies — linux-x64/arm64는 musl 정적 링크, darwin-x64/arm64,
  win32-x64). 릴리스 워크플로(`release.yml`)가 태그 `vX.Y.Z`에서 빌드·npm
  배포·GitHub Release 업로드를 자동화한다. `unplugin-rl`은 설치된
  `rl-lang`의 바이너리를 자동으로 찾는다 (`rl-lang`의 `binaryPath()` 공개
  API, 없으면 종전대로 PATH의 `rlc`). (TASK-048)

- 통일된 타입·빌드 파이프라인 (TASK-036 계획, TASK-037):
  - 기본 모드가 **build**가 되어 디렉터리 입력에서 손으로 쓴
    TypeScript(`.ts`/`.mts`/`.cts`)도 함께 수집한다 — 바이트 그대로
    통과하되 상대 경로 `.rl` 지정자(및 `@rl/std`)만 재작성되어, 출력
    트리가 그 자체로 완결된다. 소스는 단독(tsc)이든 번들러 플러그인이든
    같은 모양(`"./x.rl"` import)으로 쓴다. 출력이 입력 파일 자신이 되는
    경우는 `output would overwrite the input` 에러로 거부. 숨김
    디렉터리와 `node_modules`는 순회하지 않는다.
  - `rlc --types`: "캐시 트리 컴파일 → tsc `--emitDeclarationOnly` →
    에디터 사이드카" 체인을 한 명령으로 내재화 (`.rl-build/` 캐시,
    `.rl-types/` 산출, `--tsc`로 바이너리 지정, `-w` 조합 지원). 사이드카
    선언은 소스 지정자(`"./x.rl"`, `"@rl/std"`)를 그대로 보존해 소비 측
    `rootDirs`/`paths` 설정만으로 `tsc --noEmit`과 에디터가 동작한다.

- VSCode 언어 서버에 TypeScript 언어 서비스 위임: rl 심볼(enum·케이스)이
  아닌 **일반 TS 심볼(변수·함수·타입·import된 값)의 정의 이동·호버·
  자동완성(`obj.` 멤버 포함)·참조 찾기·이름 변경**이 `.ts` 파일에서처럼
  동작한다 (완성은 rl 항목이 우선, rl 심볼의 이름 변경은 안전하게 거부).
  `.rl` 파일을 TS로 서빙하고 `./x.rl` 지정자를 커스텀 모듈 해석으로
  연결하며, TS 진단은 표출하지 않는다 (에러는 rlc가 정본).
  (TASK-024, TASK-025)

- 심볼 인터페이스: `rlc --symbols <file>`이 rl enum 선언(1-기반 위치 포함)과
  직접 `.rl` import(참조 파일의 exported 선언 포함)를 JSON으로 출력. VSCode
  언어 서버가 이를 소비해 **크로스 파일 정의 이동·자동완성·호버·빠른 수정**
  제공 (named import 별칭 반영). 라이브러리 API: `enum_symbols` /
  `EnumSymbol`/`CaseSymbol`/`FieldSymbol` / `line_col`. 모듈 그래프 로드맵
  3단계 완결. (TASK-023)

- 프로젝트 단위 소진성 검사: 직접 import한 `.rl` 파일의 exported enum에
  대한 match도 빠진 케이스를 컴파일 에러로 보고
  (`match on enum Token (imported from "./token.rl") is not exhaustive`).
  CLI가 import 절 이름(별칭·`* as ns` 포함)대로 선언을 자동 수집하며,
  섀도잉은 로컬 > 임포트 > 내장 순. 라이브러리 API: `rl_imports` /
  `exported_enums` / `ExternEnum` / `Options::extern_enums`. 모듈 그래프
  로드맵의 2단계. (TASK-022)

- `.rl` 간 import: 상대 경로 `.rl` import 지정자를 방출 시 재작성
  (`import { E } from "./error.rl"` → `"./error.js"`). 정적 import 선언과
  re-export 대상, 동적 import·비상대 경로는 통과. CLI `--rewrite-imports
  <js|bare|off>` (기본 `js`), 라이브러리 `Options::rewrite_imports`
  (`ImportRewrite`). 모듈 그래프 로드맵의 1단계
  (`docs/design/module-graph.md`). (TASK-020)

- `try` 문 — Rust의 `?`에 해당하는 에러 전파: `const n = try f();` /
  `try f();`가 `Err`면 둘러싼 함수에서 즉시 return하는 문장으로 컴파일된다
  (IIFE 없음, `await` 호환). TypeScript의 `try/catch` 블록·`try` 멤버
  이름은 그대로 통과. match 내부·템플릿 보간에서는 명확한 컴파일 에러.
  (TASK-012)
- `Option`/`Result` 표준 라이브러리: `rlc --emit-std <file>`이 함수형
  콤비네이터(`map`/`andThen`/`unwrapOr` 등)를 담은 순수 TypeScript 모듈을
  생성 (`docs/reference/std.md`, 라이브러리 API `rlc::STD_SOURCE`).
  `Option`(Some/None)·`Result`(Ok/Err)는 내장 enum으로 인식되어 파일에 선언이
  없어도 match 소진성 검사를 받는다 — 같은 이름의 로컬 rl enum이 있으면
  로컬이 우선. (TASK-011)

- 태스크 관리 체계 (`docs/tasks/`) 및 `CLAUDE.md` 작업 가이드. (TASK-001)
- 린트 게이트: `Cargo.toml [lints]` — `unsafe_code` 금지, `missing_docs` 경고,
  clippy `dbg_macro`/`todo`/`unimplemented`. (TASK-003)
- 거버넌스 문서: `LICENSE`(MIT), `CHANGELOG.md`, `CONTRIBUTING.md`. (TASK-004)
- 패키지 메타데이터: `repository`, `rust-version`, `keywords`, `categories`,
  릴리스 프로파일(lto, strip). (TASK-004)
- CI 파이프라인: fmt/clippy/test 게이트, tsc·node 통합 테스트 포함. (TASK-005)
- 라이브러리 수준 문서화: 규범 레퍼런스 `docs/reference/`(언어·CLI·에러) 신설,
  공개 API rustdoc·doctest 확충, README 문서 안내 섹션. (TASK-007)

### Changed

- `--emit-std`가 stdout 전용 무인자 옵션이 되었다 (번들러 플러그인의 가상
  모듈용). 파일 방출은 `@rl/std` 자동 방출이 대체한다. vite 플러그인도
  새 형태로 호출한다. (TASK-037)
- 파서 프런트엔드를 swc 스타일 렉서/토큰 커서 구조로 재구성: `lexer.rs`가
  소스를 유의 토큰 스트림으로 변환(정규식 휴리스틱·템플릿 중첩 렉싱을 한
  곳에 집중)하고, 파서 전체가 `parser/cursor.rs`의 토큰 커서 위에서
  동작한다. 동작 변경 없음 — 기존 테스트 전체와 구/신 컴파일러 차등 비교로
  출력 바이트 동일성을 확인. (TASK-021)
- `src/transform.rs`를 `src/transform/{mod,enums,matches}.rs` 모듈로 분리 —
  동작 변경 없음. (TASK-002)
- 전체 코드베이스를 rustfmt 기본 스타일로 정규화. (TASK-003)
- 레퍼런스 문서(`docs/reference/`)를 사용자 관점으로 단순화 — 스캔 규칙,
  판별 규칙 안전성 증명, 소진성 검사 알고리즘 등 내부 구현 상세를 제거하고
  사용자가 관찰 가능한 동작만 서술. README의 "동작 원리" 절 제거. (TASK-009)

### Removed

- `--rewrite-imports bare` 모드 — 번들러 경로는 플러그인(`off`)이
  대체했다. (TASK-037)

## [0.3.0] - 2026-08-16 이전

### Added

- Rust 재작성: 바이트 스캔 기반 변환기 + swc 검증 (조각 검증·출력 자가 검사).
- `enum` 키워드 통합: 페이로드/제네릭 규칙으로 rl enum과 TS enum 구분,
  TS enum은 그대로 통과.
- 소진성 검사를 rlc 수준 에러로 이동 (`파일:행:열` 보고, tsc 비위임).
- CLI: 디렉터리 재귀 컴파일, `-o`/`-p`/`--check`/`--no-banner`/`--no-verify`.
- 테스트 3계층: 컴파일 출력 단위 테스트, 통과(passthrough) 계약 테스트,
  tsc/node 통합 테스트.
