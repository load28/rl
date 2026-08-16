# TASK-011: Option/Result 표준 라이브러리와 내장 enum 소진성 검사

- **상태**: 진행 중
- **시작일**: 2026-08-16
- **완료일**: —
- **커밋**: —

## 목적

Rust처럼 `Option<T>`/`Result<T, E>` 기반의 함수형 프로그래밍을 rl에서 바로 할
수 있게 한다. 사용자가 매 파일마다 enum을 다시 선언하거나 콤비네이터
(map/andThen/unwrapOr 등)를 손으로 작성하지 않아도 되도록, 컴파일러가 표준
라이브러리 모듈을 제공하고 두 타입을 내장 enum으로 인식한다.

## 범위

- 포함:
  - `rlc --emit-std <파일>` — `Option`/`Result` 타입·생성자·함수형
    콤비네이터가 담긴 순수 TypeScript 표준 라이브러리 모듈 방출.
  - 라이브러리 공개 API `rlc::STD_SOURCE` (모듈 소스 문자열).
  - sema 소진성 검사가 `Option`(Some/None), `Result`(Ok/Err)를 내장 enum으로
    인식 (같은 이름의 파일 로컬 rl enum이 있으면 로컬이 우선).
  - 세 계층 테스트(compile/passthrough/integration) + std 전용 계약 테스트.
  - 레퍼런스 문서 갱신: `std.md` 신설, `language.md`/`cli.md`/`errors.md`,
    README, CHANGELOG, CLAUDE.md 아키텍처 맵.
- 제외:
  - 새 구문 추가 없음 (파서/AST/코드젠 변경 없음). `?` 연산자, if-let 등
    Rust의 다른 편의 구문은 다루지 않는다.
  - std 모듈 자동 import/자동 주입 없음 — import는 사용자가 명시한다
    (통과 계약 위반 위험, 아래 결정 2).
  - 프로젝트 단위(파일 간) 소진성 검사는 여전히 로드맵.

## 의사결정

### 결정 1: Option/Result를 "표준 라이브러리 모듈 + 내장 enum 인식"으로 제공

- **상황**: Option/Result를 언어 차원에서 제공하는 방법을 정해야 했다.
- **검토한 대안**:
  - A. **사용자가 직접 선언** (현상 유지): 제네릭 enum이 이미 있어
    `enum Option<T> { Some(value: T), None }`은 오늘도 동작한다. 하지만 매
    파일 재선언(파일 단위 소진성 검사 때문)과 콤비네이터 부재로 함수형
    스타일이 번거롭다.
  - B. **컴파일러가 선언을 자동 주입**: 통과 영역(순수 TS)에서 `Option` 사용을
    감지해 선언을 주입. 감지가 휴리스틱이고, 자기 `Option`을 import하는 순수
    TS 파일에 선언이 주입되면 "유효한 TS는 바이트 그대로 통과" 계약(설계
    계약 1)이 깨진다.
  - C. **표준 라이브러리 모듈 방출 + 내장 enum 인식** (선택): std 모듈은
    사용자가 명시적으로 import하는 순수 TS 파일이므로 통과 계약이 그대로
    유지되고, 콤비네이터도 자연스럽게 담을 수 있다. 소진성 검사는 sema의
    기존 태그 집합 추론에 내장 enum 두 개를 등록하는 것으로 충분하다.
- **선택과 근거**: C. 계약 위반 없이(passthrough.rs로 확인 가능) 함수형
  콤비네이터와 소진성 검사를 모두 제공하는 유일한 안이다. rl의 철학
  ("TS + 최소 추가, 나머지는 그대로")과도 일치한다.

### 결정 2: std 모듈의 선언 형태는 rl enum 방출 결과와 바이트 단위로 동일하게

- **상황**: std 모듈 안의 `Option`/`Result` 선언을 어떤 형태로 쓸지.
- **검토한 대안**: 자유로운 손글씨 TS (클래스, 메서드 체이닝 등) vs. rl enum이
  컴파일되는 형태(`kind` 태그드 유니언 + 생성자 const)와 동일하게 유지.
- **선택과 근거**: 동일 형태 유지. `match`가 `kind` 필드 기반이므로 std 값에
  match가 그대로 동작해야 하고, 사용자가 직접 선언하던 코드와 형태가 같아야
  마이그레이션이 무손실이다. 드리프트는 테스트로 방지 —
  `tests/stdlib.rs::std_declarations_match_rl_enum_emission`이 enum 선언을
  실제로 컴파일한 결과의 모든 줄이 `STD_SOURCE`에 그대로 존재하는지 검사한다.

### 결정 3: 콤비네이터는 데이터-우선 정적 함수 (`Option.map(o, f)`)

