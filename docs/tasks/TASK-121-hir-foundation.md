# TASK-121: HIR 기반 — Phase 1 (ID 체계, arena, lowering, source map)

- **상태**: 완료
- **시작일**: 2026-08-21
- **완료일**: 2026-08-21
- **커밋**: —

## 목적

컴파일러 중심부([TASK-119](./TASK-119-compiler-core-design.md), §4)의 분석
표현을 세운다: AST(원본 보존용 syntax tree)와 별개로, sugar가 정규화되고
모든 노드가 index 기반 identity를 갖는 HIR. AST 문자열·source offset을
symbol identity로 쓰는 관행을 끊는 것이 목적이다 — 이후 Phase 2(resolve)가
이 위에 declaration/scope를 얹고, Phase 3이 usefulness를 resolved identity
위로 옮긴다.

## 범위

- 포함: `src/hir/` 신설 — `ids.rs`(newtype ID 12종 + `Arena`),
  `mod.rs`(HIR 노드: `HirFile`/`Item`/`Body`/`Stmt`/`Expr`/`PatternSite`/
  `Pat`/`UnresolvedPath` + `HirSourceMap`), `lower.rs`(AST→HIR lowering +
  테스트). `pub mod hir` 노출(불안정 표면 명시).
- 제외: 소비자 이동 — sema/analysis/codegen은 이 태스크에서 HIR를 쓰지
  않는다(기존 compile 출력 불변이 검증 조건). 이름 해석(Phase 2), flow
  (Phase 5). `DefId`/`LocalId`/`ScopeId`는 타입만 예약.

## 의사결정

### 결정 1: HIR는 AST를 대체하지 않고 병행한다

- **상황**: lowering을 어디에 끼울지 — AST를 HIR로 바꿔치기하면 codegen의
  byte-faithful 방출이 흔들린다.
- **선택과 근거**: AST는 통과 계약·방출의 소유자로 유지하고, HIR는 분석
  전용으로 병행한다(compiler-core.md §4의 계획 그대로). codegen의 HIR
  이동은 Phase 7에서 검증된 lowering plan을 통해서만 한다.

### 결정 2: TS 통과 식은 `OpaqueTs`로 보존하고 재파싱하지 않는다

- **상황**: scrutinee·guard·pipeline step 같은 표현 자리의 TypeScript를
  HIR가 이해해야 하는가.
- **선택과 근거**: 지시된 계약대로 재파싱하지 않는다. rl이 가리키기만 하면
  되는 식은 `Expr::OpaqueTs(node)`(span은 source map), 내부에 rl 구문이
  있는 식만 `Expr::Seq{body}`로 내려간다. 타입은 Phase 4의 backend 질문.

### 결정 3: 네 구문 전부를 `PatternSite`로 정규화, let-else 몸은 `None`

- **상황**: match·tuple match·if let·let-else의 공통 형태.
- **검토한 대안**: if let/let-else를 별도 노드로 두고 분석이 세 형태를
  각각 처리 — 현행 analysis의 구조(match와 site가 분리)를 답습하게 된다.
- **선택과 근거**: 전부 `PatternSite { kind, subjects, arms }` 하나로.
  arm이 "패턴+guard+매칭 시 실행 몸"을 소유하고, 구문별 stmt는 else 흐름만
  소유한다. let-else의 arm은 body가 `None`(바인딩이 이후 문장으로 흐름) —
  구문 차이가 필드 유무로 표현되고 분석 경로는 하나가 된다.

### 결정 4: ID는 파일-로컬 index, span은 `HirSourceMap`으로만

- **상황**: identity 표현.
- **선택과 근거**: `Idx` trait + `Arena<I, T>`(u32 index newtype). HIR
  노드는 span을 들고 다니지 않는다 — `node_spans`/`pattern_spans`/
  `ast_origins`가 유일한 귀환 경로라서, 분석이 offset을 identity로 쓰려는
  유혹이 구조적으로 차단된다. 테스트
  `ids_are_identity_and_spans_are_only_a_way_back`이 바이트 동일한 두
  match가 서로 다른 identity를 가짐을 고정한다.

## 작업 내역

- 2026-08-21: `src/hir/ids.rs` — `Idx` trait, `define_ids!` 매크로로
  `FileId`/`NodeId`/`OwnerId`/`DefId`/`LocalId`/`BodyId`/`ExprId`/
  `PatternId`/`PatternSiteId`/`VariantId`/`FieldId`/`ScopeId`, `Arena`
  (alloc/iter/Index) + 단위 테스트.
- `src/hir/mod.rs` — 노드 정의(위 결정들 반영), `HirSourceMap`
  (`node_spans`/`def_spans`/`pattern_spans`/`ast_origins`), `AstOrigin`.
- `src/hir/lower.rs` — `lower_source`/`lower_program`: 세그먼트 →
  `Stmt`/`Item`, 표현 자리 Program → `OpaqueTs`/`Seq`, enum →
  variants/fields arena(owner 링크), or-패턴 → `Pat::Or`, 중첩 →
  `FieldBinding::Nested` 재귀, 리터럴 값 정규화 유지, try/result 바인딩
  텍스트를 `BindingText` 노드로 기록(single-evaluation 의미는 노드 문서에
  명시), if let else 체인 재귀.
- lowering 테스트 11건: 통과 파일 opaque 하나, enum identity 수집, 세 구문
  동일 site 형태, or/중첩/튜플/리터럴+가드, try/result 바인딩 스팬,
  identity vs span 분리, else 체인.
- `src/lib.rs`에 `pub mod hir` 등록.

## 이슈 및 해결

### 이슈 1: `missing_docs` 린트로 경고 60여 건

- **증상**: `cargo build`가 hir의 모든 공개 필드·variant에 문서 누락 경고.
- **원인**: 저장소 lint 설정이 공개 항목 문서를 요구(`Cargo.toml [lints]`).
- **해결**: 전 공개 항목에 문서 작성(스크립트로 일괄 편집 후 수동 확인).
  불안정 표면이라도 규칙은 규칙이다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` (11개 스위트 593건 전부 통과 — 기존 compile 출력 불변,
  HIR 테스트 11건 추가)

## 결과

`src/hir/{ids,mod,lower}.rs` 신설, `lib.rs`에 모듈 등록. 기존 파이프라인
동작 불변. 후속: Phase 2 — 선언 수집과 이름 해석(TASK-122)이
`UnresolvedPath`를 `Res`로 바꾼다.
