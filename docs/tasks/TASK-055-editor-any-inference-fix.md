# TASK-055: 에디터에서 `Option`/`Result`·파이프라인이 `any`로 추론되는 문제

- **상태**: 완료
- **시작일**: 2026-08-18
- **완료일**: 2026-08-18
- **커밋**: `46a5c5c`

## 목적

사용자 보고: 아래 코드에서 `.trim()`의 리턴 타입이 `any`로 보이고, `Result`·
`Option` 등 rl 표준 라이브러리 값이 전부 `any`로 추론된다. 타입이 안 잡혀서
`ok`/`okPair`/`errPair` 같은 주석용 헬퍼 함수를 억지로 만들어야 했다.

```rl
export function calculate(input: string): Result<number, CalcError> {
  return input
    |> .trim()
    |> tokenize
    |> Result.andThenP(parse)
    |> Result.andThenP(evaluate);
}
```

## 범위

- 포함: 원인 규명(컴파일러 출력 vs 에디터), 언어 서버(`editors/vscode/server`)의
  타입 해석 경로 수정, 회귀 테스트, 확장 README 갱신.
- 제외: 컴파일러(`src/`) 변경. 방출 코드·`--types` 사이드카는 이번 조사에서
  정상으로 확인됐다. `tsconfig` 없이 `tsc`를 돌리는 프로젝트 설정 문제도 제외
  (레퍼런스 `cli.md` §타입 생성이 이미 규범).

## 의사결정

### 결정 0: 먼저 "컴파일러 문제인가 에디터 문제인가"를 분리한다

- **상황**: 보고는 "타입이 any"라는 증상만 있고, 원인이 방출 코드일 수도
  에디터일 수도 있었다. 방출 코드 문제라면 설계 계약 2번(rlc가 방출한 코드
  때문에 tsc 에러가 나면 안 된다) 위반이므로 우선순위가 완전히 달라진다.
- **검토한 대안**: (A) 보고된 파일만 보고 추정한다. (B) 보고 코드를 다중 파일
  프로젝트로 재현해 `rlc` → `tsc --strict`까지 돌린다.
- **선택과 근거**: (B). `calc.rl`/`parser.rl`/`evaluate.rl`/`error.rl`을 만들어
  `rlc <src> -o out` 후 `tsc --noEmit --strict`로 검사 → **에러 0개**. 헬퍼
  없이 쓴 `Result.Ok([a, b])`도 `[number, number]`로 정확히 추론됐다(match
  IIFE를 통해 컨텍스트 타입이 전달됨). 즉 방출 코드는 정상이고 증상은 에디터
  전용이라는 것이 측정으로 확정됐다.

### 결정 1: `"@rl/std"`를 언어 서비스가 프로젝트 설정 없이도 해석하게 한다

- **상황**: `TsProject`의 `COMPILER_OPTIONS`는 하드코딩이라 사용자의
  `tsconfig.json`(`paths: { "@rl/std": ["./.rl-types/rl.d.ts"] }`)을 읽지 않는다.
  그래서 `.rl` 버퍼 안에서 `@rl/std`는 **항상** 미해석 → `Result`/`Option`이
  전부 `any`. 보고의 핵심 증상이다.
- **검토한 대안**:
  - (A) 워크스페이스 `tsconfig.json`을 읽어 `paths`/`baseUrl`을 반영한다.
    정석이지만 사용자가 `rlc --types`를 돌리고 tsconfig까지 설정해야 동작한다
    — 설정 전에는 여전히 `any`. tsconfig 탐색·확장(extends)·프로젝트별 캐시
    무효화까지 서버가 떠안게 된다.
  - (B) 표준 라이브러리는 컴파일러 소유물이므로, 미해석일 때 컴파일러 자신의
    모듈(`rlc --emit-std`)을 임시 디렉터리에 실체화해 그 파일로 해석한다.
    설정 0으로 동작하고, 타입 정의가 컴파일러 버전과 항상 일치한다.
  - (C) 둘 다: 표준 해석을 먼저 시도하고 실패할 때만 (B)로 폴백.
- **선택과 근거**: (C). 프로젝트가 스스로 해석하면(=`paths`가 실제로 잡히는
  구성이거나 디스크에 패키지가 있으면) 그쪽을 존중하고, 아니면 컴파일러 사본이
  들어간다. `ts.resolveModuleName`을 먼저 호출하고 `resolvedModule`이 없을
  때만 폴백하는 방식이라 구현도 몇 줄이다. 실체 파일로 만들기 때문에 std
  심볼의 "정의로 이동"도 실제 파일로 열린다.

