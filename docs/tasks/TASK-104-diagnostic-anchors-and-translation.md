# TASK-104: 진단 앵커와 진단 번역 (P4 계층 1·3)

- **상태**: 완료
- **시작일**: 2026-08-20
- **완료일**: 2026-08-20
- **커밋**: `142ae83`

## 목적

[TASK-101](./TASK-101-rust-parity-review.md) §10의 계층 1(진단 앵커)과
계층 3(진단 번역). 사용자 코드가 잘못돼 tsc가 **rlc가 쓴 글루**에서 에러를
낼 때, 그 진단의 **위치와 문안**을 rl에게 되돌려 준다.

실측한 출발점(TASK-101 §10.1):

```
errty.ts(7,57): error TS2322: Type 'Err<string>' is not assignable to type 'Result<number, number>'.
```

`errty.ts`는 사용자의 파일이 아니고 7번째 줄은 사용자가 쓰지 않은 줄이다
(`return $rl_t0;`). rl이 지고 있는 것은 **위치와 문안**이지 타입 지식이 아니다.

## 범위

- 포함: `EmitAnchor`(글루 → 구문의 단방향 기록), codegen의 앵커 방출,
  `(구문, TS 코드)` 화이트리스트 번역, CLI(typed)와 에디터 양쪽 소비, 문서.
- 제외:
  - 계층 2(seam의 `TypeQuery` 확장) — 별도 태스크. 이 컨테이너에 tsgo가 없어
    e2e 검증이 불가능하고, 계층 1·3은 툴체인 없이 검증된다(아래 결정 4).
  - 방출 코드 변경 — 앵커는 길이 0이라 출력 바이트가 바뀌지 않는다.

## 의사결정

### 결정 1: 앵커를 `EmitMapping`과 **다른 자료**로 둔다

- **상황**: 글루에도 "어디서 왔는지"를 기록하려면 기존 매핑에 항목을 추가하는
  것이 가장 쉽다.
- **검토한 대안**:
  - (a) `EmitMapping`에 글루 항목 추가 — 지금 매핑은 hover·definition·**rename**·
    진단이 **전부 공유**한다. 글루가 양방향으로 매핑되면 rename 편집이 글루에서
    소스로 되매핑되어 **엉뚱한 바이트를 고친다**. 프로그램이 깨진다.
  - (b) 별도의 단방향 자료(`EmitAnchor`) — 진단만 소비한다.
- **선택과 근거**: (b). 계약이 다르다: 매핑은 "이 바이트가 곧 그 바이트"라는
  양방향 동일성이고, 앵커는 "이 글루는 저 구문이 썼다"는 단방향 출처다. 한
  자료에 두 계약을 넣으면 rename이 깨진다.

### 결정 2: 앵커는 구문 단위로 하나, 세분화는 TS 코드가 한다

- **상황**: `try` 하나가 만드는 글루는 셋이다 — `.kind` 검사, `return $rl_t0`,
  `= $rl_t0.value`. 각각 뜻이 다른 에러를 그린다(대상이 Result가 아님 /
  Err 타입 불일치).
- **검토한 대안**: 글루 조각마다 다른 앵커 종류 / 구문당 앵커 하나 + `(종류,
  TS 코드)`로 문안 결정.
- **선택과 근거**: 후자. 코드가 이미 뜻을 구분한다(2339 = 없는 프로퍼티,
  2322 = 할당 불가). 앵커 종류를 늘리면 codegen이 진단 문안의 구조를 알게 되어
  단계 분리가 무너진다.

### 결정 3: 화이트리스트만 옮기고, 원문을 함께 싣는다

- **상황**: 글루에서 나온 진단을 전부 rl 문안으로 바꿀 것인가.
- **검토한 대안**: 전부 옮김 / 아는 것만 옮김.
- **선택과 근거**: 아는 것만. 모르는 진단을 그럴듯한 rl 문장으로 바꾸면 **틀린
  설명을 확신 있게 하는 것**이 되고, 그것은 못생긴 원문보다 나쁘다. 옮긴
  경우에도 원문을 괄호 안에 함께 실어 사용자가 검증할 수 있게 했다. 표에 없는
  코드는 기존 `(in code rlc generated for this construct)` 경로 그대로다.

### 결정 4: 계층 2(seam 확장)보다 계층 1·3을 먼저 한다

- **상황**: §10의 세 계층 중 무엇부터인가. 계층 2가 더 "정확한" 답을 준다.
- **검토한 대안**: 계층 2 먼저 / 계층 1·3 먼저.
- **선택과 근거**: 계층 1·3. ① 이 환경에 tsgo가 없어 계층 2(host.mjs의 질의
  추가)는 **검증할 수 없다** — 검증 없이 백엔드 프로토콜을 바꾸는 것은 이
  저장소의 게이트 문화와 맞지 않는다. ② 계층 1·3은 순수 Rust라 툴체인 없이
  단위·통합 테스트가 된다(`ProjectedDocument::project`는 파일시스템도
  TypeScript도 필요 없다). ③ 사용자가 잃고 있던 가장 큰 둘이 위치와 문안이다.

