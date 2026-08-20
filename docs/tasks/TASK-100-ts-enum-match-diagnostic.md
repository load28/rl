# TASK-100: TS enum을 scrutinee로 쓴 `match`를 rl 진단으로

- **상태**: 완료
- **시작일**: 2026-08-20
- **완료일**: 2026-08-20
- **커밋**: TASK-104와 같은 커밋

## 목적

TASK-073 이슈 3이 남긴 근본 해결이다. 괄호 없는 케이스만 가진
`enum Plain { A, B }`는 판별 규칙상 **TypeScript enum**이므로 그대로
통과되지만(계약 1), 거기에 `match`를 쓰면 방출 코드가 `$rl_m.kind` switch라
`Property 'kind' does not exist on type 'Plain'`이 나온다. 지금은 그 진단이
**rlc가 만든 글루 코드**에 떨어져 있어(TASK-089가 위치만 가장 가까운 앞선
verbatim 바이트로 보정하고 `(in code rlc generated for this construct)`를
붙인다) 사용자는 "왜 내 match가 TS 에러를 내는가"를 스스로 번역해야 한다.

에러 계층 계약(계약 2)상 이것은 **rl 수준 에러**다 — "이 scrutinee는 rl
enum이 아니다"는 rl의 판단이고, rlc가 위치와 함께 직접 보고해야 한다.
TASK-073~077로 타입을 물을 수 있게 됐으므로 이제 가능하다.

## 범위 (착수 시 확정)

- 포함 후보:
  - typed 경로(`--check-types`/`--types`/`--server`)에서 match scrutinee의
    타입을 물어, 판별자(`kind`) 필드가 없는 타입이면 rl 진단으로 보고.
  - 진단 문안과 `docs/reference/errors.md` 항목, `docs/ai/rl.md` 반영.
- 제외 후보:
  - untyped 배치 빌드에서의 판정 — 타입 없이는 알 수 없으므로 현행 유지.
  - TS enum에 대한 `match` 지원(방출 형태 변경) — 별도 사안.

## 의사결정

### 결정 1: 계층 2(타입 질의)가 아니라 진단 번역으로 닫는다

- **상황**: 원래 계획은 typed 경로에서 scrutinee의 타입을 물어 판별자 필드가
  없으면 rl 진단을 내는 것이었다. 그 사이 [TASK-104](./TASK-104-diagnostic-anchors-and-translation.md)가
  진단 앵커와 번역을 넣었고, 그것만으로 같은 결과가 나온다.
- **검토한 대안**:
  - (a) 계획대로 타입 질의 추가 — 백엔드(host.mjs) 프로토콜을 넓혀야 하고,
    같은 판정이 두 곳(질의와 번역)에 생겨 드리프트한다.
  - (b) 번역으로 닫기 — `$rl_m.kind`에서 나온 `TS2339`를
    `` match on a tag pattern needs a value with a `kind` discriminant — this
    scrutinee has none (a plain TypeScript `enum` is not one) `` 로 옮긴다.
- **선택과 근거**: (b). 이 경우 tsc는 **항상** 에러를 낸다(`kind` 프로퍼티가
  없으므로), 그래서 번역이 놓치는 경우가 없다. 사용자에게 보이는 결과 —
  `.rl`의 `match` 위치에서 rl 문안 — 도 같다.

## 작업 내역

- 2026-08-20: TASK-104에서 `(AnchorKind::Match, 2339 | 2571)` 항목으로 구현.
  이 태스크의 코드 변경은 없다.

## 이슈 및 해결

없음.

## 검증

- [x] TASK-104의 게이트로 갈음 (같은 커밋)

## 결과

`match`를 TypeScript `enum` 위에 쓰면 이제 `.rl`의 `match` 위치에서 rl 문안으로
보고된다. 원문(`ts2339: Property 'kind' does not exist on type 'Plain'.`)이
괄호 안에 함께 실린다. 규범은
[`errors.md`](../reference/errors.md#생성된-코드에서-난-타입-에러).

제외 항목은 그대로다: untyped 배치 빌드에서는 판정하지 않고(타입이 없다),
TS enum에 대한 `match` 지원(방출 형태 변경)은 별개 사안이다.