### 결정 2: import한 `.rl` 모듈도 방출물로 서빙한다

- **상황**: 언어 서비스는 열린 버퍼만 가상 문서로 서빙하고, import된 `.rl`은
  디스크 원문을 그대로 읽었다. TS는 `enum Expr { Num(value: number) }`를
  TS `enum`으로 오류 복구하므로, 하니스로 확인하면 `expr: Expr`의 타입이
  `(alias) enum Expr`로 나오고 match 암 안에서 `Property 'left' does not exist
  on type 'Number'` 류의 잘못된 의미가 생긴다(진단은 표출되지 않지만 호버·
  완성 타입이 오염된다).
- **검토한 대안**:
  - (A) 그대로 둔다 — TS 오류 복구에 기댄다.
  - (B) 서버가 import 그래프를 추적해 비동기로 미리 컴파일해 캐시에 채운다.
  - (C) 언어 서비스 호스트가 `.rl` 파일을 읽을 때 동기로 `rlc --emit-map`을
    돌리고 mtime 기준으로 캐시한다.
- **선택과 근거**: (C). 언어 서비스 호스트의 `readFile`은 **동기**라 (B)는
  "아직 안 채워졌으면 원문" 상태가 남고 결국 같은 오염이 재발한다. (C)는
  import당 최대 1회 프로세스 실행이고 mtime이 바뀔 때만 재실행이라 비용이
  한정적이다. 저장 전 편집본은 열린 버퍼 경로가 먼저 잡히므로 캐시가
  오래되는 창도 없다.
- **부수 요구**: 서빙 텍스트가 방출물로 바뀌므로, 그 파일로 간 정의/참조/
  이름변경 결과는 **디스크 파일의 매핑을 통해 원본 좌표로 되돌려야** 한다.
  `fromServiceOffset`이 열린 버퍼뿐 아니라 디스크 `.rl`의 `MappedDoc`도
  거치도록 고쳤다(매핑 없는 위치는 기존 규칙대로 답하지 않음).

### 결정 3: 호버·완성·정의 요청은 최신 방출물을 기다린다

- **상황**: 가상 문서는 진단과 같은 300ms 디바운스로 갱신되고
  `activeVirtual`은 **버전 정확 일치**를 요구한다. 즉 타이핑 직후의 요청은
  원문 `.rl`로 답하게 되는데, TS는 `|>`를 파싱하지 못하므로 파이프라인 식
  전체가 에러 노드가 되어 `.trim()`조차 `any`가 된다. 보고의 "trim이 any"가
  정확히 이 경로다.
- **검토한 대안**:
  - (A) 오래된 가상 문서라도 계속 쓴다 — 오프셋이 어긋나 **틀린** 답을 준다.
    "답 없음"보다 나쁘다.
  - (B) 디바운스를 줄인다 — 근본 해결이 아니고 컴파일 빈도만 올린다.
  - (C) 요청 시점에 방출을 한 번 돌려 기다린다(같은 버전의 동시 요청은 하나의
    프로미스를 공유).
- **선택과 근거**: (C). 호버·완성·정의는 사용자 조작으로만 발생하고 빈도가
  낮으며, `rlc --emit-map`은 프로세스 1회 실행이다. (A)의 잘못된 좌표 리스크가
  없고 (B)처럼 상시 비용을 올리지도 않는다. 컴파일러가 없으면 기존대로 원문
  폴백이 그대로 남는다.

## 작업 내역

- 2026-08-18: 보고 코드를 다중 파일로 재현(`calc.rl`/`parser.rl`/`evaluate.rl`/
  `error.rl`) → `rlc -o out` → `tsc --noEmit --strict`로 **에러 0** 확인.
  `rlc --types`로 사이드카도 정상(`export declare function calculate(input:
  string): Result<number, CalcError>`) 확인. 컴파일러 무혐의 확정.
- 2026-08-18: `editors/vscode/server/src/tsproject.ts`의 언어 서비스 호스트를
  그대로 재현한 하니스(node + typescript)로 호버 타입 측정. 결과:
  가상 문서 서빙에서 `.trim()`은 `String.trim(): string`이지만
  `Result.andThenP`는 `any`; 원문 서빙에서는 `.trim()`까지 `any`. import한
  `.rl`을 원문으로 서빙하면 `expr: Expr`가 `(alias) enum Expr`로 나옴.
  세 가지를 모두 고친 하니스에서는 `Result.andThenP`가
  `<string[], CalcError, Expr>(f: ...) => ...`로 완전히 인스턴스화되고
  의미 진단 0개.