### 결정 5: TASK-100을 이 경로로 닫는다

- **상황**: TASK-100("TS enum을 scrutinee로 쓴 match를 rl 진단으로")은 계층 2
  (scrutinee 타입 질의)로 하려던 것이다. 번역이 같은 결과를 낸다:
  `$rl_m.kind`에서 나온 2339를 `match on a tag pattern needs a value with a
  \`kind\` discriminant ...`로 옮긴다.
- **검토한 대안**: TASK-100을 따로 남겨 계층 2로 다시 구현 / 이 경로로 닫기.
- **선택과 근거**: 닫는다. 사용자에게 보이는 결과가 같고(그 경우 tsc는 **항상**
  에러를 낸다 — `kind`가 없으므로), 같은 판정을 두 곳에 두면 드리프트한다.
  TASK-100 문서에 기록했다.

## 작업 내역

- 2026-08-20: `lib.rs` — `AnchorKind`/`EmitAnchor` 추가, `MappedEmit.anchors`와
  `MappedEmit::anchor_at`.
- 2026-08-20: `codegen/rope.rs` — `Piece::Open`/`Piece::Close`(길이 0),
  `Rope::anchored`, `flatten`이 4번째 값으로 앵커를 돌려준다. 중첩은 닫히는
  순서대로 쌓여 **안쪽이 먼저** 온다.
- 2026-08-20: `codegen/mod.rs` — 세그먼트 방출을 구문별로 감쌌다(match/tuple
  match/try/let-else/if let/pipe), `result` 블록은 `<-` 바인딩마다.
- 2026-08-20: `engine/semantics.rs` — `translate(kind, code, message)`
  화이트리스트. `report`가 글루 진단을 먼저 번역하고 같은 뜻의 중복을 합친다.
- 2026-08-20: `engine/projection.rs` — `translate_on_glue`(매핑이 있으면
  번역하지 않는다는 규칙이 여기 있다).
- 2026-08-20: `engine/language.rs` — `ServiceDoc`에 앵커를 싣고
  `service_diagnostics`가 같은 표로 번역. 에디터와 CLI가 한 표를 쓴다.
- 2026-08-20: 테스트 — `tests/emit_map.rs` +3(앵커 위치·중첩 순서·**출력 바이트
  불변**), `src/engine/projection.rs` +4(글루 번역, 사용자 코드는 번역 안 함,
  모르는 코드는 추측 안 함, 안쪽 구문이 자기 글루를 소유).
- 2026-08-20: 문서 — `errors.md` 새 절, `cli.md`, `language.md` §5.3,
  `docs/ai/rl.md`, `CHANGELOG.md`, `rust-parity-analysis.md` §10 상태 표시.

## 이슈 및 해결

### 이슈 1: `try (match ...)`로 쓴 중첩 테스트가 앵커를 못 찾음

- **증상**: `anchors_nest_innermost_first`가 "try anchored"에서 실패.
- **원인**: `try` 식은 `(`로 시작할 수 없다(`language.md` §5.1 — 인터페이스의
  `try(x);` 멤버 시그니처와 구분 불가). 그래서 그 문장은 애초에 `try`로 청구되지
  않았다 — 앵커가 아니라 입력이 문제였다.
- **해결**: `try wrap(match (e) { ... })`로 고쳤다.

### 이슈 2: 한 구문이 같은 뜻의 진단을 여러 개 그림

- **증상**: `try plain()` 하나에 TS2339(`.kind`)와 TS2551(`.value`)이 함께 나와
  같은 rl 문장이 두 번 보고됐다.
- **원인**: 글루가 임시 변수를 두 번 건드리고, 둘 다 같은 앵커에 속한다.
- **해결**: 번역된 진단은 (위치, 문안)이 같으면 합친다. CLI와 에디터 양쪽에
  같은 규칙을 넣었다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` — 11개 바이너리 전부 통과 (emit_map 15, lib 49, compile 237)
- 방출 불변: `anchors_do_not_change_the_emitted_bytes`가 7개 구문이 섞인 파일에서
  `emit_mapped`의 출력과 `compile`의 출력이 바이트 단위로 같음을 고정한다.
- 미검증: tsgo가 필요한 e2e(실제 체커가 만든 진단이 이 표를 타는지). 번역
  함수와 앵커 조회는 합성 진단으로 단위 검증했고, 실제 코드·오프셋은
  `ProjectedDocument::project`가 만든 진짜 방출물이다.

## 결과

- 공개 API: `AnchorKind`, `EmitAnchor`, `MappedEmit::anchors`,
  `MappedEmit::anchor_at`.
- 사용자가 보는 변화: `try`/`<-`/let-else/`if let`/`match`의 글루에서 나던 TS
  진단이 그 구문의 위치에서 rl의 문장으로 나온다(원문 동봉).
- 후속: 계층 2(`TypeQuery`) — 오타가 아닌 틀린 태그, 스크루티니가 정말 그
  enum인지 같은 **tsc가 에러를 내지 않는** 질문들은 여전히 물어봐야 답한다.
