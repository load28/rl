# TASK-091: 에디터 typed 진단 지연 단축 — debounce 축소와 예약 위치 이동

- **상태**: 완료
- **시작일**: 2026-08-20
- **완료일**: 2026-08-20
- **커밋**: 이 커밋

## 목적

`val` mutation 에러처럼 typed rl layer에서만 나오는 진단이 VS Code에서 약 2초
늦게 표시된다. `match` 누락 같은 텍스트 수준 진단(300ms debounce)과 체감이
비슷해지도록 typed pass의 지연을 줄인다.

## 범위

- 포함: `editors/vscode/server/src/server.ts`의 `TYPED_CHECK_DELAY_MS` 축소,
  `validate()` 안에서 typed check 예약 위치를 TypeScript 진단 await 앞으로
  이동, `editors/vscode/package.json`의 `rl.typedChecks` 설명 갱신.
- 제외: 버전 gate(`fresh.version !== doc.version`)와 진단 병합
  규칙(`mergeTyped`)의 변경. 엔진/컴파일러 쪽 변경 없음.

## 배경 — 왜 원래 보수적이었나

TASK-072에서 typed check가 에디터에 처음 들어올 때는 실행마다 프로젝트를
새로 열고 TypeScript 컴파일러를 구동하는 비싼 작업이었다. 그래서
① 1200ms의 긴 debounce로 "타이핑이 확실히 멈춘 뒤"에만 시작하고,
② 예약을 `validate()`의 끝(기본 검사 + TS 진단 매핑 완료 후)에 두어 무거운
요청이 같은 세션에 겹치지 않게 직렬화했다. 이후 TASK-076(증분 검사 — 서버
세션 유지)·TASK-083/084(host batch·스케일)·TASK-086/087(engine 재구성)을
거치며 typed check가 살아있는 세션 위의 증분 질의로 바뀌어 이 비용 전제가
사라졌다. 정확성(오래된 버전의 결과 폐기)은 debounce가 아니라 버전 gate가
지키므로 지연만 줄이면 된다.

## 의사결정

### 결정 1: `TYPED_CHECK_DELAY_MS` 1200 → 250

- **상황**: typed 진단의 체감 지연이 구조적으로 최소
  300(기본 debounce) + TS 진단 매핑 + 1200 ≈ 1.5~2초였다.
- **검토한 대안**:
  - 300(기본과 동일): 기본 검사와 typed check가 매 키 입력마다 정확히 같은
    시점에 발화 — 엔진에 동시 부하가 몰리는 창이 가장 넓다.
  - 250: 기본 debounce보다 약간 짧아 예약 시점 차이를 상쇄하면서도,
    타이핑 중 연속 재예약(clearTimeout)으로 실제 발화는 멈춘 뒤 한 번이다.
  - 600 등 중간값: 증분 엔진에서는 근거 없는 여유분.
- **선택과 근거**: 250. 증분 엔진(TASK-076)에서 검사 자체가 싸졌고, stale
  결과는 버전 gate가 버리므로 debounce는 발화 빈도 조절만 하면 된다.

### 결정 2: 예약을 `typeDiagnostics()` await 앞으로

- **상황**: 기존에는 TS 진단 매핑(엔진 왕복)이 끝난 뒤에야 typed check
  타이머가 시작되어, debounce를 줄여도 그 대기 시간이 지연에 그대로
  더해졌다.
- **검토한 대안**:
  - 예약을 `scheduleValidation`(문서 변경 시점)으로 더 앞당기기: 컴파일러
    경로 해석(settings)이 아직 없고, not-found/failed 조기 반환 전에
    예약되어 무의미한 실행이 생긴다.
  - `validate()` 안, 기본 진단 변환 직후(채택): 컴파일러가 존재하고 버퍼
    버전이 유효함이 확인된 가장 이른 지점.
- **선택과 근거**: 후자. `publish()`가 base/typed 양쪽에서 버전이 일치할
  때만 병합하므로 typed 결과가 base 게시보다 먼저 도착해도 안전하다 —
  typed 결과는 캐시에 저장되고, base가 게시될 때 함께 병합된다.

### 결정 3: 버전 gate와 병합 규칙은 그대로

- **상황**: 지연을 줄이면 stale 결과 빈도가 늘 수 있다.
- **선택과 근거**: `typedCheck()`의 `fresh.version !== doc.version` 검사와
  `mergeTyped`의 "먼저 실행된 pass가 이긴다" 규칙을 유지한다. 오래된 버전에
  대한 `val` 에러가 엉뚱한 위치를 가리키는 것보다 한 박자 늦는 쪽이 낫다는
  기존 계약은 그대로 유효하다.

## 작업 내역

- 2026-08-20: `editors/vscode/server/src/server.ts` —
  `TYPED_CHECK_DELAY_MS`를 1200에서 250으로 축소하고, 주변 주석의 "whole
  project / longer debounce" 서술을 증분 엔진 기준으로 갱신.
- 2026-08-20: 같은 파일 `validate()` — `scheduleTypedCheck(doc, compiler)`
  호출을 함수 끝에서 `const diagnostics = result.diagnostics.map(...)` 직후,
  `settings.typeDiagnostics`의 `typeDiagnostics()` await 앞으로 이동.
- 2026-08-20: `editors/vscode/package.json` — `rl.typedChecks` 설명에서
  "trails the other diagnostics by about a second"를
  "may trail the other diagnostics briefly"로 갱신.
- 검증: `cd editors/vscode && npm install && npm run compile`,
  `node --test "server/out/test/typedcheck.test.js"`, 루트에서
  `cargo test val`.

## 이슈 및 해결

없음.

## 검증

- [x] `cargo fmt --check` (Rust 변경 없음 — 통과 확인)
- [x] `cargo clippy --all-targets -- -D warnings` (Rust 변경 없음 — 통과 확인)
- [x] `cargo test val`
- [x] `cd editors/vscode && npm run compile`
- [x] `node --test "server/out/test/typedcheck.test.js"`

## 결과

변경 파일: `editors/vscode/server/src/server.ts`(debounce 250ms, 예약 위치
이동), `editors/vscode/package.json`(설정 설명). `val` mutation 등 typed
진단의 체감 지연이 약 1.5~2초에서 기본 진단과 비슷한 수백 ms 수준으로
줄어든다. 진단 내용·정확성(버전 gate, 병합 규칙)은 변경 없음.
