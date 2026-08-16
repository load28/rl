# TASK-010: swc 스타일 컴파일러 아키텍처 재구성

- **상태**: 완료
- **시작일**: 2026-08-16
- **완료일**: 2026-08-16
- **커밋**: (커밋 후 기입)

## 목적

기능 추가(새 구문, 새 검사, 새 방출 형태)를 앞두고 있어, 파싱·의미 검사·코드
방출이 한 번의 스캔 루프에 뒤섞인 단일 패스 구조를 swc처럼 단계가 분리된
파이프라인(AST → parser → 의미 검사 → codegen)으로 재구성한다. 각 단계가
독립 모듈이 되면 새 기능은 해당 단계에만 손대면 되고, 단계 간 계약(AST)이
명시적이어서 회귀 범위가 좁아진다.

## 범위

- 포함: `src/` 내부 구조 재편 — `ast.rs`(타입드 AST), `parser/`(구조 파싱,
  무오류), `sema.rs`(의미 검사 + 소진성), `codegen/`(TypeScript 방출).
  `lib.rs` 파이프라인을 parse → check → emit → verify로 교체.
  CLAUDE.md 아키텍처 맵과 설계 문서 갱신.
- 제외: 언어 표면 변경 없음 — 구문, 판별 규칙, 방출 코드, 에러 메시지·위치,
  CLI 동작 모두 바이트 단위로 동일해야 한다(기존 테스트가 회귀 기준).
  `scanner.rs`/`error.rs`/`verify.rs`/`main.rs`는 유지(스캐너에 유틸 이동만 허용).

## 의사결정

### 결정 1: 전체 AST 파싱이 아니라 "세그먼트 + rl 노드" 하이브리드 AST

- **상황**: swc처럼 만들려면 무엇을 AST로 표현할지 정해야 했다. swc는 파일
  전체를 AST로 만들지만, rl의 설계 계약 1(유효한 TS는 바이트 그대로 통과)은
  파일 전체 파싱과 충돌한다 — rl 구문이 섞인 파일은 TS 파서로 파싱 불가하고,
  전체 AST를 재출력하면 바이트 보존이 깨진다.
- **검토한 대안**:
  - A. swc AST로 전체 파일 파싱: rl 구문 때문에 불가능 + 바이트 보존 불가.
  - B. 현행 유지(단일 패스): 기능 추가 시 관심사 얽힘 지속.
  - C. `Program = Vec<Segment>` 하이브리드 — rl 구문만 타입드 노드로
    들어올리고 나머지는 `Verbatim(Span)`: 바이트 보존이 구조적으로 보장되고,
    단계 분리는 swc 수준으로 가능.
- **선택과 근거**: C. 통과 계약을 AST 표현 자체가 보장하면서(verbatim 구간은
  복사 외의 연산이 없음) swc의 단계 분리(ast/parser/sema/codegen)를 그대로
  가져올 수 있다. 확인: passthrough.rs 22개 계약 테스트 + 차등 검증(아래).

### 결정 2: 파서를 무오류(infallible)로 만들고 모든 에러를 sema로 이동

- **상황**: 기존 코드는 구조 파싱 실패(→ 통과)와 rl 수준 에러(중복 케이스 등)
  가 같은 함수에서 나와 `Result<Option<...>>`가 파서·방출 전체에 퍼져 있었다.
- **검토한 대안**:
  - A. 파서가 에러도 보고: 현행과 동일한 얽힘 유지.
  - B. 구문 여부는 순수 구조 판단으로 파서에, 위반 검사는 전부 sema에:
    파서는 `Option`, sema만 `Result`, codegen은 무오류.
- **선택과 근거**: B. 기존 코드에서 rl 수준 에러 검사는 모두 "구문임이 확정된
  후"에 수행되고 있어(주석 "by now this is clearly meant to be an rl match")
  구문 여부 판단과 독립적임을 확인했다. 따라서 이동해도 통과/에러 판정이
  달라질 수 없다. 부수 효과로 `Ctx`의 `RefCell` 가변 상태가 사라졌다
  (파서는 불변, 레지스트리는 sema 소유).

