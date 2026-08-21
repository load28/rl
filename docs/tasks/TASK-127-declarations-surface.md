# TASK-127: 서버 declarations 표면 — 에디터 shadow 대체 재료 (D6 1/2)

- **상태**: 완료
- **시작일**: 2026-08-21
- **완료일**: 2026-08-21
- **커밋**: —

## 목적

컴파일러 중심부 완료 기준([TASK-119](./TASK-119-compiler-core-design.md)
§14) 중 미해결로 남은 "regex 기반 editor shadow semantics 제거"(D6,
GAP-3)의 전반부: 에디터가 자체 구현(`editors/vscode/server/src/
analysis.ts`의 `parseEnums`/`visibleEnums`/`BUILTIN_ENUMS`/`parseMatches`)
으로 지어내던 답을 **컴파일러가 직접 주는 표면**을 만든다 — resolve 기반
선언 표(로컬·import·builtin, 컴파일러의 shadowing 그대로)와 match 사이트
(암 삽입 지점 포함). 에디터 전환·shadow 삭제는 후반부(TASK-128)다.

또한 Phase 7(codegen 정리)의 실측 정산을 함께 기록한다(아래 의사결정 2).

## 범위

- 포함: `engine::rl_declarations`(신설 `src/engine/declarations.rs` —
  `RlDeclarations`/`RlEnumDecl`/`RlEnumOrigin`/`RlCaseDecl`/`RlFieldDecl`/
  `RlMatchSite`), `--server`의 `declarations` 메서드와 프로토콜 문서,
  재료 배관: AST `MatchExpr`/`TupleMatchExpr`의 `body_close`(파서가 채움),
  resolve `EnumDef`/`ExternDecl`의 `generics`(builtin은 `<T>`/`<T, E>`),
  단위 테스트 3건.
- 제외: 에디터(TS) 쪽 소비 전환과 shadow 삭제(TASK-128), documentSymbol의
  match 개요(현 에디터는 enum만 개요에 씀).

## 의사결정

### 결정 1: 표면의 단위는 "보이는 선언 표 + match 사이트" 하나

- **상황**: 에디터의 shadow 소비 지점(완성의 enum 목록·케이스 스니펫,
  개요, 소진성 quick-fix의 삽입 지점, match 키워드 도움말)을 실측한 결과,
  전부 "이 버퍼에서 보이는 enum들"과 "이 버퍼의 match 위치"라는 두
  질문으로 환원된다.
- **선택과 근거**: 한 메서드로 답한다. 선언 표는 resolve의 산출물이라
  컴파일러의 shadowing·alias·builtin identity가 그대로 실려, shadow 층의
  구조적 오탐(`inferEnum`의 다른 규칙, "아무 enum이나" 폴백)이 원천에서
  사라진다. 좌표는 서버의 `emitMap`과 같은 바이트 오프셋.

### 결정 2: Phase 7(codegen)은 실측 정산 — 재계산이 없다

- **상황**: 설계 §13의 Phase 7은 "codegen이 이름 해석·exhaustiveness를
  재계산하지 않게"였다.
- **실측**: codegen을 검토한 결과 이름 해석·소진성의 재계산은 **원래
  없다**. codegen의 자체 판단은 (a) `contains_await` 구문 스캔(방출 형태
  결정 — 분석 산출물에 없는 구문 속성), (b) or-패턴 공유 구조 분해에서
  sema 보장("모든 대안이 같은 집합을 바인딩")의 소비(`alts[0]`)뿐이다.
  완료 기준의 해당 항목은 충족 상태다.
- **선택과 근거**: 형식적 lowering-plan 인자 전달로 커밋을 만드는 대신
  이 정산을 기록하고, HIR codegen 이동(검증된 plan 소비의 실질 형태)은
  안정화된 HIR node가 생기는 시점의 후속 태스크로 남긴다.

## 작업 내역

- 2026-08-21: `src/ast.rs`·`src/parser/matches.rs` — `body_close`.
  `src/resolve/mod.rs` — `generics` 배관(로컬/`ExternDecl`/builtin).
  `src/engine/declarations.rs` 신설(+`engine/mod.rs` 재수출) — resolve
  기반 enum 목록(로컬 스팬·outline 범위 포함), AST 순회로 중첩 포함 match
  사이트 수집. `src/server.rs` — `declarations` 메서드와 모듈 문서.
  `docs/reference/cli.md` — 서버 절에 명세.
- 단위 테스트: 로컬 스팬/builtin generics/순서, 로컬의 builtin shadowing
  단일 노출, 중첩 match의 삽입 지점.

## 이슈 및 해결

없음.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` (618건 전부 통과)

## 결과

에디터 shadow의 대체 재료가 서버 표면으로 준비됐다. 후속:
**TASK-128** — VSCode 서버가 이 표면을 소비하고 `analysis.ts`의 의미론
구현을 삭제한다(D6 완결).
