# TASK-056: 대규모 코드베이스 대비 컴파일러 성능 개선

- **상태**: 완료
- **시작일**: 2026-08-18
- **완료일**: 2026-08-18
- **커밋**: —

## 목적

rlc는 swc/tsc처럼 프로젝트 전체(수천 개 파일, 수십~수백 MB)를 대상으로 도는
것을 전제로 개발되어야 한다. 기존 구현은 단일 파일 정확성 위주로 짜여 있어
파일 수가 늘어날수록 낭비가 선형 이상으로 커지는 지점이 있었다 — 특히 공유
모듈을 임포트하는 파일이 늘어날수록 그 모듈을 그만큼 다시 읽고 다시 파싱했다.
이 태스크는 **방출 바이트와 진단 메시지를 하나도 바꾸지 않으면서** 처리량을
끌어올린다.

## 범위

- 포함: 드라이버(파일당 중복 읽기·중복 파싱 제거, 임포트 선언 캐시, 파일 단위
  병렬 컴파일과 `-j/--jobs`), 코드젠(로프 무복사화, 줄 끝 주석 검사 범위 축소),
  렉서(토큰 벡터 사전 확보, 토큰 크기 축소, 키워드 판정), sema(소진성 후보
  테이블 호이스팅), 공개 API `scan_module()`.
- 제외: 언어 표면 변경, 방출 코드 형태 변경, 에러 메시지 변경, 증분 컴파일
  디스크 캐시, 새 외부 크레이트 도입(병렬화는 `std::thread::scope`로).

## 측정 환경과 기준선

4코어 리눅스, `--release`(`lto = "thin"`). 벤치 코퍼스 3종을 생성해 각각 3회
실행 중 최소값을 취했다.

| 코퍼스 | 구성 |
|--------|------|
| A | 2.4 MB / 121 파일 — enum·match·템플릿·클래스가 섞인 일반적 트리 |
| B | 12 MB / 601 파일 — A와 같은 모양을 5배로 |
| C | 1 MB 짜리 공유 모듈(enum 200개) 하나를 200개 파일이 임포트 — 팬인 최악 |

`valgrind --tool=callgrind`로 명령어 수 기준 프로파일도 떴다(벽시계는 ±5%
흔들려 개별 변경의 효과를 판별하기 어려웠다).

기준선 프로파일(A, `--check --no-verify`, 총 153.9 M instructions):

| 구간 | 비중 |
|------|------|
| `malloc`/`free` 계열 | 32% |
| `memcpy` | 10% |
| `parser::parse`(누적) | 49.5% — 그중 **약 22%p는 `compile()` 바깥**(드라이버의 중복 파싱) |
| `codegen`(누적) | 33% (`expr_body_text` 9%, `Rope::flatten` 5.4%) |
| `sema` | 8% |

검증을 켠 프로파일(A, `--check`)에서는 `verify_output`(방출물 swc 재파싱)이
31.7%, `check_type_fragment`가 1.3%였다.

## 의사결정

### 결정 1: 드라이버가 파일을 한 번만 읽고 한 번만 스캔한다

- **상황**: `compile_jobs`는 파일마다 (1) `std_placement`에서 읽고
  `imports_std()`로 파싱, (2) 다시 읽고 `collect_extern_enums`에서
  `rl_imports()`로 파싱, (3) `compile()`이 또 파싱 — 읽기 2회, 파싱 3회였다.
  프로파일상 파싱의 약 22%p가 이 중복분이었다.
- **검토한 대안**:
  - (A) `rl_imports()`와 `imports_std()`를 그대로 두고 드라이버에서 두 번
    부른다 — 코드 변화 없음, 중복 파싱 유지.
  - (B) 두 사실을 한 번의 파싱으로 주는 공개 API를 추가한다 — 공개 표면이
    한 항목 늘지만 중복이 사라지고, 기존 두 함수를 그 위에 다시 정의하면
    두 뷰가 어긋날 수 없다.
  - (C) `compile()`이 파싱 결과를 돌려주게 해서 세 번째 파싱까지 없앤다 —
    가장 빠르지만 AST가 공개 표면에 새어 나오고(`ast`는 내부 계약),
    `compile(source, options) -> String` 이라는 단순한 API가 깨진다.
