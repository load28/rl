# TASK-102: 패턴 사이트 일반화와 이름 해석 진단 (P1+P2)

- **상태**: 진행 중
- **시작일**: 2026-08-20
- **완료일**: —
- **커밋**: —

## 목적

[TASK-101](./TASK-101-rust-parity-review.md)의 제안 P1·P2를 구현한다. rustc가
패턴을 다루는 첫 두 단계 — **경로 해석(resolve)** 과 **타입 붙은 패턴** — 을
rl에 놓는다. `match`에만 있던 typed 분석을 패턴을 가진 모든 구문으로 넓히고,
패턴의 태그·필드가 선언에 닿지 않을 때 rlc가 직접 보고한다.

이것이 고치는 실제 버그: 태그 오타 하나로 후보 표에서 enum이 사라져
**소진성 검사가 조용히 꺼진다**(TASK-101 §GAP-1).

## 범위

- 포함:
  - `analysis.rs`: let-else·`if let`을 패턴 사이트로 분석(subject 해석 +
    바인딩 타입), 해석 실패 사실(`UnresolvedName`)의 계산.
  - `sema.rs`: 그 사실을 위치 있는 rl 에러로 보고(계산과 보고의 분리는
    `Coverage`와 동일).
  - `ast.rs`/`parser/lets.rs`: let-else 패턴 태그의 바이트 오프셋 기록
    (에러를 태그 자리에 찍기 위해 필요).
  - `errors.md`·`language.md`·`docs/ai/rl.md` 갱신.
- 제외:
  - 타입이 필요한 판정(스크루티니가 정말 그 enum인가) — P4.
  - 에디터 표면 이관 — P3.
  - 소진성 알고리즘 교체 — P5(별도 태스크).
  - `EnumId`/`CaseId` 같은 안정 식별자 — P3에서 rename 합성에 필요해질 때.

## 의사결정

### 결정 1: 해석 실패를 그 자체로 에러 삼지 않고 "오타 면허"를 요구한다

- **상황**: 태그가 선언 표에 없을 때 곧바로 에러로 만들면 어떻게 되는가.
  `language.md` §3.2는 태그 패턴의 대상을 "rl enum 값과 **`kind` 문자열 필드를
  가진 모든 태그드 유니언**"으로 규정한다. 즉 손으로 쓴 TS 유니언에 태그
  패턴을 쓰는 것은 **지원되는 정상 사용**이고, 그 태그들은 선언 표에 없다.
  무조건 에러로 만들면 정상 프로그램이 깨진다.
- **검토한 대안**:
  - (a) 무조건 보고 — 손으로 쓴 유니언 사용자를 전부 깨뜨린다. 기각.
  - (b) 사이트의 태그를 하나라도 포함하는 enum이 유일하면 나머지를 보고 —
    태그 이름이 우연히 겹치는 유니언(`enum Shape { Empty, ... }`와
    `type Msg = {kind:"Empty"} | {kind:"Full"}`)에서 오탐. 지금은 침묵하는
    프로그램이 에러가 되므로 회귀다.
  - (c) 확증 태그를 2개 이상 요구 — 안전하지만 대표 사례
    (`Circel(r) => .., Empty => ..`, 확증 태그 1개)를 놓친다.
  - (d) **오타 면허**: 해석 실패한 이름이 지목된 enum의 어떤 이름의 **오타로
    보일 때만** 보고한다(대소문자 무시 일치 또는 편집 거리 ≤ 2).
- **선택과 근거**: (d). 근거는 "보고할 수 있다"가 아니라 **"고칠 이름을 댈 수
  있다"** 를 보고의 자격으로 삼는 것이다. 실측한 오탐 시나리오가 전부 걸러진다:
  `Full` vs `{Circle, Empty}`는 거리 4로 침묵, 손으로 쓴 `{kind:"Some"; v: T}`의
  필드 `v` vs 내장 `Option`의 `value`도 거리 4로 침묵. 반대로 실제 오타
  (`Circel`/`radiuz`)는 거리 1~2로 전부 잡힌다. 잡지 못하는 것(오타가 아닌
  틀린 이름)은 타입이 있어야 알 수 있으므로 P4의 몫이며, 그 경계를 문서에
  명시한다.

### 결정 2: 해석 실패 시 그 사이트의 이후 판정을 중단한다

- **상황**: 태그 오타를 보고한 뒤 소진성도 함께 보고할 것인가.
- **검토한 대안**: (a) 둘 다 보고 — 오타 하나로 "빠진 케이스" 목록까지 쏟아져
  원인이 묻힌다. (b) 해석 에러만 보고하고 멈춘다 — rustc가 resolve 실패 시
  타입 검사로 넘어가지 않는 것과 같다.
