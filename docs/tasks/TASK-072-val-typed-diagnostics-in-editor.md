# TASK-072: 에디터에 타입 기반 `val` 진단 노출

- **상태**: 대기
- **시작일**: —
- **완료일**: —
- **커밋**: —

## 목적

TASK-071로 `val` 경로의 built-in 변경 메서드 판정이 `rlc --types`로 옮겨졌다.
에디터(LSP)는 rl 진단을 `rlc --check`로 받으므로(`editors/vscode/server/src/rlc.ts`),
`map.set("a", 1)` 같은 확실한 built-in 변경이 편집 중에는 표시되지 않는다.
타입 진단 경로(`tsproject.ts`)는 이미 같은 프로그램의 TypeChecker를 들고 있으므로
거기서 `rlc::val_method_calls`의 프로브를 답해 인라인 진단으로 띄울 수 있다.

## 범위

- 포함(예정): `rlc --emit-map`/가상 문서 매핑으로 프로브 위치를 옮기고,
  `tsproject.ts`에서 `types_host.mjs`와 **같은 판정**(심볼 선언이 기본 lib의
  `Array`/`Map`/`Set`/`WeakMap`/`WeakSet`/TypedArray 변경 메서드인지)을 수행.
- 제외: 판정 규칙 자체의 변경 (규범은 `language.md` §10.4).

## 의사결정

(작업 시작 시 기록)

## 작업 내역

## 이슈 및 해결

## 검증

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo test`

## 결과