- **선택과 근거**: (B). `scan_module() -> ModuleScan { imports, imports_std }`
  를 추가하고 `rl_imports()`/`imports_std()`를 그 위에 재정의했다. 파일당
  파싱 3 → 2, 읽기 2 → 1. (C)는 얻는 것(1회 더 감소)에 비해 공개 계약의
  손상이 커서 접었다. 두 뷰의 등가성은 `scan_module_answers_both_questions_in_one_pass`
  테스트가 지킨다.

### 결정 2: 임포트된 모듈의 선언 테이블을 실행당 한 번만 만든다

- **상황**: `collect_extern_enums`가 파일마다 임포트 대상을 `fs::read_to_string`
  하고 `exported_enums()`로 파싱했다. 공유 모듈 하나를 N개 파일이 임포트하면
  그 모듈을 N번 읽고 N번 파싱한다 — 팬인에 대해 사실상 제곱이다. 코퍼스 C가
  이 모양이다.
- **검토한 대안**:
  - (A) 캐시 없이 유지 — 단일 파일 컴파일에는 문제가 없다.
  - (B) 경로별 캐시(`HashMap<PathBuf, Arc<Vec<ExternEnum>>>`).
  - (C) (B) + 임포트 대상이 이번 실행의 입력이기도 하면 이미 메모리에 있는
    소스를 쓴다 — 디스크 읽기까지 0회.
- **선택과 근거**: (C). 프로젝트 내부 임포트는 대부분 자기 자신도 입력이므로
  읽기가 통째로 사라진다. 캐시 키는 `dir.join(specifier)` 그대로 두었다 —
  `Path`의 `Eq`/`Hash`는 컴포넌트 기준이라 `a/./b`와 `a/b`가 같은 키가 되고,
  정규화(`canonicalize`)는 심볼릭 링크 의미를 바꿀 수 있어 피했다.
  읽기 실패는 **빈 테이블로 캐시**한다 — 기존 동작(그 import를 조용히 건너뜀)과
  결과가 같고, 실패한 읽기를 반복하지 않는다.
  효과: 코퍼스 C `--check` 837 ms → 75 ms (순차 실행 기준, 즉 병렬화 없이도
  11배).

### 결정 3: 파일 단위 병렬 컴파일을 기본으로

- **상황**: `compile()`은 파일 하나짜리 순수 함수이고 파일 간 가변 상태를
  공유하지 않는다. 그런데 드라이버는 완전히 순차였다. 프로파일에서 가장 큰
  단일 항목인 `verify_output`(31.7%)은 줄일 방법이 없지만 완벽히 병렬화된다.
- **검토한 대안**:
  - (A) 순차 유지 — 가장 단순, 코어를 1개만 쓴다.
  - (B) `rayon` 도입 — 몇 줄이면 되지만 의존성 트리가 커진다(현재 의존성은
    swc 3개뿐이고 CLAUDE.md의 범위 원칙상 새 크레이트는 아낀다).
  - (C) `std::thread::scope` + `AtomicUsize` 워크 커서로 직접 구현 —
    약 40줄, 의존성 0.
- **선택과 근거**: (C). 필요한 것은 "슬라이스를 순서 보존해서 map" 하나뿐이라
  rayon의 나머지 기능은 쓸 데가 없다. `par_map`은 결과를 **입력 순서로**
  돌려주므로 진단·출력이 스레드 수와 무관하게 동일하다.
  효과: 코퍼스 B 빌드 1655 ms → 445 ms (4코어에서 3.7배; 순차분 개선 포함).

### 결정 4: 병렬 실행의 관측 가능한 결과를 순차와 완전히 동일하게 유지

