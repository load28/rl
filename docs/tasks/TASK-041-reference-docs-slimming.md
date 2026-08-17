# TASK-041: 레퍼런스 문서를 읽는 문서로 정리

- **상태**: 완료
- **시작일**: 2026-08-17
- **완료일**: 2026-08-17
- **커밋**: 41a2653

## 목적

`docs/reference/`의 네 문서가 개발 과정에서 계속 덧붙여지며 "왜 이렇게 됐는지"의
서술이 규범 내용보다 많아졌다. 레퍼런스는 찾아 읽는 문서인데 처음부터 읽어야
이해되는 형태가 되어 있었다. 사용자 요구는 "리드미처럼 간결하게" —
README(TASK 이전 285 → 130줄)와 같은 밀도로 맞춘다.

## 범위

- 포함: `docs/reference/language.md`, `cli.md`, `errors.md`, `std.md`.
- 포함: 정리 중 발견한 실제 구현과의 불일치 수정 (`--node`의 값 누락 에러
  문구가 `--tsc` 시절 문장이었다).
- 제외: `docs/tasks/**`. 그 시점의 기록이므로 손대지 않는다.
- 제외: `docs/design/**`, `CLAUDE.md`, `CONTRIBUTING.md`. 사용자가 레퍼런스로
  범위를 한정했다.

## 의사결정

### 결정 1: 근거 서술을 지우고 사실은 표로 옮긴다

- **상황**: 문장 대부분이 "이렇게 한 이유"였다. 예를 들어 `match` 방출 절은
  switch/if-체인 선택 이유를 세 문단으로 설명했다.
- **검토한 대안**:
  - 설계 근거를 `docs/design/`으로 이동: 근거가 살아남지만 옮기는 작업 자체가
    설계 문서의 서술 구조와 충돌하고, 이 태스크의 범위를 넘는다.
  - 근거를 지우고 **결과만** 남긴다: 규범 문서에 필요한 것은 "무엇이 참인가"다.
    근거는 태스크 문서에 이미 전부 기록되어 있다.
- **선택과 근거**: 후자. 근거의 보관 장소는 `docs/tasks/`이고, 레퍼런스가 그것을
  중복 보유할 이유가 없다. 대조 축이 있는 사실(케이스 형태별 방출, 암 검사,
  출처별 소진성 메시지)은 표로 바꿨다 — 축이 없는 것을 표로 만들지는 않았다.

### 결정 2: 별칭 절을 만들지 않고 원래 절 번호를 유지한다

- **상황**: `match`/`try`/let-else 절에서 두 소절을 합치면 뒤 소절의 번호가
  당겨지고, 다른 문서가 걸어둔 `#54-사용-위치-제약` 같은 링크가 깨진다.
- **검토한 대안**:
  - "(별칭)" 스텁 소절을 추가해 옛 앵커를 살린다: 처음에 이렇게 썼는데,
    군더더기를 지우는 태스크에서 군더더기를 새로 만드는 셈이었다.
  - 참조하는 쪽 링크를 모두 갱신한다: 외부(커밋 메시지, 이슈)에서 온 링크는
    갱신할 수 없다.
  - **번호를 바꾸지 않도록 소절 구성을 맞춘다**.
- **선택과 근거**: 셋째. 합치는 대신 소절 경계를 원래대로 두고 각 소절 안의
  분량만 줄였다 (`3.3 본문 형태`, `5.3 컴파일 결과`, `6.3 컴파일 결과`를 되살려
  `3.6`/`5.4`/`6.4`가 제자리에 오게 했다). 스텁 절 세 개가 사라졌고 링크는
  전부 유효하다.

### 결정 3: 앵커 검증을 눈이 아니라 스크립트로 한다

- **상황**: 절 번호가 대량으로 움직였고 문서 간 링크가 20개 이상이다.
- **선택과 근거**: 모든 `.md`에서 heading을 GitHub 슬러그 규칙으로 변환해
  집합을 만들고, 링크의 `#fragment`가 그 집합에 있는지 확인하는 파이썬
  스크립트를 돌렸다 (`docs/tasks/**` 제외). 결과 "앵커 전부 해석됨".
  이 과정에서 **정리 이전부터 깨져 있던 링크**를 하나 찾았다 — `cli.md`와
  `std.md`가 `language.md#8-제한사항`을 가리키는데 §8은 예약어, §9가
  제한사항이었다. 예약어를 §1.1(기본 원칙의 하위)로 옮기고 제한사항을 §8로
  올려 해결했다.

## 작업 내역

- 2026-08-17: 분량 측정 — `language.md` 669, `cli.md` 323, `errors.md` 186,
  `std.md` 134 (합 1,312줄).
- 2026-08-17: 문서 간 링크 수집 → `language.md#8-제한사항` 불일치 발견.
- 2026-08-17: `language.md` 재작성 (669 → 480). 예약어를 §1.1로, 제한사항을
  §8로. 절 번호는 §3.6/§5.4/§6.4를 포함해 전부 보존.
