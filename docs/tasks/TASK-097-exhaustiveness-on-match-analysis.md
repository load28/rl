# TASK-097: sema 소진성을 MatchAnalysis 위로 — coverage 단일 원천

- **상태**: 완료
- **시작일**: 2026-08-20
- **완료일**: 2026-08-20
- **커밋**: —

## 목적

TASK-096이 남긴 유일한 구현 부채를 갚는다. 지금 match의 소진성 규칙은 **두
곳에 각각 구현**돼 있다 — `src/sema.rs`의 `check_exhaustiveness` /
`check_tuple_exhaustiveness`(보고 주체)와 `src/analysis.rs`의
`coverage_of`(모델 노출). 후보 enum 표(로컬 > 임포트 > 내장 섀도잉),
"무엇이 커버로 쳐지는가"(무가드·비중첩 arm만), subject 해석이 양쪽에
중복돼 있고, `docs/design/match-analysis.md` §5가 이 중복을 "어긋나면 버그로
취급한다"고 명시해 뒀다.

## 범위

- 포함:
  - `analysis.rs`가 소진성의 **단일 원천**이 된다: 후보 표·subject 해석·
    커버 규칙·튜플 곱집합(odometer)을 모두 소유하고, 결과를 `Coverage`
    데이터로 노출한다.
  - `Coverage`를 튜플 match까지 표현하도록 일반화(위치별 subject + 결손
    조합 witness). 단일 match는 arity 1의 특수 경우.
  - sema는 **보고만** 한다: 에러 문안·바이트 오프셋·보고 순서(walk 에러 →
    단일 match 소진성 → 튜플 소진성)를 그대로 유지.
  - sema의 extern 입력(`ExternEnum`, 태그만)과 분석의 extern 입력
    (`EnumSymbol`, 필드까지)을 같은 표 빌더가 받도록 내부 뷰 하나로 통일.
- 제외:
  - 에러 메시지 문안 변경 (`tests/compile.rs` 스냅샷이 불변 게이트).
  - 리터럴 match 소진성 — 계속 TypeScript 백엔드의 몫
    (`crate::literal_matches`).
  - `defer_to_checker`(네이티브 백엔드 위임) 경로의 동작 변경.
  - 패턴 binding 타입용 subject 해석 규칙 변경 (hover 동작 불변).

## 의사결정

### 결정 1: 계산은 analysis, 보고는 sema — 절반이 아니라 전부 옮긴다

- **상황**: 중복을 없애는 방법은 여러 가지다. sema가 자기 표를 유지한 채
  arm 태그 추출만 분석에서 받아 오는 절충도 가능했다.
- **검토한 대안**:
  - A. 부분 이관(태그 추출만) — 변경은 작지만 후보 표·커버 규칙·subject
    해석이 여전히 두 곳에 남아 중복의 핵심이 그대로다.
  - B. 전부 이관하되 에러 생성까지 analysis로 — 순수 단계가 `RlError`와
    보고 순서를 알게 되고, 에디터(무오류 소비자)에게 필요 없는 개념이
    모델에 들어온다.
  - C. 계산(표·해석·커버·곱집합)은 analysis, 문안·오프셋·보고 순서는 sema.
- **선택과 근거**: C. 계약상 "모든 rl 수준 에러는 rlc가 위치와 함께 직접
  보고한다"는 sema의 몫이고, "무오류로 항상 계산 가능한 모델"은
  analysis의 몫이다(TASK-096 결정 2·4). 확인: `tests/compile.rs`의 에러
  문안 단언 219건이 문안·위치·순서를 그대로 통과한다.

### 결정 2: `Coverage`를 arity로 일반화한다 (단일 = arity 1)

- **상황**: 튜플 소진성까지 옮기려면 모델이 조합을 표현해야 하는데, 기존
  `Coverage { covered: Vec<String>, missing: Vec<String> }`는 단일 match
  전용이었다.
