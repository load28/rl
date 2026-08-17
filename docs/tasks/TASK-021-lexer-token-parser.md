# TASK-021: swc 스타일 렉서 도입 — 토큰 기반 파서 재구성

- **상태**: 완료
- **시작일**: 2026-08-17
- **완료일**: 2026-08-17
- **커밋**: bce14c1

## 목적

TASK-010이 파이프라인(parse → sema → codegen → verify)을 swc 스타일로
분리했지만, 파서 내부는 여전히 바이트 훑기다: 메인 루프가 `prev_sig`/
`prev_word` 원시 상태를 들고 다니고, 서브파서 다섯 개가 각자
`skip_ws_comments`/`ident_end`/수동 인덱스 연산을 반복하며, 표현식 스캐너
세 개(`scan_guard_end`/`scan_expr_end`/`scan_stmt_expr_end`)가 문자열·주석
건너뛰기를 각자 재구현한다. swc가 TypeScript를 렉서 → 토큰 → 파서로
처리하듯, 토큰화를 렉서 단계로 한 번만 수행하고 파서 전체가 토큰 커서
위에서 동작하게 재구성한다.

## 범위

- 포함: `src/lexer.rs` 신설(전체 소스를 유의 토큰 스트림으로 변환, 정규식
  휴리스틱·템플릿 중첩 포함), `src/parser/cursor.rs` 신설(토큰 커서 +
  공용 토큰 스캔), `parser/{mod,enums,matches,tries,lets,imports}.rs`를
  토큰 기반으로 재작성. 문서 갱신(`compiler-architecture.md`, `CLAUDE.md`).
- 제외: AST·sema·codegen·verify 변경 없음. 언어 표면(구문·판별 규칙·방출
  코드·에러) 변경 없음 — 기존 테스트 전체(스냅샷·통과 계약 포함)를 무수정
  통과하는 것이 동작 계약이다.

## 의사결정

### 결정 1: 전체 파일을 한 번에 렉싱한다 (온디맨드 렉싱이 아니라)

- **상황**: 토큰화를 어디서 할지 — 파서가 필요할 때마다 부분 렉싱 vs
  파일 전체를 선행 렉싱.
- **검토한 대안**: 온디맨드(스캔 위치마다 렉서 호출)는 기존 바이트 루프와
  구조가 비슷해져 얻는 것이 적고, 정규식 휴리스틱의 "직전 토큰" 상태를
  호출자마다 다시 이어붙여야 한다. 선행 렉싱은 토큰 벡터 메모리를 쓰지만
  중첩 재귀가 전역 토큰의 부분 슬라이스로 단순해진다.
- **선택과 근거**: 선행 렉싱. 중첩 코드(스크루티니/arm body/보간)의 재귀
  파싱이 기존의 "바이트 범위 재스캔"에서 "토큰 슬라이스 전달"로 바뀌어
  이중 스캔이 사라지고, 정규식 판정이 렉서 안 한 곳에만 남는다. 템플릿은
  보간마다 토큰 스트림을 품는 계층 토큰(`TplPart`)으로 렉싱해 재귀 구조를
  유지했다.

### 결정 2: 융합(2바이트) 토큰은 파서가 단위로 소비하는 4개만

- **상황**: `=>`처럼 다바이트 연산자를 렉서에서 결합할지, 어디까지 할지.
- **검토한 대안**: JS 연산자 전체 결합(swc처럼) / 결합 없음(인접성 검사) /
  파서가 단위로 취급해야 하는 것만 결합.
- **선택과 근거**: `=>`(Arrow — `=` 판정·`< >` 괄호 매칭에서 항상 제외),
  `||`(OrOr — or-패턴 구분자가 될 수 없음), `?.`/`??`(삼항 `?`가 아님)
  4개만. 이 넷은 기존 바이트 파서가 개별 지점마다 lookahead로 처리하던
  것들이라 토큰 종류로 만들면 그 특례들이 전부 사라진다. 나머지 연산자는
  파서가 단위로 쓸 일이 없어 1바이트 `Punct`로 두었다 (`==` 배제 검사
  하나만 스팬 인접성으로 처리).

