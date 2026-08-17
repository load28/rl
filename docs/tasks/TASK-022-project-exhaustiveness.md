# TASK-022: 선언 수집과 프로젝트 단위 소진성 검사 (모듈 그래프 2단계)

- **상태**: 완료
- **시작일**: 2026-08-17
- **완료일**: 2026-08-17
- **커밋**: 3638641

## 목적

[모듈 그래프 제안](../design/module-graph.md)의 2단계. 1단계(TASK-020)로
`.rl`끼리 import는 되지만 소진성 검사가 파일 단위라, import한 enum에 대한
`match`는 검사 없이 런타임 가드만 남는다. 직접 import된 `.rl` 파일에서
**exported enum 선언(이름 + 태그 집합)만** 수집해 현재 파일의 sema에
넘김으로써 `import { Token } from "./token.rl"` 한 줄로 빠진 케이스를
rlc가 잡게 한다. 타입 검사는 여전히 tsc의 책임이다 — 이 단계는 enum 태그
집합 이상을 알지 않는다.

## 범위

- 포함: import 절의 이름(별칭 포함) 수집(parser), 공개 API
  `exported_enums`/`rl_imports`/`ExternEnum`/`Options::extern_enums`,
  sema의 extern 후보 통합(로컬 > 임포트 > 내장 섀도잉), CLI의 1-홉 선언
  수집, 3계층 테스트, 레퍼런스 문서 갱신.
- 제외: 재귀적/전이적 수집(re-export 체인 추적), 증분 캐시, 모듈 해석
  (`node_modules`/절대 경로), 파일 존재 검사(없는 모듈은 tsc `TS2307`의
  영역 — rlc는 조용히 건너뛴다).

## 의사결정

### 결정 1 (제안의 결정 지점 3): 수집을 1-홉으로 한정해 순환을 구조적으로 제거

- **상황**: `a.rl ↔ b.rl` 상호 import 시 선언 수집이 무한히 돌 수 있다.
- **검토한 대안**: 방문 집합을 든 재귀 수집(re-export 체인 추적 가능) /
  직접 import만 1-홉 수집.
- **선택과 근거**: 1-홉. 사용자는 match할 enum을 직접 import하는 것이
  자연스러운 사용이고(간접 의존 enum에 match하는 코드는 드물다), 1-홉이면
  재귀가 없어 순환·방문 집합 문제 자체가 사라진다. 미수집의 비용은 "그 enum이
  검사되지 않음"(기존과 동일)이라 안전하게 열화한다. re-export 체인 추적은
  필요가 확인되면 별도 태스크.

### 결정 2 (결정 지점 4): 에러 위치는 match 키워드, 출처는 메시지로

- **상황**: 다른 파일의 enum 때문에 나는 에러에 선언 위치를 보여줄지.
- **검토한 대안**: `... declared at token.rl:7:1` 부가 표시(수집 API가
  파일·오프셋을 운반해야 함) / 메시지에 import 지정자만 표시.
- **선택과 근거**: 후자 — `match on enum Token (imported from
  "./token.rl") is not exhaustive: ...`. 고칠 위치는 어차피 match이고,
  출처 파일명은 지정자로 충분히 특정된다. 선언 위치 운반은 3단계(심볼
  API)에서 어차피 다룰 정보라 그때 함께 설계한다.

### 결정 3 (결정 지점 5): 라이브러리는 IO 없이 — 수집 API + 주입구만 추가

- **상황**: `compile(source, &Options)`은 파일 하나 API인데 그래프는 여러
  파일을 읽어야 한다.
- **검토한 대안**: `compile_project(path)` 같은 IO 진입점 신설 / 로더
  콜백(`Fn(&str) -> Option<String>`) / 순수 함수 2개 + Options 주입.
- **선택과 근거**: 순수 함수 + 주입. `rl_imports`(간선 나열)와
  `exported_enums`(선언 추출)는 IO가 없어 어떤 빌드 도구에서도 조합
  가능하고, `Options::extern_enums`로 주입하면 `compile`은 계속 순수하다.
  파일을 읽는 순회는 CLI의 `collect_extern_enums`가 담당 — 라이브러리의
  "IO 없음" 성질과 에러 계층(모듈 해석은 tsc 책임)이 유지된다.

### 결정 4: import 절의 이름대로만 등록하고 별칭·네임스페이스를 반영

- **상황**: 참조 파일의 모든 exported enum을 후보로 넣을 수도 있다(수집이
  단순해짐).