- **검토한 대안**:
  - A. `Coverage`(단일)와 `ProductCoverage`(튜플) 두 필드 — 소비자가 둘 다
    분기해야 하고, "이 match의 소진성"이라는 하나의 질문이 두 자리로 쪼개진다.
  - B. `enum Coverage { Tags(..), Product(..) }` — 표현은 정확하지만 단일
    match가 흔한 경우인데도 매번 match 분기를 강요한다.
  - C. 위치 벡터로 일반화: `positions: Vec<Option<CoveredEnum>>`,
    `missing: Vec<Vec<Option<String>>>`(행 = 조합), 단일 match는 arity 1.
- **선택과 근거**: C. 파서가 튜플 스크루티니를 2개 이상으로 강제하므로
  (`parser/matches.rs`의 `parts.len() < 2`) `positions.len() == 1`이 곧
  "단일 match"라는 판별이 성립하고, sema의 두 보고 패스가 그 판별 하나로
  갈린다. 단일 match 소비자를 위해 `Coverage::missing_tags()`를 뒀다.
  `covered`는 단일 match에서만 의미가 있어(튜플 arm은 태그가 아니라 조합을
  커버한다) 그 사실을 타입이 아니라 문서로 명시했다 — 튜플에서는 빈 벡터.

### 결정 3: extern 입력 두 종류를 표 빌더가 흡수한다

- **상황**: sema는 `ExternEnum`(태그 + 지정자)를, 에디터는
  `EnumSymbol`(필드 타입까지)을 받는다. 같은 표를 두 입력으로 만들어야 했다.
- **검토한 대안**:
  - A. 공개 API를 하나로 통일(`Options::extern_enums`를 `EnumSymbol`로) —
    CLI의 수집기와 공개 표면이 함께 바뀌고, 소진성에 필요 없는 필드 타입을
    컴파일 경로가 읽게 된다.
  - B. 표 빌더에 어댑터 둘(`Table::build` / `Table::build_from_tags`)을 두고,
    태그만 있는 입력은 `fields: None`인 생성자로 싣는다.
- **선택과 근거**: B. 공개 API 변경 없이 규칙이 하나가 된다. 다만 태그만
  실린 표로는 binding 타입을 답할 수 없으므로, 그 경로는 binding 분석을
  아예 하지 않는다(`Depth::CoverageOnly`) — "반쯤 아는 타입"을 모델에
  실어 내보내지 않기 위해서고, 덤으로 컴파일 경로의 일감도 준다.

### 결정 4: 후보 동률의 순서는 선언 순서로 통일한다

- **상황**: 두 구현의 후보 순서가 실제로 달랐다 — sema는 로컬 enum을
  `BTreeMap`에 담아 **이름 오름차순**으로, analysis는 **선언 순서**로 봤다.
- **검토한 대안**: 알파벳 순 유지(기존 sema 동작 보존) / 선언 순서로 통일.
- **선택과 근거**: 선언 순서. 알파벳 순은 `BTreeMap`을 쓴 부작용이지 규칙이
  아니었고("로컬 > 임포트 > 내장"만이 규범 —
  `docs/reference/language.md` §3.6), 레퍼런스도 동률 시 어느 이름을
  부르는지는 규정하지 않는다. 차이가 보이는 경우는 **두 로컬 enum이 모든 arm
  태그를 포함하면서 결손 개수까지 같을 때 에러가 부르는 이름**뿐이고, 만족
  후보가 있으면 에러 자체가 없다는 판정은 순서와 무관하다.

### 결정 5: 단일 match와 튜플의 후보 선택 규칙 차이는 그대로 옮긴다

- **상황**: 이관 전 sema는 단일 match에서 "만족 후보 우선, 없으면 결손 최소"
  로, 튜플 위치에서는 "첫 후보"로 해석했다. 이관하면서 통일할 수도 있었다.
