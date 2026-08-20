# Changelog

이 프로젝트의 주목할 만한 변경 사항을 기록합니다.
형식은 [Keep a Changelog](https://keepachangelog.com/ko/1.1.0/)를 따르고,
버전은 [Semantic Versioning](https://semver.org/lang/ko/)을 따릅니다.

## [Unreleased]

### Added

- **에디터가 엔진의 rl 표면을 쓴다** (TASK-107). VS Code 서버의 hover·definition·
  rename·완성이 `rlSymbol`/`rlCompletions`를 호출하고, `analysis.ts`의 해석
  재구현(`symbolAt`·`armContextAt`·`inferEnum`·`armTags`·`matchBodyAt`·
  `enumSignature`)이 삭제됐다.

  - `if let`·let-else·중첩 패턴에서 hover·정의 이동·완성이 **처음으로** 동작한다.
  - 케이스와 이름이 같은 지역 변수가 enum 케이스로 hover되던 오탐이 사라졌다.
  - 규칙이 하나가 됐다 — 이전의 정규식 구현은 컴파일러와 다른 후보 선택 규칙을
    갖고 있었다.

- **패턴 자리 자동완성** (TASK-106). 케이스 태그와 페이로드 필드 이름은 방출
  TypeScript에 존재하지 않아 체커가 완성할 수 없다. 이제 rl이 답한다 —
  `rlc --server`의 `rlCompletions`, 라이브러리의 `engine::rl_completions_at`.

  - match 암(이미 쓴 케이스는 `covered`로 표시), **`if let` 뒤**, 패턴 괄호 안의
    **필드 이름**(match·let-else·`if let` 모두), `Tag(field: ` 뒤의 **중첩 태그**.
  - 자리 판정은 **토큰 스트림**으로 한다 — 완성이 필요한 순간은 구문이 아직
    파싱되지 않는 순간이므로, 파서에 기대면 정작 그때 침묵한다.

- **rl 이름의 semantic 표면이 엔진에 생겼다** (TASK-105). enum 이름·케이스
  태그·페이로드 필드는 방출 TypeScript에 존재하지 않아(선언은 매핑 없는 합성
  텍스트, 태그는 문자열 리터럴, 필드는 구조 분해 키) 체커에게 물을 수 없다.
  이제 rl이 직접 답한다 — `rlc --server`의 `rlSymbol`, 라이브러리의
  `engine::rl_symbol_at`.

  - hover 서명과 정의 위치를 `match`뿐 아니라 **let-else·`if let`·중첩 패턴**
    에서도 답한다(에디터의 기존 구현은 match 본문만 알았다).
  - **체커에게 물을 수 있는 자리에는 답하지 않는다.** `Shape.Circle(1)` 같은
    사용처나 타입 주석은 평범한 TypeScript이므로 서비스가 답한다 — 케이스와
    이름이 같은 지역 변수가 enum 케이스로 hover되던 오탐이 사라진다.
  - 툴체인도 프로젝트도 필요 없다(`semanticTokens`와 같은 가용성).
  - 공개 API: `FieldSymbol::offset`, `PatternAnalyses::resolved`,
    `PatternAnalyses::declarations`.

- **생성된 코드에서 난 타입 에러를 rl의 말로 옮긴다** (TASK-104). 사용자 코드가
  잘못돼 tsc가 rlc의 글루에서 에러를 내면, 이제 그 구문의 위치에서 rl의 문안으로
  보고한다.

  ```
  before: errty.ts(7,57): error TS2322: Type 'Err<string>' is not assignable to ...
  after:  rlc: f.rl:2:13: the `Err` this `try` propagates does not fit the enclosing
               function's return type — ... (ts2322: Type 'Err<string>' is ...)
  ```

  - 새 emit 산출물 `EmitAnchor` — 각 구문이 쓴 글루의 출력 범위와 그 구문의
    소스 위치. `EmitMapping`과 **분리된 단방향** 자료다: 진단만 소비하고
    navigation·rename은 절대 글루로 들어가지 않는다.
  - 옮기는 대상은 `(구문, TS 코드)` **화이트리스트**다. 표에 없으면 옮기지 않고
    그대로 전달한다. 원문은 항상 괄호 안에 함께 실린다.
  - `match`를 TS `enum` 위에 쓴 경우도 이 경로로 rl 진단이 된다
    ([TASK-100](docs/tasks/TASK-100-ts-enum-match-diagnostic.md)).

- **소진성이 중첩 패턴 안쪽까지 검사된다** (TASK-103). 계산이 태그 집합의
  곱집합에서 rustc가 쓰는 **usefulness 알고리즘**(Maranget)으로 바뀌었다.

  ```rl
  const a = match (r) {
    Ok(value: Some(value: v)) => v,
    Ok(value: None()) => 0,
    Err(error) => -1,
  };
  ```

  - 이전에는 중첩 패턴 arm이 "아무것도 커버하지 못한다"고 취급되어 위 코드처럼
    **실제로 소진된 match가 거절**됐다(`missing "Ok"`). 이제 통과한다.
  - 빠진 것은 태그가 아니라 **패턴**으로 지목된다 — 그대로 arm으로 붙여넣을 수
    있다: `missing "Ok(value: None)"`, `missing "Wrap(inner: No)"`.
  - 안쪽 위치의 enum은 필드의 선언된 타입으로, 그것이 enum을 지목하지 않으면
    (제네릭 페이로드 `T`) 그 자리에 쓰인 패턴들로 정한다 — match의 스크루티니를
    arm 태그로 정하는 것과 같은 규칙이다.
  - 도달 불가 arm도 같은 재귀가 답하지만 **보고하지 않는다**: rl에는 경고 계층이
    없어 에러로 만들면 지금 컴파일되는 프로그램이 깨진다. 기존 중복 arm 검사는
    그대로다.

- **패턴의 이름 해석** (TASK-102). 패턴의 케이스 태그와 필드 이름을 선언에
  대조하고, 오타로 보이면 rlc가 위치와 함께 보고한다 — `match`(튜플·중첩 포함),
  let-else, `if let`이 같은 규칙을 쓴다.

  ```
  rlc: shape.rl:2:23: enum Shape has no case `Circel` — did you mean `Circle`?
  rlc: shape.rl:5:29: enum Shape: case `Circle` has no field `radiuz` — did you mean `radius`?
  ```

  - 이전에는 이런 오타가 rlc를 그냥 통과해 **생성된 코드 위에서** tsc 에러
    (`TS2678`/`TS2367`/`TS2339`)로 나타났고, 태그 오타의 경우 후보 표에서 enum이
    사라져 **그 match의 소진성 검사가 조용히 꺼졌다.**
  - 보고 조건은 "해석 실패"가 아니라 **"고칠 이름을 댈 수 있음"** 이다. 태그
    패턴은 손으로 쓴 `kind` 유니언에도 쓸 수 있으므로(`language.md` §3.2),
    선언 표에 없는 태그가 곧 오류는 아니다. 오타가 아닌 틀린 이름은 타입이
    필요하므로 검사하지 않는다 ([§3.10](docs/reference/language.md)).

### Changed

- **공개 API: `match_analyses` → `pattern_analyses`, `MatchAnalyses` →
  `PatternAnalyses`** (TASK-102). 분석이 match 밖의 패턴 사이트(let-else,
  `if let`)도 담게 되어 이름을 내용에 맞췄다. 새 필드는 `sites`(사이트별
  subject와 바인딩 타입)와 `unresolved`(이름 해석 답)다.

- **`andThen`이 에러 타입을 유니언으로 누적한다** (TASK-066). 이어 붙이는
  함수가 자기 방식으로 실패할 수 있다는 사실이 타입에 반영된다:

  ```
  Result<T, E>  +  (T) => Result<U, F>   →   Result<U, E | F>
  ```

  - 이전에는 `andThen`이 앞뒤에 같은 `E`를 요구해서
    (`(r: Result<T, E>, f: (T) => Result<U, E>)`), 에러 타입이 다른 두 함수를
    이으면 `TS2345`로 거절됐다. 이제 `Result.andThen(getUser(), getCompany)`가
    `Result<Company, UserError | CompanyError>`가 된다.
  - `Result.andThenP`도 같은 규칙이라 파이프라인 스텝마다 에러가 쌓인다:
    `loadUser() |> Result.andThenP(fetchProfile) |> Result.andThenP(validate)`
    → `Result<Profile, ConfigError | TokenError | FetchError | ValidationError>`.
    `try`와 `result` 블록이 만드는 흩어진 형태(`Ok<T> | Err<E1> | Err<E2>`)도
    그대로 받는다.
  - `map`/`mapP`는 새 실패를 만들지 않으므로 그대로다 (`Result<U, E>`).
  - 표준 라이브러리가 `ErrorOf<R>`(결과 타입에서 에러 쪽만 뽑는 타입)을 함께
    export한다. 에러 타입 수집은 여전히 tsc의 몫이고, rlc가 방출하는 코드에는
    조건부 타입이 없다.
  - **주의**: `andThenP`는 넘겨준 함수에서 입력 타입을 읽는다. 이름 붙은 함수는
    그대로 되지만, 인라인 화살표 함수는 매개변수 주석이 필요하다
    (`Result.andThenP((u: User) => f(u))`).
  - 방출되는 런타임 값과 코드는 바이트 단위로 그대로다.

- **`Result` 생성자가 자기 변종의 타입만 받는다** (TASK-065). 표준
  라이브러리가 두 케이스에 이름을 주고(`Ok<T>`/`Err<E>`, 타입만 export)
  `Result<T, E>`를 그 합으로 정의한다:

  ```ts
  Result.Ok(123)     // Ok<number>
  Result.Err("bad")  // Err<string>
  ```

  - 이전에는 `Ok`/`Err`가 제네릭을 둘 받아 값에 없는 타입까지 요구했다.
    `Result.Ok(1)`은 `E`를 추론할 정보가 없어 `Result<number, unknown>`이
    됐고, 그 때문에 반환 타입을 적지 않고 `try`를 여러 번 쓴 함수가
    `Result<T, E1 | E2>`에 대입되지 않았다 (`TS2322`). 이제 추론 결과가
    `Ok<T> | Err<E1> | Err<E2>`라 그대로 대입된다 — 에러 타입 수집은
    여전히 rlc가 아니라 tsc가 한다.
  - **파괴적 변경**: 타입 인자를 두 개 넘기던 호출은
    `Result.Ok<number, string>(1)` → `Result.Ok<number>(1)`,
    `Result.Err<number, string>("x")` → `Result.Err<string>("x")`.
    타입 인자를 넘기지 않던 코드는 그대로 동작한다. 전체 `Result` 타입은
    변수 주석·함수 반환 타입 등 주변 문맥에서 지정한다.
  - 콤비네이터 시그니처와 **방출되는 런타임 값은 바이트 단위로 그대로**다
    (`match`·소진성 검사·`JSON.stringify` 영향 없음).

- 대규모 소스 트리를 전제로 컴파일러 처리량을 끌어올렸다 — 방출 바이트와
  진단 메시지는 그대로다 (TASK-056):
  - 파일 단위 **병렬 컴파일**이 기본이 됐다 (코어 수만큼 동시에, `-j <n>`으로
    조절, `-j 1`이면 순차). 출력과 진단은 스레드 수와 무관하게 동일하며,
    진단은 실행 도중이 아니라 끝난 뒤 입력 순서대로 나온다.
  - 드라이버가 입력을 **파일당 한 번만 읽고 한 번만 파싱**한다 (신규 공개
    API `scan_module()`가 `.rl` import 목록과 `@rl/std` 사용 여부를 한 번에
    준다). 임포트된 모듈의 enum 선언 테이블도 실행당 한 번만 만든다 —
    공유 모듈을 N개 파일이 임포트할 때 N번 읽고 파싱하던 것이 1번이 됐다.
  - 코드젠 로프가 원본을 복사하지 않고 빌려오며, 줄 끝 주석 검사가 전체
    출력이 아니라 마지막 줄만 본다. 렉서는 토큰 벡터를 미리 확보하고
    토큰 크기를 줄였으며, 키워드 판정이 선형 스캔에서 `match`로 바뀌었다.
    sema의 소진성 후보 테이블은 match마다가 아니라 파일당 한 번 만든다.
  - 측정 (4코어, `--check`): 2.4 MB/121 파일 314 ms → 81 ms, 12 MB/601 파일
    빌드 1675 ms → 461 ms, 1 MB 공유 모듈을 200개 파일이 임포트하는 트리
    852 ms → 81 ms.

### Added

- **에디터가 타입 기반 `val` 진단을 표시한다** (TASK-072). `val` 바인딩을 통한
  built-in 변경 메서드 호출(`map.set(...)`, `xs.push(...)`)과 실제 타입 기준
  소진성은 타입이 있어야 판정되므로 `rlc --check`가 내지 못했고, 편집 중에는
  보이지 않았다. 이제 VSCode 확장이 편집 중인 버퍼를 그 파일이 프로젝트에서
  차지하는 자리에 그대로 얹어 컴파일러에게 묻고, 돌아온 문장을 그대로
  표시한다. `rl.typedChecks` 설정으로 끌 수 있다.

  이를 위해 `rlc --check-types`에 두 옵션이 생겼다:

  - `--overlay <path>` — `<path>`의 내용을 stdin에서 받는다. 경로는 그대로
    두고 내용만 바꾸므로, 저장되지 않은 버퍼의 import와 그 버퍼를 import하는
    쪽이 디스크에서와 똑같이 해석된다. `--watch`·`--types`와는 조합되지 않고,
    `<path>`는 실재해야 한다.
  - `--rl-only` — rl 계층만 보고하고 타입 에러(`ts(코드)`)는 생략한다. 살아
    있는 언어 서버를 이미 들고 있는 소비자가 같은 에러를 두 번 그리지 않게
    한다.

  진단의 문안·위치·형식은 그대로다 — 에디터는 무엇이 변경인지 판단하지 않고
  rlc가 쓴 문장을 옮긴다.

- **`result` 바인딩이 방출된 선언으로 매핑된다** (TASK-078). `result` 블록의
  `<-` 바인딩이 AST에 문자열로 복사돼 있어 `--emit-map`에 구간이 실리지
  않았다. 이제 스팬으로 들고 다니며 원본 바이트를 그대로 방출하므로, 에디터가
  `const x = $rl_r0.value;`의 `x`를 `.rl` 원문의 `const x <- ...`로 되짚는다.
  방출되는 코드는 바이트 단위로 그대로다.

- **`result` 계산 블록** — `Result`를 돌려주는 연산을 여러 단계 이을 때
  콜백 중첩 없이 평탄한 문장으로 쓴다 (TASK-064):

  ```rl
  const data = result {
    const user <- getUser(id);
    const company <- getCompany(user.companyId);
    { user, company }
  };
  ```

  - `const|let|var <바인딩> <- <식>;`이 **Result 바인딩**이다. `Ok`면 값을
    묶고 다음 문장으로, `Err`면 그 `Err`가 블록 전체의 값이 된다. 바인딩
    사이에는 평범한 TypeScript·rl 문장을 쓴다. 블록의 **마지막 값 식**
    (세미콜론 없이)이 `Ok`로 감싸진다.
  - `result`는 **문맥 키워드**다 — 블록에 `<-` 바인딩이 하나 이상 있을 때만
    rl 구문이므로, `result`라는 변수·클래스·속성과 뒤따르는 블록 문은 그대로
    통과한다. 선언 키워드 뒤의 `<-`는 유효한 TypeScript일 수 없어, 바인딩이
    있는데 파싱에 실패하면 위치를 담은 rl 에러가 된다.
  - **에러 타입이 저절로 합쳐진다.** 블록은 이른 `return`들의 IIFE로
    방출되므로(타입 트릭·헬퍼 없음) `Result<_, E1>`과 `Result<_, E2>`를 잇는
    블록은 `Result<T, E1 | E2>`에 그대로 대입된다 — 타입 추론은 전부 tsc가
    한다. `await`가 있으면 async IIFE가 된다.
  - 블록 안의 `return`은 블록에서 빠져나가므로 `try` 문·let-else는 블록 안에
    쓸 수 없다(위치를 담은 rl 에러) — `<-`를 쓴다. `if let`은 그대로 쓴다.
- **함수 합성 `flow`** — 파이프라인 head 자리의 `flow`가 값을 흘려보내는 대신
  스텝 함수들을 합성해 새 함수를 만든다 (TASK-063):
  `const label = flow |> half |> Option.mapP(x => x + 1) |> .toFixed(1);`
  뒤에 `label(4)`. 스텝 규칙은 `|>`와 같고, 합성은 이항 헬퍼 `$rl_fl`의
  중첩으로 방출되어 단계 수 제한이 없으며 첫 스텝의 다인자 arity가 보존된다.
  - `flow`는 **문맥 키워드**다 — head가 정확히 `flow` 하나일 때만 합성이므로
    `flow`라는 변수·import·속성은 그대로 통과한다 (`(flow) |> f`로 파이프).
  - 입력 타입은 **첫 스텝**이 정한다. 첫 스텝은 메서드 스텝이 될 수 없고
    (위치를 담은 rl 에러), 제네릭·커링 함수를 첫 스텝으로 쓰려면 타입 인자를
    명시한다 (`flow |> wrap<number> |> ...`).
- **`.rl` 안의 타입 에러가 원본 위치로 보고된다** (TASK-057). tsc가 보는
  것은 각 `.rl`이 컴파일된 TypeScript지만 그 파일은 디스크에 없으므로,
  방출 매핑을 거꾸로 타 원본 `.rl`의 행·열로 옮긴다. `match` 암과 `|>`
  파이프라인 **안쪽**의 타입 에러까지 잡힌다.
  - `rlc --types`: `rlc: t.ts:2:34: ...`처럼 존재하지 않는 파일을 가리키던
    진단이 `rlc: src/eval.rl:3:48: ...`이 됐다.
  - VSCode 확장: TS 타입 진단을 표출한다 (`source: ts`). 가상 문서를 서빙
    중일 때만, 원본에 매핑되는 스팬만 — 컴파일러 글루에 걸린 진단과 원문
    서빙 중의 오류 복구 진단은 표시하지 않는다. `rl.typeDiagnostics`로 끔.
  - 공개 API `compile_mapped()` — `compile()`과 같은 파이프라인에 방출
    매핑까지 함께 돌려준다.
- `-j, --jobs <n>` — 동시에 컴파일할 파일 수 (기본: 코어 수). (TASK-056)

### Fixed

- TypeScript 7(네이티브 컴파일러)만 해석되는 환경에서 `rlc --types`가
  원인 불명의 `TypeError` 스택 대신 명확한 안내를 낸다 — TS 7 패키지에는
  JS 컴파일러 API가 없어 `--types`가 구동할 수 없으므로, API 없는 설치는
  건너뛰고(프로젝트에 7, 전역에 6이 있으면 6으로 동작) 끝내 없으면
  `typescript@6` 설치를 안내한다. CI 게이트는 `typescript@6`으로 고정.
  (TASK-051)

### Added

- 방출 매핑 기반 TS 위임: `rlc --emit-map`(신규, 공개 API `emit_mapped()`)이
  방출 TypeScript와 원본↔출력 바이트 매핑을 내고 — 파싱+방출만 수행해
  편집 중인 버퍼도 항상 방출 — VSCode 언어 서버가 이를 **가상 TypeScript
  문서**로 TS 언어 서비스에 서빙한다. 방출물이 순수 TS이므로 match 암
  본문·스크루티니·`try`/`let-else`/`if let` 식·파이프라인 스텝 내부에서도
  호버·자동완성·정의 이동·참조·이름 변경이 온전한 타입 추론으로 동작하고,
  match 암 자동완성은 스크루티니의 TS 추론 타입으로 대상 enum을 특정한다
  (구조적 추론 실패 시, 선언 파일 교차 검증). 컴파일러가 없거나 방출이
  버퍼를 못 따라온 순간에는 종전의 원문 서빙으로 자동 폴백한다. (TASK-050)

- npm 패키징: `npm install --save-dev rl-lang`으로 rlc가 프리빌트
  바이너리로 설치된다 (bin `rlc`, esbuild/swc 방식의 플랫폼 패키지
  optionalDependencies — linux-x64/arm64는 musl 정적 링크, darwin-x64/arm64,
  win32-x64). 릴리스 워크플로(`release.yml`)가 태그 `vX.Y.Z`에서 빌드·npm
  배포·GitHub Release 업로드를 자동화한다. `unplugin-rl`은 설치된
  `rl-lang`의 바이너리를 자동으로 찾는다 (`rl-lang`의 `binaryPath()` 공개
  API, 없으면 종전대로 PATH의 `rlc`). (TASK-048)

- 통일된 타입·빌드 파이프라인 (TASK-036 계획, TASK-037):
  - 기본 모드가 **build**가 되어 디렉터리 입력에서 손으로 쓴
    TypeScript(`.ts`/`.mts`/`.cts`)도 함께 수집한다 — 바이트 그대로
    통과하되 상대 경로 `.rl` 지정자(및 `@rl/std`)만 재작성되어, 출력
    트리가 그 자체로 완결된다. 소스는 단독(tsc)이든 번들러 플러그인이든
    같은 모양(`"./x.rl"` import)으로 쓴다. 출력이 입력 파일 자신이 되는
    경우는 `output would overwrite the input` 에러로 거부. 숨김
    디렉터리와 `node_modules`는 순회하지 않는다.
  - `rlc --types`: "캐시 트리 컴파일 → tsc `--emitDeclarationOnly` →
    에디터 사이드카" 체인을 한 명령으로 내재화 (`.rl-build/` 캐시,
    `.rl-types/` 산출, `--tsc`로 바이너리 지정, `-w` 조합 지원). 사이드카
    선언은 소스 지정자(`"./x.rl"`, `"@rl/std"`)를 그대로 보존해 소비 측
    `rootDirs`/`paths` 설정만으로 `tsc --noEmit`과 에디터가 동작한다.

- VSCode 언어 서버에 TypeScript 언어 서비스 위임: rl 심볼(enum·케이스)이
  아닌 **일반 TS 심볼(변수·함수·타입·import된 값)의 정의 이동·호버·
  자동완성(`obj.` 멤버 포함)·참조 찾기·이름 변경**이 `.ts` 파일에서처럼
  동작한다 (완성은 rl 항목이 우선, rl 심볼의 이름 변경은 안전하게 거부).
  `.rl` 파일을 TS로 서빙하고 `./x.rl` 지정자를 커스텀 모듈 해석으로
  연결하며, TS 진단은 표출하지 않는다 (에러는 rlc가 정본).
  (TASK-024, TASK-025)

- 심볼 인터페이스: `rlc --symbols <file>`이 rl enum 선언(1-기반 위치 포함)과
  직접 `.rl` import(참조 파일의 exported 선언 포함)를 JSON으로 출력. VSCode
  언어 서버가 이를 소비해 **크로스 파일 정의 이동·자동완성·호버·빠른 수정**
  제공 (named import 별칭 반영). 라이브러리 API: `enum_symbols` /
  `EnumSymbol`/`CaseSymbol`/`FieldSymbol` / `line_col`. 모듈 그래프 로드맵
  3단계 완결. (TASK-023)

- 프로젝트 단위 소진성 검사: 직접 import한 `.rl` 파일의 exported enum에
  대한 match도 빠진 케이스를 컴파일 에러로 보고
  (`match on enum Token (imported from "./token.rl") is not exhaustive`).
  CLI가 import 절 이름(별칭·`* as ns` 포함)대로 선언을 자동 수집하며,
  섀도잉은 로컬 > 임포트 > 내장 순. 라이브러리 API: `rl_imports` /
  `exported_enums` / `ExternEnum` / `Options::extern_enums`. 모듈 그래프
  로드맵의 2단계. (TASK-022)

- `.rl` 간 import: 상대 경로 `.rl` import 지정자를 방출 시 재작성
  (`import { E } from "./error.rl"` → `"./error.js"`). 정적 import 선언과
  re-export 대상, 동적 import·비상대 경로는 통과. CLI `--rewrite-imports
  <js|bare|off>` (기본 `js`), 라이브러리 `Options::rewrite_imports`
  (`ImportRewrite`). 모듈 그래프 로드맵의 1단계
  (`docs/design/module-graph.md`). (TASK-020)

- `try` 문 — Rust의 `?`에 해당하는 에러 전파: `const n = try f();` /
  `try f();`가 `Err`면 둘러싼 함수에서 즉시 return하는 문장으로 컴파일된다
  (IIFE 없음, `await` 호환). TypeScript의 `try/catch` 블록·`try` 멤버
  이름은 그대로 통과. match 내부·템플릿 보간에서는 명확한 컴파일 에러.
  (TASK-012)
- `Option`/`Result` 표준 라이브러리: `rlc --emit-std <file>`이 함수형
  콤비네이터(`map`/`andThen`/`unwrapOr` 등)를 담은 순수 TypeScript 모듈을
  생성 (`docs/reference/std.md`, 라이브러리 API `rlc::STD_SOURCE`).
  `Option`(Some/None)·`Result`(Ok/Err)는 내장 enum으로 인식되어 파일에 선언이
  없어도 match 소진성 검사를 받는다 — 같은 이름의 로컬 rl enum이 있으면
  로컬이 우선. (TASK-011)

- 태스크 관리 체계 (`docs/tasks/`) 및 `CLAUDE.md` 작업 가이드. (TASK-001)
- 린트 게이트: `Cargo.toml [lints]` — `unsafe_code` 금지, `missing_docs` 경고,
  clippy `dbg_macro`/`todo`/`unimplemented`. (TASK-003)
- 거버넌스 문서: `LICENSE`(MIT), `CHANGELOG.md`, `CONTRIBUTING.md`. (TASK-004)
- 패키지 메타데이터: `repository`, `rust-version`, `keywords`, `categories`,
  릴리스 프로파일(lto, strip). (TASK-004)
- CI 파이프라인: fmt/clippy/test 게이트, tsc·node 통합 테스트 포함. (TASK-005)
- 라이브러리 수준 문서화: 규범 레퍼런스 `docs/reference/`(언어·CLI·에러) 신설,
  공개 API rustdoc·doctest 확충, README 문서 안내 섹션. (TASK-007)

### Changed

- `--emit-std`가 stdout 전용 무인자 옵션이 되었다 (번들러 플러그인의 가상
  모듈용). 파일 방출은 `@rl/std` 자동 방출이 대체한다. vite 플러그인도
  새 형태로 호출한다. (TASK-037)
- 파서 프런트엔드를 swc 스타일 렉서/토큰 커서 구조로 재구성: `lexer.rs`가
  소스를 유의 토큰 스트림으로 변환(정규식 휴리스틱·템플릿 중첩 렉싱을 한
  곳에 집중)하고, 파서 전체가 `parser/cursor.rs`의 토큰 커서 위에서
  동작한다. 동작 변경 없음 — 기존 테스트 전체와 구/신 컴파일러 차등 비교로
  출력 바이트 동일성을 확인. (TASK-021)
- `src/transform.rs`를 `src/transform/{mod,enums,matches}.rs` 모듈로 분리 —
  동작 변경 없음. (TASK-002)
- 전체 코드베이스를 rustfmt 기본 스타일로 정규화. (TASK-003)
- 레퍼런스 문서(`docs/reference/`)를 사용자 관점으로 단순화 — 스캔 규칙,
  판별 규칙 안전성 증명, 소진성 검사 알고리즘 등 내부 구현 상세를 제거하고
  사용자가 관찰 가능한 동작만 서술. README의 "동작 원리" 절 제거. (TASK-009)

### Removed

- `--rewrite-imports bare` 모드 — 번들러 경로는 플러그인(`off`)이
  대체했다. (TASK-037)

## [0.3.0] - 2026-08-16 이전

### Added

- Rust 재작성: 바이트 스캔 기반 변환기 + swc 검증 (조각 검증·출력 자가 검사).
- `enum` 키워드 통합: 페이로드/제네릭 규칙으로 rl enum과 TS enum 구분,
  TS enum은 그대로 통과.
- 소진성 검사를 rlc 수준 에러로 이동 (`파일:행:열` 보고, tsc 비위임).
- CLI: 디렉터리 재귀 컴파일, `-o`/`-p`/`--check`/`--no-banner`/`--no-verify`.
- 테스트 3계층: 컴파일 출력 단위 테스트, 통과(passthrough) 계약 테스트,
  tsc/node 통합 테스트.