- **상황**: 순진하게 병렬화하면 두 가지가 깨진다. ① 진단이 뒤섞인 순서로
  나온다. ② `a.rl`과 손으로 쓴 `a.ts`가 **같은 출력 경로**를 요구할 때
  (둘 다 `a.ts`) 기존에는 "뒤에 오는 입력이 이긴다"가 결정적이었는데 병렬
  쓰기에서는 승자가 비결정적이 된다. `examples/`가 실제로 이 모양이다.
- **검토한 대안**:
  - (A) 모든 출력을 부모 스레드로 모아 순서대로 쓴다 — 완전히 결정적이지만
    출력 트리 전체를 메모리에 들고 있어야 한다.
  - (B) 워커가 곧바로 쓰되, 출력 경로가 **경합하는 잡만** 부모로 되돌린다.
  - (C) 경합을 에러로 만든다 — 동작 변경이라 범위 밖.
- **선택과 근거**: (B). 경합은 `.rl`/`.ts` 스템 충돌이라는 드문 경우뿐이므로,
  일반 경로에서는 추가 메모리 0이면서 승자 규칙은 그대로다. 진단은 잡별로
  모아 입력 순서로 낸다 — 그 대가로 **진행 중 스트리밍이 사라지고 실행이
  끝난 뒤 한꺼번에** 나온다. 이 점은 `cli.md`와 AI 가이드에 명시했다.
  확인: `jobs_does_not_change_outputs_or_diagnostics` 테스트가 `-j 1/2/3/8`의
  (쓰인 파일 전체 내용, stderr, 종료 코드)가 모두 같음을 검사한다. 추가로
  스크래치 하네스가 세 코퍼스 × `-j 1/2/3/8`을 바이트 비교했다.

### 결정 5: `-j/--jobs` 플래그를 노출한다

- **상황**: 병렬화를 넣으면 "끄는 방법"이 필요한지 결정해야 했다.
- **검토한 대안**: (A) 자동 감지만, 플래그 없음 — 표면이 늘지 않는다.
  (B) `-j <n>` 노출 — CLI 표면 + 문서 + 테스트가 늘지만, 코어가 과도한 CI
  컨테이너에서 조절할 수 있고 무엇보다 "병렬 = 순차"라는 계약을 테스트로
  고정할 수 있다.
- **선택과 근거**: (B). 병렬 컴파일러 드라이버의 표준 인터페이스이고, `-j 1`이
  없으면 결정성 회귀를 테스트로 잡을 방법이 없다. `cli.md`·`docs/ai/rl.md`를
  같은 커밋에서 갱신했다.

### 결정 6: 로프 조각이 원본을 복사하지 않고 빌려온다

- **상황**: `Rope::push_src`가 원본 조각마다 `text.to_string()`으로 힙 복사를
  했다. 통과 위주 파일이면 파일 전체가 조각으로 복사되고, `flatten()`에서 또
  한 번 복사된다. 프로파일의 malloc 32% / memcpy 10%가 여기에 크게 걸려 있다.
- **검토한 대안**: (A) 유지. (B) `Piece::Src`를 `&'a str`로, `Piece::Lit`을
  `Cow<'a, str>`로 바꾸고 `Rope<'a>`에 수명을 도입 — 코드젠 전체에 수명이
  전파되지만 복사가 사라진다.
- **선택과 근거**: (B). `Emitter<'a>`가 이미 `src: &'a str`을 들고 있어
  전파는 시그니처 수정으로 끝났다. 누적 길이(`len`)를 들고 있다가
  `flatten()`에서 `String::with_capacity`로 한 번에 확보한다. `trim()`은
  빌린 조각은 다시 슬라이싱하고 소유 조각은 `drain`/`truncate`로 제자리에서
  줄여 재할당 없이 처리한다.

### 결정 7: 줄 끝 주석 검사가 로프 전체가 아니라 마지막 줄만 본다

- **상황**: `body.text().rsplit('\n').next()...contains("//")` 패턴이 arm 본문·
  파이프 스텝·let-else 본문마다 로프 **전체**를 평탄화한 다음 마지막 줄만
  봤다. 중첩된 match/파이프라인에서는 안쪽 본문이 바깥 본문 평탄화에 다시
  포함되어 깊이에 대해 제곱이 된다. 프로파일에서 `expr_body_text` 9%.
