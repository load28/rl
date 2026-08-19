# TASK-084: 큰 프로젝트에서의 host 스케일 — 응답 파이프 데드락 수정과 공유 타입 메모이제이션

- **상태**: 완료
- **시작일**: 2026-08-19
- **완료일**: 2026-08-19
- **커밋**: 9f81c70

## 목적

작은 프로젝트에서는 드러나지 않고 큰 프로젝트에서만 나타나는 host의 문제
두 가지를 고친다: ① 진단이 수백 건이면 `--check-types`가 **영원히 멈추는
데드락**(버그), ② match가 공유하는 스크루티니 타입에 대해 match 수만큼
반복되던 checker 왕복(스케일 비용).

## 범위

- 포함: host 응답의 동기 flush, ask 단위 타입 메모이제이션(constituents ·
  `kind` 심볼)과 `getTypeOfSymbol` 중복 제거, 데드락 회귀 테스트, 재현·측정.
- 제외: checker 자체의 검사 시간(전 프로그램 semantic diagnostics — tsgo의
  몫), `getPropertyOfType`/`getTypesOfType`의 batch endpoint 부재(upstream).

## 의사결정

### 결정 1: 데드락의 수정 지점은 host의 stdout 쓰기다

- **상황**: 큰 프로젝트를 흉내 낸 벤치(모듈 4개 × match 300개)에서
  `--check-types`가 10분 넘게 멈췄다. 변경 전(main) 바이너리도 동일 —
  **pre-existing 버그**다.
- **검토한 대안(원인 후보)**: ① tsgo checker의 병리적 느려짐, ② rlc↔host
  stderr 파이프 포화, ③ host↔tsgo sync 채널의 대용량 응답 처리, ④ host의
  stdout 비동기 쓰기.
- **선택과 근거**: ④. 이분탐색으로 절벽이 "진단 ~250건(답 ≈62KB) 통과 /
  300건(≈75KB) 행업"임을 확인 — 파이프 버퍼 64KB와 일치. 행업 중
  `/proc/<pid>/syscall`로 rlc는 `read` 대기, tsgo는 유휴(futex), node는
  스핀 중임을 확인. `process.stdout.write`는 파이프 버퍼를 넘는 나머지를
  이벤트 루프 flush 큐에 넣는데, host는 곧바로 `fs.readSync(stdin)`으로
  이벤트 루프를 막으므로 꼬리가 영영 나가지 못한다 — rlc는 줄의 나머지를
  기다리고 host는 다음 요청을 기다리는 상호 대기. 수정: 답 한 줄을
  `fs.writeSync`로 부분 쓰기·EAGAIN 재시도하며 **완전히 밀어낸 뒤** 루프를
  돈다(`writeLine`). `--types`의 대형 `.d.ts` 응답도 같은 경로라 함께
  고쳐진다.

### 결정 2: 타입 파생 답은 ask 단위로 타입 id에 메모한다

- **상황**: API 클라이언트는 Type/Symbol 객체를 id로 dedupe하지만
  `getTypes()`(union constituents)와 `getPropertyOfType()`은 호출마다
  IPC다. 큰 프로젝트의 match 수백 개는 소수의 스크루티니 타입을 공유하므로
  같은 질문이 match 수만큼 반복된다.
- **검토한 대안**: ① 클라이언트/서버에 batch endpoint 추가(upstream 변경),
  ② host에서 타입 id 키 메모(constituents, constituent별 `kind` 심볼) +
  `getTypeOfSymbol` batch를 distinct 심볼로 축소.
- **선택과 근거**: ②. 타입의 constituents와 `kind`는 그 타입만의 사실이라
  메모가 안전하고, 타입 id는 snapshot 스코프이므로 메모의 수명을 ask
  하나로 잡으면 정확하다(ask마다 새 snapshot). upstream 제안은 기록만.

## 작업 내역

- 2026-08-19: 재현 — enum 하나를 150곳에서 match하는 모듈 4개 벤치에서
  행업 확인(신·구 바이너리 동일). 진단 수 기준 이분탐색(150×1=756ms,
  125×2=1.0s, 300×1·150×2=행업)으로 64KB 절벽 확정, `/proc` syscall
  검사로 스핀/대기 상태 확인.
- 2026-08-19: `src/typescript/host.mjs` — `writeLine`(동기 flush) 추가,
  ack·answer 쓰기를 교체. ask 스코프 `constituentCache`/`kindCache` 추가,
  `missingLiterals`/`tagKindSymbols`가 이를 사용, tag의
  `getTypeOfSymbol` batch를 distinct 심볼로 축소해 흩뿌리기.
- 2026-08-19: `tests/native.rs` —
  `an_answer_past_the_pipe_buffer_still_arrives`(진단 400건, 답 >64KB) 추가.
- 2026-08-19: 검증 — 행업하던 4개 케이스 전부 1.0~4.2초로 완주(진단
  300~600건 전부 도착). native 22개 + 전체 447개 테스트 통과.
- 2026-08-19: 측정(3회 평균, cold, 진단 0건의 공유 타입 벤치):
  | 프로젝트 | 변경 전 | 변경 후 |
  |---|---|---|
  | tag+literal match 300, 모듈 1 | 1130 ms | 1045 ms |
  | 600, 모듈 2 | 1983 ms | 1823 ms |
  | 1200, 모듈 4 | 3694 ms | 3315 ms (−10%) |
  남는 시간의 대부분은 checker의 전 프로그램 검사(tsgo의 몫)다.

## 이슈 및 해결

### 이슈 1: 최초 벤치의 행업을 새 변경의 회귀로 오인할 뻔함

- **증상**: 메모이제이션 측정용 벤치에서 신 바이너리가 10분 초과.
- **원인 조사**: 변경 전 바이너리도 동일하게 행업 → pre-existing. 이후
  결정 1의 경로로 원인 확정.
- **해결**: 데드락을 먼저 수정하고 측정을 재개.

### 이슈 2: 벤치 생성 코드의 패턴 오타가 진단 폭주를 만들었음

- **증상**: `Square(x)`(필드명은 `s`)로 인해 모든 match에 TS2339 발생.
- **해결**: `Square(s: v)`로 수정 — 다만 이 오타 덕분에 "진단이 많은
  프로젝트"라는 데드락 재현 조건을 발견했다. 오타 케이스는 데드락 재현
  벤치로 남겨 활용.

## 검증 게이트

- `cargo fmt --check` — 통과
- `cargo clippy --all-targets -- -D warnings` — 통과
- `cargo test` — 447개 전부 통과 (native 22개 포함, 실 toolchain 구동)

## 변경 파일

- `src/typescript/host.mjs` — `writeLine` 동기 flush, ask 단위 타입 메모,
  distinct 심볼 batch
- `tests/native.rs` — >64KB 응답 회귀 테스트
