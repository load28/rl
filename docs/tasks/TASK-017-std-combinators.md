# TASK-017: std 콤비네이터 확장 (zip/flatten/transpose/collect/fromPromise)

- **상태**: 완료
- **시작일**: 2026-08-16
- **완료일**: 2026-08-16
- **커밋**: 431dd0f

## 목적

TASK-014~016으로 언어 표면이 넓어진 데 맞춰, Rust 표준 라이브러리에 대응하는
값 조합 콤비네이터를 std 모듈에 채운다. 컴파일러 변경 없이 순수 TypeScript
모듈 확장만으로 되는 저위험 작업이다.

## 범위

- 포함: `Option.zip` / `Option.flatten` / `Option.transpose` /
  `Option.collect`, `Result.flatten` / `Result.transpose` /
  `Result.collect` / `Result.fromPromise`. std.md 표 갱신, 통합 테스트
  (tsc `--strict` 타입체크 + node 런타임 실행).
- 제외: 커링 버전(`mapP` 등 — TASK-013 파이프라인 제안에 묶임), 메서드
  체이닝, iterator 어댑터.

## 의사결정

### 결정 1: Rust 대응 이름·의미를 그대로 따름

- **상황**: 추가할 콤비네이터의 이름과 의미 선택.
- **검토한 대안**: ① fp-ts 스타일(`sequenceArray`, `separate` 등) — rl의
  "Rust처럼 읽힌다" 방향과 어긋남. ② Rust 이름 그대로: `zip`, `flatten`,
  `transpose`, `collect`(`Vec<Option<T>> → Option<Vec<T>>`에 해당).
- **선택과 근거**: ②. 기존 std가 이미 Rust 이름(`unwrapOr`, `andThen`,
  `okOr`)을 쓰고 있고, 레퍼런스 문서도 Rust 의미로 설명한다. `collect`는
  배열 입력을 받는 데이터-우선 정적 함수로 옮겼다(첫 `None`/`Err`에서 중단).

### 결정 2: `fromPromise`는 Rust에 없지만 추가

- **상황**: TS 생태계에서 비동기 실패 포획(`Promise` rejection → `Err`)은
  `fromThrowable`(동기)만큼 흔한 요구다.
- **검토한 대안**: ① 순수 Rust 정합만 유지하고 제외 — 사용자가 매번
  `fromThrowable`+await 조합을 재발명. ② `fromPromise: (p: Promise<T>) =>
  Promise<Result<T, unknown>>` 추가 — 기존 `fromThrowable`의 비동기 짝.
- **선택과 근거**: ②. 에러 타입을 `unknown`으로 두는 기존 `fromThrowable`
  계약과 대칭이고, 값 형태 계약(순수 `kind` 태그드 객체)을 건드리지 않는다.

### 결정 3: 값 형태 계약은 불변

- **상황**: 콤비네이터 추가가 std 계약 테스트(`tests/stdlib.rs` — 선언부가
  rl enum 방출과 바이트 일치, 모듈 전체가 통과 영역)와 충돌하지 않아야 함.
- **검토한 대안**: 해당 없음 (확인 사항).
- **선택과 근거**: 새 함수는 전부 기존 `Option`/`Result` const 객체의
  프로퍼티 추가라 타입/생성자 선언부는 그대로다. `cargo test`(stdlib 계약
  테스트 포함)로 확인.

## 작업 내역

- 2026-08-16: `src/stdlib/rl_std.ts` — Option에 `zip`/`flatten`/`transpose`/
  `collect`, Result에 `flatten`/`transpose`/`collect`/`fromPromise` 추가
  (기존과 같은 데이터-우선 화살표 함수 + doc 주석 스타일).
- 2026-08-16: `docs/reference/std.md` — 두 표에 8개 항목 추가.
- 2026-08-16: `tests/integration.rs` — `runtime_std_new_combinators` 추가:
  std 모듈과 함께 tsc `--strict`(nodenext) 타입체크 후 node로 실행해 12개
  출력 값을 검증 (비동기 `fromPromise`의 resolve/reject 경로 포함).
- 검증: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`.

## 이슈 및 해결

### 이슈 1: 제네릭 추론이 약한 호출 형태

- **증상**: `Result.flatten(Result.Ok(Result.Ok(4)))`처럼 바깥 생성자의 `E`가
  추론 사이트 없이 `unknown`으로 남아 내부 `E`와 충돌할 수 있음.
- **원인**: `Ok: <T, E>(value: T)`의 `E`는 인자에서 추론되지 않는다 (기존
  API의 알려진 성질).
- **해결**: 콤비네이터 자체는 문제 없고 호출부에서 명시가 필요한 경우가
  있다는 것이므로, 통합 테스트에 명시적 타입 인자를 쓰는 예를 포함해 계약을
  고정했다. API 변경 없음.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`

## 결과

변경 파일: `src/stdlib/rl_std.ts`, `docs/reference/std.md`,
`tests/integration.rs`. 커링 버전 콤비네이터는 파이프라인 연산자(TASK-013)
구현 시점에 함께 다룬다.
