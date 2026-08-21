# TASK-119: 컴파일러 중심부 전환 설계 (umbrella)

- **상태**: 완료
- **시작일**: 2026-08-21
- **완료일**: 2026-08-21
- **커밋**: —

## 목적

rl 구문에 대해 rustc 수준의 프런트엔드(안정 identity, 이름 해석, typed
분석, usefulness, flow, 구조화 다중 진단, 컴파일러 소유 에디터 semantic,
증분 query)를 세우는 전환의 **설계와 페이즈 분해**를 확정한다. 개별 구현은
페이즈별 태스크(TASK-120~127)로 진행한다.

## 범위

- 포함: `docs/design/compiler-core.md` 작성 — 보존 자산과 결함 목록(D1~D10),
  목표 파이프라인, HIR/ID 체계, resolve, typed facts, flow, 구조화 진단,
  codegen 경계, query 계획, 페이즈 → 태스크 매핑, 완료 기준.
- 제외: 코드 변경 일체 (이 태스크는 설계 기록만).

## 의사결정

### 결정 1: 새 컴파일러를 옆에 만들지 않고 기존 단계 사이에 중심부를 세운다

- **상황**: "rustc 수준"을 어떻게 달성할지 — 전면 재작성이냐 확장이냐.
- **검토한 대안**: (a) HIR 중심의 새 파이프라인을 병행 구축 후 교체 —
  기간 내내 두 벌의 의미론이 공존하고 통과 계약 회귀 위험이 큼.
  (b) 기존 lossless parser/analysis/codegen/engine을 유지하고 그 사이에
  HIR → resolution → typed facts → flow → structured diagnostics를 삽입 —
  각 페이즈가 독립 커밋으로 완료 가능하고 회귀선(기존 테스트)이 살아 있음.
- **선택과 근거**: (b). `analysis/usefulness.rs`(Maranget)와
  `MatchAnalysis`(THIR 대응)는 이미 rustc의 단계 구성과 동형이므로
  (rust-parity-analysis.md §9), 빠진 것은 identity와 resolve 단계다 —
  교체가 아니라 공급이 맞다.

### 결정 2: Phase 0(다중 진단)을 전체 전환의 선행 조건으로 둔다

- **상황**: 어느 페이즈부터 시작할지.
- **검토한 대안**: HIR부터(구조 우선) / 진단부터(관측 가능한 버그 우선).
- **선택과 근거**: 진단부터. TASK-117 증상 3(rl 에러 하나가 typed 진단
  전체를 가림)은 **진단이 사라지는 버그**이고, 이후 모든 페이즈가 "진단을
  누적하고 계속 검사한다"는 실행 모델을 전제한다. 첫-에러-종료 구조 위에
  HIR를 쌓으면 페이즈마다 같은 개조를 반복하게 된다.

### 결정 3: 태스크 번호 매핑을 설계 문서에 고정하되 INDEX를 진실로 둔다

- **상황**: 8개 페이즈의 태스크 번호를 미리 배정할지.
- **선택과 근거**: 문서에는 착수 시점 기준 예정 번호(TASK-120~127)를 적고,
  "확정 번호는 INDEX가 진실"임을 명시한다. 병행 작업으로 번호가 밀릴 수
  있기 때문이다.

## 작업 내역

- 2026-08-21: 저장소 실측 검토 — `src/lib.rs`·`sema.rs`·`error.rs`·
  `analysis/mod.rs`(Table/Names/analyze_match)·`engine/{project,snapshot,
  projection,semantics}.rs`·`val.rs`(Sink/run)·`main.rs`(배치 경로)·
  `server.rs`(check 프로토콜)를 읽고, `rust-parity-analysis.md`·
  `TASK-117`과 대조해 결함 목록 D1~D10을 확정.
- 2026-08-21: `docs/design/compiler-core.md` 작성, INDEX에 TASK-119 등록.

## 이슈 및 해결

없음.

## 검증

- [x] `cargo fmt --check` (코드 변경 없음 — 문서만)
- [x] `cargo clippy --all-targets -- -D warnings` (코드 변경 없음)
- [x] `cargo test` (코드 변경 없음)

## 결과

`docs/design/compiler-core.md` 신설. 후속: TASK-120(Phase 0 — 구조화 다중
진단)부터 페이즈별 구현.
