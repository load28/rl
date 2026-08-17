# TASK-039: 예제를 표준 라이브러리 정식 참조 방식으로

- **상태**: 완료
- **시작일**: 2026-08-17
- **완료일**: 2026-08-17
- **커밋**: ffdbe1f

## 목적

`@rl/std`(TASK-035)가 표준 라이브러리를 가져오는 정식 방식이 된 뒤에도
저장소의 대표 예제는 `Option`을 **직접 선언**하고 `unwrapOr`를 손으로
구현하고 있었다. 예제가 권장 방식과 어긋나면 문서가 아니라 오해를 만든다.

## 범위

- 포함: `examples/shapes.rl`을 `@rl/std` 사용으로 바꾸고 `shapes.ts` 재생성,
  `docs/design/pipeline-operator.md`의 예시 지정자 갱신, README 보강,
  자동 방출물 gitignore.
- 제외: 과거 태스크 문서. 그 시점의 기록이므로 손대지 않는다.
- 제외: `source/rl-interop`. 상호호환 최소 예제로 `Option`/`Result`를 쓰지
  않으므로 바꿀 것이 없다 (억지로 넣지 않는다).

## 의사결정

### 결정 1: 예제의 중복 선언과 중복 구현을 지운다

- **상황**: `examples/shapes.rl`이 `enum Option<T>`를 선언하고
  `unwrapOr<T>`를 직접 구현했다. 둘 다 표준 라이브러리에 있는 것이다.
- **검토한 대안**:
  - 그대로 두고 std 예시를 따로 추가: 예제가 두 가지 방식을 동시에 보여줘
    "어느 쪽이 권장인가"가 흐려진다.
  - 중복을 지우고 `@rl/std`로 대체.
- **선택과 근거**: 후자. 직접 선언은 `Shape`가 이미 보여주므로 `Option`을
  또 선언할 이유가 없고, 표준 라이브러리를 쓰는 모습이 그 자리를 채우는 게
  낫다. `unwrapOr`는 `Option.unwrapOr`로 대체했다.

### 결정 2: `Option`에 대한 match는 남긴다

- **상황**: `unwrapOr`를 지우면 `Option`을 match하는 코드가 사라진다.
- **선택과 근거**: 이름을 `label`로 바꿔 남겼다. **선언이 이 파일에 없어도**
  `Some`/`None` 소진성 검사가 걸린다는 것(내장 enum, language.md §4.2)이
  예제에서 보여줄 가치가 있는 성질이다. 주석으로 그 사실을 적었다.

### 결정 3: 자동 방출된 `examples/rl.ts`는 gitignore한다

- **상황**: 예제가 `@rl/std`를 import하므로 `rlc shapes.rl`이 `rl.ts`를
  함께 만든다.
- **검토한 대안**: 커밋해서 "이런 파일이 나온다"를 보여주기 / gitignore.
- **선택과 근거**: gitignore. 생성물이고 `--emit-std`로 언제든 다시 나온다.
  대신 README의 예제 참조 문단에 "컴파일하면 `rl.ts`도 함께 실체화되고
  지정자가 `./rl.js`로 바뀐다"고 적어 눈에 보이지 않는 단계를 설명했다.

## 작업 내역

- 2026-08-17: 현황 조사 — `@rl/std`로 옮겨야 할 곳을 찾았다.
  `docs/reference/language.md`·`std.md`·`README.md`의 std 절과 `tests/`는
  이미 TASK-035/037에서 갱신돼 있었고, 남은 것은 `examples/shapes.rl`과
  `docs/design/pipeline-operator.md` 두 곳이었다.
- 2026-08-17: `examples/shapes.rl` — `import { Option } from "@rl/std";`
  추가, 로컬 `enum Option<T>` 제거, `unwrapOr` → `label` + `Option.unwrapOr`.
- 2026-08-17: `rlc shapes.rl`로 `shapes.ts` 재생성. 방출된 지정자가
  `"./rl.js"`이고 `rl.ts`가 함께 나오는 것을 확인했다.
- 2026-08-17: 재생성물 검증 —
  ```
  $ tsc --noEmit --moduleResolution bundler --strict shapes.ts   # exit 0
  $ esbuild shapes.ts --bundle ... && node /tmp/shapes.mjs
  some shape with area 3.14 3.141592653589793 42 1
  rect (ratio 0.75) 12 42 1
  some shape with area 0.00 0 42 1
  7 42
  ```
- 2026-08-17: `docs/design/pipeline-operator.md`의 예시 import를 `@rl/std`로
  갱신, README 예제 참조 문단 보강, `.gitignore`에 `examples/rl.ts` 추가.

## 이슈 및 해결

### 이슈 1: README 상단 스니펫에 잘못된 주석을 달았다

- **증상**: `$ rlc shapes.rl # → shapes.ts`에 "(+ rl.ts)"를 덧붙였는데, 그
  스니펫의 코드는 std를 import하지 않는 `Shape`만의 최소 예시였다.
- **원인**: 파일 이름이 같아 같은 예제라고 착각했다.
- **해결**: 그 줄을 원래대로 되돌리고, 실제 `examples/shapes.rl`을 가리키는
  문단에 설명을 넣었다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` — 8개 스위트 전부 통과
- [x] 재생성한 `examples/shapes.ts` — tsc 타입 검사 통과, 번들·실행 결과 확인

## 결과

- 수정: `examples/shapes.rl`, `examples/shapes.ts`, `README.md`,
  `docs/design/pipeline-operator.md`, `.gitignore`, `docs/tasks/INDEX.md`
- 추가: `docs/tasks/TASK-039-examples-use-std-specifier.md`

후속: 없음. `source/rl-calc`은 TASK-035에서 이미 `@rl/std`를 쓰고,
`source/rl-interop`은 표준 라이브러리를 쓰지 않는다.
