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

## 이슈 및 해결

### 이슈 1: 선언 emit이 node_modules 모듈을 건너뛴다

- **증상**: 표준 라이브러리를 `node_modules/@rl/std/index.ts`로 옮긴 뒤
  `getDeclarationEmit`이 그 모듈에 아무것도 내놓지 않는다.
- **원인**: `isSourceFileFromExternalLibrary`가 true — external library는
  emit 대상이 아니다 (실측).
- **해결**: 지금은 해석용으로만 둔다. 레거시의 `rl.d.ts`가 필요한지는
  `--types` 정리(TASK-075)에서 결정한다.

## 검증

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo test`

## 결과

*작업 완료 시 기록.*
