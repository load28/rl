# TASK-043: 파이프라인 연산자 `|>` 구현

- **상태**: 완료
- **시작일**: 2026-08-17
- **완료일**: 2026-08-17
- **커밋**: —

## 목적

TASK-013에서 설계·승인된 파이프라인 연산자 `|>`를 구현한다
(`docs/design/pipeline-operator.md`). F# 스타일 적용 스텝 + 메서드 스텝,
`$rl_ap` 헬퍼 방출, std data-last 커링 콤비네이터(`*P`) 동시 도입.

## 범위

- 포함: 렉서(`|>` 융합 토큰), 파서(head 추적 + parser/pipes.rs), sema(스트레이
  `|>` 에러, step/head 내 try 금지), codegen(헬퍼 중첩 방출 + 파일당 1회
  `$rl_ap`), std `*P` 콤비네이터, 세 계층 + stdlib 테스트, 레퍼런스 문서 갱신.
- 제외: `?.` 시작 스텝(설계대로 1차 제외), Hack 스타일 topic.

## 의사결정

### 결정 1: `|>`를 렉서에서 융합 토큰(TokenKind::PipeOp)으로

- **상황**: `|` Punct + `>` Punct 인접 쌍을 파서가 조합해 인식할 수도 있었다.
- **검토한 대안**: (A) 파서에서 인접 Punct 쌍 검사 — 기존 스캐너들(가드/암
  본문/try 식)이 `|`와 `>`를 각각 만나 오작동할 여지가 남는다. (B) 렉서 융합 —
  `=>`/`||`/`?.`/`??`와 같은 기존 원칙("파서가 단위로 다뤄야 하는 연산자는
  융합")에 정확히 부합하고, 기존 스캐너들은 PipeOp를 불투명 토큰으로 자연
  통과시킨다(let-else의 `<`/`>` 깊이 계산이 `|>`의 `>`에 흔들리지 않는 부수
  효과 포함).
- **선택과 근거**: (B). `||`(OrOr) 융합이 먼저 매칭되므로 `a ||> b`가 파이프로
  오인되는 일도 없다. `|>`는 유효 TS에 없는 바이트 열이라 통과 계약 무손상.

### 결정 2: head는 역방향 스캔이 아니라 메인 루프의 전방 식-시작 추적

- **상황**: 파이프는 rl 최초의 중위 구문이라 `|>` 발견 시점에 head의 시작을
  알아야 한다. 설계 문서 §5는 "스캔 루프가 식 시작 오프셋을 갱신"을 제안했다.
- **검토한 대안**: (A) `|>`에서 역방향 스캔 — `if (c) { d() } x |> f` 같은
  문장 경계에서 블록/괄호 그룹을 식 일부로 오인해 head를 크게 잡는 반례를
  구성 단계에서 발견(『if』가 경계어인지 식 일부인지 역방향으로는 판별 불가).
  (B) 전방 추적 + 괄호 스택 — 여는 괄호에서 (식 시작, 삼항 오염) 상태를
  푸시하고 닫는 괄호에서 복원. `f(a(b) |> g)`의 head가 `b`가 아니라 `a(b)`로
  잡히는 것이 스택 복원 덕분이다. `}`는 다음 토큰이 식을 계속하는지(연산자·
  `.`·`|>`...)를 보고 복원/리셋을 가른다(`function(){} |> g`는 복원,
  `if(c){} x |> f`는 리셋).
- **선택과 근거**: (B). 반례들이 (A)에서는 구조적으로 해결 불가였고, (B)는
  claim이 일어나지 않는 한 동작에 아무 영향이 없어(추적만) 통과 계약이
  구성적으로 보존된다.

### 결정 3: head가 이미 분리된 세그먼트(템플릿·match)를 포함하면 되감기

- **상황**: TASK-013 이슈 1 — `` `s${x}` |> f ``에서 `|>` 발견 시점에 head의
  일부가 이미 Segment::Template로 방출되어 있다.
- **검토한 대안**: (A) 미리 파이프 위치를 프리스캔 — 루프 이중화. (B) claim
  시점에 head 시작 이후의 세그먼트를 pop(경계에 걸친 verbatim은 절단)하고
  head 토큰들을 하위 Program으로 재파싱 — 세그먼트가 연속(contiguous)이라는
  불변식 덕에 pop된 첫 세그먼트의 시작이 곧 새 플러시 지점이 된다.
- **선택과 근거**: (B). 재파싱 비용은 head에 rl 구문이 있는 드문 경우에만
  발생하고, 구현이 국소적(rewind_segments 함수 하나)이다.

### 결정 4: 스트레이 `|>`는 Program에 기록하고 sema가 보고

- **상황**: 설계 §5.1 — 파싱 실패한 `|>`를 통과시키면 생성물이 유효하지 않은
  TS가 되어 위치 없는 verify 실패가 된다(에러 계층 계약 위반).
- **검토한 대안**: (A) sema가 verbatim 구간을 재렉싱해 `|>` 검출 — 문자열/주석
  오탐을 피하려면 렉서 재실행이 필요하고 sema에 src 전달이 필요. (B) 파서가
  claim 실패 시 오프셋을 `Program::stray_pipes`에 기록, sema는 재귀 방문 중
  첫 기록을 에러로 — 파서는 무오류(기록만) 원칙 유지, 토큰 기반이라 문자열/
  주석/정규식 오탐이 원천 배제.
- **선택과 근거**: (B). 추가 스캔 없음 + 정확한 위치.

### 결정 5: `$rl_ap` 헬퍼는 파일 끝에 function 선언으로

- **상황**: 헬퍼를 파일 상단에 const로 두면 모든 원본 행이 1줄씩 밀린다
  (소스맵 없는 설계에서 행 대응은 실질 가치).
- **검토한 대안**: (A) 상단 const 화살표 — TDZ 안전하지만 행 밀림.
  (B) 파일 끝 function 선언 — 호이스팅되므로 모듈 최상위 파이프라인이 선언
  이전 줄에서 평가되어도 동작하고, 행 대응이 완전 보존된다.
- **선택과 근거**: (B). tsc/노드 실측으로 최상위 파이프라인 동작 확인.

### 결정 6: 삼항·괄호 없는 화살표는 head/step 최상위에서 에러 (설계 유지)

- **상황**: `c ? a : b |> f`를 조용히 `c ? a : ($rl_ap(b, f))`로 잡으면 TC39
  의미(`(c ? a : b) |> f`)와 어긋나는 무언의 의미 차이가 생긴다.
- **선택과 근거**: 설계 §5대로 오염 플래그(`?`/`:` 리셋 시 taint)로 claim을
  중단시켜 stray 에러("parenthesize ...")로 수렴시킨다. 객체 리터럴 콜론은
  `{`/`,` 리셋이 taint를 지우므로 오탐이 없다(`{ a: x |> f }`는 정상 컴파일).

## 작업 내역

- 2026-08-17: 설계 문서(TASK-013)와 파서/렉서/코드젠 전체 정독. head 판별
  전략을 역방향 스캔으로 초안했다가 문장 경계 반례로 폐기(결정 2).
- 2026-08-17: lexer.rs에 PipeOp 융합, ast.rs에 Segment::Pipe/PipeExpr/
  PipeStep/Program::stray_pipes, parser/pipes.rs(스텝 전방 스캔 + 중단 규칙),
  parser/mod.rs(식-시작 추적 track_expr_boundary/brace_ends_expression +
  rewind_segments), sema.rs(stray 에러 + head/step 재귀 방문), codegen/mod.rs
  (emit_pipe + 파일 끝 헬퍼) 구현. tries.rs의 TokenKind 매치에 PipeOp 추가.
- 2026-08-17: stdlib/rl_std.ts에 Option 8종·Result 7종 `*P` 커링 콤비네이터
  추가.
- 2026-08-17: 스모크 테스트 — 커링 스텝/메서드 스텝/match·템플릿 내부/중첩
  괄호 head 방출 확인 후, tsc 6.0.2 `--strict`로 커링 인자 추론(`x: number`)·
  제네릭 구체화·await head를 검증하고 node로 좌→우 평가 순서 확인.
- 2026-08-17: 테스트 — compile.rs 16건(방출 스냅샷·되감기·에러), passthrough.rs
  2건(비트 OR/유니언/문자열·주석·정규식 안의 `|>`), integration.rs 5건(추론·
  실행), stdlib 계약 테스트는 기존 것이 신규 `*P` 포함 소스로 자동 커버.
- 2026-08-17: 문서 — language.md에 §7 신설(모듈 §8, 제한사항 §9로 재번호 +
  참조 문서 4곳 갱신), errors.md 파이프라인 절, std.md `*P` 표, README·
  CLAUDE.md "다섯 구문", 설계 문서 상태를 '구현됨'으로.
- 2026-08-17: 검증 게이트(fmt/clippy/test) 통과 후 커밋.

## 이슈 및 해결

### 이슈 1: 역방향 head 스캔이 문장 경계에서 구조적으로 실패

- **증상**: 구현 전 설계 검토에서 `if (c) { d() } x |> f`의 역방향 스캔이
  `{ d() }`·`(c)`를 괄호 그룹으로 건너뛰고 `if`에 도달 — head가 `x`가 아니라
  `(c) { d() } x`로 잡히는 반례 구성.
- **원인**: 역방향으로는 그룹이 식의 일부(함수 표현식·객체 리터럴)인지 문장
  블록인지 앞 문맥 없이 판별할 수 없다.
- **해결**: 전방 식-시작 추적 + 괄호 스택으로 전환(결정 2). `}`의 복원/리셋
  분기(brace_ends_expression)가 두 경우를 가른다. 반례를 compile.rs
  `pipeline_head_is_the_whole_call_not_the_inner_argument` 등으로 고정.

### 이슈 2: 테스트 기대 문자열의 괄호 수 오기 2건

- **증상**: `pipeline_method_step_chains_postfix`와
  `pipeline_head_reclaims_a_lifted_match`가 기대 문자열 불일치로 실패.
- **원인**: 구현이 아니라 기대값 오기 — 적용 스텝의 acc는 이미 괄호를 갖고
  있어 재괄호하지 않는데 기대값에 한 겹 더 썼고, match head는 match 자체
  래핑 + head 래핑으로 한 겹 더 있었다.
- **해결**: 실제(올바른) 방출에 맞춰 기대 문자열 수정. 방출 형태는 설계
  문서의 형태와 일치함을 확인.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` (tsc 6.0.2 + node 22 환경 — 통합 테스트 포함)

## 결과

- 변경: `src/lexer.rs`, `src/ast.rs`, `src/parser/{mod,pipes,tries}.rs`,
  `src/sema.rs`, `src/codegen/mod.rs`, `src/stdlib/rl_std.ts`,
  `tests/{compile,passthrough,integration}.rs`,
  `docs/reference/{language,errors,std,cli}.md`, `docs/design/{pipeline-operator,module-graph,project-front-end}.md`,
  `README.md`, `CLAUDE.md`, 본 태스크 문서, `docs/tasks/INDEX.md`.
- rl은 이제 다섯 구문: enum, match, try, let-else, `|>`.
- 후속: TASK-044(튜플 match), TASK-045(중첩 패턴), TASK-046(if let) 예정.
