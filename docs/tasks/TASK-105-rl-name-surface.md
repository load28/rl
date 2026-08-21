# TASK-105: rl 이름의 semantic 표면을 엔진으로 (P3 1/2)

- **상태**: 완료
- **시작일**: 2026-08-20
- **완료일**: 2026-08-20
- **커밋**: `2c4dc39`

## 목적

[TASK-101](./TASK-101-rust-parity-review.md)의 제안 P3 전반부. rl 자신의 이름
세 가지(enum 이름·케이스 태그·페이로드 필드)에 대한 hover/definition을
**엔진**이 답하게 한다. 지금 그 답은 에디터의 정규식 재구현
(`editors/vscode/server/src/analysis.ts`, 821줄)이 하고 있고, 규칙이 컴파일러와
다르며 범위가 `match`뿐이다(TASK-101 §GAP-2·§GAP-3).

## 범위

- 포함: `engine/names.rs`(rl 이름 해석 표면), 분석이 그 재료를 내주도록 확장
  (`resolved`·`declarations`), `--server`의 `rlSymbol` 메서드, 문서.
- 제외:
  - **패턴 자리 자동완성** — 다음 태스크(P3 2/2).
  - **에디터 이관**(analysis.ts의 rl 의미 삭제) — 그 다음 태스크.
  - references/rename 합성 — 안정 식별자가 필요하고, 이관 뒤에 판단한다.

## 의사결정

### 결정 1: 체커에게 물을 수 있는 자리에는 답하지 않는다

- **상황**: 기존 에디터 구현(`symbolAt`)은 마지막 폴백으로 **파일 어디서든**
  enum 이름과 같은 식별자를 enum으로, match 본문 안에서는 **아무 enum이나**의
  케이스 태그와 같은 식별자를 케이스로 단정한다. 이관하면서 그 동작을 유지할
  것인가.
- **검토한 대안**:
  - (a) 유지 — 사용처 hover에서 rl 문법 서명이 보인다는 장점. 대신 케이스와
    이름이 같은 지역 변수(`const Point = 2`)가 enum 케이스로 hover되는 오탐이
    구조적으로 남는다.
  - (b) 체커가 답할 수 있는 자리는 넘긴다 — `Shape.Circle(1)`도 `const s: Shape`도
    방출 후에는 평범한 TypeScript라 서비스가 **더 정확히** 답한다(제네릭
    인스턴스화 포함).
- **선택과 근거**: (b). TASK-096이 세운 "체커 우선, rl은 물을 수 없는 자리만"
  규범 그대로다. 답하는 자리를 **enum 선언 안**과 **패턴 안** 둘로 좁혔고,
  그 둘이 정확히 매핑이 존재하지 않는 자리다. 오탐 테스트를 계약으로 고정했다
  (`ordinary_identifiers_are_left_to_the_checker`).

### 결정 2: 재료를 분석에서 내주고, 표면은 그것만 읽는다

- **상황**: 표면이 "이 위치의 태그가 어느 enum의 무엇인가"를 알아야 한다.
- **검토한 대안**:
  - (a) 표면이 AST를 다시 걷는다 — 해석 규칙(섀도잉·후보 선택·중첩의 타입
    해석)이 두 번째 구현이 된다. 없애려는 문제를 다시 만드는 셈이다.
  - (b) 분석이 **해석에 성공한 이름**도 span과 함께 내준다
    (`PatternAnalyses::resolved`) — TASK-102가 이미 실패한 이름
    (`unresolved`)을 같은 걸음에서 모으고 있으므로, 대칭으로 성공도 모은다.
- **선택과 근거**: (b). 한 걸음, 한 규칙. 선언 표(`declarations`)도 함께
  공개해 서명 렌더링과 (다음 태스크의) 완성 목록이 같은 표를 읽게 했다.

### 결정 3: 텍스트만으로 답한다 (프로젝트·툴체인 불필요)

- **상황**: `Project`의 메서드로 만들면 오버레이(저장되지 않은 import 대상)를
  볼 수 있다. 대신 프로젝트를 열어야 한다.
