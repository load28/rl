# TASK-141: codex 브랜치 정밀 진단 이식

- **상태**: 완료
- **시작일**: 2026-08-21
- **완료일**: 2026-08-21
- **커밋**: —

## 목적

main의 parser claim·회복 출력·부분 Snapshot 구조를 유지하면서 codex 브랜치의 구체적 오류 진단과 CLI·에디터 진단 코드 전달을 선별 이식한다.

## 범위

- 포함: 공통 조상 기준 비교, tuple arity·enum 타입 정밀 진단, 공통 anchor/translation 분류와 LSP `Diagnostic.code`, 회귀 테스트와 문서
- 제외: codex의 projection-only 부분 실패 및 typed-poison 방식으로 main 구조를 되돌리는 변경

## 의사결정

### 결정 1: 커밋 병합 대신 네 진단 요소만 재구현한다

- **상황**: codex 한 커밋에는 정밀 진단과 함께 main보다 약한 parser·projection·Snapshot 구현이 섞여 있다.
- **검토한 대안**: 커밋 전체 cherry-pick은 main 개선을 되돌린다. 파일 단위 복사는 같은 파일에 공존하는 main 개선을 잃는다. 동작 단위 재구현은 범위가 명확하다.
- **선택과 근거**: 공통 조상 `8524ed1`과 양쪽 최종 트리를 비교하고, tuple arity·enum 필드 타입·translation class·LSP code 전달만 main 구조 위에 재구현했다. 비교는 `git merge-base main codex`와 `git diff main...codex`로 확인했다.

### 결정 2: typed-poison 대신 main의 구조적 회복을 유지한다

- **상황**: codex의 typed-poison은 생성 코드 연쇄 진단을 줄이지만 match 전체에서 독립적인 사용자 타입 오류도 숨길 수 있다.
- **검토한 대안**: typed-poison 전체 이식은 억제 범위가 넓다. main의 회복 출력과 의미 기반 match 억제는 원인을 제거하거나 좁은 경계에서 처리한다.
- **선택과 근거**: typed-poison·duplicate-enum 플래그·projection-only 실패 모델은 이식하지 않았다. translation class는 동일 rl 의미의 생성 코드 진단 중복 제거에만 사용했다.

## 작업 내역

- 2026-08-21: TASK-141을 등록하고 두 브랜치의 공통 조상 기준 비교를 시작했다.
- 2026-08-21: enum 타입 스캔을 기대 닫힘 기호 스택으로 바꿔 잘못 맞물린 구분자를 필드 타입 진단으로 보존했다.
- 2026-08-21: tuple 의도가 반대편에서 확정되면 arity 1인 쪽도 AST로 복구해 정확한 원소 수 오류를 내도록 했다.
- 2026-08-21: TypeScript 코드 여러 개를 하나의 rl 의미로 묶는 `translation_class`를 CLI와 에디터에서 공동 사용했다.
- 2026-08-21: compiler protocol의 안정적인 rl rule code를 VSCode LSP `Diagnostic.code`까지 전달하고 실서버 테스트를 갱신했다.
- 2026-08-21: reference·AI·VSCode 문서를 갱신하고 Rust 전체 게이트와 현재 rlc를 사용한 LSP 회귀 테스트를 통과했다.

## 이슈 및 해결

### 이슈 1: VSCode 전체 테스트가 PATH의 이전 rlc와 macOS 임시 경로 별칭 때문에 실패했다

- **증상**: 첫 실행은 새 malformed-match 진단 대신 이전 verify 오류를 받았고 12건이 실패했다. 현재 `target/debug/rlc`를 PATH 앞에 둔 재실행에서는 기능 관련 실패가 사라지고 `/var`와 `/private/var` 비교 4건만 남았다.
- **원인**: 테스트가 저장소 빌드가 아니라 PATH의 `rlc`를 실행하며, macOS가 같은 임시 경로를 두 표기로 반환한다.
- **해결**: 현재 빌드를 PATH 앞에 둬 새 LSP 테스트와 관련 진단 테스트를 검증했다. 남은 4건은 이번 진단 이식과 무관한 기존 경로 정규화 문제이므로 범위 밖으로 유지했다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`

## 결과

main의 구조적 우위를 유지한 채 codex가 더 정확했던 네 진단 요소만 이식했다. Rust 전체 검증은 통과했고, 현재 빌드 기준 LSP 기능 테스트도 통과했다.