### 결정 3: 바이트 수준 동작을 그대로 보존하는 "충실한 포팅"을 계약으로

- **상황**: 토큰화하면 일부 엣지 동작을 "개선"할 수 있다(예: arm body 안
  콤마 포함 정규식은 기존 바이트 스캐너가 오인해 match가 통과됐지만 토큰
  스트림에선 정규식이 원자라 정상 리프팅된다).
- **검토한 대안**: 개선을 같이 넣기 / 순수 등가 리팩토링.
- **선택과 근거**: 등가 리팩토링. 검증 가능한 계약("모든 기존 테스트
  무수정 통과 + 구/신 차등 비교 동일")을 갖기 위해 관찰 가능한 동작 차이를
  의도적으로 만들지 않았다. 보존 목록: dotted 판정은 `.`뿐 아니라 `?.`
  직후도 포함, 타입 주석 텍스트는 주석을 포함한 바이트 슬라이스의 trim,
  arm body 스팬은 다음 토큰 시작이 아니라 정지 토큰의 시작 바이트까지,
  `block_diverges`의 `}`-경계 규칙 등. 정규식 토큰의 부수 효과(위 예)는
  verbatim 영역 안에서만 달라질 수 있어 출력 바이트에는 영향이 없음을
  차등 비교로 확인했다.

### 결정 4: 백트래킹은 `Copy` 커서로

- **상황**: 서브파서 실패 시 원위치 복구가 필요하다(기존엔 바이트 오프셋
  저장/복원).
- **검토한 대안**: 명시적 checkpoint/rollback API / 커서 값 복사.
- **선택과 근거**: `Cursor`를 참조+인덱스만 담은 `Copy` 타입으로 만들어
  "값으로 받고, 성공 시 전진된 커서를 반환"하는 규약으로 통일했다.
  실패(None)면 호출자의 원본이 그대로라 복구 코드 자체가 없다.

### 결정 5: `scanner.rs`는 바이트 프리미티브 층으로 존치

- **상황**: 렉서 도입 후 scanner의 역할.
- **선택과 근거**: 문자열/정규식/템플릿 스캔과 괄호 매칭은 렉서의 내부
  구현이 그대로 쓰고, `contains_await`/`scan_type_end`는 codegen이 원시
  스팬 위에서 계속 쓴다. 삭제 대상이 아니라 계층이다: scanner(바이트) →
  lexer(토큰) → parser(구조).

## 작업 내역

- 2026-08-17: TASK-021 등록. 재작성 전 파서 1,797줄(모듈 6개) 분석 —
  보존해야 할 바이트 수준 동작(dotted 판정에 `?.` 포함, `=>`만 각괄호
  매칭에서 제외, 타입 텍스트가 주석을 포함한 채 trim되는 것 등)을 목록화.
- 2026-08-17: `src/lexer.rs` 작성 — `Token { kind, span }`,
  `TokenKind::{Ident, Str, Template(Vec<TplPart>), Regex, Arrow, OrOr,
  OptChain, Coalesce, Punct(u8)}`. 정규식 휴리스틱(`regex_allowed` + 선행
  단어 목록)을 파서 메인 루프에서 렉서로 이동. 템플릿은 `lex_template`이
  보간별 토큰 스트림을 재귀 렉싱.
- 2026-08-17: `src/parser/cursor.rs` 작성 — `Copy` 커서(peek/bump/
  eat_punct/eat_ident/find_close/stop_byte_at/sub)와 공용 헬퍼
  (`dotted_at`, `skip_match_shape`, 토큰 수준 괄호 매칭).
- 2026-08-17: `parser/mod.rs` 재작성 — 메인 루프가 `prev_sig`/`prev_word`
  바이트 상태 대신 토큰을 순회. 후보 키워드에서 서브파서 호출, 성공 시
  토큰 인덱스 점프 + verbatim 경계는 바이트 스팬으로 유지.
- 2026-08-17: 서브파서 5개 재작성 — `enums`(토큰 수준 `type_end`),
  `matches`(guard/expr-body 스캔), `tries`(`stmt_expr_end`/`binding_end`),
  `lets`(`expr_until_else`/`block_diverges`), `imports`. 표현식 스캐너들의
  문자열/주석 건너뛰기 중복 코드가 전부 사라짐(토큰이 원자라 불필요).
- 2026-08-17: 검증 3종 — ① 게이트(fmt/clippy/test) 통과, 기존 테스트
  138개 무수정 통과. ② 구(HEAD 64a9123)/신 컴파일러 차등 비교:
  저장소의 실제 TS 소스(언어 서버 포함)와 엣지 케이스 파일(정규식/나눗셈,
  가드, 옵셔널 체이닝, 중첩 match 템플릿, try/let-else, 한글 멀티바이트,
  import 재작성) 9개를 `-p --no-verify`로 컴파일해 출력·종료 코드 완전
  동일. ③ 문서 갱신(`compiler-architecture.md` 파이프라인·단계 서술,
  `CLAUDE.md` 맵, `CHANGELOG.md`).

## 이슈 및 해결

### 이슈 0 (사후): CI clippy 버전 차이로 게이트 실패

- **증상**: 로컬 게이트는 전부 통과했지만 CI(`dtolnay/rust-toolchain@stable`)
  가 clippy 1.97의 신규 린트 2건으로 실패 — `while_let_loop`
  (matches.rs `parse_arms`의 `loop { let Some .. else break }`),
  `collapsible_match`(lets.rs `block_diverges`의 `;` arm 내부 `if`).
- **원인**: 로컬 clippy가 1.94로 CI의 stable(1.97)보다 낮아 두 린트가
  로컬에서 보이지 않았다.
- **해결**: 로컬 툴체인을 1.97로 올려 CI와 맞춘 뒤 두 곳을 리라이트
  (`while let` 루프, match 가드로 조건 합침 — 동작 동일, 전체 테스트로
  확인). 후속 수정 커밋으로 반영.

### 이슈 1: clippy `question_mark` 4건

- **증상**: `if cur.eat_punct(b',').is_none() { return None; }`가
  `-D warnings`에서 거부됨.
- **원인**: 바이트 파서의 제어 흐름을 그대로 옮기며 `?`로 축약 가능한
  형태를 남겼다.
- **해결**: `cur.eat_punct(b',')?;`로 교체. 그 외 실질 이슈 없음 — 토큰
  파서는 첫 전체 테스트 실행에서 138개 전부 통과했다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` — 기존 테스트 무수정 통과: 단위 80 / 통합 17(tsc·node) /
  통과 계약 35 / stdlib 2 / doctest 4
- [x] 구/신 컴파일러 차등 비교 — 저장소 TS 소스 + 엣지 케이스 9파일 출력
  바이트 동일

## 결과

- 추가: `src/lexer.rs`, `src/parser/cursor.rs`,
  `docs/tasks/TASK-021-lexer-token-parser.md`
- 재작성: `src/parser/{mod,enums,matches,tries,lets,imports}.rs`
- 수정: `src/lib.rs`(`mod lexer` 등록),
  `docs/design/compiler-architecture.md`, `CLAUDE.md`, `CHANGELOG.md`,
  `docs/tasks/INDEX.md`
- AST·sema·codegen·verify·언어 표면 변경 없음.

후속: 렉서가 생겼으므로 2단계(모듈 그래프 선언 수집,
[`module-graph.md`](../design/module-graph.md))에서 참조 파일의 enum 선언만
뽑는 경량 파싱을 토큰 스트림 위에서 바로 구현할 수 있다.