- **검토한 대안**: 지금 통일(튜플도 만족 후보 우선) / 동작 보존.
- **선택과 근거**: 동작 보존. 이번 태스크의 계약은 "에러 문안·판정 불변"
  이고, 튜플 위치의 규칙 변경은 곱집합 판정을 바꿔 새 오탐/누락을 만들 수
  있다. 차이는 `match-analysis.md` §6에 한계로 적어 두고, 바꿀지는 별도
  판단으로 남긴다.

### 결정 6: 내장 enum 표도 하나로 — `stdlib::BUILTIN_ENUMS` 제거

- **상황**: 이관 후 `BUILTIN_ENUMS`(태그만)의 유일한 소비자였던 sema가
  사라져 dead code가 됐다. analysis에는 필드까지 있는 표가 이미 있었다.
- **선택과 근거**: 제거. 같은 사실(내장 enum이 무엇인가)의 사본이 둘이면
  이번에 없앤 중복이 작게 되살아난다. `stdlib.rs`에는 표가 어디로 갔는지만
  주석으로 남겼다.

## 작업 내역

- 2026-08-20: 착수. 두 구현의 실제 차이를 먼저 확정 — 후보 표(sema
  `candidate_enums` ↔ analysis `Table::build`), 커버 규칙(양쪽 각자),
  subject 해석(첫 후보 ↔ 만족/결손 최소), 로컬 enum 순서(BTreeMap ↔ 선언
  순서), 튜플 곱집합(sema만).
- 2026-08-20: `src/analysis.rs` — `Origin`/`CoveredEnum` 추가, `Coverage`를
  위치 벡터로 일반화(+`missing_tags()`), 표를 `Entry`(name/origin/
  constructors) 기반으로 바꾸고 `build`(EnumSymbol)/`build_from_tags`
  (ExternEnum) 두 어댑터 도입, `candidates()`/`resolve_coverage()` 추가,
  `coverage_of`를 표 기반으로 재작성, `tuple_coverage_of`(odometer) 이관,
  `Depth`로 binding 분석 생략 경로 추가, `coverage_analyses()`(crate 내부)
  공개, `has_nested`를 이 모듈이 소유.
- 2026-08-20: `src/sema.rs` — `MatchCheck`/`TupleMatchCheck`/
  `check_exhaustiveness`/`check_tuple_exhaustiveness`/`candidate_enums`/
  `resolve_enum`/`Origin`/로컬 enum 레지스트리 삭제(총 -436줄 중 대부분),
  `report_coverage`/`describe` 추가. `Checker`는 `verify` 하나만 남았다.
- 2026-08-20: `src/stdlib.rs`에서 `BUILTIN_ENUMS` 제거, `src/lib.rs`에
  `Coverage`의 새 동반 타입(`CoveredEnum`, `Origin`) 재수출.
- 2026-08-20: 테스트 — `src/analysis.rs`에 4건 추가(만족 후보 우선/결손
  최소, 임포트 origin과 CoverageOnly가 binding을 건너뛴다는 것, 튜플
  곱집합, 보편 위치와 bare `_`), 기존 coverage 테스트를 새 모양으로 갱신.
  `tests/compile.rs`에 사용자 관점 단언 1건 추가
  (`a_candidate_the_arms_satisfy_makes_the_match_exhaustive`).
- 2026-08-20: 문서 — `docs/design/match-analysis.md` §5 전면 재작성(계산/
  보고 분리, 모델 모양, 두 규칙, extern 흡수), §2 다이어그램에 sema 추가,
  §6에 튜플 후보 규칙 차이 기재, `docs/design/compiler-architecture.md`
  §4의 소진성 항목, `CLAUDE.md` 아키텍처 맵(sema/analysis/stdlib) 갱신.
- 2026-08-20: 검증 게이트 3종 통과 (아래).

## 이슈 및 해결

### 이슈 1: 두 구현이 실제로 다른 답을 낼 수 있었다 (후보 선택)

