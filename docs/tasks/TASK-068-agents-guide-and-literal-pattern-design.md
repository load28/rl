# TASK-068: 저장소 밖에 있던 AGENTS.md와 리터럴 패턴 설계 문서 편입

- **상태**: 완료
- **시작일**: 2026-08-18
- **완료일**: 2026-08-18
- **커밋**: b899570 (AGENTS.md), ca4faf4 (설계 문서 이동)

## 목적

작업 트리에 추적되지 않은 문서 둘이 남아 있었다. 둘 다 저장소가 관리해야 할
성격의 문서이므로 제자리에 넣고 추적한다.

- `AGENTS.md` — Codex용 작업 가이드. `CLAUDE.md`와 같은 역할이라 루트에 있어야
  한다.
- `match-literal-pattern-design.md` — `match` 리터럴 패턴 도입에 대한 설계
  의견. 설계 문서는 `docs/design/`에 모인다.

## 범위

- 포함: `AGENTS.md` 추적, 설계 문서를 `docs/design/match-literal-patterns.md`로
  이동.
- 제외: **두 문서의 내용 수정**. 이번 태스크는 배치만 바꾼다 — 파일 내용은
  바이트 그대로다.
- 제외: 리터럴 패턴 자체의 채택 여부와 구현. 그 문서는 제안이고, 진행한다면
  별도 태스크로 등록한다.

## 의사결정

### 결정 1: 설계 문서 이름에서 `-design` 접미사를 뺀다

- **상황**: `docs/design/`의 기존 문서는 전부 주제 이름만 쓴다
  (`pipeline-operator.md`, `module-graph.md`, `ts-sidecar-declarations.md`).
  가져온 파일 이름은 `match-literal-pattern-design.md`였다.
- **검토한 대안**: 이름 그대로 두기 / 관례에 맞춰 `match-literal-patterns.md`.
- **선택과 근거**: 후자. 디렉터리 이름이 이미 `design`이라 접미사가 중복이고,
  같은 디렉터리 안에서 한 파일만 다른 규칙을 쓸 이유가 없다.

### 결정 2: 내용을 손대지 않는다

- **상황**: 문서가 제안하는 typed 소진성 검사는 현재 구현에 없다. 지금 상태와
  맞추려면 문서를 고쳐야 하는지 판단이 필요했다.
- **검토한 대안**: 현재 구현에 맞게 문서를 손보기 / 배치만 바꾸기.
- **선택과 근거**: 배치만. 이 문서는 규범 레퍼런스가 아니라 **제안**이고,
  제안은 그 시점의 판단을 그대로 남기는 편이 낫다. 채택 여부를 정하는 것은
  별도 태스크의 일이다.

## 작업 내역

- 2026-08-18: `git status --untracked-files=all`로 추적되지 않은 두 파일을
  확인했다 (`AGENTS.md`, `match-literal-pattern-design.md`). `.gitignore`에
  걸린 것이 아니라 한 번도 추가되지 않은 상태였다.
- 2026-08-18: `AGENTS.md`를 그대로 커밋했다.
- 2026-08-18: 설계 문서를 `docs/design/match-literal-patterns.md`로 옮겨
  커밋했다 (내용 변경 없음, 352줄).

## 이슈 및 해결

없음.

## 검증

문서만 바뀌므로 컴파일러 게이트는 해당 없다.

- [x] `git status`에 추적되지 않은 파일이 남지 않음
- [ ] `cargo fmt --check` / `clippy` / `cargo test` — 해당 없음

## 결과

- 추가: `AGENTS.md`, `docs/design/match-literal-patterns.md`,
  `docs/tasks/TASK-068-agents-guide-and-literal-pattern-design.md`
- 수정: `docs/tasks/INDEX.md`

후속: `docs/design/match-literal-patterns.md`의 제안(리터럴 패턴 + typed
소진성)을 채택할지는 정해지지 않았다. 진행한다면 새 태스크로 등록한다.