- **선택과 근거**: (b). 에러는 어차피 컴파일을 멈추므로 사용자는 오타를 고친
  뒤 소진성 답을 받는다. "조용히 꺼진다"는 원래 문제는 **침묵이 사라졌다는
  것만으로 해결**된다.

### 결정 3: 계산은 `analysis.rs`, 보고는 `sema.rs`

- **상황**: 오타 면허·지목 규칙을 어디에 둘 것인가.
- **검토한 대안**: sema에 직접 구현 / 분석이 사실을 만들고 sema가 문안을 만든다.
- **선택과 근거**: 후자. TASK-097이 `Coverage`에 대해 확립한 분업 그대로다 —
  규칙이 하나면 구현도 하나여야 하고, 에디터(P3)도 같은 사실을 소비해야 한다.

### 결정 4: `MatchAnalyses` → `PatternAnalyses`로 개명

- **상황**: 컨테이너가 이제 match가 아닌 사이트(let-else·`if let`)도 담는다.
- **검토한 대안**: 이름 유지(변경 최소) / 개명(정직성).
- **선택과 근거**: 개명. 이 저장소는 이름이 내용과 어긋나는 것을 버그로 취급해
  왔고(모듈 문서·설계 문서 전반), 공개 표면 소비자는 `lib.rs` 재수출과
  `engine/language.rs` 몇 줄뿐이라 비용이 작다. `MatchAnalysis`(match 하나)는
  그대로 둔다 — match는 여전히 match다.

### 결정 5: 편집 거리를 Levenshtein에서 OSA(자리바꿈 포함)로

- **상황**: 결정 1의 오타 면허를 단일 패턴 사이트(let-else·`if let`)에도 줄지.
  단일 사이트는 다른 태그의 뒷받침이 없어 근거가 얇으므로 면허를 좁히고 싶은데,
  "편집 한 번"으로 좁히면 대표 오타인 `Circel`(자리바꿈)이 Levenshtein으로는
  거리 **2**라서 걸리지 않는다.
- **검토한 대안**:
  - (a) 단일 사이트는 태그 검사를 포기 — `if let`을 match와 동급으로 만든다는
    목적의 절반을 버린다.
  - (b) 단일 사이트에도 거리 2를 허용 — 손으로 쓴 유니언과 태그 이름이 겹칠 때
    오탐 위험이 가장 큰 자리에서 가장 넓은 면허를 주게 된다.
  - (c) 거리 계산을 **OSA(Damerau)** 로 바꿔 자리바꿈을 한 번으로 세고, 단일
    사이트는 "한 번"으로 좁힌다.
- **선택과 근거**: (c). 자리바꿈은 가장 흔한 오타 종류이고, 그것을 한 번으로
  세는 것은 편집 거리 정의의 문제이지 면허를 넓히는 것이 아니다. 확인:
  `edit_distance("Circel", "Circle") == 1`, `edit_distance("Cyrcla", "Circle") == 2`
  (단위 테스트 `a_transposition_counts_as_one_edit`). 결과적으로 `Circel`은
  match·let-else·`if let` 어디서나 잡히고, 두 편집짜리 오타는 match의 다른
  암이 enum을 지목할 때만 잡힌다.

## 작업 내역

- 2026-08-20: `ast.rs`/`parser/lets.rs` — `LetElseStmt`에 `tag_off` 추가.
  let-else는 패턴을 `TagPattern`이 아니라 `tag: String` + `bindings`로 들고
  있어 태그의 바이트 위치가 없었다(에러를 태그 자리에 찍으려면 필요).
- 2026-08-20: `analysis.rs` —
  - `MatchAnalyses` → `PatternAnalyses`, `match_analyses` → `pattern_analyses`.
    새 필드 `sites: Vec<PatternSite>`, `unresolved: Vec<UnresolvedName>`.
  - `analyze_site`(let-else·`if let` 공통 본체) / `analyze_let_else` /
    `analyze_if_let` 추가. `collect_bindings`가 `&TagPattern` 대신 `&str` 태그를
    받도록 바꿔 두 구문이 같은 코드를 쓰게 했다.
  - `Table::identify` — 해석의 지목 질의(표의 세 번째 질의).
    `Table::entry_of_type` — `resolve_type`의 엔트리 반환 형태.
  - `resolve_alternatives` / `resolve_bindings` — 태그·필드 대조, 중첩은 필드의
    선언 타입을 통해 재귀.
  - `nearest` / `nearest_within` / `typo_distance` / `edit_distance`(OSA) /
    `unique_near_case` — 오타 면허.
  - `binding_at`이 `sites`의 바인딩까지 찾도록 확장.
