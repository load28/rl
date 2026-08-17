# CLAUDE.md — rl 프로젝트 작업 가이드

이 문서는 Claude(및 모든 기여자)가 이 저장소에서 작업할 때 반드시 따라야 하는
규칙과 컨텍스트를 정의합니다.

## 프로젝트 개요

**rl**은 TypeScript로 컴파일되는 초경량 전처리 언어이고, **rlc**는 Rust로 작성된
그 컴파일러입니다. rl은 TypeScript 위에 딱 여섯 가지만 추가합니다:
Rust 스타일 `enum`(태그드 유니언), `match` 표현식(or-패턴·가드·튜플 match·
중첩 패턴 포함), 에러 전파 `try` 문, 값 추출 `let-else`·`if let` 문,
파이프라인 연산자 `|>`.

### 절대 불변 원칙 (설계 계약)

이 두 가지는 어떤 리팩토링/기능 추가에서도 깨뜨릴 수 없는 계약입니다:

1. **모든 유효한 TypeScript 파일은 그대로 유효한 `.rl` 파일이다.**
   컴파일러는 rl `enum`/`match` 구문만 변환하고 나머지는 바이트 단위 그대로
   통과시킨다. 구문이 완전하게 파싱될 때만 변환하고, 조금이라도 어긋나면 원문
   그대로 통과시킨다. 유일한 예외는 상대 경로 `.rl` import 지정자의 재작성
   (TASK-020, `language.md` §8)이다 — 그런 지정자는 tsc가 어차피 해석하지
   못하므로(`TS2307`) 동작하던 TS가 달라지는 일은 없고, `--rewrite-imports
   off`로 끌 수 있다. 이 밖의 예외를 추가로 만들지 않는다.
2. **에러 계층이 분리되어 있다.** rl 수준 에러(중복 케이스, 소진되지 않은 match,
   잘못된 필드 타입)는 전부 rlc가 `파일:행:열`과 함께 직접 보고한다. 생성되는
   코드는 타입 트릭 없는 순수 TypeScript이며, rlc가 방출한 코드 때문에 tsc
   에러가 발생해서는 안 된다. 사용자가 통과 영역에 쓴 TS 코드의 타입 에러는
   tsc의 책임이다.

## 아키텍처 맵

```
src/
  main.rs        CLI 진입점 — 인자 파싱, 파일 수집, 컴파일 실행/출력
  lib.rs         공개 API: compile(source, &Options) -> Result<String, CompileError>
  error.rs       CompileError(공개) / RlError(내부, 바이트 오프셋) / line_col
  scanner.rs     바이트 단위 저수준 스캔 (문자열/템플릿/주석/정규식/괄호 매칭)
  lexer.rs       유의 토큰 스트림 생성 (정규식 휴리스틱, 템플릿 중첩 렉싱)
  ast.rs         타입드 AST — 단계 간 계약 (Program/Segment/EnumDecl/MatchExpr...)
  parser/
    mod.rs       메인 토큰 루프 → Program (무오류 구조 파싱, 템플릿 재귀)
    cursor.rs    토큰 커서 (Copy 백트래킹, 괄호 매칭, 공용 토큰 스캔)
    enums.rs     rl enum 구조 파싱 (TS enum 구분 규칙 포함)
    imports.rs   정적 import/re-export의 상대 경로 .rl 지정자 추출
    matches.rs   match 표현식 구조 파싱 (scrutinee/arm body 재귀 파싱)
    tries.rs     try 문 구조 파싱 (유효 TS의 try 형태 배제 규칙 포함)
    lets.rs      let-else 문 구조 파싱 (발산 판정 포함)
    iflets.rs    if let 문 구조 파싱 (else 체이닝 포함)
    pipes.rs     파이프라인 스텝 구조 파싱 (head는 mod.rs의 식-시작 추적)
  sema.rs        의미 검사 — 중복 케이스/암, 와일드카드 위치, 필드 타입, 소진성
                 (임포트 선언·내장 Option/Result 포함, 로컬 > 임포트 > 내장 섀도잉)
  stdlib.rs      표준 라이브러리 — STD_SOURCE(공개) / BUILTIN_ENUMS(내부)
  stdlib/
    rl_std.ts    std 모듈 본체 (Option/Result + 콤비네이터, --emit-std로 방출)
  codegen/
    mod.rs       Program → TypeScript 방출 (verbatim 구간은 바이트 그대로 복사,
                 try/let-else 문·파이프라인($rl_ap 헬퍼) 방출 포함)
    enums.rs     enum 방출 (유니언 type + 생성자 const)
    matches.rs   match 방출 (switch IIFE)
  verify.rs      swc 기반 검증 — 타입 조각 검사 + 출력 자가 검사
tests/
  compile.rs     컴파일 출력 스냅샷/에러 단위 테스트
  passthrough.rs "유효한 TS는 바이트 그대로 통과" 계약 테스트
  stdlib.rs      std 모듈 계약 테스트 (통과 + rl enum 방출 형태와 바이트 일치)
  integration.rs tsc 타입체크 + node 실행 통합 테스트 (tsc/node 없으면 skip)
docs/
  reference/     규범 레퍼런스 — language.md(언어) / cli.md / errors.md / std.md
  design/        설계 문서 (compiler-architecture.md — 파이프라인 규범 설명)
  tasks/         태스크 관리 (아래 참조)
```