- **선택과 근거**: `Rope::last_line_has_line_comment()`를 만들어 조각을 뒤에서
  앞으로 훑다가 첫 개행에서 멈춘다. `//`가 조각 경계에 걸칠 수 있으므로 그
  마지막 줄만 이어 붙여 검사한다 — 비용이 "로프 전체"에서 "한 줄"이 됐고
  의미는 동일하다.

### 결정 8: 렉서 토큰 크기 축소와 사전 확보

- **상황**: `TokenKind::Template(Vec<TplPart>)` 때문에 압도적 다수인 1바이트
  `Punct` 토큰까지 48바이트를 차지했고, 토큰 벡터는 매번 0에서 배로 자랐다
  (프로파일에서 `Vec` 증가 5.5%).
- **선택과 근거**: `Box<[TplPart]>`로 바꿔 토큰을 40바이트로 줄이고,
  `Vec::with_capacity((end - start) / 6 + 8)`로 미리 잡는다. 6바이트/토큰은
  실제 TypeScript에서의 대략적 밀도이며, 빗나가도 배증 1회로 흡수된다.

### 결정 9: 키워드 판정을 슬라이스 스캔에서 `match`로

- **상황**: `RESERVED`(41개)·`PIPE_BOUNDARY_WORDS`(22개)·`REGEX_PRECEDING_WORDS`
  (14개)를 `&[&str]::contains`로 훑었다. 파일의 **모든 식별자**가
  `is_reserved`와 파이프 경계 판정을 통과한다.
- **선택과 근거**: `matches!(word, "a" | "b" | ...)`로 바꿨다. rustc는 문자열
  `match`를 길이 스위치 + 소수의 비교로 컴파일하지만 `contains`는 전 항목을
  훑는다. 이 변경과 결정 8을 합쳐 A 코퍼스 명령어 수 148.1 M → 137.9 M.

### 결정 10: sema 소진성 후보 테이블을 파일당 한 번만 만든다

- **상황**: `check_exhaustiveness`가 match마다 로컬/임포트/내장 enum 전체를
  훑으며 enum마다 `Vec<&str>`을 새로 만들었다 — O(match 수 × enum 수) 힙
  할당. `resolve_enum`(튜플 match)도 마찬가지였다.
- **선택과 근거**: `candidate_enums()`로 섀도잉 순서(로컬 > 임포트 > 내장)를
  한 번 확정해 두고 두 소진성 패스가 공유한다. 순회 순서가 기존
  `locals.chain(externs).chain(builtins)`와 같아 결과가 바뀌지 않는다.

### 결정 11: 필드 타입 검증 메모이제이션은 하지 않는다 (측정 후 기각)

- **상황**: `check_enum`이 필드 타입마다 swc 파서를 새로 세운다. 타입 문자열은
  파일 안에서 크게 중복되므로 메모이제이션이 유효해 보였다.
- **검토한 대안**: (A) 타입 문자열 HashSet 메모. (B) 모든 필드 타입을 한 모듈로
  묶어 1회 파싱하고 실패할 때만 개별 재파싱. (C) 하지 않음.
- **선택과 근거**: (C). 검증을 켠 프로파일에서 `check_type_fragment`는 전체의
  **1.3%**였다(`verify_output`이 31.7%). 상한 1.3%를 위해 상태와 복잡도를
  더할 이유가 없다. (B)는 더 빠르지만 "묶어서 파싱하면 통과하는데 따로 하면
  실패하는" 위음성 가능성이 남아 정확성 위험이 이득보다 컸다.

### 결정 12: `verify_output`의 입력 복사도 그대로 둔다 (측정 후 기각)

- **상황**: `parse_ts_module`이 `code.to_string()`으로 방출물을 한 번 더
  복사한다(swc `new_source_file`이 `String`을 요구). 없앨 수 있는지 검토했다.