- 2026-08-20: `sema.rs` — `check`가 분석을 **한 번** 만들어 `report_resolution`
  (신규) → `report_coverage`(시그니처를 `&PatternAnalyses`로) 순으로 소비.
  해석 에러는 `defer_to_checker`(typed 경로)에서도 보고된다 — 구조 판정이므로.
- 2026-08-20: `lib.rs`/`engine/language.rs` — 개명 반영, 새 타입 재수출.
- 2026-08-20: 테스트 — `tests/compile.rs`에 13개(오타 보고, 위치, let-else·
  `if let`·중첩·튜플·내장·임포트, 손으로 쓴 유니언 무보고, 두 편집 규칙),
  `src/analysis.rs`에 5개(사이트·체인·해석 답·동점·거리).
- 2026-08-20: 문서 — `errors.md`(새 절 "패턴의 이름 해석"),
  `language.md`(§3.10 + 제한사항 행), `docs/ai/rl.md`, `CHANGELOG.md`,
  `docs/design/match-analysis.md` §7(모델의 두 번째 질문).
- 2026-08-20: 게이트 — `cargo fmt --check`, `cargo clippy --all-targets -D
  warnings`, `cargo test`(11개 테스트 바이너리 전부 ok).

## 이슈 및 해결

### 이슈 1: 튜플 match 테스트가 아무 에러도 내지 않음

- **증상**: `enum Dir { North, South }` + `enum Speed { Fast, Slow }`로 쓴 튜플
  match 테스트에서 `Nrth` 오타가 보고되지 않았다. 단일 match로 줄여도 마찬가지.
- **원인**: 괄호 있는 케이스가 하나도 없고 제네릭도 없는 선언은 **TypeScript
  enum**이라 파서가 청구하지 않는다(`parser/enums.rs`의 `is_rl_enum`). 즉
  테스트의 enum들이 애초에 선언 표에 없었다 — 해석 코드가 아니라 테스트 입력의
  문제였다.
- **해결**: 테스트를 `enum Dir { North(dx: number), South }`처럼 페이로드 케이스가
  있는 rl enum으로 고쳤다. (같은 이유로 이 구분은 TASK-100이 다루는 사안이다.)

### 이슈 2: "두 편집" 테스트가 실패

- **증상**: `Circlle`을 두 편집짜리 예로 쓴 테스트가 "보고되지 않아야 하는데
  보고됐다"로 실패.
- **원인**: `Circlle` → `Circle`은 `l` 하나를 지우는 **한 편집**이다. 예시 선택이
  틀렸다.
- **해결**: 진짜 두 편집인 `Cyrcla`(i→y, e→a)로 교체.

### 이슈 3: clippy `needless_range_loop`

- **증상**: `edit_distance`의 `for j in 0..=b.len() { grid[0][j] = j; }`가
  `-D warnings`에서 실패.
- **원인**: 인덱스로만 쓰이는 범위 루프.
- **해결**: `grid[0].iter_mut().enumerate()`로 바꿨다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` — 11개 테스트 바이너리 전부 통과 (compile 232, lib 42, 그 외 동일)

## 결과

- 코드: `src/analysis.rs`(해석 단계 + 사이트), `src/sema.rs`(보고),
  `src/ast.rs`·`src/parser/lets.rs`(`tag_off`), `src/lib.rs`·
  `src/engine/language.rs`(개명·재수출).
- 테스트: `tests/compile.rs` +13, `src/analysis.rs` +5.
- 문서: `errors.md`, `language.md`, `docs/ai/rl.md`, `CHANGELOG.md`,
  `docs/design/match-analysis.md`.
- 실제 효과(실측): 태그 오타는 이제
  `rlc: f.rl:2:23: enum Shape has no case ``Circel`` — did you mean ``Circle``?`
  로 원본 위치에서 보고되고, 이전처럼 생성된 코드 위의 `TS2678`로 새지 않는다.
  같은 입력에서 **소진성 검사가 조용히 꺼지던 문제**도 함께 사라졌다.
- 후속: 오타가 아닌 틀린 이름과 "스크루티니가 정말 그 enum인가"는 타입이 필요해
  P4(TASK-101 §6)에서 다룬다. 다음 태스크는 P5(소진성 알고리즘 교체).
