# TASK-133: let-else·`if let`의 or-패턴 (GAP-6 마지막 항목)

- **상태**: 완료
- **시작일**: 2026-08-21
- **완료일**: 2026-08-21
- **커밋**: —

## 목적

`rust-parity-analysis.md` GAP-6의 마지막 미해소 항목: rustc의
`let (A(x) | B(x)) = … else …`에 해당하는 or-패턴이 rl의 let-else·`if let`
에는 없었다(언어 표면 격차). match와 같은 대안 문법·규칙으로 지원한다:

```rl
const Circle(r) | Square(r) = s else { return 0; };
if let Circle(r) | Square(r) = s { … }
```

## 범위

- 포함: AST(`LetElseStmt`/`IfLetStmt`의 패턴을 `alternatives:
  Vec<TagPattern>`로), 파서(공유 `parse_alternative` — 첫 대안 괄호 필수,
  이후 대안은 태그만도 가능), sema(공유 `check_alternatives` — 바인딩
  집합 동등 + 중첩·or 조합 금지, let-else에 leaf 중복 검사 추가), codegen
  (let-else 부정 결합 조건 + `if let` 논리합 조건, 다중 대안의 구조 분해는
  match or-arm처럼 공유·비매핑), HIR lowering(`or_of` 재사용), resolve
  (1-편집 면허를 "태그가 하나일 때"로 한정 — or의 여러 태그는 match급
  근거), analysis(`analyze_alt_site` — 대안별 occurrence, group/alt 스팬,
  에디터 hover의 대안 격리 재료), 시맨틱 토큰·페이로드 프로브의 대안 순회,
  테스트 3계층, 레퍼런스·AI 문서·설계 문서 갱신.
- 제외: TextMate 문법의 후속 대안 태그 색(시맨틱 토큰이 전 대안을
  덮으므로 실용 영향 없음), let-else의 중첩 패턴(기존과 같이 없음),
  가드(기존과 같이 없음).

## 의사결정

### 결정 1: match와 같은 규칙, 같은 방출 형태

- **상황**: 대안별 조건/분해를 따로 방출할 수도 있다.
- **선택과 근거**: match or-arm과 동일하게 — sema가 모든 대안의
  (필드, 이름) 집합 동등을 강제하므로 구조 분해는 첫 대안 하나로 충분하고,
  다중 대안의 분해는 **비매핑**(모든 대안을 동시에 대변하므로 어느 하나에
  매핑하면 rename이 그 대안만 고친다 — match와 같은 근거)이다. let-else는
  `if (t.kind !== "A" && t.kind !== "B") { diverge }`로 tsc가 유니언으로
  좁히고, `if let`은 `(=== || ===)`로 좁힌다 — 타입 트릭 없음(통합 테스트
  가 `tsc --strict`로 검증).

### 결정 2: 첫 대안의 괄호가 구문을 청구한다 — 이후 대안은 자유

- **상황**: 통과 계약 — `const A(x) | B = …`를 언제 rl로 읽나.
- **선택과 근거**: 구문 청구는 기존 규칙 그대로 첫 대안의 `태그(`가 한다
  (유효 TS에서 선언 키워드 뒤 `식별자(`는 불가능). 그 뒤의 `| 대안`은
  이미 rl로 확정된 문장의 연속이므로 태그만도 허용한다(`const A() | C =
  …` — 소속 검사만). `||`는 OrOr 토큰으로 붙어 오므로 구분자가 될 수 없다.

### 결정 3: or의 여러 태그는 match급 식별 근거다

- **상황**: 단일 태그 사이트는 근거가 얇아 1-편집 면허로만 오타를
  보고해 왔다.
- **선택과 근거**: 대안이 여럿이면 근거가 match의 암 목록과 같으므로
  resolver의 표준 식별(전 태그 포함 후보 → 유일 최다 포함)을 그대로
  쓰고, 1-편집 면허는 `tags.len() == 1`일 때로 한정했다. 결과: `const
  Cyrcla(r) | Empty() = s else …`는 2-편집 오타도 보고된다(테스트로 고정).

### 결정 4: sema 공유 헬퍼는 사이트 전용 — match는 기존 구현 유지

- **상황**: match의 or 검사는 중복-암 부기와 교차해 있어(중복 대안은
  집합 비교를 건너뜀) 그대로 추출하면 진단 개수가 달라진다.
- **선택과 근거**: `check_alternatives`는 let-else·`if let`용으로 두고
  match/튜플 match는 기존 흐름을 유지 — 동작 보존이 코드 중복 10줄보다
  우선한다(주석으로 명시).

## 작업 내역

- 2026-08-21: `src/ast.rs` — 두 문의 패턴 필드를 `alternatives`로.
  `src/parser/matches.rs` — `parse_alternative(cur, allow_nested)` 추출
  (`parse_tag_pattern`은 그 위임). `lets.rs`/`iflets.rs` — `|` 루프.
- `src/sema.rs` — `check_alternatives`(중첩·or 조합은
  `match-nested-in-or-pattern`, 집합 불일치는 `match-or-binding-mismatch`
  코드 그대로, 접두사만 `let-else:`/`if let:`), let-else에
  `check_leaf_bindings` 추가.
- `src/codegen/mod.rs`/`matches.rs` — let-else 다중 조건, `or_conds_binds`
  (논리합 + `binding_list_lit` 공유 분해), 단일 대안은 기존 매핑 경로.
- `src/hir/lower.rs` — 두 lowering이 `lower_tag_pattern`+`or_of` 경로로
  통일(let-else의 수제 Constructor 조립 삭제).
- `src/resolve/mod.rs` — 면허 조건에 `tags.len() == 1`.
- `src/analysis/mod.rs` — `analyze_site`(단일 태그 전용)를
  `analyze_alt_site`로 대체(occurrence별 alt 스팬·alternatives 수 기록 —
  에디터 or-바인딩 hover의 대안 격리가 사이트에서도 성립).
- `src/engine/tokens.rs` — let-else가 expect_word 추정 대신 기록된
  대안 스팬으로 토큰 방출, `if let`은 대안 순회. `src/probe.rs` —
  `if let` 페이로드 프로브 대안 순회.
- 테스트: compile.rs 6건(공유 분해·bare 대안·집합 불일치·중첩 조합
  에러·`if let` 논리합·val의 or-바인딩 커버), resolve.rs 1건(결정 3),
  integration.rs 1건(`tsc --strict` + node 런타임).
- 문서: language.md(§3 이름 해석 근거 규칙, §6.1/6.2/6.5 문법·의미),
  errors.md(접두사 변형, 사이트 식별 규칙), docs/ai/rl.md(3곳),
  rust-parity-analysis.md GAP-6 해소 표기, compiler-core.md 남은 후속
  정리, 파서 모듈 헤더.

## 이슈 및 해결

없음. (초기 우려였던 `val`의 or-패턴 바인딩 커버는
`collect_pattern_names`가 이미 처리하고 있었고 — 테스트로 고정만 했다.)

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` (636 통과, 실패 0 — integration의 tsc+node 검증 포함)
- [x] `editors/vscode`: `npx tsc -b` + `npm test` 74/74 (skip 0) +
  `npm run grammar:check`

## 결과

rl의 모든 패턴 사이트(match·튜플 match·let-else·`if let`)가 같은 대안
문법과 같은 바인딩 규칙을 갖는다 — GAP-6의 언어 표면 격차 종결. 시맨틱
토큰은 부수적으로 let-else 태그 위치를 추정이 아니라 기록된 스팬으로
얻게 됐다.
