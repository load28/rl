# TASK-058: 에디터 타입 진단 오탐 — lib.d.ts 없는 프로그램이 지어낸 `TS2488`

- **상태**: 완료
- **시작일**: 2026-08-18
- **완료일**: 2026-08-18
- **커밋**: `48de812`

## 목적

정상적인 TypeScript에 에디터가 **없는 에러를 붙인다.** 신고된 코드는
`docs`의 계산기 예제(TASK-055와 같은 프로젝트)에서

```ts
const applyP =
  (f: (a: number, b: number) => number) =>
  (ops: Operands): Evaluated =>
    Result.map(ops, ([a, b]) => f(a, b));
```

의 `[a, b]`에 `TS2488: Type '[number, number]' must have a
'[Symbol.iterator]()' method that returns an iterator.`가 표시되는데,
같은 코드를 `rlc --types`와 `tsc --noEmit`은 에러 없이 통과시킨다.
`[number, number]` 튜플의 표준 구조 분해는 유효한 TypeScript이므로 에디터
쪽이 틀렸다 — 검사기(rlc/tsc)와 에디터가 어긋나면 컴파일러가 정본이다.

CLAUDE.md의 에러 계층 계약("rlc가 방출한 코드 때문에 tsc 에러가 발생해서는
안 된다")은 뒤집어도 성립한다: **에디터가 사용자 코드에 없는 타입 에러를
지어내서도 안 된다.** 이 태스크는 그 오탐의 원인을 제거하고, 같은 원인이
다시 생기더라도 오탐 대신 침묵하도록 만든다.

## 범위

- 포함:
  - `TsProject`가 TypeScript 기본 라이브러리(`lib.esnext.full.d.ts`)를
    **검증된 경로로** 찾도록 한다(후보 다중화 + 존재 확인).
  - 기본 라이브러리가 없는 프로그램에서는 타입 진단을 전부 끈다
    (`typeEnvironmentError()`), 서버가 그 사유를 출력 채널에 한 번 남긴다.
  - `.vscodeignore` — 패키징된 VSIX에 런타임에 필요한 `typescript` 모듈과
    `lib.*.d.ts`가 실제로 들어가게 한다.
  - 증상 그대로를 고정하는 회귀 테스트(서버 테스트).
- 제외:
  - **프로젝트 `tsconfig.json` 읽기.** 아래 결정 3 참조.
  - 언어 표면·방출 코드·rlc 동작 변경 (rlc는 이 건과 무관하다 — 측정으로
    확인했다).
  - `TsProject`의 `COMPILER_OPTIONS` 값 자체(`target`/`strict` 등) 변경.
    아래 결정 4 참조.

## 의사결정

### 결정 0: 먼저 rlc가 아니라 에디터가 틀렸다는 것을 측정으로 확정한다

- **상황**: 증상은 "정상 코드에 타입 에러". rlc 방출물의 문제인지 에디터
  환경의 문제인지부터 갈라야 했다.
- **검토한 대안**: (A) 방출 코드를 의심하고 codegen부터 본다. (B) 신고
  코드를 그대로 재현해 방출물을 tsc에 직접 먹여본다.
- **선택과 근거**: (B). 신고된 `eval.rl`/`parser.rl`/`error.rl`을 재현해
  `rlc --emit-map`으로 방출물을 뽑고, `TsProject`의 호스트 구성을 그대로
  베낀 하니스에 먹였다 — **진단 0건**. 즉 방출 코드도 정상이고 확장의
  하드코딩된 컴파일러 옵션 자체도 정상이다. 남은 변수는 그 옵션으로 실제
  로드되는 **lib**뿐이었다.

### 결정 1: 원인은 "기본 라이브러리가 로드되지 않은 프로그램"이다

- **상황**: `TS2488`은 배열/튜플에 `[Symbol.iterator]`가 없을 때 나는
  에러다. 튜플 타입에 이게 뜨려면 `Array`의 iterable 선언이 없어야 한다.
- **검토한 대안**과 각각의 실측:
  - (A) `lib`이 `es5`로 좁혀졌다 → TypeScript 5.9.3에서
    `tsc --target esnext --lib es5`로 `([a, b]: [number, number]) => a + b`
    를 검사하면 **`TS2488` 재현**. 원인 후보로 성립.
  - (B) 기본 라이브러리가 통째로 없다(`noLib` 상태) → 하니스의
    `getDefaultLibFileName`을 존재하지 않는 경로로 바꿔 재현하면 신고와
    **완전히 같은 진단 1건**이 나온다:
    `TS2488 ... '[number, number]' ... @2051` (= `[a, b]`의 위치).
  - 참고: TypeScript 6.0.2에서는 같은 조건에서 `TS2488`이 나지 않는다
    (튜플 경로가 바뀌었다). 확장이 쓰는 5.x에서만 보이는 증상이다.
- **선택과 근거**: (B). 결정적인 근거는 **왜 사용자에게 이 한 건만
  보이는가**이다. lib이 없으면 `Cannot find name 'Error'` /
  `'JSON'`(TS2304)도 함께 나오는데, 그것들은 전부 rlc가 쓴 글루
  (`throw new Error(... JSON.stringify ...)`) 안이라 TASK-057의 매핑
  필터가 조용히 버린다. 남는 것은 사용자가 직접 쓴 `[a, b]` 한 곳뿐 —
  신고 화면과 정확히 일치한다. (A)는 이 "한 건만 보임"을 설명하지 못한다
  (확장은 `lib`을 설정하지 않는다).

### 결정 2: 침묵하는 열화를 없애고, 그래도 깨지면 오탐 대신 침묵한다

- **상황**: `getDefaultLibFileName: (options) => ts.getDefaultLibFilePath(options)`
  는 **경로를 계산만** 한다. 그 파일이 없어도 아무도 알아채지 못하고,
  프로그램은 전역 타입이 하나도 없는 채로 계속 검사한다.
- **검토한 대안**:
  - (A) 후보 경로를 늘려 더 잘 찾게만 한다.
  - (B) 진단 단계에서 "환경이 온전한가"를 확인하고, 아니면 진단을 끈다.
  - (C) 둘 다.
- **선택과 근거**: (C). (A)만으로는 다음 번 설치 사고에서 같은 오탐이
  그대로 재발한다. (B)만으로는 정상 설치에서도 필요 이상으로 진단이
  꺼질 수 있다. 그래서 ① 후보를 세 갈래로 두고 **디스크 존재를 확인한**
  경로만 채택하고(`findDefaultLib`), ② 진단 직전에 프로그램이 그 파일을
  실제로 로드했는지 확인해(`typeEnvironmentError()`) 아니면 한 건도
  내보내지 않는다. 침묵은 사유와 함께여야 하므로 서버가 출력 채널에
  경고를 한 번 남긴다.
- 후보 순서와 이유:
  1. `ts.getDefaultLibFilePath()` — 정상 설치에서 맞는 답.
  2. `require.resolve("typescript")`의 디렉터리 — 호스트가 `ts.sys`의
     실행 경로를 다르게 보는 경우(번들링 등)의 보정.
  3. 워크스페이스 루트에서 위로 올라가며 `node_modules/typescript/lib` —
     확장 쪽이 깨졌어도 프로젝트가 가진 TypeScript로 검사할 수 있다.

### 결정 3: 프로젝트 `tsconfig.json`은 읽지 않는다

- **상황**: 신고자의 1차 진단은 "확장이 프로젝트 tsconfig(`lib: ES2022`)를
  못 읽어서"였다. 그러면 tsconfig를 읽는 것이 자연스러운 수리로 보인다.
- **검토한 대안**: (A) `ts.findConfigFile` + `parseJsonConfigFileContent`로
  프로젝트 옵션을 채택한다. (B) 컴파일러 소유의 고정 환경을 유지한다.
- **선택과 근거**: (B). ① 이 저장소의 **정본 검사기인 `rlc --types`도
  tsconfig를 읽지 않는다** — `src/main.rs`의 `TYPES_COMPILER_OPTIONS`를
  데이터로 넘겨 검사한다(TASK-040의 인메모리 타입 설계). 에디터만 tsconfig를
  읽으면 "에디터와 `rlc --types`가 일치해야 한다"는 바로 그 기준이 **반대
  방향으로** 깨진다. ② 확장이 쓰는 고정 옵션은 `target: ESNext`(=
  `lib.esnext.full`)라 프로젝트 `lib`의 상위집합이다. 즉 tsconfig를 무시해서
  생길 수 있는 것은 놓친 에러(거짓 음성)뿐이고, 이번 신고 같은 **거짓 양성은
  구조적으로 나올 수 없다** — 실제로 결정 0의 실측이 이를 확인했다.
  ③ TASK-055 결정 1에서 같은 이유로 이미 (A)를 기각했다. tsconfig 채택은
  `rlc --types`까지 함께 바꿔야 하는 별도 설계 결정이므로 이 태스크에
  얹지 않는다.

### 결정 4: `COMPILER_OPTIONS`의 `target`/`strict`는 건드리지 않는다

- **상황**: 확장(`target: ESNext`, `strict` 미설정)과 `rlc --types`
  (`target: es2022`, `strict: true`)의 옵션이 다르다. 맞추고 싶은 유혹이
  있다.
- **검토한 대안**: (A) `rlc --types`와 동일하게 맞춘다. (B) 그대로 둔다.
- **선택과 근거**: (B). 현재 차이는 전부 **안전한 방향**이다 — 에디터의
  lib은 상위집합이고 `strict`는 더 느슨하므로, 차이가 만들 수 있는 것은
  거짓 음성뿐이다. `strict`를 켜면 프로젝트가 strict가 아닌 사용자에게
  갑자기 대량의 새 진단이 뜬다 — 이번 버그(거짓 양성 제거)와 무관한 위험을
  같은 커밋에 섞지 않는다. 별도 판단이 필요하면 새 태스크로 다룬다.

### 결정 5: 패키징에서 `typescript`를 실제로 동봉한다

- **상황**: `.vscodeignore`가 `node_modules/**`를 제외하고 LSP 패키지만
  다시 포함시키는데, 서버가 **런타임에 `import * as ts from "typescript"`**
  를 하므로 `typescript`가 빠진 VSIX는 애초에 동작할 수 없다. `**/*.ts`
  제외 규칙까지 있어 `lib.*.d.ts`도 함께 날아간다 — 결정 1이 짚은 "lib
  없는 프로그램"을 만드는 가장 그럴듯한 경로다.
- **검토한 대안**: (A) `!server/node_modules/typescript/**` 전체 포함.
  (B) 런타임에 필요한 것만(`package.json`, `lib/typescript.js`,
  `lib/lib.*.d.ts`).
- **선택과 근거**: (B). `tsc.js`/`tsserver.js`/`typingsInstaller.js` 등
  패키지 용량의 대부분은 확장이 쓰지 않는다. `.vscodeignore`는 뒤에 오는
  패턴이 이기므로 `**/*.ts` 뒤에 둔 `!...lib.*.d.ts`가 정상 동작한다.

## 작업 내역

- 2026-08-18: 신고 코드를 `eval.rl`/`parser.rl`/`error.rl`로 재현하고
  `rlc --emit-map`으로 방출물을 확인. `applyP`는 통과 영역이라 바이트
  그대로였고, 방출물 전체에도 문제가 없었다.
- 2026-08-18: `TsProject`의 `LanguageServiceHost` 구성을 그대로 베낀
  하니스(`mimic.mjs`)를 만들어 TypeScript 6.0.2 / 5.5.4 양쪽으로 검사 —
  **진단 0건**. 확장의 고정 옵션 자체는 무죄임을 확정.
- 2026-08-18: 조건을 하나씩 바꿔가며 `TS2488` 재현 실험.
  `tsc --target esnext --lib es5`에서 재현(5.5.4/5.9.3), 6.0.2에서는 미재현.
  하니스의 `getDefaultLibFileName`을 없는 경로로 바꾸자 신고와 동일한
  단일 진단이 재현됨(다른 진단은 전부 글루 영역).
- 2026-08-18: `editors/vscode/server/src/tsproject.ts` —
  `DEFAULT_LIB_NAME`/`findDefaultLib()` 추가, `getDefaultLibFileName`을
  검증된 경로로 교체, `typeEnvironmentError()` 추가,
  `diagnosticsFor()`가 그 상태에서 `[]`를 반환하도록. 테스트용
  `defaultLib` 생성자 인자(생략=자동 탐색, `null`=없음) 추가.
- 2026-08-18: `editors/vscode/server/src/server.ts` — `typeDiagnostics()`가
  환경 오류를 만나면 세션당 한 번 `connection.console.warn`으로 사유를
  남기고 진단을 비운다.
- 2026-08-18: `editors/vscode/.vscodeignore` — `typescript`의 런타임
  필수 파일 3종 재포함.
- 2026-08-18: `editors/vscode/server/src/test/emitmap.test.ts` —
  `stdProject()`에 `defaultLib` 통과 인자 추가, 신고 코드를 줄인
  `TUPLE_SOURCE`로 두 개의 테스트 추가(정상 환경=진단 0건 / lib 없는
  환경=환경 오류 보고 + 진단 0건).
- 2026-08-18: `editors/vscode/README.md` "타입 진단"의 안전장치 목록에
  세 번째 항목(타입 환경이 온전할 때만 검사) 추가.
- 2026-08-18: 검증 — `cd editors/vscode && npm install && npx tsc -b`
  후 `PATH=<repo>/target/release:$PATH node --test "server/out/test/*.test.js"`
  → 59/59 통과(skip 0). 새 테스트가 실효적인지 확인하기 위해 빌드 산출물
  `server/out/tsproject.js`에서 가드 한 줄을 지우고 다시 돌려
  `TS2488 ... '[number, number]' ...` 1건으로 **실패**하는 것을 확인한 뒤
  원복. rlc 쪽 게이트(`cargo fmt --check`, `cargo clippy --all-targets
  -- -D warnings`, `cargo test`)도 실행.

## 이슈 및 해결

### 이슈 1: 처음 세운 "프로젝트 tsconfig 미반영" 가설이 재현되지 않음

- **증상**: 확장이 `tsconfig.json`을 읽지 않는 것은 사실이지만, 그 상태
  (`target: ESNext`, `lib` 미설정)로 신고 코드를 검사하면 진단이 0건이라
  증상이 재현되지 않았다.
- **원인**: 확장의 기본값은 프로젝트 `lib`의 **상위집합**(`lib.esnext.full`)
  이라 tsconfig를 무시하는 것만으로는 거짓 양성이 나올 수 없다. 진짜
  변수는 "어떤 lib을 고르는가"가 아니라 "lib이 로드되기는 하는가"였다.
- **해결**: 가설을 버리고 lib 로드 여부를 직접 조작해 재현(결정 1). 결과가
  결정 3의 근거가 되었다 — tsconfig 채택은 이 버그의 수리가 아니며,
  `rlc --types`와의 일치를 오히려 깨뜨린다.

### 이슈 2: TypeScript 버전에 따라 증상이 사라진다

- **증상**: 같은 조건(lib 없음)에서 TypeScript 6.0.2는 `TS2488`을 내지
  않는다. 6.0으로 확인했다면 "재현 불가"로 종결했을 수 있다.
- **원인**: 6.0에서 튜플 구조 분해의 iterable 검사 경로가 바뀌었다.
  확장이 실제로 설치하는 것은 `^5.5.0` → 5.9.3이다.
- **해결**: 재현·회귀 테스트를 모두 확장이 실제로 쓰는 버전
  (`editors/vscode/server/node_modules/typescript`, 5.9.3)에서 돌리도록
  했다. 테스트는 확장의 의존성을 그대로 쓰므로 버전이 올라가도 같은
  환경을 검사한다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`
- [x] `cd editors/vscode && npm test` 상당 (`npx tsc -b` + `node --test`,
      rlc를 PATH에 둔 상태) — 59/59 통과

## 결과

| 파일 | 변경 |
|------|------|
| `editors/vscode/server/src/tsproject.ts` | `findDefaultLib()`로 검증된 기본 라이브러리 경로 탐색, `typeEnvironmentError()`, `diagnosticsFor()` 가드 |
| `editors/vscode/server/src/server.ts` | 타입 환경이 깨졌을 때 세션당 한 번 경고하고 진단을 비움 |
| `editors/vscode/server/src/test/emitmap.test.ts` | 튜플 구조 분해 회귀 테스트 2건 |
| `editors/vscode/.vscodeignore` | VSIX에 `typescript` 런타임 파일 동봉 |
| `editors/vscode/README.md` | "타입 진단" 안전장치에 타입 환경 가드 추가 |

rlc(컴파일러) 쪽은 변경 없음 — 언어 표면·방출 코드·CLI가 그대로이므로
`docs/reference/`와 `docs/ai/rl.md` 갱신 대상이 아니다.
