# TASK-074: 에디터를 네이티브 백엔드로 — 사이드카 규약 통일

- **상태**: 진행 중
- **시작일**: 2026-08-19
- **완료일**: —
- **커밋**: —

## 목적

TASK-073이 만든 네이티브 백엔드의 선언 트리와, 에디터가 소비하는 기존
사이드카 규약이 서로 다르다. 둘을 하나로 만들고 확장 프로그램을 그쪽으로
옮긴다.

## 범위

- 포함: 사이드카 파일 이름·레이아웃·`@rl/std` 지정자 규약 결정,
  `.d.ts.map` 방출(`rlc::build_sidecar` 재사용 또는 대체), VSCode 확장의
  조회 규칙 갱신, Language Service 질의(hover/definition/completion)를
  네이티브 백엔드로 넘길 수 있는 범위 조사.
- 제외: `--types` 제거(TASK-075).

## 배경 (TASK-073 실측)

| 항목 | 레거시 `--types` | 네이티브 `--native-check` |
|------|------------------|---------------------------|
| 파일 이름 | `<name>.rl.d.ts` | `<name>.d.ts` |
| 레이아웃 | 입력 디렉터리 기준 평탄화 | 프로젝트 루트 미러 |
| `@rl/std` | 바 지정자 (에디터가 해석) | `../__rl_std__.ts` (자체 해석) |
| `.d.ts.map` | 있음 | 없음 |

두 규약은 양립하지 않는다 — 파일 이름을 `<name>.rl.d.ts`로 바꾸면 트리
안의 상대 지정자(`./x.ts`)가 더 이상 맞지 않는다. 그래서 이름 규약과
`.d.ts.map`은 하나의 결정이다.

## 의사결정

### 결정 1: lowering 결과를 `<원본>.ts`가 아니라 `<원본>.rl.ts`로 얹는다

- **상황**: TASK-073은 `src/token.rl`의 lowering 결과를 `src/token.ts`에
  얹고 `.rl` 지정자를 `./token.ts`로 재작성했다. 그러면 소비자 tsconfig에
  `allowImportingTsExtensions`가 필요하고, 방출된 선언도 `./token.ts`를
  가리켜 기존 사이드카 규약(`token.rl.d.ts`)과 어긋난다.
- **검토한 대안**: (A) 두 규약을 유지하고 확장 프로그램이 변환 —
  변환 계층이 하나 더 생긴다. (B) lowering 결과를 `token.rl.ts`에 얹는다 —
  `"./token.rl"` 지정자가 **평범한 TypeScript 해석**으로 그 파일을 찾고,
  방출된 선언은 `token.rl.d.ts`에 떨어진다. 즉 컴파일러가 없을 때
  같은 지정자가 찾는 사이드카 이름과 정확히 같다.
- **선택과 근거**: (B). 실측으로 확인했다 — `src/a.ts`가
  `import { Shape } from "./b.rl"`을 쓰고, 가상 FS가 `src/b.rl.ts`를
  제공하면 진단 0으로 통과하고 선언은 `src/b.rl.d.ts`로 나온다.
  **사용자 tsconfig에 rl 전용 옵션이 하나도 필요 없다** — TASK-073이
  요구하던 `allowImportingTsExtensions`가 사라졌다.
  덤으로 지정자를 재작성할 필요가 없어져 `ImportRewrite::Off`로 컴파일한다.

### 결정 2: `@rl/std`는 가상 `node_modules/@rl/std/index.ts`로 해석한다

- **상황**: TASK-073은 표준 라이브러리를 프로젝트 루트 모듈로 얹고 지정자를
  상대 경로로 재작성했다. 그러면 방출된 선언이 `../__rl_std__.ts`를 가리켜
  기존 사이드카(바 지정자 `@rl/std`)와 어긋난다.
- **선택과 근거**: 가상 FS에 `node_modules/@rl/std/index.ts`로 얹으면 평범한
  node 해석이 찾아내고, **지정자가 소스에서도 선언에서도 바 상태로 남는다**.
  사용자의 실제 `node_modules`에는 아무것도 쓰지 않는다.
- **남은 것**: node_modules 아래 모듈은 external library로 취급돼 선언 emit
  대상이 아니다(실측: `isSourceFileFromExternalLibrary: true`). 레거시
  `--types`가 만들던 `rl.d.ts`가 네이티브 경로에는 없다 — CLI 모드 정리와
  함께 TASK-075에서 결정한다.

### 결정 3: 사이드카의 맵은 `rlc::build_sidecar`를 그대로 쓴다

선언 본문은 컴파일러의 것이고, 그것이 원본의 어디에서 왔는지만 rlc가 맵으로
채운다 — `--sidecar` 모드가 이미 하던 일이라 같은 함수를 재사용한다.

### 결정 4: 릴리스된 npm 패키지도 툴체인으로 받는다

- **상황**: 에디터를 네이티브 백엔드로 옮기려면 실사용자가 tsgo를 가질 수
  있어야 하는데, TASK-073은 typescript-go를 직접 빌드한 트리만 받았다.
  Go 툴체인을 요구하는 것은 제품이 아니다.
