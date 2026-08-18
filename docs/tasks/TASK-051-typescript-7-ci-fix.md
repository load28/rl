# TASK-051: TypeScript 7 릴리스로 인한 CI 복구 — --types 진단과 게이트 고정

- **상태**: 완료
- **시작일**: 2026-08-18
- **완료일**: 2026-08-18
- **커밋**: 4708ebf

## 목적

npm의 `typescript` latest가 7.0.2(네이티브 Go 컴파일러)로 올라오면서 CI의
`npm install -g typescript`(latest)가 TS 7을 설치하게 됐고, `rlc --types`의
호스트(`types_host.mjs`)가 쓰는 JS 컴파일러 API가 TS 7 패키지에 존재하지
않아(`require("typescript")`가 `version`만 노출) `--types` 통합 테스트 3건이
`TypeError: ts.convertCompilerOptionsFromJson is not a function`으로 실패했다.
main의 최근 CI 3개 런도 동일하게 실패 — 브랜치와 무관한 베이스 실패다.
CI를 복구하고, TS 7만 있는 환경에서 rlc가 원인 불명의 스택 대신 명확한
안내를 내도록 한다.

## 범위

- 포함: `types_host.mjs`의 해석 규칙(JS API 없는 typescript는 건너뛰고 다음
  후보 시도, 전부 없으면 종료 코드 4), rlc의 종료 코드 4 전용 진단,
  `--types` 성공 테스트 3건의 스킵 가드(사용 가능한 TS 기준), CI의
  typescript 메이저 고정(`typescript@6`), `cli.md` 지원 버전 명시.
- 제외: TS 7 네이티브 컴파일러 자체 지원 — 패키지가 노출하는 것은 `tsc`
  바이너리와 `unstable/*` API뿐이라 인메모리 호스트(TASK-040)의 재설계가
  필요하다. 별도 태스크로 판단할 사안.

## 의사결정

### 결정 1: CI는 typescript@6으로 고정한다

- **상황**: CI가 latest를 설치해 TS 릴리스 당일 게이트가 깨졌다.
- **검토한 대안**: (A) latest 유지 + TS 7 지원을 즉시 구현 — 지원 자체가
  호스트 재설계 규모라 CI 복구 수단이 될 수 없음. (B) `typescript@6` 고정 —
  게이트는 rlc가 지원하는 메이저를 검증하는 것이 본분이고, 외부 릴리스에
  결정론적이 된다.
- **선택과 근거**: (B). 게이트가 "지원 범위"를 검증하고, 미지원 범위(TS 7)는
  명확한 진단 + 스킵 가드로 다룬다.

### 결정 2: JS API 없는 typescript는 건너뛰고, 전부 없을 때만 실패한다

- **상황**: 해석 순서는 프로젝트 → PATH의 tsc 패키지다. 프로젝트에 TS 7이
  있어도 PATH에 TS 6이 있으면 동작해야 하는가?
- **검토한 대안**: (A) 처음 해석된 것을 그대로 사용(현행) — TS 7이 걸리면
  즉시 크래시. (B) `convertCompilerOptionsFromJson` 존재를 사용 가능성
  기준으로 삼아 없으면 다음 후보로 — 사용 가능한 설치가 하나라도 있으면
  동작.
- **선택과 근거**: (B). "동작할 수 있으면 동작한다"가 폴백 해석을 도입한
  기존 취지(전역 tsc 폴백)와 일관되고, 실측으로 두 경로를 확인했다:
  프로젝트 TS 7 + PATH TS 6 → 사이드카 정상 방출, TS 7만 → 명확한 진단
  (종료 코드 4 → rlc가 `typescript@6` 설치 안내, 종료 1).

### 결정 3: 테스트 스킵 가드는 호스트의 해석 규칙을 그대로 미러링

- **상황**: `--types` 성공 테스트는 tsc/node 존재만 확인해, TS 7만 있는
  환경(고정 전 CI 포함)에서 실패했다.
- **선택과 근거**: `usable_typescript_for_types()` 신설 — `require` 경로와
  PATH tsc 소유 패키지 경로 모두에서 **JS API 존재**까지 확인해, 없으면
  스킵. `cli_types_without_typescript_says_so`의 기존 가드(설치 여부)는
  의미가 다르므로 그대로 둔다 — TS 7이 설치돼 있으면 "미설치" 시나리오가
  아니어서 여전히 스킵된다. TASK-042 이슈 3에서 "가드는 호스트의 실제 해석
  규칙과 일치해야 한다"고 배운 것의 반복 적용이다.

## 작업 내역

- 2026-08-18: CI 실패 로그 확인 — `--types` 테스트 3건이
  `ts.convertCompilerOptionsFromJson is not a function`으로 실패. npm
  dist-tags 실측: latest = 7.0.2. main 최근 3개 런도 동일 실패임을 확인
  (베이스 실패). typescript@7.0.2를 설치해 API 표면 실측 — export가
  `version`/`versionMajorMinor`뿐, JS 컴파일러 API 전무, bin `tsc`(네이티브)
  와 `unstable/*` API만 존재.
- 2026-08-18: `src/types_host.mjs` — `resolveTypescript()`가 API 없는
  후보를 건너뛰고, 끝내 없으면 stderr에 버전을 남기고 종료 코드 4.
  프로토콜 주석에 코드 4 추가.
- 2026-08-18: `src/main.rs` — 종료 코드 4 분기: `--types needs typescript
  5 or 6 (npm i -D typescript@6)` 안내.
- 2026-08-18: `tests/integration.rs` — `usable_typescript_for_types()` +
  `require_types_typescript!` 매크로 신설, `--types` 성공 테스트 3건에 적용.
- 2026-08-18: `.github/workflows/ci.yml` — `npm install -g typescript@6`
  (사유 주석 포함). `docs/reference/cli.md` — `--types`에 지원 버전(5·6)과
  TS 7 미지원 사유·안내 문구 명시. CHANGELOG Fixed 항목.
- 2026-08-18: 실측 검증 — 케이스 1(프로젝트 TS 7 + PATH TS 6): 사이드카
  정상 방출. 케이스 2(TS 7만, node만 있는 PATH): 새 진단 + 종료 코드 1.
  검증 게이트 전체 통과 (컨테이너는 PATH tsc가 TS 6이라 `--types` 테스트가
  스킵 없이 실행됨).

## 이슈 및 해결

### 이슈 1: CI 실패가 이 브랜치 변경 때문으로 보일 수 있었음

- **증상**: PR #17의 CI가 실패 — 직전 커밋들이 대규모(codegen 재구성)라
  브랜치 원인으로 의심될 상황.
- **원인**: main 최근 3개 런(머지 커밋 포함)도 동일 실패 — 실패 시각과 npm
  dist-tags로 TS 7.0.2 릴리스가 원인임을 특정. 실패 테스트도 이 브랜치가
  건드리지 않은 `--types` 계열뿐.
- **해결**: 본 태스크로 베이스 실패를 브랜치에서 수정 (main에는 PR 머지로
  전파).

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` (--types 테스트 3건 포함 전부 통과 — TS 6 환경)
- [x] TS 7 실측 2케이스 (위 작업 내역)

## 결과

- 갱신: `src/types_host.mjs`, `src/main.rs`, `tests/integration.rs`,
  `.github/workflows/ci.yml`, `docs/reference/cli.md`, `CHANGELOG.md`,
  `docs/tasks/INDEX.md`, 본 태스크 문서.
- 후속 여지: TS 7 네이티브 컴파일러 지원(`unstable` API 또는 `tsc` 바이너리
  구동으로 `--types` 재설계) — 필요해지면 별도 태스크로 등록.
