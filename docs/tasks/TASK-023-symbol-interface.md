# TASK-023: 심볼 인터페이스와 언어 서버 크로스 파일 기능 (모듈 그래프 3단계)

- **상태**: 완료
- **시작일**: 2026-08-17
- **완료일**: 2026-08-17
- **커밋**: 8eab912

## 목적

[모듈 그래프 제안](../design/module-graph.md)의 3단계. 2단계(TASK-022)가
만든 선언 수집을 언어 서버가 쓸 수 있게 내보낸다: `rlc --symbols <file>`이
파일의 rl enum 선언(위치 포함)과 직접 `.rl` import의 exported 선언을 JSON으로
출력하고, VSCode 언어 서버가 이를 소비해 **크로스 파일 정의 이동·완성·호버**를
제공한다. 서버가 rl 문법을 다시 구현하지 않고 컴파일러 하나를 정본으로 두는
구조("에디터의 에러 = 컴파일러의 에러"와 같은 원리)를 심볼 해석으로 확장한다.

## 범위

- 포함: AST에 enum 이름 오프셋 추가, 공개 API
  `enum_symbols`/`EnumSymbol`/`CaseSymbol`/`FieldSymbol`/`line_col`,
  CLI `--symbols`(JSON 출력, 1-홉 import 포함), 언어 서버의
  `rlc --symbols` 소비(named import 별칭 반영, 정의 이동/완성/호버 통합),
  테스트·문서.
- 제외: 네임스페이스 import(`* as ns`)의 서버 측 통합(JSON에는 포함하되
  서버 병합은 named import만 — `ns.Token.` 멤버 체인 해석은 별도 작업),
  재수출 체인, 워크스페이스 전역 심볼 검색, 선언 파일 watch에 따른 캐시
  무효화 고도화(문서 버전 기준 캐시만).

## 의사결정

### 결정 1: JSON 출력에 1-홉 import의 선언까지 포함한다

- **상황**: `--symbols`가 파일 하나의 선언만 낼지, import 그래프 정보까지
  낼지.
- **검토한 대안**: 파일 단위 출력(서버가 import를 직접 해석해 파일마다
  재호출) / 1-홉 포함 출력(호출 한 번).
- **선택과 근거**: 1-홉 포함. 서버가 import 해석(경로 결합, 절 이름
  필터링)을 재구현하면 "컴파일러가 정본"이라는 구조가 깨진다. 컴파일러는
  2단계의 `collect_extern_enums`와 같은 해석을 이미 갖고 있으므로 JSON에
  `imports[].names`/`resolved`/`enums`를 실어 서버는 변환만 한다. 해석
  범위도 소진성 검사와 동일한 1-홉이라 두 기능의 그림이 일치한다.

### 결정 2: 위치는 1-기반 행/열(UTF-8 코드포인트) — 에러 보고와 동일 규약

- **상황**: JSON 위치를 바이트 오프셋으로 낼지 행/열로 낼지.
- **검토한 대안**: 바이트 오프셋(정밀하지만 소비자가 대상 파일을 읽어
  변환해야 함) / 행/열(에러 규약과 동일, LSP Position에 근사 직결).
- **선택과 근거**: 행/열. 진단이 이미 이 규약이고(서버의 기존 변환 경로
  재사용), 식별자는 ASCII라 길이 = 열 폭이므로 범위 계산이 자명하다.
  라이브러리 수준에서는 바이트 오프셋(`EnumSymbol::offset`)을 유지하고
  변환기 `line_col`을 공개해 두 세계를 잇는다. TASK-022 결정 2에서 미룬
  "선언 위치 운반"이 이것으로 해소됐다.

### 결정 3: JSON 방출은 의존성 없이 손으로 쓴다

- **상황**: serde/serde_json을 추가할지.
- **선택과 근거**: 수동 방출(`json_str` 이스케이프 + 포맷 문자열).
  방출 전용이고 스키마가 작아 직렬화 프레임워크의 비용(컴파일 시간,
  직접 의존성)이 이득을 넘는다. 유효성은 통합 테스트에서 node의
  `JSON.parse`로 실제 파서 검증한다.

### 결정 4: 서버 병합은 named import만, 저장된 파일 기준

- **상황**: `* as ns` 네임스페이스 import 통합과 미저장 버퍼의 import 반영.
- **선택과 근거**: named import(별칭 적용)만 병합 — `ns.Token.Num` 멤버
  체인 해석은 analysis.ts의 단일 레벨 `Enum.` 가정을 바꾸는 별도 작업이라
  범위에서 제외했다(JSON에는 네임스페이스 정보가 이미 있어 서버만 고치면
  된다). `--symbols`는 디스크의 파일을 읽으므로 import 줄의 미저장 편집은
  저장까지 한 박자 늦는다 — 버퍼를 임시 파일로 넘기면 상대 경로 해석이
  깨지므로 채택하지 않았고, 한계를 README에 명시했다. 캐시는 문서 버전
  기준이고, 다른 문서가 편집되면 그 문서 외의 캐시를 비운다(선언 파일
  편집 반영).

### 결정 5: 별칭 import의 정의 이동 범위는 원선언 이름 길이

- **상황**: `import { Token as Tok }`에서 `Tok`의 정의로 이동하면 대상
  파일의 `Token`을 가리켜야 하는데, 별칭과 원이름의 길이가 다르다.
