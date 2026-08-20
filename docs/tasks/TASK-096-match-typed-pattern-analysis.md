# TASK-096: match 구조 개선 — typed pattern analysis (MatchAnalysis)

- **상태**: 완료
- **시작일**: 2026-08-20
- **완료일**: 2026-08-20
- **커밋**: —

## 목적

match를 "필요한 순간마다 부분 추론"하는 구조에서 벗어나, Rust 컴파일러처럼
**타입이 붙은 공통 분석 결과(MatchAnalysis)** 를 중심에 두는 기반을 만든다.
직접 계기가 된 버그: `A(x) | B(x) => x` 같은 or-pattern에서 **패턴 내부**
binding 위치(`A(x)`의 `x`, `B(x)`의 `x`)에 에디터 hover가 아무것도 답하지
못한다 — codegen이 or-arm의 destructuring을 의도적으로 매핑 없이 방출하기
때문에(어느 한 alternative에 귀속시키면 rename이 그 하나만 고치게 된다),
그 span은 TS 좌표로 번역되지 않아 언어 서비스 질의가 도달하지 않는다.

기대 동작:

- `A(x)`의 `x` hover → **A의 payload 타입**
- `B(x)`의 `x` hover → **B의 payload 타입**
- body의 `x` hover → **A payload | B payload** (기존 TS 경로가 이미 답한다)

## 범위

- 포함:
  - `TagPattern`에 alternative 끝 span 기록 (파서).
  - 새 공통 모델 `engine::analysis` — 파일의 모든 match에 대해
    `MatchAnalysis`(subject/constructors, pattern binding span→타입,
    arm body binding 병합 타입, coverage)를 만드는 순수 분석 계층.
    로컬 enum > import된 enum > 내장 `Option`/`Result` 순의 선언 테이블.
  - hover 연결: 기존 TS 경로가 답하지 못한 위치가 패턴 binding span이면
    ① **alternative 격리 프로브**(해당 or-group을 그 alternative 하나로 치환한
    합성 소스를 방출해 TS 서비스에 질의 — 제네릭 인스턴스화까지 정확한 타입)
    → ② 실패 시 선언 테이블의 필드 타입 텍스트로 폴백.
  - 서비스(tsgo) 부재 시에도 hover가 에러 대신 선언 기반 폴백으로 응답.
  - definition 폴백: or-arm body의 binding 참조 → 각 alternative의 패턴
    binding span (TS 답이 비었을 때만).
  - sema or-pattern 진단 메시지 구체화 — 어떤 이름이 어느 alternative에서
    빠졌는지 명시.
  - 문서: `docs/design/match-analysis.md`(신규), errors.md, CLAUDE.md 맵.
- 제외 (비목표, 요구사항 명시):
  - RL 자체 타입체커의 TS 수준 확장 / narrowing·generic 재구현.
  - exhaustive 알고리즘 재작성 — sema의 기존 검사가 계속 보고 주체.
    (MatchAnalysis의 `coverage`는 같은 subject 해석에서 파생한 데이터로
    노출만 하고, sema 통합은 후속 태스크.)
  - or-pattern binding span의 rename/references 지원 — rename은 전체가
    원자적으로 되지 않으면 거부한다는 기존 계약 유지.
  - lowering(방출 코드) 변경 없음.

## 의사결정

### 결정 1: hover의 1차 답은 "alternative 격리 프로브", 2차가 선언 테이블

- **상황**: or-pattern 내부 binding span에 constructor별 payload 타입을
  보여줘야 한다. 방출 코드는 arm당 하나의 공유 destructuring이라 span이
  매핑되지 않고, 매핑을 붙여도 TS가 보여줄 타입은 병합된 union이다.
- **검토한 대안**:
  - A. 선언 테이블만으로 답한다 — 구현 단순, 서비스 불필요. 단점: 제네릭
    enum(`Option<number>`)에서 `T`를 보여준다. narrowing된 scrutinee도 모른다.
  - B. 방출 형태를 바꿔 alternative별 destructuring을 만든다 — lowering
    변경(비목표 위반), 런타임 코드가 달라진다.
  - C. completion probe와 같은 방식의 합성 소스: or-group을 hover 대상
    alternative 하나로 치환해 방출하면 codegen이 단일-alternative 경로로
    **매핑된** destructuring을 그 tag로 narrowing된 위치에 방출한다. 그
    위치에 TS hover를 물으면 인스턴스화된 정확한 payload 타입이 나온다.
- **선택과 근거**: C를 1차, A를 폴백으로. TS/tsgo가 이미 아는 타입 정보를
  match 분석의 중심에 두라는 요구 우선순위(TS semantic query → RL 심볼
  테이블 → lightweight 폴백)와 정확히 일치하고, lowering은 그대로다.
  기존 completion probe(`build_probe`)와 같은 "질의 한 번 동안만 다른
  텍스트를 서빙" 패턴이라 서비스 계층에 새 개념이 없다.