- **상황**: 콤비네이터 API 형태 결정 (메서드 체이닝 vs 정적 함수).
- **검토한 대안**:
  - 프로토타입/클래스 메서드 체이닝(`o.map(f).unwrapOr(x)`): 읽기 좋지만 값이
    더 이상 순수 데이터 객체가 아니게 되어 `match`/`JSON.stringify`/구조적
    타이핑과 충돌하고, rl enum 방출 형태와 달라진다.
  - 데이터-우선 정적 함수(`Option.map(o, f)`): 값은 순수 `kind` 태그드
    객체로 유지되고, 생성자 객체 네임스페이스에 자연스럽게 얹힌다.
- **선택과 근거**: 데이터-우선. rl의 값 표현(순수 데이터)과 형태 계약을
  유지하는 쪽이 우선이다. 본문은 `Option`/`Result` 자기 참조 없이 리터럴을
  직접 반환해 순환 타입 추론 이슈도 원천 차단했다 (tsc --strict로 확인).

### 결정 4: 내장 enum은 같은 이름의 로컬 rl enum이 있으면 물러난다 (섀도잉)

- **상황**: 파일에 `enum Option { ... }`을 직접 선언한 기존 코드와의 충돌.
- **검토한 대안**: 내장이 항상 우선 / 로컬이 항상 우선 / 에러.
- **선택과 근거**: 로컬 우선. 기존에 동작하던 코드(직접 선언)가 의미 변화
  없이 계속 동작해야 한다. 구현은 sema의 소진성 해석 시 로컬 레지스트리에
  이름이 있으면 해당 내장을 후보에서 제외하는 것뿐이다. 내장에 걸린 에러는
  `match on built-in enum ...`으로 구분해 보고한다.
- **알려진 트레이드오프**: 손으로 쓴 유니언이 `Some`/`None`/`Ok`/`Err` 태그의
  진부분집합만 커버하는 `_` 없는 match는 이제 검사에 걸린다. `_` 암을 두거나
  태그를 바꾸면 된다 — language.md에 명시.

## 작업 내역

- 2026-08-16: 코드베이스 조사 — sema의 소진성 추론(태그 집합 → 후보 enum),
  codegen의 enum 방출 형태, CLI 구조, 통과 계약 테스트 확인. 위 결정 1~4 확정.
- 2026-08-16: `src/stdlib/rl_std.ts` 작성 (std 모듈 본체), `src/stdlib.rs`
  신설 (`STD_SOURCE` 공개 상수 + `BUILTIN_ENUMS`), `src/lib.rs`에 재수출.
- 2026-08-16: `src/sema.rs` — `check_exhaustiveness`가 로컬 enum 뒤에 내장
  enum(섀도잉 제외)을 후보로 추가, 내장이면 `built-in enum` 문구로 보고.
- 2026-08-16: `src/main.rs` — `--emit-std <file>` 옵션 (배너 포함, 단독 사용
  가능, 입력과 병행 가능).
- 2026-08-16: 테스트 추가 — `tests/stdlib.rs` (통과+검증 계약, 방출 형태
  일치), `tests/compile.rs` (내장 소진성 에러/만족/섀도잉/와일드카드),
  `tests/passthrough.rs` (Some/None을 쓰는 순수 TS 통과 불변),
  `tests/integration.rs` (std import + match + 콤비네이터 tsc/node 실행).
- 2026-08-16: 문서 — `docs/reference/std.md` 신설, `language.md`에 §4(내장
  enum과 표준 라이브러리) 추가 및 §5/§6 재번호, `cli.md`/`errors.md`/README/
  CHANGELOG/CLAUDE.md 갱신.
- 2026-08-16: 검증 게이트 3종 실행 및 통과 확인, 커밋·푸시.

## 이슈 및 해결

### 이슈 1: tsc 6.x가 확장자 없는 상대 import를 nodenext에서 거부

- **증상**: 통합 테스트 초안에서 `import ... from "./rl"`이
  `TS2835: Relative import paths need explicit file extensions` 실패.
- **원인**: 테스트가 node 실행까지 하므로 `--module nodenext`를 쓰는데,
  nodenext 해석은 ESM에서 명시적 `.js` 확장자를 요구한다.
- **해결**: 테스트와 문서 예시 모두 `from "./rl.js"`로 통일 (tsc가 `rl.ts`로
  매핑, node는 방출된 `rl.js`를 로드). 번들러 환경 사용자는 `./rl`도 가능
  하다고 std.md에 병기.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` (tsc 6.0.2 / node v22 환경 — 통합 테스트 포함 71개 통과)

## 결과

- 새 파일: `src/stdlib.rs`, `src/stdlib/rl_std.ts`, `tests/stdlib.rs`,
  `docs/reference/std.md`.
- 수정: `src/lib.rs`(STD_SOURCE 재수출), `src/sema.rs`(내장 enum 소진성),
  `src/main.rs`(--emit-std), `tests/{compile,passthrough,integration}.rs`,
  `docs/reference/{language,cli,errors}.md`, `README.md`, `CHANGELOG.md`,
  `CLAUDE.md`.
- 후속 후보: 프로젝트 단위 소진성 검사(기존 로드맵), std 콤비네이터 확장
  (`zip`, `transpose` 등)은 수요가 생기면 새 태스크로.