파이프라인 (swc 스타일 단계 분리): `compile()` = parser::parse(lexer::lex
토큰화 → 무오류 구조 파싱 → AST) → sema::check(모든 rl 수준 에러 + 소진성) →
codegen::emit(무오류 방출) → verify_output(swc 파싱 자가 검사, `--no-verify`로
생략 가능).
새 기능은 해당 단계에만 손댄다: 새 구문 = ast + parser(+codegen), 새 검사 =
sema, 방출 형태 변경 = codegen.

## 명령어

```sh
cargo build                        # 빌드
cargo test                         # 전체 테스트 (tsc/node 있으면 통합 테스트 포함)
cargo fmt --check                  # 포매팅 검사 (rustfmt 기본 스타일)
cargo clippy --all-targets -- -D warnings   # 린트 (경고 = 실패)
cargo run -- file.rl               # 컴파일 실행
```

## 검증 게이트 (머지 전 필수)

모든 변경은 커밋 전에 아래를 통과해야 합니다:

1. `cargo fmt --check`
2. `cargo clippy --all-targets -- -D warnings`
3. `cargo test`

CI(`.github/workflows/ci.yml`)가 동일한 게이트를 강제합니다.

## 태스크 관리 규칙 (필수)

**이 저장소의 모든 작업은 태스크 문서로 관리되고 기록되어야 합니다.**

- 단일 진실 소스: **`docs/tasks/INDEX.md`** — 전체 태스크 목록과 상태.
- 개별 태스크 문서: `docs/tasks/TASK-NNN-<slug>.md`
  (`docs/tasks/TEMPLATE.md` 템플릿 사용).

### 워크플로

1. **작업 시작 전**: `INDEX.md`에서 다음 태스크 번호를 확인하고, 템플릿을 복사해
   태스크 문서를 만들고 목적/범위를 기록한 뒤 INDEX에 `진행 중`으로 등록한다.
2. **작업 중**: 아래 "기록 상세 기준"에 따라 의사결정·작업 내역·이슈를
   태스크 문서에 남긴다.
3. **작업 완료 시**: 검증 게이트 통과 결과와 변경 파일 요약을 기록하고,
   상태를 `완료`로 바꾸고 완료일과 커밋 해시를 기입한다.
4. **커밋 메시지**는 해당 태스크 ID로 시작한다: `TASK-004: split transform into modules`.

### 기록 상세 기준 (필수)

태스크 문서는 나중에 처음 보는 사람이 "왜 이렇게 됐는지"를 문서만 읽고 재구성할
수 있을 만큼 상세해야 한다. 템플릿의 세 섹션을 모두 채운다:

- **의사결정**: 유의미한 결정마다 ① 어떤 선택이 필요했는지(상황),
  ② 검토한 대안들과 각각의 장단점, ③ 최종 선택과 그 근거를 기록한다.
  근거가 확인 가능한 것이면 확인 방법(명령, 측정치)까지 남긴다.
- **작업 내역**: 실제 수행한 작업을 시간순으로, 어떤 파일을 어떻게 바꿨고
  어떤 명령으로 확인했는지 재현 가능할 만큼 구체적으로.
- **이슈 및 해결**: 만난 문제마다 증상(에러 메시지 포함) → 원인(조사 과정 포함)
  → 해결 방법을 기록한다. 우회한 경우 사유와 남은 부채를 명시한다.
  이슈가 없었다면 "없음"이라고 명시적으로 적는다 (누락과 구분하기 위해).

### 상태 값

| 상태 | 의미 |
|------|------|
| 대기 | 등록됐지만 시작 전 |
| 진행 중 | 작업 중 |
| 완료 | 검증 게이트 통과 + 기록 완료 |
| 보류 | 의사결정 대기 / 차단됨 (사유를 문서에 기록) |
| 취소 | 진행하지 않기로 결정 (사유를 문서에 기록) |

## 코딩 컨벤션

- 포매팅: **rustfmt 기본값** (커스텀 설정 없음). 커밋 전 `cargo fmt`.
- 린트: clippy 경고 0개 유지. lint 설정은 `Cargo.toml`의 `[lints]`에 선언.
- `unsafe` 금지 (`#![forbid(unsafe_code)]` 수준으로 유지).
- 스캐너/변환기는 바이트 기반: ASCII 바이트로만 판단하고 멀티바이트 UTF-8은
  불투명하게 통과시킨다는 전제를 지킬 것.
- 에러는 반드시 바이트 오프셋을 담아 `RlError::at`으로 만들고, 위치 변환은
  `compile()` 경계에서만 한다.
- 새 기능에는 반드시 세 계층 테스트를 추가: 출력 단위 테스트(compile.rs),
  통과 계약이 걸리면 passthrough.rs, 타입/런타임 의미가 걸리면 integration.rs.
- **언어 표면(구문, 판별 규칙, 방출 코드, 에러 메시지, CLI 동작)을 바꾸는
  변경은 반드시 `docs/reference/`의 해당 문서를 함께 갱신한다.** 레퍼런스는
  규범 문서다 — 구현과 어긋나면 버그로 취급한다.