- 2026-08-17: `cli.md` 재작성 (323 → 229). 옵션 표를 사용자용/도구용으로 나누고,
  입력 수집·출력 경로·종료 코드를 표로. `--types`의 tsconfig 예시를 루트
  프로젝트 형태로 통일.
- 2026-08-17: `errors.md` 재작성 (186 → 119). 항목마다 원인/위치/해결 세 불릿을
  카테고리별 표 한 행으로 압축. 메시지 목록을 소스에서 직접 뽑아 대조:
  ```
  grep -ohE 'eprintln!\(\s*"rlc: [^"]*"' src/main.rs | sed 's/eprintln!( *"//; s/"$//' | sort -u
  ```
  이 대조에서 `--tsc` 잔재 두 곳을 찾았다 (아래 이슈 1). `--node`, shadow,
  `declaration emit failed`, `no declarations emitted` 등 TASK-040에서 생긴
  메시지가 문서에 없던 것도 채웠다.
- 2026-08-17: `std.md` 재작성 (134 → 105). API 표 두 개는 그대로 두고 앞뒤
  서술만 줄였다. 사용 예의 `import ... from "./rl.js"`를 `@rl/std`로 고쳤다
  (TASK-039 sweep이 놓친 곳).
- 2026-08-17: 앵커 검증 스크립트 통과 ("앵커 전부 해석됨").
- 2026-08-17: 문서에 적은 동작을 `/tmp/docverify`에서 실제로 실행해 확인:
  ```
  $ rlc -o build src/
  rlc: std → build/rl.ts
  rlc: src/shapes.rl → build/shapes.ts
  $ grep "Point:\|Active:" build/shapes.ts
    Point: { kind: "Point" } as const,
    Active: (): Status => ({ kind: "Active" }),
  $ rlc --check bad.rl
  rlc: bad.rl:2:25: match on enum Shape is not exhaustive: missing "Point" ...
  $ rlc --node          → rlc: --node requires a path to the node binary
  $ rlc src/plain.ts    → rlc: src/plain.ts: output would overwrite the input ...
  $ rlc --types src/     → .rl-types/{notice.rl.d.ts,.map,rl.d.ts}
  $ tsc -p tsconfig.json → exit 0   # cli.md의 rootDirs/paths 스니펫 그대로
  $ rlc --types src/     → rlc: src/notice.rl would shadow src/notice.ts — ...
  ```
  타입 에러가 있어도 사이드카가 갱신되고 종료 코드만 1이라는 문서 서술도
  같은 fixture에서 확인했다.

## 이슈 및 해결

### 이슈 1: 코드에 `--tsc` 시절 문구가 남아 있었다

- **증상**: 소스에서 뽑은 메시지 목록에
  `rlc: --tsc requires a path to the tsc binary`가 있었다. `--tsc` 옵션은
  TASK-040에서 `--node`로 대체되어 사라졌는데, `--node`의 값 누락 분기가 옛
  문장을 그대로 출력하고 있었다.
- **원인**: TASK-040에서 옵션 이름만 바꾸고 그 분기의 문자열은 확인하지 않았다.
  `errors.md`에도 `--tsc requires ...`와 `tsc not found — ... pass --tsc <path>`
  두 행이 남아 있었다.
- **해결**: `src/main.rs:463`을
  `rlc: --node requires a path to the node binary`로 고치고 문서를 실제 메시지에
  맞췄다. 문서를 코드에서 생성한 목록과 대조한 덕에 드러났다.

### 이슈 2: 별칭 절로 앵커를 살리려 했다

- **증상**: 첫 번역에서 `### 3.6 소진성 검사 (별칭)`처럼 "기존 링크를 위해
  남겨둔 앵커입니다"만 적힌 소절 세 개를 만들었다.
- **원인**: 소절을 합치면서 번호가 밀린 것을 링크 쪽에서 메우려 했다.
- **해결**: 소절 경계를 원래대로 복원해 번호가 밀리지 않게 하고 스텁을 지웠다.
  줄어든 분량은 그대로다.

### 이슈 3: 줄이다가 예시를 불완전하게 만들었다

- **증상**: `std.md` 사용 예에서 `parseNum` 정의를 지웠는데 아래 스니펫이 그
  함수를 호출하고 있었다.
- **원인**: 분량만 보고 지웠다.
- **해결**: 정의를 되살렸다. 예시는 그 자체로 완결이어야 한다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` — 171개 통과 (integration 26 포함)
- [x] 앵커 스크립트 — 문서 간 링크 전부 해석
- [x] 문서에 적은 CLI 동작·에러 메시지·tsconfig 스니펫을 실제 실행으로 확인

## 결과

- 수정: `docs/reference/language.md` (669 → 480),
  `docs/reference/cli.md` (323 → 229), `docs/reference/errors.md` (186 → 119),
  `docs/reference/std.md` (134 → 105) — 합 1,312 → 933줄
- 수정: `src/main.rs` (`--node` 값 누락 에러 문구), `docs/tasks/INDEX.md`
- 추가: `docs/tasks/TASK-041-reference-docs-slimming.md`

후속: `docs/design/`은 이 정리의 대상이 아니다 — 서술이 목적인 문서다.