### 결정 2: MatchAnalysis는 엔진의 순수(파서-기반) 계층으로 둔다

- **상황**: 분석 결과를 hover/definition(에디터)과 향후 sema/exhaustiveness가
  공유할 위치가 필요하다.
- **검토한 대안**: sema 내부에 두기(컴파일 에러 경로와 결합, 에디터가 못 씀) /
  language.rs 내부에 두기(서비스 세션과 결합, rlc가 못 씀) /
  `engine::analysis` 독립 모듈(파서만 의존, 양쪽 소비 가능).
- **선택과 근거**: `engine::analysis`. TS 도달 여부와 무관하게 항상 계산
  가능해야 폴백이 성립하고(semantic tokens와 같은 가용성), rlc(sema)와
  LSP가 같은 모델을 소비하는 최종 구조의 자리이기도 하다.

### 결정 3: sema의 exhaustiveness는 이번에 옮기지 않는다

- **상황**: 요구사항 Step 7은 exhaustive 검사의 입력을 MatchAnalysis로
  옮기라고 하지만, 동시에 "exhaustive 알고리즘을 한 번에 갈아엎지 말 것"이
  비목표로 명시돼 있다.
- **검토한 대안**: 즉시 이관(에러 보고 경로 전면 수정, 회귀 위험 큼) /
  파생 데이터(`coverage`)만 모델에 노출하고 보고 주체는 sema 유지.
- **선택과 근거**: 후자. subject 해석 규칙(로컬 > import > 내장, 모든 arm
  tag를 포함하는 첫 후보)을 sema와 동일하게 구현해 모델에 `coverage`를
  실었고, 두 구현의 수렴(이관)은 후속 태스크로 등록한다.

### 결정 4: MatchAnalysis는 core(`src/analysis.rs`)에, 소비는 engine에

- **상황**: 처음에는 `engine/analysis.rs`로 두려 했으나, 이 저장소의
  tsgo-모사 계층(순수 파이프라인 단계 ↔ engine ↔ TS seam)에 맞는 자리를
  다시 결정해야 했다 (사용자 지시: 아키텍처를 따르라).
- **검토한 대안**: engine 내부(파서-기반이지만 rlc/sema가 소비하려면
  core→engine 역참조가 생겨 계층이 뒤집힘) / sema 내부(에러 경로와 결합,
  에디터의 무오류 소비 불가) / core 순수 모듈(probe.rs·sema.rs와 같은
  층위, `EnumSymbol` extern 입력 — sema의 `extern_enums` 입력과 같은 꼴).
- **선택과 근거**: core. "LSP와 rlc가 같은 MatchAnalysis 사용"이라는
  최종 그림이 성립하는 유일한 자리이고, 툴체인 부재 시에도 항상 계산돼
  폴백 가용성이 semantic tokens(TASK-093)와 같아진다. extern **수집**
  (파일 읽기)은 CLI가 sema에 해 주듯 소비자(engine)가 한다.

### 결정 5: completion은 어댑터의 rl 구조 계층에 그대로 둔다

- **상황**: 요구사항 Step 6이 hover/completion 연결을 말한다.
- **선택과 근거**: arm 태그 completion은 이미 어댑터(analysis.ts)의 rl
  구조 계층이 담당하고, 그 자리가 규범이다(lsp-architecture.md §33 표:
  "미완성 버퍼 내성 필요" — rlc 파서는 무오류·전량 파싱이라 미완성
  match를 인식하지 않는다). 이번 버그의 실체는 hover(그리고 definition)
  였으므로 그 둘만 엔진에 연결했다. 바인딩 위치 completion은 사용자가 새
  이름을 짓는 자리라 의미가 없다.

### 결정 6: or-pattern binding 불일치 진단은 sema의 기존 에러를 구체화

- **상황**: `A(x) | B(y)`, `A(x) | B(x, y)`는 이미 sema가
  "or-pattern alternatives must bind the same fields"로 거부한다. 요구된
  것은 어떤 이름이 어디서 빠졌는지 보여주는 진단.
- **선택과 근거**: 새 진단 채널을 만들지 않고 기존 에러 메시지에 결손
  내용을 덧붙인다(`` `x` is bound in `A(...)` but not in `B(...)` ``).
  에러 계층 계약(모든 rl 수준 에러는 rlc가 위치와 함께 보고)이 이미 이
  규칙을 다루고 있고, 에디터도 이 에러를 rl 진단으로 이미 표시한다.

## 작업 내역

- 2026-08-20: 조사 — hover 경로(`engine/language.rs`: `.rl` 좌표 →
  emit-map → tsgo --lsp), or-arm 방출(`codegen/matches.rs`:
  `binding_list_lit`은 의도적으로 매핑 없음), sema의 or-pattern 규칙,
  probe 패턴(`build_probe`), 선언 수집 API(`enum_symbols`, `rl_imports`).