### 결정 3: sema는 소스 순서 깊이 우선(노드 규칙 → 자식) 순회

- **상황**: 검사 단계를 분리하면 에러 보고 순서가 달라질 수 있다. 기존
  구현은 스캔 중 만나는 순서로 첫 에러를 보고했고, 중첩 구문에서는 바깥
  match의 자체 규칙(중복 arm 등)을 자식(scrutinee/body) 파싱보다 먼저
  검사했다.
- **검토한 대안**: A. 절대 오프셋 순 정렬 후 보고(더 "원칙적"이지만 중첩
  케이스에서 기존과 다른 에러가 나옴) / B. 기존과 동일한 순서: 노드 자체
  규칙 먼저, 그다음 scrutinee → arm body 순 재귀.
- **선택과 근거**: B. 이 태스크의 계약은 "관찰 가능한 동작 불변"이므로 기존
  순서를 정확히 재현한다. 소진성은 기존대로 순회 종료 후 일괄 해결(선언
  순서 무관 유지).

### 결정 4: 템플릿 리터럴을 AST 노드(`Template`)로 항상 승격

- **상황**: 템플릿 보간 안의 rl 구문은 재귀 변환 대상이다. 보간이 없는
  템플릿을 verbatim으로 남길 수도 있었다.
- **검토한 대안**: A. rl 구문이 있는 템플릿만 노드화(파싱을 두 번 하거나
  미리보기 필요) / B. 모든 템플릿을 Raw 청크 + 보간 `Program`으로 노드화.
- **선택과 근거**: B. 기존 `transform_template`의 동작(미종결 `${`에서 `}`
  보완 방출 등 경계 동작 포함)을 단일 코드 경로로 정확히 재현하고, Raw
  청크가 원본 Span이라 바이트 보존도 유지된다.

### 결정 5: `scan_type_end`는 `scanner.rs`로 이동

- **상황**: 타입 어노테이션 끝 스캔은 파서(enum 필드)와 codegen(제네릭
  파라미터 이름 추출)이 함께 쓴다. 기존에는 transform/enums.rs에 있었다.
- **검토한 대안**: A. 파서에 두고 codegen이 파서에 의존 / B. 저수준 스캔
  유틸이므로 scanner로 이동.
- **선택과 근거**: B. 단계 간 의존 방향을 "모든 단계 → scanner/ast"로
  단순하게 유지한다(파서와 codegen 사이 직접 의존 없음).

## 작업 내역

- 2026-08-16: 태스크 등록, 현행 구조 분석(단일 패스에서 파싱·검사·방출이
  얽힌 지점 목록화: `transform/mod.rs`의 메인 루프, enums.rs의 중복
  케이스·필드 타입 검사, matches.rs의 arm 검사와 방출 내 재귀 변환).
- 2026-08-16: `src/ast.rs` 신설 — `Span`, `Program`/`Segment`(Verbatim·
  Enum·Match·Template), `EnumDecl`/`EnumCase`/`Field`, `MatchExpr`/`Arm`/
  `Pattern`/`Binding`, `Template`/`TemplateChunk`. scrutinee·arm body·보간은
  재귀 `Program`, 에러 보고용 오프셋(`tag_off`/`ty_off`/`pattern_off`/
  `keyword_off`)과 `await` 감지용 원시 Span을 노드에 보존.
- 2026-08-16: `src/parser/` 신설 — mod.rs(메인 스캔 루프를 기존
  `transform()`에서 이식하되 방출 대신 세그먼트 경계 기록; RESERVED/
   regex 판정 등 토큰 규칙 이동; `parse_template`), enums.rs·matches.rs
  (기존 파싱 로직에서 에러 검사를 제거하고 `Option` 반환으로 단순화).
- 2026-08-16: `src/sema.rs` 신설 — 기존 검사(중복 케이스, 필드 타입 swc
  검증, 와일드카드 위치, 중복 arm, 소진성)를 AST 순회로 이식. 순회 순서는
  결정 3대로 기존 동작 재현.