- **선택과 근거**: 없애려면 swc의 `Lrc<SourceFile>`에서 소유권을 되찾아야 해
  버전 간 깨지기 쉬운 코드가 된다. 반면 비용은 파일당 memcpy 1회로, 같은
  함수 안에서 도는 swc 파싱(31.7%)에 비하면 무시할 수준이다. 이 구간은
  **병렬화로** 갚는 쪽을 택했다.

### 결정 13: 입력 소스를 실행 내내 메모리에 유지한다 (메모리/IO 트레이드)

- **상황**: 결정 1·2의 결과로 모든 입력 소스가 실행이 끝날 때까지 메모리에
  남는다. 12 MB 트리에서 최대 RSS 10.1 MB → 26.5 MB로 늘었다(트리 크기의
  약 1.3배 + 상수).
- **검토한 대안**:
  - (A) 현행 — 파일당 읽기 1회, 메모리 O(트리).
  - (B) 스캔 단계에서 `ModuleScan`만 남기고 소스를 버린 뒤 컴파일 단계에서
    다시 읽는다 — 메모리 O(스레드 × 파일 + 캐시), 읽기 2회.
- **선택과 근거**: (A). 계수가 트리 크기의 1.3배 수준이면 실제 TypeScript
  모노레포(수십 MB)에서 수십 MB에 그치고, 같은 규모에서 tsc가 쓰는 양보다
  한 자릿수 작다. 반대로 (B)는 파일당 읽기를 2배로 만들고 코드도 복잡해진다
  (`Mutex<Option<String>>`로 워커가 소스를 꺼내 가는 구조). 트리 크기 대비
  상수배 상주가 문제가 되는 시점이 오면 (B)가 남아 있는 레버다.

## 작업 내역

- 2026-08-18: 벤치 코퍼스 3종 생성 스크립트와 벽시계 벤치 하네스 작성.
  기준선 측정(위 표). `--release`를 `CARGO_PROFILE_RELEASE_STRIP=none
  CARGO_PROFILE_RELEASE_DEBUG=1`로 다시 빌드해 callgrind 프로파일 확보
  (`callgrind_annotate --inclusive=yes`).
- **등가성 하네스 구축**: 기준선 바이너리를 따로 보관하고, 세 코퍼스 ×
  `--rewrite-imports js|ts|off` × `--no-verify`/`--no-banner`, 그리고
  `--emit-map`/`--symbols`/`--print`/`--check`의 출력 트리·stdout·stderr를
  전부 바이트 비교하는 스크립트를 만들었다. 여기에 `-j 1/2/3/8` 상호 비교를
  더했다. 이후 모든 단계마다 이 하네스를 돌려 "IDENTICAL"을 확인했다.
  하네스용 `tricky` 코퍼스는 enum(제네릭 포함)·or 패턴·가드·튜플 match·
  중첩 패턴·블록 arm·줄 끝 주석·`try`·let-else·if-let 체이닝·파이프라인·
  중첩 템플릿·정규식/나눗셈 모호성·유니코드·TS enum을 한 파일에 모은 것이다.
- `src/codegen/rope.rs` 재작성: `Piece<'a>`(`Cow`/`&str`), 누적 길이,
  `last_line_has_line_comment()`, `trim()`의 제자리 축소.
  `src/codegen/mod.rs`·`matches.rs`를 `Rope<'a>` 시그니처로 맞추고
  `text()` 호출 4곳을 새 검사로 교체. → 148.1 M instructions (-3.8%).
- `src/lexer.rs`: `Template(Box<[TplPart]>)`, 토큰 벡터 사전 확보,
  `regex_preceding_word` `match`화. `src/parser/mod.rs`: `is_reserved`와
  `is_pipe_boundary_word`를 `match`로. → 137.9 M instructions (기준선 대비 -10.4%).
- `src/sema.rs`: `candidate_enums()` 호이스팅, `resolve_enum`을 그 테이블
  위의 연관 함수로 변경.
- `src/lib.rs`: `ModuleScan`/`scan_module()` 추가, `rl_imports()`·
  `imports_std()`를 그 위에 재정의(doctest 포함).