- 2026-08-18: `rlc.ts`에 동기 호출 두 개 추가 — `stdModulePath()`
  (`rlc --emit-std`를 임시 디렉터리에 실체화, 컴파일러 경로별 캐시),
  `runEmitMapFileSync()`(디스크 `.rl`의 방출 + 매핑).
- 2026-08-18: `tsproject.ts` — 생성자에 `getStdModule` 훅 추가,
  `resolveModuleNameLiterals`에서 `"@rl/std"`를 표준 해석 우선 + 폴백으로 처리.
- 2026-08-18: `server.ts` — 디스크 `.rl` 가상 문서 캐시(`diskVirtuals`, mtime
  키)와 `currentCompiler()`/`servedCompiler` 도입, 언어 서비스 문서 공급자가
  디스크 `.rl`도 방출물로 넘기도록 확장, `fromServiceOffset`이 디스크 매핑으로
  역변환하도록 수정, `ensureVirtual()`을 추가해 완성·호버·정의·참조·이름변경
  핸들러가 최신 방출물을 기다리도록 함(`onReferences`는 async로 전환).
- 2026-08-18: `test/emitmap.test.ts`에 회귀 테스트 3개 추가 — 파이프라인의
  std 스텝이 `any`가 아님, 후위 스텝(`.trim()`)이 리시버 타입을 유지함,
  import한 `.rl` 모듈이 방출물로 서빙됨. `npm test`(rlc를 PATH에) → 53/53 통과
  (기존 50개 중 10개는 rlc 없으면 skip되던 것들).
- 2026-08-18: `editors/vscode/README.md`의 TS 위임 설명을 새 동작으로 갱신.

## 이슈 및 해결

### 이슈 1: `execFileSync` 옵션 리터럴이 타입 체크에 걸림

- **증상**: `npx tsc -b`에서 `error TS2769: No overload matches this call. ...
  Type 'readonly ["ignore", "pipe", "ignore"]' is not assignable to type
  'StdioOptions | undefined'`.
- **원인**: 공용 옵션 상수를 `as const`로 만들어 `stdio` 배열이 `readonly`가
  됐는데, `execFileSync`의 오버로드는 가변 배열을 요구한다.
- **해결**: 상수의 타입을 `ExecFileSyncOptionsWithStringEncoding`으로 명시하고
  `as const`를 뺐다.

### 이슈 2: 새 테스트가 서버 글루(server.ts)를 직접 검증하지 못함

- **증상**: `server.ts`는 import 시점에 `createConnection`을 호출하므로 단위
  테스트에서 불러올 수 없다.
- **원인**: LSP 진입점 모듈 구조상 부작용이 모듈 로드에 묶여 있다.
- **해결**: 기존 `emitmap.test.ts`와 같은 방식으로, 서버가 하는 배선을
  테스트에서 재현해 `TsProject`·`rlc` 계층을 검증했다(`stdModulePath` 훅
  주입, 디스크 모듈을 `runEmitMapFileSync`로 방출해 공급). server.ts의 배선
  자체는 수동 확인 + 타입 체크에 의존한다 — 남은 부채로 기록한다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`
- [x] `npx tsc -b` (editors/vscode)
- [x] `npm test` (editors/vscode, rlc를 PATH에 두고 53/53 통과)

## 결과

에디터에서 `.rl` 버퍼의 타입 추론이 방출 코드와 같은 수준으로 올라왔다:
`@rl/std`가 설정 없이 해석되고, import한 `.rl` 모듈이 방출물로 서빙되며,
호버·완성·정의 요청이 최신 방출물을 기다린다. 보고된 파이프라인 코드에서
`ok`/`okPair`/`errPair` 같은 주석용 헬퍼는 더 이상 필요 없다.

변경 파일:

- `editors/vscode/server/src/rlc.ts` — `stdModulePath`, `runEmitMapFileSync`
- `editors/vscode/server/src/tsproject.ts` — `"@rl/std"` 해석 폴백 훅
- `editors/vscode/server/src/server.ts` — 디스크 `.rl` 가상 문서, 역매핑,
  `ensureVirtual`
- `editors/vscode/server/src/test/emitmap.test.ts` — 회귀 테스트 3개
- `editors/vscode/README.md` — 동작 설명 갱신
