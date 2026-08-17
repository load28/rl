# TASK-028: TypeScript 사이드카 선언 설계 제안

- **상태**: 완료
- **시작일**: 2026-08-17
- **완료일**: 2026-08-17
- **커밋**: 4551488

## 목적

`.ts` 파일이 `.rl`을 import할 때 에디터가 `TS2307`을 내고 정의 이동이 되지
않는 문제를, `.rl.d.ts` + `.rl.d.ts.map` 사이드카로 푸는 방향을 정리한다.
정의 이동이 `.d.ts`가 아니라 **원본 `.rl`로** 가야 한다는 것이 요구사항이다.

## 범위

- 포함: `docs/design/ts-sidecar-declarations.md` 작성 — 문제, 검증된 해법,
  역할 분담, 결정 지점 5개.
- 제외: 구현(`rlc --sidecar`). 결정이 내려진 뒤 별도 태스크로 등록한다.
- 제외: 런타임 소스맵(`.ts.map`), 타입 추론.

## 의사결정

### 결정 1: `.d.ts` 본문은 tsc가, 위치 대응은 rlc가 만든다

- **상황**: 사이드카를 누가 생성할지 정해야 했다.
- **검토한 대안**:
  - rlc가 전부 생성: 외부 도구 없이 끝나지만, 통과 영역 함수·상수의 반환
    타입을 추론해야 한다. rlc는 타입 검사기가 아니므로 불가능하거나 반쪽이
    된다.
  - tsc가 본문, rlc가 맵: 각자 아는 것만 한다. rlc는 codegen이 통과 구간을
    바이트로 복사하므로 rl↔ts 대응을 정확히 알고, `--symbols`가 이미 enum
    위치를 낸다.
- **선택과 근거**: 후자. 에러 계층 분리 계약("타입은 tsc의 책임")과
  일치하고, rlc가 새로 만들 부분이 마지막 한 단계로 줄어든다.

### 결정 2: 요구사항을 프로토타입으로 먼저 검증한다

- **상황**: "정의 이동이 원본 `.rl`로 간다"가 TypeScript에서 실제로 가능한지
  불확실했다. 가능하지 않다면 설계 전체가 무의미하다.
- **검토한 대안**: 문서부터 쓰고 구현에서 확인 / 예제에서 먼저 검증.
- **선택과 근거**: 검증 먼저. `source/rl-interop`에 사이드카 생성
  스크립트를 만들고 tsserver를 직접 구동해 확인했다.
  ```
  진단: 없음
  main.ts:23  render  → src/notice.rl:21:17
  main.ts:10  Notice  → src/notice.rl:9:13
  ```
  덕분에 설계 문서가 실측 위에 서고, 구현 시 지켜야 할 조건(아래 이슈 1)도
  미리 드러났다.

## 작업 내역

- 2026-08-17: 원인 확인. `.ts` 문서는 tsserver가 소유하고 rl 언어 서버는
  `documentSelector: language: rl`이라 개입하지 않는다.
- 2026-08-17: 사이드카 성립 여부를 `/tmp/rldts`에서 확인했다. `x.rl.d.ts`가
  있으면 `nodenext`·`bundler` 양쪽에서 `./x.rl` import가 해결되고, 없으면
  `TS2307`이 난다.
- 2026-08-17: 선언 맵 추적을 `/tmp/rlmap`에서 확인했다. tsc가 만든
  `.d.ts.map`의 `sources`를 `.rl`로 바꾸고 tsserver를 구동하니
  `definition → notice.rl:3:17`로 원본을 가리켰다. 참고로 raw
  `LanguageService.getDefinitionAtPosition`은 `.d.ts`를 돌려준다 — 맵 추적은
  tsserver 세션 계층의 동작이다.
- 2026-08-17: `source/rl-interop`에 프로토타입을 만들었다 —
  `tsconfig.types.json`(선언 추출), `scripts/sidecar.mjs`(맵 재작성),
  `src/tsconfig.json`(에디터용 프로젝트), `npm run sidecar` 스텝.
- 2026-08-17: `docs/design/ts-sidecar-declarations.md` 작성,
  `docs/tasks/INDEX.md`에 TASK-028 등록.

## 이슈 및 해결

### 이슈 1: 0열 세그먼트만으로는 정의 이동이 `.d.ts`에 선다

- **증상**: 사이드카를 붙여 `TS2307`은 사라졌는데 정의 이동이
  `src/notice.rl.d.ts:24:25`에 그대로 섰다.
- **원인**: 생성한 맵이 각 선언 줄의 **0열**에만 세그먼트를 뒀다. 정의
  이동은 심볼 이름이 시작하는 열(위 예에서 24열)의 대응을 묻는데, 그
  위치에 세그먼트가 없으니 tsserver가 매핑을 찾지 못하고 원래 위치를
  그대로 돌려준다.
- **해결**: 각 선언 줄에 0열과 **이름 열** 두 세그먼트를 두도록 인코더를
  고쳤다. 재생성 후 `src/notice.rl:21:17`로 원본에 착지한다. 구현 시
  반드시 지켜야 하는 조건이므로 설계 문서에 별도 절로 적었다.

### 이슈 2: 추론 프로젝트에서는 선언 맵을 따라가지 않는다

- **증상**: 사이드카가 있는데도 정의 이동이 `.d.ts`에 서고, 별개로
  `TS2802`(downlevelIteration) 진단이 떴다.
- **원인**: `src/`를 포함하는 `tsconfig.json`이 없어 tsserver가 추론
  프로젝트로 열었다. 컴파일러 옵션이 기본값이 되고 선언 맵 추적도 걸리지
  않는다.
- **해결**: `src/tsconfig.json`(에디터 전용, `noEmit`)을 추가했다. 진단이
  사라지고 맵 추적이 동작한다. 설계 문서의 결정 지점 4번으로 남겼다.

## 검증

이 태스크는 문서만 변경하므로 Rust 게이트는 해당 없다 (`src/`, `tests/`
변경 없음).

- [ ] `cargo fmt --check` — 해당 없음
- [ ] `cargo clippy --all-targets -- -D warnings` — 해당 없음
- [ ] `cargo test` — 해당 없음
- [x] 해법을 tsserver 구동으로 실측 (위 "작업 내역"·"이슈")

## 결과

- 추가: `docs/design/ts-sidecar-declarations.md`
- 추가: `docs/tasks/TASK-028-ts-sidecar-declarations.md`
- 수정: `docs/tasks/INDEX.md`

후속: 결정 지점 5개에 답이 정해지면 `rlc --sidecar` 구현을 별도 태스크로
등록한다. 참조 구현은 `source/rl-interop/scripts/sidecar.mjs`(프로토타입).
