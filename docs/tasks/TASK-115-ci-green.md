# TASK-115: CI 복구 — 새 clippy 린트와 낡아 가는 테스트 개수 guard

- **상태**: 완료
- **시작일**: 2026-08-21
- **완료일**: 2026-08-21
- **커밋**: `TBD`

## 목적

PR #43의 CI가 두 곳에서 빨간색이었다.

1. **`fmt / clippy / test` 잡**: `tests/sidecar.rs:103`의
   `rest.find(['"']).map_or(rest.len(), |i| i)`가
   `clippy::map_or_identity`에 걸린다. 로컬 툴체인(1.94)에는 없는 **새 린트**이고
   CI의 `dtolnay/rust-toolchain@stable`은 1.98이라 거기서만 났다.
2. **`type checking (typescript-go)` 잡**: `grep -q "23 passed" /tmp/native.log`가
   실패. native 스위트는 이제 30개다(TASK-108·109·111에서 늘었다).

## 범위

- 포함: 린트 수정, guard 방식 교체, 로컬 툴체인을 CI와 맞추기.
- 제외: 코드 동작 변경 없음.

## 의사결정

### 결정 1: 개수 guard를 버리고 스위트 자신이 빨개지게 한다

- **상황**: guard의 목적은 "native 테스트가 조용히 **건너뛰지** 않았는가"다.
  툴체인이 없으면 `require_tsgo!`가 `return`하고, 건너뛴 테스트는 **통과로
  집계**되므로 잡이 초록색이 된다. 그래서 개수를 세고 있었다.
- **검토한 대안**:
  - (a) 숫자를 30으로 올린다 — 지금 초록이 되지만, 테스트를 하나 추가할 때마다
    다시 깨진다. 실제로 [TASK-088](./TASK-088-ci-native-guard-count.md)이 이미
    같은 이유로 존재하는 태스크였고, 이번이 두 번째다.
  - (b) "최소 N개 이상"으로 느슨하게 — 여전히 숫자이고, 목적(건너뛰지 않았는가)을
    **간접적으로** 표현한다.
  - (c) 스위트에 "건너뛰면 실패" 스위치를 넣는다.
- **선택과 근거**: (c). `RLC_REQUIRE_TSGO=1`이 켜져 있으면 `toolchain()`이
  아무것도 못 찾았을 때 `return` 대신 **panic**한다. guard가 목적을 그대로
  말하게 되고("툴체인이 반드시 있어야 한다"), 테스트를 더 추가해도 손댈 것이
  없다. CI의 두 잡이 그 환경 변수를 켠다.
- **확인**:

  ```sh
  RLC_REQUIRE_TSGO=1 RLC_TSGO_ROOT=/nonexistent cargo test --test native   # panic
  RLC_REQUIRE_TSGO=1 RLC_TSGO_ROOT=<빌드된 트리> cargo test --test native  # 30 passed
  RLC_TSGO_ROOT=/nonexistent cargo test --test native                      # 조용히 통과 (기존 동작)
  ```

- `require_emit!`(선언 방출까지 되는 컴파일러를 요구)는 그대로 둔다. `check`
  잡은 **릴리스된** API 클라이언트를 쓰고 그것은 선언 방출을 못 하므로, 거기서
  건너뛰는 것은 정상이다 — 이 스위치가 막는 것은 "툴체인이 아예 없음" 하나다.

### 결정 2: 로컬 툴체인을 CI와 맞춘다

CI가 `stable`(1.98)인데 로컬이 1.94라 새 린트를 못 봤다. `rustup toolchain
install stable`로 올려 1.98에서 게이트를 다시 돌렸다. 앞으로 게이트를 돌릴
때는 이쪽이 기준이다.

## 작업 내역

1. `tests/sidecar.rs:103` — `map_or(rest.len(), |i| i)` → `unwrap_or(rest.len())`.
   (clippy가 제안한 그대로. 의미 동일.)
2. `tests/native.rs` — `toolchain()`을 `resolve()`(기존 해석 규칙 그대로) +
   `required()`(스위치) 두 조각으로 나누고, 스위치가 켜진 채로 못 찾으면 panic.
   모듈 헤더에 그 규칙을 적었다.
3. `.github/workflows/ci.yml` — 두 잡에서 `grep -q "23 passed"` 제거,
   `RLC_REQUIRE_TSGO: "1"` 추가. 왜 개수 guard를 버렸는지 주석으로 남겼다.
4. 로컬 툴체인 1.94 → 1.98.

## 이슈 및 해결

- **증상**: 로컬에서는 `cargo clippy --all-targets -- -D warnings`가 깨끗한데
  CI만 빨갛다. **원인**: 린트가 1.98에 새로 들어왔고 로컬은 1.94였다.
  **해결**: 로컬을 stable로 올리고 같은 버전에서 게이트 재실행 — 그 뒤로는
  같은 진단이 나온다.

## 검증

1.98 stable에서:

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` (`RLC_TSGO_ROOT` + `RLC_REQUIRE_TSGO=1`) — 11개 바이너리 전부
- [x] `cargo +1.88 build` (MSRV 잡과 동일)

## 결과

- CI가 초록으로 돌아온다.
- native 스위트에 테스트를 추가해도 CI 파일을 고칠 일이 없다 — TASK-088이
  했던 종류의 후속 작업이 더는 생기지 않는다.
