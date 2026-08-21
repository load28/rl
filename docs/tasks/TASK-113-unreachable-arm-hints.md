# TASK-113: 도달 불가 arm을 에디터 힌트로

- **상태**: 완료
- **시작일**: 2026-08-21
- **완료일**: 2026-08-21
- **커밋**: `df55133`

## 목적

[TASK-101](./TASK-101-rust-parity-review.md) GAP-6의 마지막 항목(or-패턴 언어
확장 제외): **도달 불가 arm 검사가 `match`의 중복 태그에 한정된다.**

[TASK-103](./TASK-103-usefulness-exhaustiveness.md)이 usefulness 알고리즘을
들여오면서 죽은 암은 이미 **계산되고** 있었다(`Coverage::unreachable`,
`unreachable_arms`). 다만 아무도 그것을 소비하지 않았다 — 필드의 주석이
"Nothing reports these yet ... the editor is where a hint belongs"라고 적어 둔
상태였다. 이번 태스크가 그 소비자를 만든다.

sema가 잡는 중복 암(무가드 암이 이미 덮은 **태그**의 반복)보다 넓다. 예를 들어
중첩 패턴이나 튜플 조합 때문에 앞선 암이 이미 전부 잡는 암은 중복 암 규칙에
걸리지 않지만 usefulness는 죽은 것으로 판정한다.

## 범위

- 포함: 엔진의 새 표면 `rl_hints`, `--server`의 `rlHints` 메서드, VS Code
  확장의 표시, 테스트, 문서.
- 제외: 이것을 에러로 만드는 것(결정 1), 힌트의 quick fix(암 삭제), `val`·
  파이프라인 등 다른 구문의 힌트(지금 계산되는 것이 없다).

## 의사결정

### 결정 1: 에러가 아니라 힌트

- **상황**: 도달 불가 arm을 어디에 넣을 것인가.
- **검토한 대안**:
  - (a) 컴파일 에러 — Rust는 이것을 **린트**(`unreachable_patterns`)로 다룬다.
    에러로 만들면 **지금 컴파일되는 프로그램을 거절**하게 되고, 그것은 언어
    표면의 하위 호환을 깨는 변경이다.
  - (b) 경고 — rl에는 경고 계층이 없다. `errors.md`는 에러만 규정하고 CLI는
    "컴파일하거나, 위치를 대고 멈춘다"뿐이다. 경고 계층을 만드는 것은 이
    태스크보다 훨씬 큰 결정이다(종료 코드, `--deny-warnings` 같은 표면).
  - (c) 에디터 힌트 — 빌드를 막지 않고, 범위에 붙고, CLI는 인쇄하지 않는다.
- **선택과 근거**: (c). `Coverage::unreachable`의 주석이 이미 같은 결론을
  적어 뒀고(TASK-103), 근거가 그대로 유효하다. 사용자가 보는 것이 목적이지
  거절하는 것이 목적이 아니다.

### 결정 2: 새 표면(`rlHints`)이지 `check`의 진단이 아니다

- **상황**: 서버의 `check`가 이미 rl 수준 진단을 준다. 거기 얹을 수도 있다.
- **검토한 대안**:
  - (a) `check`의 `diagnostics`에 심각도 필드를 추가 — 그러면 `check`가
    `rlc::compile`의 답 그대로가 아니게 된다. 지금 `check`는 "one-shot CLI와
    같은 답"이 계약이고(`server.rs` 헤더), CLI는 힌트를 모른다. 두 소비자가
    갈라진다.
  - (b) 별도 메서드 `rlHints`.
- **선택과 근거**: (b). 힌트는 컴파일의 답이 아니다. 분리하면 "CLI는 힌트를
  인쇄하지 않는다"가 타입으로 강제된다 — 힌트가 `Diagnostic`이 아니므로
  섞일 수가 없다.

### 결정 3: 파싱만으로 답한다

`rlSymbol`·`rlCompletions`와 같은 층에 둔다(`engine::hints`). 소진성 계산은
선언 표만 있으면 되고, 그 표는 파서와 디스크의 import로 만들어진다. 그래서
TypeScript 툴체인이 없어도, 저장하지 않은 버퍼에서도 답이 나온다. typed
경로에서 더 정확한 답(좁혀진 타입 기준)을 낼 수도 있지만, 그것은 지연이 큰
경로이고 힌트는 즉시성이 값어치의 대부분이다.

### 결정 4: 범위는 패턴부터 본문 끝까지

- **상황**: 무엇을 흐리게 할 것인가 — 패턴만? 암 전체?
- **선택과 근거**: 암 전체. 죽은 것은 패턴이 아니라 **그 암**이고, VS Code의
  `Unnecessary` 태그는 흐리게 하는 표시이므로 죽은 코드 전체에 걸리는 것이
  맞다. 본문 span은 암 구분자까지 뻗을 수 있어 끝을 공백 앞으로 당긴다.
- 이를 위해 `AnalyzedArm`에 `pattern_start`를 추가했다(전에는 본문 span만
  있었다). 파서의 `Arm::pattern_off`/`TupleArm::pattern_off`를 그대로 옮긴다.

## 작업 내역

1. `src/analysis/mod.rs` — `AnalyzedArm::pattern_start`.
2. `src/engine/hints.rs`(신설) — `RlHint`/`RlHintKind::UnreachableArm`,
   `rl_hints(path, source)`. `analyses_for`로 분석을 받아
   `coverage.unreachable`의 인덱스를 암 범위로 바꾼다. 단위 테스트 4개
   (단일·튜플·가드 암·죽은 것 없음).
3. `src/engine/mod.rs` — 재수출.
4. `src/server.rs` — `rlHints` 메서드와 프로토콜 문서.
5. `editors/vscode/server/src/engine.ts` — `EngineRlHint`, `rlHints()`.
6. `editors/vscode/server/src/server.ts` — 진단 계산의 마지막에 힌트를 덧붙인다
   (`DiagnosticSeverity.Hint` + `DiagnosticTag.Unnecessary`, `source: "rl"`),
   버전 게이트 포함.
7. `editors/vscode/server/src/test/engine.test.ts` — 테스트 2개. guard는
   rlc만 본다(툴체인이 필요 없는 표면이므로).
8. 문서: `cli.md`(프로토콜 + 힌트 표), `language.md`(암 검사·한계 표),
   `docs/ai/rl.md`, `CHANGELOG.md`, `rust-parity-analysis.md` GAP-6.

확인:

```sh
cargo test                                   # 66 lib (hints 4개 추가)
cd editors/vscode && npm test                # 80 pass, 0 skip
```

## 이슈 및 해결

- **증상**: 첫 단위 테스트에서 범위 끝이 한 바이트 더 갔다(56 기대, 57 실측).
  **원인**: 암 본문 span이 구분자까지 뻗어 뒤 공백을 포함한다. **해결**:
  `source[..body_end].trim_end().len()`으로 코드 끝까지만 잡는다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` (tsgo 있음)
- [x] `npm test` (editors/vscode) — 80 pass

## 결과

- 죽은 암이 편집 중에 보인다. 컴파일은 그대로 성공한다.
- GAP-6에서 남은 것: 중첩 패턴 내부 소진성 v2, let-else·`if let`의 or-패턴
  (사용자가 범위에서 제외).