- 2026-08-20: `ast.rs` `TagPattern.end` 추가(대안 끝 byte — 격리 치환의
  재료), 파서 3개 생성 지점(`parser/matches.rs` ×2, `parser/iflets.rs`)
  갱신.
- 2026-08-20: `src/analysis.rs` 신규 — `MatchAnalyses`/`MatchAnalysis`/
  `MatchSubject`/`MatchConstructor`/`PayloadField`/`AnalyzedArm`/
  `PatternBinding`/`BodyBinding`/`Coverage` 모델과 빌더,
  `binding_at`/`body_definitions`/`body_binding_at` 조회, 선언 테이블
  (로컬 > `EnumSymbol` extern > 내장 Option/Result), lib.rs 공개
  (`match_analyses` + 모델 re-export, doc 예시 포함). 단위 테스트 14개
  (요구 테스트 1–8 대응: 단일/or/동일 타입 병합/상이 타입 union/alias·
  optional/nested/generic base/tuple 위치별/내장·섀도잉/extern/미해석
  subject/coverage/body 정의/중첩 match).
- 2026-08-20: `engine/language.rs` — hover를 3단으로 재구성
  (`service_hover` → `match_binding_hover`): ① 기존 TS 경로 무수정
  (회귀 방지), ② or-group binding span이면 `isolate_alternative`(합성
  소스·매핑, 순수 함수로 분리해 단위 테스트) + completion probe 패턴의
  일회 서빙으로 TS에 질의, ③ 실패 시 `declared_binding_hover`(선언
  타입 + 출처 문서화), body 참조는 병합 타입. `serve` 실패(툴체인 부재)
  시 `declared_hover_unserved` 폴백. definition은 TS 답이 빌 때만
  `match_binding_definitions`(binding 자신 / body 참조 → 대안별 span).
  extern 수집 `analyses_of`는 CLI의 `collect_extern_enums`를 overlay
  우선으로 미러링. 단위 테스트 3개(격리 방출 narrowing·매핑, 폴백 문안,
  extern 수집의 overlay 우선).
- 2026-08-20: sema or-pattern 메시지 지목형으로 구체화
  (`binding_mismatch` — 이름 결손/필드-이름 불일치 구분),
  `tests/compile.rs`의 기존 단언 갱신 + 케이스 4개 추가(arity·`_`·필드
  불일치·튜플 요소), errors.md·docs/ai/rl.md 갱신.
- 2026-08-20: `docs/design/match-analysis.md` 작성(문제→계층 배치→hover
  우선순위→진단→coverage 단계적 통합→한계), CLAUDE.md 아키텍처 맵에
  analysis.rs 추가, INDEX 등록.
- 2026-08-20: 검증 게이트 3종 통과 확인 (아래 검증 절; `cargo test`는
  tsc/node 통합 테스트 포함 11개 스위트 + doctest 21개 전부 통과.
  tsgo 부재 환경이라 격리 프로브의 e2e는 후속 — 순수 부분은 단위 테스트).

## 이슈 및 해결

### 이슈 1: or-arm 패턴 binding에 emit-map을 붙이는 방법으로는 풀 수 없음

- **증상**: hover 미동작의 표면 원인은 매핑 부재지만, 매핑을 붙이면
  (TASK-080의 계약대로) rename이 한 alternative만 고쳐 프로그램을 깨뜨리고,
  보이는 타입도 constructor별이 아니라 병합 union이 된다.
- **원인**: 하나의 destructuring이 여러 alternative를 대표하는 방출 형태
  자체의 성질.
- **해결**: 방출은 그대로 두고 질의 시점에 alternative를 격리한 합성 소스로
  묻는다 (결정 1). rename/references는 계속 의도적으로 비활성.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`

## 결과

변경 파일: `src/analysis.rs`(신규), `src/lib.rs`, `src/ast.rs`,
`src/parser/matches.rs`, `src/parser/iflets.rs`, `src/engine/language.rs`,
`src/sema.rs`, `tests/compile.rs`, `docs/design/match-analysis.md`(신규),
`docs/reference/errors.md`, `docs/ai/rl.md`, `CLAUDE.md`,
`docs/tasks/INDEX.md`, 본 태스크 문서.

핵심 결과: `A(x) | B(x) => x`에서 `A(x)`의 `x`는 A payload, `B(x)`의
`x`는 B payload, body `x`는 병합 union으로 hover된다 — 1차 답은 tsgo
(격리 프로브, 인스턴스화 포함), 폴백은 선언 테이블. lowering·emit-map·
rename 계약은 불변.

후속 태스크 후보(미등록): sema exhaustiveness의 MatchAnalysis 이관,
tsgo 통합 환경에서의 hover 프로브 e2e 테스트(vscode engine.test.ts 계층),
nested/tuple 패턴 폴백 타입의 제네릭 인스턴스화.
