# TASK-107: 에디터가 엔진의 rl 표면을 쓴다 (P3 3/3)

- **상태**: 완료
- **시작일**: 2026-08-20
- **완료일**: 2026-08-20
- **커밋**: —

## 목적

[TASK-105](./TASK-105-rl-name-surface.md)·[TASK-106](./TASK-106-pattern-completions.md)이
만든 엔진 표면(`rlSymbol`·`rlCompletions`)을 VS Code 서버가 실제로 쓰게 하고,
`analysis.ts`에 있던 **해석 규칙의 두 번째 구현**을 지운다
(TASK-101 §GAP-3).

## 범위

- 포함: `engine.ts` 클라이언트 함수, `server.ts`의 hover·definition·rename·
  완성 경로 교체, `analysis.ts`에서 죽은 함수 삭제와 그 테스트 정리,
  e2e 테스트 추가.
- 제외:
  - 빠진 암 quick fix와 문서 심볼 — 여전히 `parseEnums`/`parseMatches`를 쓴다.
    별도 표면(코드 액션)이 필요하고, 이 태스크의 목적(해석 규칙 단일화)과는
    다른 문제다.
  - references/rename 합성 — rename은 지금도 rl 이름 위에서 **거부**한다.

## 의사결정

### 결정 1: rename은 계속 거부한다 (판정만 엔진으로)

- **상황**: 기존 서버는 `symbolAt`이 rl 심볼을 알아보면 rename을 거부한다.
  엔진 표면으로 바꾸면서 "이제 할 수 있게" 만들 수도 있었다.
- **검토한 대안**: 지금 rename 합성까지 구현 / 판정만 옮기고 거부는 유지.
- **선택과 근거**: 후자. 태그 이름 바꾸기를 **완전하게** 하려면 선언·생성자
  호출부(tsc가 아는 것)·모든 패턴 자리(rl만 아는 것)를 한 번에 고쳐야 하고,
  그 합성은 안정 식별자가 전제다(TASK-101 §GAP-2). 반쪽 rename보다 거부가
  옳다는 기존 계약을 지키면서, **무엇이 rl 이름인가**의 판정만 엔진으로
  옮겨 두 번째 의견을 없앴다.

### 결정 2: 이미 쓴 케이스를 완성 목록에서 빼지 않는다 (동작 변경)

- **상황**: 기존 arm 완성은 커버된 태그를 뺐다. 엔진은 `covered` 플래그로
  표시만 한다(TASK-106 결정 2).
- **검토한 대안**: 에디터에서 다시 필터 / 표시만 하고 정렬로 밀기.
- **선택과 근거**: 후자. 가드 암(`A if c => ...`)을 여러 개 쓰는 것은 정상이고,
  뺐다면 두 번째 가드 암을 쓸 때 완성이 침묵한다. `sortText`로 뒤에 오게 했다.

### 결정 3: 서비스가 필요한 테스트의 skip 가드를 고친다

- **상황**: `server.test.ts`의 완성 테스트 4개가 `rlc`만 확인하고 tsgo는
  확인하지 않아, rlc가 PATH에 있고 tsgo가 없는 환경에서 **실패**했다(이번
  작업 전부터). `toolchain.ts`의 주석은 "skip은 도구가 없다는 뜻이어야 한다"고
  적고 있다.
- **검토한 대안**: 그대로 두기(내 변경과 무관하므로) / 가드 수정.
- **선택과 근거**: 수정. 그 4개는 TypeScript 서비스의 답을 검사하므로 tsgo가
  전제다. 가드를 고치지 않으면 이 태스크의 e2e 결과를 읽을 수 없다 — 실패가
  섞여 있으면 무엇이 회귀인지 구분되지 않는다.

## 작업 내역

- 2026-08-20: `engine.ts` — `EngineRlSymbol`/`EngineRlCompletion` 타입과
  `rlSymbol`/`rlCompletions` 클라이언트 함수.
- 2026-08-20: `server.ts` —
  - hover: `analysis.symbolAt` 블록 → `engine.rlSymbol`. `match` 키워드 hover는
    이름이 아니라 구문 설명이라 그대로 둔다.
  - definition: rl 이름이면 엔진이 준 위치로, 아니면 기존 경로.
  - rename: 판정만 `engine.rlSymbol`로.
  - completion: `armContextAt`의 두 분기(필드·암) → `engine.rlCompletions`
    한 번. 엔진이 아는 자리가 더 많다(`if let`, let-else 페이로드, 중첩).
  - 모듈 문서의 계층 설명 갱신.
- 2026-08-20: `analysis.ts` — `matchBodyAt`·`ArmContext`·`armContextAt`·
  `armTags`·`splitArms`·`inferEnum`·`SymbolAt`·`symbolAt`·`enumSignature` 삭제
  (821 → 633줄). `analysis.test.ts`에서 그 11개 테스트 삭제.
- 2026-08-20: `server.test.ts` — 서비스 의존 4개에 `skipTyped` 가드,
  rl 표면 e2e 4개 추가(태그 hover는 match와 `if let` 양쪽, 필드 hover,
  태그 definition, 패턴 자리 완성 3종).
- 2026-08-20: 문서 — `lsp-architecture.md` §33 갱신, `CHANGELOG.md`.

## 이슈 및 해결

### 이슈 1: 새 e2e 테스트가 `_`를 못 찾음

- **증상**: `pattern positions complete cases and fields`가
  `missing _ in: Circle,Rect,Point`로 실패.
- **원인**: 테스트는 PATH의 `target/debug/rlc`를 쓰는데, 직전에 `cargo test
  --lib`만 돌려 **바이너리가 재빌드되지 않았다**. 와일드카드 항목은 그 빌드에
  없었다.
- **해결**: `cargo build` 후 재실행 — 통과. (에디터 e2e는 항상 방금 빌드한
  바이너리로 돌려야 한다.)

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` — 11개 바이너리 전부 통과
- [x] `npm test` (editors/vscode, PATH에 방금 빌드한 rlc) — 78개 중 36 통과,
      42 skip(tsgo 없음), **0 실패**. 작업 전 같은 환경에서 4개가 실패했고,
      그 4개는 이제 정직하게 skip된다.

## 결과

- 사용자가 보는 변화: `if let`·let-else·중첩 패턴에서 hover·정의 이동·완성이
  **처음으로** 동작한다. 케이스와 이름이 같은 지역 변수가 enum 케이스로
  hover되던 오탐이 사라졌다.
- 구조: rl 이름에 대한 답이 한 곳(엔진)에서 나온다. `analysis.ts`에 남은 것은
  이 프로세스가 스스로 읽는 구조뿐이다(`match` 키워드 위치, 멤버 접근 판정,
  문서 심볼, quick fix).
- 후속: quick fix·문서 심볼의 이관, references/rename 합성(안정 식별자 필요),
  P4 계층 2(`TypeQuery`).
