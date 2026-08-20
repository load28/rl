# TASK-109: 중첩 열의 알파벳을 체커에게 묻는다 (P4 계층 2, 2/2)

- **상태**: 완료
- **시작일**: 2026-08-20
- **완료일**: 2026-08-20
- **커밋**: `4845384`

## 목적

[TASK-108](./TASK-108-typed-coverage-on-usefulness.md)이 남긴 마지막 구멍.
중첩 패턴의 **안쪽 열**은 여전히 선언 표로 해석했고, 선언이 없는 타입
(손으로 쓴 유니언을 페이로드 타입으로 쓴 경우)은 알파벳을 알 수 없어
typed 경로가 침묵했다.

이것은 rl이 원리상 알 수 없는 것이다 — 필드의 선언 타입 텍스트는 텍스트일 뿐
타입이 아니고, 그 타입이 어떤 구성원을 갖는지는 TypeScript만 안다. 그러니
물어본다.

## 범위

- 포함: 중첩 패턴이 테스트하는 **수신자 위치**를 방출 시점에 기록하고
  (`PayloadTemp`), 그 위치의 구성원 목록을 기존 `TagQuery`로 묻고, 답을
  usefulness의 열 알파벳으로 주입.
- 제외:
  - 튜플 match의 typed 프로브 — 여전히 없다(TASK-101 §GAP-6).
  - `Discriminant`/`Property`/`Display` 질의 — 필요가 관측되면.

## 의사결정

### 결정 1: 새 질의를 만들지 않고 `TagQuery`를 그대로 쓴다

- **상황**: 안쪽 열의 알파벳을 물으려면 어떤 질의가 필요한가.
- **검토한 대안**:
  - (a) `ask::Members` 같은 새 질의 종류를 seam에 추가 — 설계 문서 §10.3의
    원안.
  - (b) 기존 `TagQuery`를 `covered: []`로 보낸다 — 질문이 **문자 그대로 같다**:
    "이 위치의 타입은 어떤 `kind` 값을 허용하는가". TASK-108이 이미
    `tagMembers`로 구성원 전체를 답하게 해 두었으므로 추가 배선이 없다.
- **선택과 근거**: (b). 같은 질문에 두 이름을 두지 않는다. 답의 인덱스는 한
  리스트에서 나오므로, match 질문을 **전부** 먼저 넣고 payload 질문을 그
  뒤에 넣어 `index < tags.len()`으로 구분한다(아래 이슈 2).

### 결정 2: 물어볼 자리는 방출된 조건의 **필드 이름**이다

- **상황**: `Ok(value: Some(v))`는 `$rl_m.value.kind === "Some"`으로 낮춰진다.
  이 조건의 어디를 물어야 페이로드의 타입이 나오는가.
- **검토한 대안**:
  - (a) 조건의 시작(`$rl_m`) — 실측 결과 **스크루티니 자신의 타입**이 나온다
    (`Outer`). 당연하다, 그 위치의 노드는 `$rl_m`이다.
  - (b) 필드 이름(`value`) — 프로퍼티 접근의 이름 노드이므로 그 프로퍼티의
    타입이 나온다.
- **선택과 근거**: (b). 실측으로 확인했다: (a)에서는 안쪽 열의 알파벳이
  바깥 enum의 태그(`Wrap`/`Bare`)로 나왔고, (b)에서는 `Yes`/`No`가 나왔다.
  방출 텍스트는 바뀌지 않는다 — 길이 0인 마크를 그 자리에 둘 뿐이다.

### 결정 3: 열 알파벳의 키는 `(생성자, 필드)`다

- **상황**: 답을 알고리즘 어디에 꽂을 것인가. 재귀는 열을 위치로만 다루고
  경로를 들고 다니지 않는다.
- **검토한 대안**: 재귀에 경로를 흘려보내기 / `(생성자, 필드)`로 키를 잡기.
- **선택과 근거**: 후자. 한 생성자의 한 필드는 **선언된 타입이 하나**이므로,
  그 쌍이 어디에 쓰이든 같은 열을 가리킨다. 재귀 시그니처가 그대로고
  (`Alphabets`가 표와 override를 함께 든다), 깊이 2 이상도 자연히 처리된다.

## 작업 내역

- 2026-08-20: `codegen/rope.rs` — `Piece::Mark`에 `MarkKind`(Scrutinee/Payload),
  `flatten`이 구조체 `Flat`을 돌려주도록(4-튜플이 5가 되는 것을 피했다).