- **검토한 대안**: `Project` 메서드 / `semanticTokens`처럼 텍스트 기반 함수.
- **선택과 근거**: 후자. 이 답은 타입이 필요 없고 **툴체인이 없어도** 나와야
  한다(TASK-093이 semantic tokens에 대해 세운 가용성 기준). 대가는 import된
  파일을 디스크에서 읽는다는 것 — 저장되지 않은 편집은 보이지 않는다. 문서에
  한계로 적었다.

## 작업 내역

- 2026-08-20: `ast::Field`에 `name_off`, `FieldSymbol`에 `offset` 추가
  (필드 선언으로 이동하려면 위치가 필요하다). `parser/enums.rs`가 채운다.
- 2026-08-20: `analysis/mod.rs` — `ResolvedName`·`DeclaredEnum` 공개,
  `PatternAnalyses::resolved`/`declarations`. 해석 함수들이
  `Names { unresolved, resolved }` 캐리어를 받도록 바꿔 성공도 같은 걸음에서
  기록한다.
- 2026-08-20: `engine/names.rs` 신규 — `rl_symbol_at(path, source, position)`.
  선언 안(1)과 패턴 안(2) 두 경로, 서명·설명·정의 위치 렌더링, import된 enum의
  선언 파일 탐색.
- 2026-08-20: `engine/language.rs`에 `analyses_for`(오버레이 없는 분석)와
  `span_range`(바이트→UTF-16 Range) 노출.
- 2026-08-20: `server.rs` — `rlSymbol` 메서드와 프로토콜 문서.
- 2026-08-20: 테스트 — `engine/names.rs` 단위 5개(선언·패턴·`if let`·내장·
  오탐 없음), `tests/cli.rs` 1개(서버가 툴체인 없이 답한다).
- 2026-08-20: 문서 — `cli.md`(`rlSymbol` 절), `lsp-architecture.md` §33 행,
  `CHANGELOG.md`.

## 이슈 및 해결

### 이슈 1: 필드가 `resolved`에 하나도 기록되지 않음

- **증상**: `if let` 패턴의 필드 hover 테스트가 "no rl symbol"로 실패. 디버그
  출력에서 `resolved`에 `Case`만 있고 `Field`가 없었다.
- **원인**: 편집 스크립트가 두 개의 치환을 담고 있었는데 **두 번째 치환에서
  AssertionError가 나면서 파일을 아예 쓰지 않았다** — 첫 치환(필드 기록 추가)도
  함께 사라졌다. 코드가 아니라 편집 절차의 문제였다.
- **해결**: 치환을 나눠 다시 적용했다. 교훈은 기록해 둔다: 여러 치환을 한
  스크립트에 담을 때는 **쓰기가 마지막**이므로 중간 실패가 전부를 되돌린다.

### 이슈 2: `source_range`가 UTF-16 오프셋을 받는데 표면은 바이트를 가짐

- **증상**: 분석이 주는 span은 바이트 오프셋이고, 기존 `source_range`는 UTF-16
  오프셋을 받는다.
- **원인**: 좌표계가 둘(내부는 바이트, 프로토콜은 UTF-16)이고 경계가 명시적으로
  하나 있어야 한다.
- **해결**: `language::span_range(text, byte_start, byte_end)`를 그 경계로 두고
  표면은 그것만 쓴다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` — 11개 바이너리 전부 통과 (lib 54, cli 29)

## 결과

- 공개 API: `engine::rl_symbol_at`/`RlSymbol`/`RlSymbolKind`,
  `PatternAnalyses::resolved`/`declarations`, `ResolvedName`, `DeclaredEnum`,
  `FieldSymbol::offset`.
- 프로토콜: `rlSymbol`.
- 아직 사용자에게 보이지는 않는다 — 에디터가 이 표면을 쓰기 시작하는 것은
  다음 태스크다.
- 후속: 패턴 자리 자동완성(P3 2/2) → 에디터 이관과 `analysis.ts`의 rl 의미 삭제.