- 2026-08-16: `src/codegen/` 신설 — mod.rs(Program/Template 방출, verbatim
  바이트 복사), enums.rs(`emit_enum`·`generic_param_names` 이식),
  matches.rs(`emit_match` 이식; 방출 문자열 형식은 문자 그대로 유지).
- 2026-08-16: `src/lib.rs` 파이프라인 교체(parse → sema::check →
  codegen::emit → verify_output), `src/transform/` 삭제, `scanner.rs`에
  `scan_type_end` 이동·`at` 공개.
- 2026-08-16: 차등 검증 — 재구성 전(HEAD cbe3915) 바이너리를 워크트리에서
  release 빌드, 새 바이너리와 함께 22개 샘플(.rl: 기본/제네릭/중첩 match/
  템플릿 보간/await/블록 body/에러 5종/주석·문자열/멀티바이트/미종결
  템플릿/옵셔널 필드/복합 타입/유사 rl 통과/scrutinee 중첩/무효 TS/정규식)
  × (`-p`, `-p --no-verify`)를 비교하는 스크립트 실행 → 출력·에러 메시지·
  종료 코드 전부 동일(`ALL SAMPLES IDENTICAL`).
- 2026-08-16: 문서 갱신 — CLAUDE.md 아키텍처 맵·파이프라인 설명,
  `docs/design/compiler-architecture.md` 신설(규범), rust-rewrite.md에
  대체 안내 추가. 검증 게이트 3종 통과 확인 후 커밋.

## 이슈 및 해결

### 이슈 1: 로컬 `main` 브랜치가 초기 커밋에 머물러 차등 검증 기준이 틀렸음

- **증상**: 차등 검증용 워크트리를 `main`으로 만들자 `Cargo.toml`이 없어
  빌드 실패 (`error: could not find Cargo.toml`).
- **원인**: 로컬 `main`이 `fb466b9 Initial commit`(README만 존재)에 머물러
  있었다. 실제 재구성 직전 코드는 작업 브랜치의 시작점인 `cbe3915`.
- **해결**: 워크트리를 `cbe3915`로 다시 만들어 비교 기준으로 사용.

### 이슈 2: 순회 순서를 잘못 잡으면 중첩 구문의 에러 보고가 달라짐

- **증상**: (구현 전 분석에서 발견) 단순히 오프셋 순으로 검사하면, 바깥
  match의 중복 arm과 arm body 안 중첩 구문의 에러가 공존할 때 기존과 다른
  에러가 먼저 보고될 수 있다.
- **원인**: 기존 구현은 match의 자체 규칙을 검사한 뒤에야 방출 단계에서
  자식을 재귀 변환했다 — 즉 "노드 규칙 → 자식" 순서였다.
- **해결**: sema 순회를 같은 순서(노드 규칙 → scrutinee → arm body)로 설계
  (결정 3). 기존 테스트의 에러 위치 검증과 차등 검증으로 확인.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` — 28(compile) + 22(passthrough) + 9(integration, tsc/node
  포함) + 4(doctest) 전부 통과
- [x] (추가) 재구성 전후 바이너리 차등 검증: 22 샘플 × 2 플래그 조합에서
  출력·에러·종료 코드 바이트 단위 동일

## 결과

- 신설: `src/ast.rs`, `src/parser/{mod,enums,matches}.rs`, `src/sema.rs`,
  `src/codegen/{mod,enums,matches}.rs`, `docs/design/compiler-architecture.md`
- 삭제: `src/transform/{mod,enums,matches}.rs`
- 수정: `src/lib.rs`(파이프라인 교체), `src/scanner.rs`(`scan_type_end` 이동,
  `at` 공개), `CLAUDE.md`(아키텍처 맵), `docs/design/rust-rewrite.md`(대체
  안내), `docs/tasks/INDEX.md`
- 언어 표면 변화 없음(레퍼런스 문서 갱신 불필요). 이후 기능 추가는
  `docs/design/compiler-architecture.md`의 "기능 추가 가이드"를 따른다.