- 2026-08-20: `codegen/matches.rs` — `pattern_conds_binds`가 조건을 `String`이
  아니라 `Rope`로 돌려주고, 중첩 링크의 필드 이름 자리에 payload 마크를 둔다.
  호출부(단일 if-체인·튜플 if-체인·`if let`)를 그에 맞게 바꿨다.
- 2026-08-20: `lib.rs` — `PayloadTemp`, `MappedEmit::payload_temps`.
- 2026-08-20: `probe.rs` — `payload_probes(source)`: 중첩 패턴마다
  `{ offset, tag, field }`.
- 2026-08-20: `engine/projection.rs` — payload 질문을 **별도 패스**로 모아
  `query.tags` 뒤에 붙이고 `probes.payloads`에 (파일, 생성자, 필드)를 기록.
- 2026-08-20: `engine/semantics.rs` — 인덱스로 match/payload를 갈라
  `checked_coverage`에 넘긴다.
- 2026-08-20: `analysis/usefulness.rs` — `Alphabets { table, payloads }`,
  `descend`가 override를 최우선으로 본다.
- 2026-08-20: 테스트 — native +2(손으로 쓴 유니언 페이로드: 구멍/전부 덮음),
  analysis +1(`certain` 플래그의 두 방향). 기존 native 1개는 이 태스크가
  바꾼 동작을 고정하고 있어 다시 썼다.
- 2026-08-20: `tests/native.rs`의 툴체인 가드를 절대 경로로 고쳤다(이슈 1).

## 이슈 및 해결

### 이슈 1: 형제 체크아웃이 생기자 native 테스트가 실패

- **증상**: `RLC_TSGO_ROOT` 없이 돌리면 `no tsgo executable at
  ../typescript-go/built/local/tsgo`로 실패. 이전에는 조용히 skip됐다.
- **원인**: 가드가 기본값으로 상대 경로 `../typescript-go`를 쓴다. 저장소
  루트에서는 존재하지만, 각 테스트는 **임시 프로젝트 디렉터리에서** rlc를
  실행하므로 거기서는 존재하지 않는다. 체크아웃이 없을 때는 skip돼서 이
  결함이 드러나지 않았다.
- **해결**: 가드가 루트를 `canonicalize`해 절대 경로로 넘긴다. 이제 환경
  변수 없이도 형제 체크아웃으로 27개가 전부 돈다.

### 이슈 2: 파일이 둘 이상이면 답이 엉뚱한 질문에 붙음

- **증상**: 두 `.rl` 파일이 있는 프로젝트에서 A파일의 payload 답이 B파일의
  match 답으로 해석돼, 최상위 소진성이 `missing "Yes", "No"`처럼 안쪽 태그를
  말했다.
- **원인**: 질의를 **파일별 루프 안에서** match→payload 순으로 넣었다. 그러면
  전체 순서가 `[A:match, A:payload, B:match, ...]`가 되는데, 되돌릴 때 쓰는
  `probes.tags`/`probes.payloads`는 종류별로 모여 있어 인덱스가 어긋난다.
- **해결**: payload 질문을 **모든 파일의 match 질문 뒤**에 별도 패스로 넣는다.
  그러면 `index < probes.tags.len()`이 곧 "match 답"이라는 불변식이 성립한다.
  주석으로 그 불변식을 못박았다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` — 11개 바이너리 전부 통과 (native 28개)
- [x] `npm test` (editors/vscode, RLC_TSGO_ROOT) — 78/78, skip 0
- [x] 수동 실측: 손으로 쓴 유니언 페이로드에 구멍이 있으면
      `missing "Wrap(inner: No)"`, 전부 덮으면 침묵

## 결과

- typed 경로의 소진성이 **중첩 열까지 타입 기준**이 됐다. rl이 원리상 알 수
  없는 것(선언이 없는 타입의 구성원)을 체커에게 묻고, 세는 일은 rl이 한다.
- `Uncovered::certain`은 남는다 — 체커도 확정된 답을 못 주는 타입(유한한
  리터럴 유니언이 아닌 것)이 있고, 그때는 여전히 침묵한다.
- 방출된 바이트는 바뀌지 않았다(마크는 길이 0).
- 후속: 튜플 match의 typed 프로브(§GAP-6).