- `src/main.rs`: `ExternCache`(경로별 선언 캐시 + 입력 소스 재사용),
  `par_map`/`worker_count`/`load_jobs`, `Outcome`, `write_output`,
  `compile_jobs` 재작성(적재 → std 배치 → 병렬 컴파일 → 순서대로 출력),
  `types_once`도 같은 방식으로(그림자 검사는 선행 패스로 분리),
  `-j/--jobs` 파싱과 `usage()`·`BuildOptions`·`TypesOptions` 확장.
- 테스트: `tests/cli.rs`에 `jobs_does_not_change_outputs_or_diagnostics`(공유
  모듈 임포트 + 소진성 실패 파일이 섞인 프로젝트를 `-j 1/2/3/8`로 돌려
  쓰인 파일·stderr·종료 코드 전체 비교)와 `jobs_rejects_zero_and_garbage`,
  `tests/compile.rs`에 `scan_module_answers_both_questions_in_one_pass`.
- 문서: `docs/reference/cli.md`(옵션 표 + "병렬 컴파일" 절),
  `docs/ai/rl.md`(Workflow 한 줄 — `rlc help`가 이 파일을 그대로 서빙하므로
  `help_all_prints_the_whole_guide` 테스트가 동기화를 강제한다),
  `docs/design/compiler-architecture.md`("프로젝트 단위 실행(드라이버)" 절과
  로프 설명), `CHANGELOG.md`.

## 이슈 및 해결

### 이슈 1: 등가성 하네스가 실제 차이가 아닌 것을 차이로 잡았다

- **증상**: 첫 하네스 실행에서 세 코퍼스 모두 "STDERR differs"가 났다.
- **원인**: 기준선과 후보를 서로 다른 출력 디렉터리(`o_base`/`o_cand`)에
  쓰게 해 놓고, 진단에 찍히는 그 경로까지 비교하고 있었다. 또
  `--sidecar <입력파일>`처럼 잘못 호출한 케이스가 있어 양쪽 모두 usage를
  출력했고, 새로 추가한 `-j` 항목 때문에 usage가 달라 보였다.
- **해결**: 비교 전에 출력 디렉터리 경로를 `OUT`으로 정규화하고, 디렉터리
  인자를 요구하는 `--sidecar`는 하네스에서 뺐다(계약은 `tests/sidecar.rs`가
  이미 검사한다).

### 이슈 2: `tricky` 코퍼스가 컴파일되지 않았다

- **증상**: `rlc: .../a.rl:29:3: `if let` could not be parsed here (pattern
  parens are mandatory, ...)`.
- **원인**: 하네스용으로 손으로 쓴 파일에서 `if let None = ...`처럼 단위
  케이스에 패턴 괄호를 빠뜨렸고, `try const x = r;`도 문법이 아니었다
  (`const x = try r;`가 맞다). 컴파일러가 아니라 코퍼스의 버그.
- **해결**: `docs/ai/rl.md`의 해당 절대로 고쳤다. 고친 뒤 `--check`가 0으로
  끝나고, 방출물에 `switch ($rl_m` 3회·`$rl_ap` 2회·`$rl_t` 4회·유니코드가
  그대로 있음을 확인해 코퍼스가 실제로 대상 구문을 훑는지 검증했다.

### 이슈 3: 새 테스트가 clippy 게이트에 걸렸다

- **증상**: `cargo clippy --all-targets -- -D warnings`가 `ptr_arg`
  (`&PathBuf` 대신 `&Path`)와 `type_complexity`
  (`Option<(Vec<(String, String)>, String, bool)>`)로 실패.
- **원인**: 테스트 코드도 게이트 대상이라는 점을 놓쳤다.
- **해결**: 시그니처를 `&Path`로 바꾸고 `type RunResult` 별칭을 도입했다.

### 이슈 4: `--types`의 진단 순서가 미묘하게 달라진다

