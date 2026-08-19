# TASK-089: 에디터 TS 진단의 glue 위치 보정

- **상태**: 완료
- **시작일**: 2026-08-20
- **완료일**: 2026-08-20
- **커밋**: 이 커밋

## 목적

`try`가 생성한 조기 반환 코드에서 TypeScript 타입 에러가 발생하면
`rlc --check-types`는 원본 `.rl` 위치로 보고하지만, VSCode 확장의
`rl.typeDiagnostics` 경로는 아무 진단도 표시하지 않았다.

에디터 TS 진단 경로도 배치 타입체크와 같은 위치 보정 정책을 쓰게 한다.

## 범위

- 포함: `engine/language.rs`의 `tsDiagnostics` 매핑 정책을 배치 타입체크와
  맞춘다.
- 포함: glue 위치에 걸린 TS 진단을 가장 가까운 원본 구문 위치로 되돌리고,
  생성 코드에서 온 진단임을 메시지에 표시한다.
- 제외: `--rl-only` 정책 변경. 이 옵션은 typed rl 진단 경로에서 일반 TS
  진단 중복 표시를 막기 위한 것이므로 유지한다.
- 제외: navigation, definition, references, rename의 glue 역매핑 정책 변경.

## 의사결정

### 결정 1: `--rl-only`는 유지한다

- **상황**: `rlc --check-types`는 `try decode(...)`의 누락된 에러 타입을
  `ts(2322)`로 잡지만, `--check-types --rl-only`는 출력하지 않는다.
- **검토한 대안**: `--rl-only` 제거 / 에디터 쪽 문자열 필터 / 기존 설계 유지.
- **선택과 근거**: 기존 설계를 유지한다. TASK-072의 결정대로 typed rl 진단
  경로는 `val`과 타입 기반 소진성 같은 rl 계층만 중계해야 한다. 일반 TS
  진단은 `rl.typeDiagnostics` 경로가 담당한다.

### 결정 2: 에디터 TS 진단은 CLI처럼 glue fallback을 쓴다

- **상황**: TASK-087 이후 `tsDiagnostics`는 엔진이 `.rl` 좌표로 답한다.
  그런데 `engine/language.rs`는 진단 span 양끝이 정확히 원본에 매핑될 때만
  표시하고, glue 위치에 걸린 진단은 버렸다.
- **검토한 대안**: glue 진단 계속 폐기 / 모든 기능의 glue 역매핑 허용 /
  진단에만 배치 타입체크와 같은 fallback 적용.
- **선택과 근거**: 진단에만 fallback을 적용한다. CLI의 typed 경로는 이미
  `mapper::to_source_or_nearest()`로 glue 진단을 원본 구문에 붙이고
  `(in code rlc generated for this construct)`를 덧붙인다. 에디터
  `tsDiagnostics`도 같은 의미의 사용자 피드백이므로 같은 정책을 쓴다.

### 결정 3: navigation 계열은 정확 매핑만 유지한다

- **상황**: TASK-087은 rename 원자성을 엔진 규칙으로 두고, glue로 역매핑할 수
  없는 edit은 전체를 거부한다고 정했다.
- **검토한 대안**: 공용 `from_service_span()`을 fallback 지원으로 변경 /
  진단 전용 helper 추가.
- **선택과 근거**: 진단 전용 `diagnostic_source_span()`을 추가한다. hover,
  definition, references, rename은 사용자가 실제로 이동하거나 수정할 수 있는
  원본 위치만 반환해야 하므로 기존 `from_service_span()` 정책을 건드리지
  않는다.

## 작업 내역

- 2026-08-20: `rl-tour`의 `src/_try-error-propagation-smoke.rl`에서
  `loadConfigSmoke(): Result<Config, ConfigError>`가 `try decode(...)`로
  `DecodeError`를 전파하게 해 증상을 재현했다.
- 2026-08-20: `rlc --check-types src/_try-error-propagation-smoke.rl`는
  `ts(2322)`를 출력하지만, 에디터 engine API
  `engine.tsDiagnostics("rlc", path)`는 빈 배열을 반환하는 것을 확인했다.
- 2026-08-20: `src/engine/language.rs`에 진단 전용
  `diagnostic_source_span()`을 추가했다. 정확 매핑이 되면 기존 span을 쓰고,
  실패하면 `mapper::to_source_or_nearest()`로 가장 가까운 원본 위치를 쓴다.
- 2026-08-20: glue fallback으로 나온 진단 메시지에
  `(in code rlc generated for this construct)`를 붙였다.
- 2026-08-20: `cargo install --path .`로 새 `rlc`를 설치하고,
  `editors/vscode`에서 `npm run compile`,
  `npx @vscode/vsce package --no-dependencies`,
  `code --install-extension rl-language-0.1.0.vsix --force`를 실행했다.

## 이슈 및 해결

### 이슈 1: 에디터 engine API가 `try` 타입 에러를 빈 배열로 반환

- **증상**: `rlc --check-types`는 `ts(2322)`를 출력하지만
  `engine.tsDiagnostics()`는 `[]`를 반환했다.
- **원인**: TypeScript 진단 위치가 `try` lowering이 생성한
  `return $rl_t1` 쪽 glue에 걸렸다. 에디터 진단 매핑은 정확 매핑 실패 시
  진단을 버리고 있었다.
- **해결**: 진단 전용 helper에서 CLI와 같은 glue fallback을 사용했다.

### 이슈 2: 일부 에디터 테스트가 macOS canonical path 차이로 실패

- **증상**: `node --test server/out/test/engine.test.js --test-name-pattern diagnostics`
  실행 중 definition/references/rename 3건이 `/var/...`와 `/private/var/...`
  차이로 실패했다.
- **원인**: macOS 임시 디렉터리 경로가 TypeScript 서비스 응답에서는
  canonical path로 돌아오지만, 테스트 기대값은 `os.tmpdir()` 원문 경로였다.
- **해결**: 이번 태스크의 진단 경로와 무관한 기존 테스트 환경 차이로 보고
  건드리지 않았다. 같은 실행에서 diagnostic 관련 케이스는 통과했다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test engine::language -- --nocapture`
- [x] `node --test server/out/test/engine.test.js --test-name-pattern diagnostics`
      — diagnostic 케이스 통과, 경로 canonicalization 관련 3건 실패
- [x] `engine.tsDiagnostics()` 수동 확인 — `try` glue 위치의 `ts(2322)`가
      `.rl` 좌표로 반환됨

## 결과

에디터의 일반 TS 진단 경로가 배치 타입체크와 같은 glue fallback 정책을 쓴다.
`try`가 생성한 조기 반환 코드에서 발생한 타입 에러도 VSCode에서 표시된다.

변경 파일:

- `src/engine/language.rs`
- `docs/tasks/TASK-089-editor-diagnostic-glue-fallback.md`
- `docs/tasks/INDEX.md`
