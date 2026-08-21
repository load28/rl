# TASK-147: semantic 패턴 진단의 완전한 primary span

- **상태**: 완료
- **시작일**: 2026-08-22
- **완료일**: 2026-08-22
- **커밋**: —

## 목적

semantic 패턴 오류가 시작 offset만 전달해 에디터가 숫자·튜플 패턴의 표시 폭을
추측하는 문제를 없앤다. parser AST부터 diagnostic까지 완전한 primary span을
단계 계약으로 유지한다.

## 범위

- 포함: 단일·튜플 match arm의 AST pattern span, HIR·analysis 소비자 이관,
  semantic 진단 범위, 회귀 테스트와 문서
- 제외: 진단 메시지와 의미 규칙 변경, 에디터의 범위 추측 규칙 변경

## 의사결정

1. **에디터의 단어 범위 추측을 수정하지 않는다.** 숫자와 괄호의 단어 경계는
   진단 의미가 아니며 소비자마다 다르다. 컴파일러가 알고 있는 원문 범위를 끝까지
   전달한다.
2. **arm AST의 offset 필드를 span으로 교체한다.** 별도 end 필드를 덧붙이면 두
   위치가 어긋날 수 있다. `Arm`·`TupleArm`에서 `pattern_off`를 제거하고 완전한
   `pattern_span` 하나를 parser→HIR→analysis→sema 계약으로 사용한다.
3. **semantic 노드 오류는 non-empty span으로 만든다.** mixed-pattern과 tuple
   arity만 고치는 대신, result binding 진단에 남아 있던 offset 수집도 name/run
   span으로 이관했다. semantic checker에는 `RlError::at` 호출이 남지 않는다.

## 작업 내역

- 2026-08-22: mixed 숫자 패턴과 tuple arity 패턴이 시작 문자만 표시되는 현상을
  확인했다.
- 2026-08-22: 단일·튜플 arm parser가 소비한 마지막 패턴 토큰까지의 span을 AST에
  기록하고 HIR·analysis의 기존 offset 소비를 모두 이관했다.
- 2026-08-22: mixed-pattern과 tuple arity 진단이 AST primary span을 사용하게 했다.
  result의 missing-keyword·nested-binding 수집도 span으로 바꿔 semantic 단계의
  point 진단을 제거했다.
- 2026-08-22: Rust source-slice 테스트와 실제 LSP publishDiagnostics 테스트에서
  `222`와 `(Up(value))` 전체 범위를 고정했다.
- 2026-08-22: 전체 Rust 검증 후 release 컴파일러·VSCode 확장·rl-tour 패키지를
  재설치했다. 실제 tour typedCheck 응답에서 mixed pattern은 163:5–8, tuple
  pattern은 199:5–20의 완전한 범위를 확인했다.

## 이슈 및 해결

- **증상**: mixed match의 `222`와 arity가 틀린 `(Up(value))`가 첫 문자만
  밑줄로 표시됐다. **원인**: semantic 진단이 `RlError::at(pattern_off)`을 만들어
  서버의 `endLine`/`endCol`이 비었다. **해결**: parser AST가 완전한 pattern span을
  소유하고 semantic 진단이 그 span을 primary range로 사용하게 했다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` — 665개 통과
- [x] VSCode 패턴 범위 회귀 테스트 통과
- [x] semantic checker의 `RlError::at` 호출 0개 확인
- [x] release 컴파일러·VSCode 확장·rl-tour 패키지 재설치
- [x] rl-tour 실제 typedCheck의 mixed·tuple `endLine`/`endCol` 확인

## 결과

단일·튜플 match arm의 primary span은 parser AST에서 만들어져 HIR·analysis·sema와
서버 직렬화까지 유지된다. semantic 구문 오류는 더 이상 시작 offset의 표시 폭을
소비자에게 맡기지 않는다. 이후 패턴 문법도 `pattern_span` 단계 계약을 사용하므로
숫자·식별자·괄호 형태와 무관하게 실제 RL 원문 범위로 표시된다.