- **증상/원인**: `types_once`는 "가상 모듈이 같은 이름의 실제 파일을 가리면
  즉시 실패"를 파일 루프 안에서 하고 있었다. 병렬화를 위해 이 검사를 선행
  패스로 분리하면서, 읽기 실패 진단과 그림자 실패가 동시에 있을 때 이제
  그림자 실패만 나오고 즉시 종료한다(전에는 앞선 읽기 실패들이 먼저 찍혔다).
- **해결**: 그대로 두었다. 그림자 실패는 어차피 실행 전체를 중단시키는 치명적
  조건이라 먼저 보고하는 편이 낫고, 진단 **내용**과 종료 코드는 같다. 여기
  기록해 남긴다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` (258 passed, 0 failed — 신규 3건 포함)
- [x] 등가성 하네스: 세 코퍼스 × 5개 모드 × 4개 도구 플래그, 출력 트리·stdout·
      stderr 바이트 동일. `-j 1/2/3/8` 상호 동일.

## 결과

### 성능 (4코어, 3회 최소값)

| 코퍼스 | 모드 | 이전 | 이후 | 배수 | 이후(`-j 1`) |
|--------|------|------|------|------|--------------|
| A 2.4 MB / 121 파일 | `--check` | 321 ms | **86 ms** | 3.7× | 276 ms |
| A | 빌드 | 346 ms | **101 ms** | 3.4× | 307 ms |
| B 12 MB / 601 파일 | `--check` | 1675 ms | **389 ms** | 4.3× | 1320 ms |
| B | 빌드 | 1655 ms | **445 ms** | 3.7× | 1547 ms |
| C 1 MB 공유 모듈 + 200 파일 | `--check` | 837 ms | **78 ms** | 10.7× | 75 ms |
| C | 빌드 | 876 ms | **83 ms** | 10.6× | 124 ms |

`-j 1` 열이 병렬화 없이 얻은 몫이다 — 일반 트리에서 1.16~1.27×, 팬인이 큰
트리에서 11× (선언 캐시). 최대 RSS는 12 MB 트리에서 10.1 MB → 26.5 MB
(결정 13).

### 변경 파일

| 파일 | 내용 |
|------|------|
| `src/codegen/rope.rs` | 조각이 원본을 빌려옴, 용량 사전 확보, 마지막 줄 주석 검사 |
| `src/codegen/mod.rs`, `src/codegen/matches.rs` | `Rope<'a>` 전파, `text()` 전체 평탄화 제거 |
| `src/lexer.rs` | 토큰 벡터 사전 확보, `Template(Box<[TplPart]>)`, 정규식 키워드 `match` |
| `src/parser/mod.rs` | `is_reserved`/`is_pipe_boundary_word`를 `match`로 |
| `src/sema.rs` | `candidate_enums()` 호이스팅, `resolve_enum` 재작성 |
| `src/lib.rs` | `ModuleScan`/`scan_module()` 추가, 기존 두 헬퍼를 그 위에 재정의 |
| `src/main.rs` | `ExternCache`, `par_map`, 스테이지형 `compile_jobs`/`types_once`, `-j/--jobs` |
| `tests/cli.rs`, `tests/compile.rs` | 병렬 결정성 · `--jobs` 인자 검증 · `scan_module` 등가성 |
| `docs/reference/cli.md`, `docs/ai/rl.md`, `docs/design/compiler-architecture.md`, `CHANGELOG.md` | `-j`와 드라이버 규칙 문서화 |

### 후속으로 남긴 것 (필요해지면 새 태스크)

- `verify_output`이 여전히 파일당 최대 항목(검증 켠 실행의 31.7%)이다. 지금은
  병렬화로 갚고 있고, `--no-verify`가 탈출구다. 더 줄이려면 방출물 자가 검사
  자체를 재설계해야 하는데 그건 에러 계층 계약을 건드리는 일이다.
- 입력 소스의 실행 내내 상주(결정 13)를 스레드 수 비례로 낮추는 대안 (B).
- `compile()`이 파싱 결과를 재사용하게 해 파일당 파싱 2 → 1로 줄이는 안
  (결정 1의 대안 C) — 공개 API 설계를 다시 해야 한다.