- **검토한 대안**: 파일 단위 전부 등록 / 절 이름 필터링.
- **선택과 근거**: 절 이름 필터링. sema는 태그 집합으로 enum을 식별하므로,
  import하지 않은 enum까지 등록하면 우연히 태그가 겹치는 손으로 쓴 유니언에
  거짓 에러를 낼 수 있다. `import { Token as Tok }`은 로컬 이름 `Tok`으로
  등록해 에러 메시지와 내장 섀도잉이 사용자가 보는 스코프와 일치하고,
  `import * as ns`는 `ns.<이름>`으로 등록해 내장 `Option`/`Result`를 잘못
  가리지 않는다. 이를 위해 parser의 import 절 스캔이 이름을 수집하도록
  확장했다 — 수집은 best-effort이며 리프팅 판정(TASK-020 동작)은 바꾸지
  않는다.

### 결정 5: 섀도잉 순서는 로컬 > 임포트 > 내장

- **상황**: 세 출처에 같은 이름이 있을 때의 우선순위.
- **선택과 근거**: 가까운 스코프 우선 — TypeScript의 이름 해석 직관과
  같다. 로컬 선언은 기존처럼 전부를 가리고, import한 `Option`은 내장
  `Option`을 가린다(사용자가 명시적으로 가져온 선언이 규범).
  `extern_enum_shadows_builtin_of_same_name` 테스트로 고정.

## 작업 내역

- 2026-08-17: TASK-022 등록.
- 2026-08-17: `ast.rs`의 `Segment::RlImport`를 `RlImportDecl { spec,
  names }`로 확장 (`RlImportNames::Namespace/Named/None`).
  `parser/imports.rs`의 절 스캔이 같은 토큰 순회에서 이름을 수집하도록
  변경 — `{...}` 항목은 `[type] name [as alias]`만 인정하는 관대한
  파싱(그 외 항목은 건너뜀), `* as ns` 인식, default 바인딩은 무시(rl
  enum은 named export). 리프팅 판정 로직은 변경 없음.
- 2026-08-17: 공개 API 추가 — `ExternEnum { name, tags, from }`,
  `exported_enums(source)`(exported rl enum 선언 추출, doctest 포함),
  `rl_imports(source)`(`RlImport { specifier, names }` 나열, doctest 포함),
  `Options::extern_enums: &[ExternEnum]`(기본 빈 슬라이스).
- 2026-08-17: `sema.rs` — 소진성 후보 체인을 로컬 → 임포트 → 내장으로
  확장, `Origin` enum으로 출처별 메시지 생성(`(imported from "...")`),
  내장 필터에 임포트 이름 섀도잉 추가.
- 2026-08-17: `main.rs` — `collect_extern_enums(file, source)`: 파일의
  직접 `.rl` import를 읽어 절 이름대로 선언 수집(별칭 적용, 네임스페이스는
  `ns.<이름>`), 읽기 실패는 조용히 건너뜀.
- 2026-08-17: 테스트 — compile.rs 7건(extern으로 검사됨/전체 커버/로컬
  섀도잉/내장 섀도잉/무관 match 비검사/`exported_enums` 필터링/`rl_imports`
  형태별), integration.rs 3건(`CARGO_BIN_EXE_rlc`로 CLI 자체 실행: 2파일
  소진성 에러 메시지·위치 확인 후 암 추가로 성공, 없는 모듈 조용히 건너뜀,
  크로스 파일 tsc+node 실행). CLI 스모크 테스트로 별칭(`Tok`) 메시지와
  비공개 enum 미유출도 확인.
- 2026-08-17: 문서 — `language.md` §3.6·§7.3(신규 규범 서술)·§9,
  `errors.md` 소진성 항목(출처 3종), `cli.md` 수집 동작, `README.md`,
  `module-graph.md`(2단계 구현됨, 결정 3·4·5 해소), `CLAUDE.md` sema 설명,
  `CHANGELOG.md`.

## 이슈 및 해결

없음 — 렉서/토큰 커서 기반(TASK-021) 위에서 절 이름 수집과 sema 확장이
계획대로 진행됐고, 전체 게이트를 첫 실행에서 통과했다 (fmt 재정렬 제외).

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` — 단위 87 / 통합 20(CLI 바이너리 실행 3건 포함) /
  통과 계약 35 / stdlib 2 / doctest 6, 전부 통과

## 결과

- 추가: `docs/tasks/TASK-022-project-exhaustiveness.md`
- 수정: `src/ast.rs`, `src/parser/{imports,mod}.rs`, `src/codegen/mod.rs`,
  `src/sema.rs`, `src/lib.rs`(공개 API: `ExternEnum`, `RlImport`,
  `RlImportNames`, `exported_enums`, `rl_imports`,
  `Options::extern_enums`), `src/main.rs`,
  `tests/{compile,integration}.rs`,
  `docs/reference/{language,errors,cli}.md`,
  `docs/design/module-graph.md`, `README.md`, `CLAUDE.md`, `CHANGELOG.md`,
  `docs/tasks/INDEX.md`

후속: 3단계(심볼 인터페이스 — `rlc --symbols`류 JSON 출력과 언어 서버
크로스 파일 정의 이동)는 결정 2에서 미룬 선언 위치 운반과 함께 별도
태스크로 등록한다.