- **선택과 근거**: `ImportedOrigin`에 원선언 이름(`name`)을 실어 범위를
  `Token`의 길이로 계산한다. 별칭 길이를 쓰면 선택 범위가 어긋난다.

## 작업 내역

- 2026-08-17: TASK-023 등록. 언어 서버 현황 분석 — `analysis.ts`가 파일
  단위 구조 파싱, `rlc.ts`가 `--check` 진단 실행, `onDefinition`은 로컬
  선언만. CI는 Rust 게이트만 강제하고 확장은 `npm test`(tsc + node --test).
- 2026-08-17: Rust — `ast::EnumDecl`에 `name_off` 추가(parser가 이름 스팬
  기록), 공개 API `EnumSymbol`/`CaseSymbol`/`FieldSymbol`(+doctest),
  `enum_symbols(source)`(exported 여부 포함 전체 rl enum), `line_col` 공개.
  `main.rs`에 `--symbols` 모드: 입력별 로컬 선언 + 1-홉 import(절 이름,
  해석 경로, 참조 파일의 exported 선언)를 JSON 배열로 stdout 출력,
  `json_str` 이스케이프 수동 구현.
- 2026-08-17: 서버 — `rlc.ts`에 `runSymbols`(실패·구버전 rlc는 null로
  열화), `analysis.ts`에 `ImportedOrigin`/`EnumInfo.imported`와
  `visibleEnums(declared, imported)`(로컬 > 임포트 > 내장)·`symbolAt` 확장,
  `server.ts`에 버전 키 캐시 `importedEnums`, `toImportedEnumInfos` 변환
  (named import 별칭 적용), 완성·호버·정의·빠른 수정 핸들러 async화 및
  imported 풀 통합, 크로스 파일 `onDefinition`(선언 파일 URI + 행/열 범위),
  소진성 빠른 수정 정규식을 새 메시지 형식(`(imported from "...")`)에 대응.
- 2026-08-17: 테스트 — Rust: `enum_symbols` 위치·필드 형태 단위 테스트,
  `--symbols` CLI 통합 테스트(별칭 entries·위치·미해석 import null 확인 +
  node `JSON.parse`로 유효성 검증). 서버: `visibleEnums` 3단 섀도잉,
  imported enum 패턴 태그 `symbolAt` 해석 테스트 추가 (25개 통과).
- 2026-08-17: 문서 — `cli.md` `--symbols` 옵션·"심볼 출력" 절(JSON 스키마),
  `editors/vscode/README.md` 기능 표·크로스 파일 설명, `module-graph.md`
  3단계 구현됨(세 단계 완결), `CHANGELOG.md`.

## 이슈 및 해결

### 이슈 1: `npm test`가 테스트 실행 전에 실패

- **증상**: `node --test server/out/test/`가
  `Cannot find module .../server/out/test` (MODULE_NOT_FOUND)로 실패.
  테스트 파일을 직접 지정하면 전부 통과.
- **원인**: 이 환경의 Node v22.22.2가 `--test`의 마지막 디렉터리 인자를
  테스트 루트가 아니라 모듈 경로로 취급했다 (기존 스크립트는 이전 Node
  버전에서 동작하던 형태).
- **해결**: 스크립트를 `node --test "server/out/test/*.test.js"` 글롭
  형태로 변경 — node가 패턴을 직접 해석하므로 셸 의존도 없다.

### 이슈 2: `line_col` 공개 시 이름 충돌

- **증상**: `lib.rs`의 `use error::{RlError, line_col}`과 새 공개 함수
  `line_col`이 `E0255`로 충돌.
- **원인**: 같은 스코프에 같은 이름의 import와 정의.
- **해결**: import를 `use error::RlError`로 줄이고 내부 호출은 새 공개
  함수를 그대로 쓰게 했다(동일 구현 위임이므로 동작 동일).

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` — 단위 88 / 통합 21(`--symbols` JSON 검증 포함) /
  통과 계약 35 / stdlib 2 / doctest 7, 전부 통과
- [x] `editors/vscode`: `npm test` — tsc 컴파일 + node --test 25개 통과

## 결과

- 추가: `docs/tasks/TASK-023-symbol-interface.md`
- 수정: `src/ast.rs`(`EnumDecl::name_off`), `src/parser/enums.rs`,
  `src/codegen/enums.rs`, `src/lib.rs`(공개 API: `EnumSymbol`,
  `CaseSymbol`, `FieldSymbol`, `enum_symbols`, `line_col`),
  `src/main.rs`(`--symbols`), `tests/{compile,integration}.rs`,
  `editors/vscode/{package.json,README.md}`,
  `editors/vscode/server/src/{rlc,analysis,server}.ts`,
  `editors/vscode/server/src/test/analysis.test.ts`,
  `docs/reference/cli.md`, `docs/design/module-graph.md`, `CHANGELOG.md`,
  `docs/tasks/INDEX.md`

모듈 그래프 로드맵(TASK-019)의 세 단계가 모두 완결됐다.

후속 후보 (필요가 확인되면 별도 태스크로): `* as ns` 네임스페이스 import의
서버 측 멤버 체인 해석, re-export 체인 전이 수집, 증분 컴파일·캐시
(제안 문서의 "범위 밖" 항목).
