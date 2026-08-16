# 컴파일러 아키텍처 — 단계 분리 파이프라인

이 문서는 rlc 내부 구조의 규범 설명이다. TASK-010에서 단일 패스 구조를
swc 스타일의 단계 분리 파이프라인으로 재구성했다. 모듈 배치가 이 문서와
어긋나면 버그로 취급한다. (이전 구조의 역사적 배경은
[`rust-rewrite.md`](./rust-rewrite.md) 참조 — 그 문서의 모듈 배치 설명은
이 문서가 대체한다.)

## 왜 단계를 분리했나

초기 구현은 한 번의 바이트 스캔 루프 안에서 파싱·의미 검사·코드 방출을
동시에 수행했다. 구현은 작았지만, 기능을 추가할 때마다 세 관심사를 한
함수에서 같이 건드려야 했고, 에러 전파(`Result`)가 파싱·방출 전체에
퍼져 있었다. swc가 `swc_ecma_ast` / `swc_ecma_parser` /
`swc_ecma_transforms` / `swc_ecma_codegen`으로 단계를 나누듯, rlc도
단계마다 독립 모듈을 두고 단계 간 계약을 타입드 AST로 명시한다.

## 파이프라인

```
소스 텍스트
   │  parser::parse          — 무오류(infallible) 구조 파싱
   ▼
ast::Program                 — 단계 간 계약
   │  sema::check            — 모든 rl 수준 에러 (Result<(), RlError>)
   ▼
ast::Program (검증됨)
   │  codegen::emit          — 무오류 방출
   ▼
TypeScript 텍스트
   │  verify::verify_output  — swc 파싱 자가 검사 (--no-verify로 생략)
   ▼
최종 출력
```

### 1. `ast` — 단계 간 계약

파싱된 파일은 `Program` = 소스 순서의 `Segment` 목록이다:

- `Verbatim(Span)` — rl 구문이 아닌 모든 것. 원본 바이트 범위 그대로.
- `Enum(EnumDecl)` / `Match(MatchExpr)` / `Try(TryStmt)` /
  `LetElse(LetElseStmt)` — 완전하게 파싱된 rl 구문.
- `RlImport(Span)` — 정적 import/re-export의 상대 경로 `.rl` 지정자 문자열
  (따옴표 포함). 문장의 나머지는 verbatim으로 남고, codegen이
  `ImportRewrite` 모드에 따라 확장자를 재작성한다.
- `Template(Template)` — 템플릿 리터럴. 보간(`${ }`)마다 재귀 `Program`.

match의 scrutinee와 arm body도 재귀 `Program`이라 트리가 균일하다. 모든
Span/오프셋은 원본 소스의 절대 바이트 위치다 — 이것이 의미 에러를
`파일:행:열`로 되돌리는 연결 고리다.

### 2. `parser` — 무오류 구조 파싱

파서는 **에러를 내지 않는다**. 구문이 완전하게 파싱될 때만 AST 노드로
들어올리고, 조금이라도 어긋나면 그 후보를 verbatim으로 남긴다. "유효한
TS는 바이트 그대로 통과" 계약이 여기서 구현된다: 구문 여부는 순수하게
구조적 판단이고, rl 수준 *에러*(중복 케이스 등)는 전부 sema의 몫이다.

저수준 스캔(문자열/주석/정규식/괄호 매칭)은 기존 `scanner.rs`를 그대로
사용한다. TS enum 구분 규칙(payload 케이스 또는 제네릭이 있어야 rl enum)과
`const enum`/`declare enum` 제외, 예약어 규칙도 파서 소관이다.

### 3. `sema` — 의미 검사

AST를 소스 순서로 깊이 우선 순회하며(노드 자체 규칙 → 자식 순),
첫 위반을 바이트 오프셋과 함께 `RlError`로 보고한다:

- enum: 중복 케이스 금지; 검증 활성 시 필드 타입이 TS 타입 조각으로
  파싱되는지(swc) 검사.
- match: 와일드카드 `_`는 마지막 arm; 중복 arm 금지.
- 소진성: 순회 중 수집한 enum 레지스트리로 순회 **종료 후** 해결한다
  (match가 enum 선언보다 앞서도 무관). 알 수 없는 태그의 match는 검사하지
  않는다 — rlc에 타입 정보가 없다.

에러 계층 계약이 여기서 지켜진다: 모든 rl 수준 에러는 sema가 직접 보고하고,
tsc에 위임하지 않는다.

### 4. `codegen` — 무오류 방출

sema를 통과한 AST에서 텍스트로의 순수 매핑이다. verbatim 구간은 원본
바이트를 그대로 복사하고, enum은 유니언 `type` + 생성자 `const`로, match는
`switch` IIFE로 방출한다(코드 형태의 규범은
[`../reference/language.md`](../reference/language.md)). `await` 감지는
AST에 남긴 원시 Span 위로 `scanner::contains_await`를 돌려 수행한다.

## 기능 추가 가이드

| 변경 종류 | 손대는 단계 |
|-----------|-------------|
| 새 구문 | `ast`에 노드 추가 → `parser`에 구조 파싱 → `codegen`에 방출 (+ sema 검사 필요 시) |
| 새 의미 규칙/에러 | `sema`만 |
| 방출 코드 형태 변경 | `codegen`만 (+ `docs/reference/language.md` 갱신) |
| 새 토큰 수준 인식 | `scanner` |

어느 경우든 CLAUDE.md의 세 계층 테스트(compile / passthrough /
integration)와 레퍼런스 문서 갱신 규칙을 따른다.

## 재구성의 등가성 검증

이 재구성은 언어 표면을 바꾸지 않았다. 기존 테스트 전부(단위·계약·통합
59개 + doctest 4개) 통과에 더해, 재구성 전(HEAD) 바이너리와 후
바이너리를 22개 샘플 × (`-p`,
`-p --no-verify`)로 비교해 출력·에러 메시지·종료 코드가 바이트 단위로
동일함을 확인했다 (TASK-010 기록 참조).