- **선택과 근거**: 릴리스된 `typescript@7`은 API 클라이언트와 **네이티브
  실행 파일을 함께** 배포한다 (`node_modules/@typescript/typescript-<플랫폼>/lib/tsc`,
  클라이언트가 `getExePath`로 자기 옆의 것을 찾는다). 그래서 설치된 패키지를
  만나면 rlc는 **클라이언트만 지정하고 실행 파일은 지정하지 않는다** —
  클라이언트가 자기와 같은 빌드의 실행 파일을 고르므로 "한 빌드" 요구가
  저절로 지켜진다. 해석 순서는 환경 변수 → typescript-go 트리 → 설치된 패키지.
- **확인**: `npm i typescript@7`만 한 프로젝트에서 Go 없이
  `rlc --native-check src`가 소진성 진단을 냈다.

## 작업 내역

- 2026-08-19: 확장 프로그램 조사 — `server/src/tsproject.ts`가 **인프로세스
  TypeScript 언어 서비스**(`import * as ts from "typescript"`)를 돌리고,
  `server/src/sidecar.ts`가 인프로세스 선언 emit을 한다. CLI보다 JS API에
  더 깊게 묶여 있다.
- 2026-08-19: 배치 실측 (`/home/user/spike2`) — `.rl.ts` 이름과 가상
  node_modules로 진단 0 + 선언 `b.rl.d.ts` 확인.
- 2026-08-19: `project.rs` — `module_path_of`를 `<원본>.rl.ts`로,
  `rewrite_imports: Off`, `STD_MODULE`을 `node_modules/@rl/std/index.ts`로.
- 2026-08-19: `check.rs` — `-o`가 사이드카(`<이름>.rl.d.ts` + `.map`)를 쓴다.
- 2026-08-19: 진단을 프로그램 전체로 — 손으로 쓴 `.ts`의 타입 에러도 보고한다
  (같은 프로젝트 그래프이므로).
- 2026-08-19: `tests/native.rs` — 새 규약 테스트 2건 추가(총 15건). 픽스처
  tsconfig에서 `allowImportingTsExtensions`를 **제거**했다.
- 2026-08-19: `native.rs` — 툴체인 해석에 설치된 npm 패키지 추가(결정 4),
  경로를 절대 경로로 정규화(호스트는 프로젝트 디렉터리에서 실행되므로 상대
  경로는 어느 쪽으로도 해석되지 않는다). 해석 기준점을 cwd가 아니라
  프로젝트 루트로 — 워크스페이스 자신의 TypeScript를 쓴다.
- 2026-08-19: 그래프를 프로젝트 전체로, 인자는 "무엇을 쓸지"만 정하게 변경.
  tsconfig 없는 워크스페이스는 inferred project로 처리(에디터가 낱개 파일에
  하는 것과 같다).
- 2026-08-19: `--native-sidecar` 추가(쓰기 모드, 배치는 레거시 그대로) 후
  확장 프로그램 `server/src/sidecar.ts`를 그 한 번의 호출로 재작성 —
  **인프로세스 TypeScript 선언 emit이 사라졌다** (`import * as ts` 제거).
- 2026-08-19: 확장 테스트 갱신 — 가드가 실제로 쓰는 명령을 프로브하고,
  "소진성 실패 시 사이드카 유지" 테스트는 lowering을 막는 에러(중복 케이스)로
  바꿨다. 소진성은 이제 emit을 막지 않으므로 그 동작을 새 테스트로 남겼다.
  전체 71건 통과.
- 2026-08-19: CI — `native` 잡 신설(typescript-go를 커밋 고정해 빌드,
  `cargo test --test native`와 확장 사이드카 테스트를 스킵 0으로 강제).
  `check` 잡에는 `typescript@7`을 설치해 검사 경로가 스킵되지 않게 했다.
- 2026-08-19: `docs/reference/cli.md`에 네이티브 백엔드 절 추가.

## 이슈 및 해결

### 이슈 1: 선언 emit이 node_modules 모듈을 건너뛴다

- **증상**: 표준 라이브러리를 `node_modules/@rl/std/index.ts`로 옮긴 뒤
  `getDeclarationEmit`이 그 모듈에 아무것도 내놓지 않는다.
- **원인**: `isSourceFileFromExternalLibrary`가 true — external library는
  emit 대상이 아니다 (실측).
- **해결**: 지금은 해석용으로만 둔다. 레거시의 `rl.d.ts`가 필요한지는
  `--types` 정리(TASK-075)에서 결정한다.

### 결정 5: 사이드카 배치는 레거시 규약 그대로

`--native-sidecar`는 `--types`/`--sidecar`와 같은 자리에 쓴다 — 원본 옆이거나
`-o` 아래 입력 레이아웃 기준. 확장 프로그램이 찾는 위치가 그대로여야
갈아끼우기가 드롭인이 된다.

### 결정 6: 내장 여부는 경로가 아니라 컴파일러에게 묻는다

- **상황**: `val` 판정이 선언 파일 경로가 `bundled:///libs/`로 시작하는지로
  내장을 가렸다. 릴리스된 7.0.2는 lib을 디스크(`node_modules/@typescript/...`)
  에서 읽으므로 그 접두사가 없다 — 실측으로 테스트 1건이 실패했다.
- **해결**: 호스트가 `program.isSourceFileDefaultLibrary`로 묻는다. 두 배포
  형태에서 같은 사실을 얻는다. 두 툴체인 모두에서 16건 통과 확인.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` (`tests/native.rs` 16건 — 빌드된 체크아웃과 릴리스
      npm 패키지 양쪽에서)
- [x] 확장 프로그램 `node --test` 71건 (스킵 0)

## 결과

*작업 완료 시 기록.*