- **증상**: 이관 전 코드를 나란히 놓고 보니, 모든 arm 태그를 포함하는 후보가
  둘일 때 sema는 "만족 후보가 하나라도 있으면 에러 없음, 없으면 결손 최소"
  로, analysis의 `coverage_of`는 "첫 후보의 결손"으로 답했다. 즉
  `enum Big { A, B, C }` / `enum Small { A, B }`에 대해 `A`·`B`만 다루는
  match를 sema는 통과시키고 모델은 `missing: ["C"]`라고 말했다.
- **원인**: TASK-096이 모델의 coverage를 "sema와 같은 규칙"이라고 적었지만,
  실제로는 subject 해석(첫 후보)을 그대로 쓴 채 결손만 계산했다. 설계
  문서가 경고한 "두 구현이 어긋나면 버그"가 이미 실현돼 있었다.
- **해결**: 통일된 규칙은 sema 쪽(만족/결손 최소)으로 잡고
  `Table::resolve_coverage`에 한 번만 구현했다. 타입 질문은 여전히 첫
  후보(`Table::resolve`)를 쓰며, 두 질의가 같은 표를 본다는 점을
  `match-analysis.md` §5에 명시했다. 새 단위 테스트
  `coverage_prefers_the_candidate_the_arms_satisfy`와 컴파일 테스트
  `a_candidate_the_arms_satisfy_makes_the_match_exhaustive`가 이 규칙을
  양쪽 관점에서 고정한다.

### 이슈 2: 테스트 픽스처가 rl enum이 아니었다

- **증상**: 새로 쓴 coverage 테스트 3건이 `coverage: None`으로 실패했다
  (`.expect("resolved")` 패닉).
- **원인**: 픽스처를 `enum Big { A, B, C }`처럼 **괄호 없는 케이스만**으로
  썼다. 판별 규칙상 그것은 TypeScript enum이라 통과 대상이고, rl enum
  선언 표에 들어가지 않는다 (계약 1).
- **해결**: 각 픽스처에 페이로드 케이스를 하나씩 넣어 rl enum으로 만들었다
  (`enum Big { A(s: string), B, C }`). 규칙 자체는 정상 동작이었다.

### 이슈 3: clippy `never_loop`

- **증상**: `report_coverage`를 "첫 미커버 match에서 반환"하는 `for` 루프로
  쓰자 `-D warnings`에서 `clippy::never_loop`로 막혔다.
- **원인**: 미커버 항목만 미리 걸러 두었으므로 루프의 첫 반복이 항상
  반환한다 — 루프가 아니라 `find`였다.
- **해결**: `iter().find(...)` + `if let`으로 바꿨다. 의도(첫 항목이 에러를
  결정한다)가 코드에 드러나고 경고도 사라졌다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`

## 결과

변경 파일: `src/analysis.rs`, `src/sema.rs`, `src/stdlib.rs`, `src/lib.rs`,
`tests/compile.rs`, `tests/emit_map.rs`(TASK-098과 공유),
`docs/design/match-analysis.md`, `docs/design/compiler-architecture.md`,
`CLAUDE.md`, 본 태스크 문서.

핵심 결과: 소진성의 규칙이 한 곳(`src/analysis.rs`)에만 있다 — 후보 표,
subject 해석, 커버 판정, 튜플 곱집합. sema는 그 `Coverage`를 위치 있는
에러로 옮기는 일만 한다(`report_coverage`, -436줄). 에러 문안·위치·보고
순서는 불변이고(`tests/compile.rs` 219건 통과), 이관 과정에서 두 구현이
실제로 어긋나 있던 지점(후보 선택)을 찾아 sema 쪽 규칙으로 통일했다.

사용자 체감 동작은 바뀌지 않았으므로 `docs/ai/rl.md`는 갱신 대상이 아니다
(언어 표면·CLI·표준 라이브러리 변화 없음).
