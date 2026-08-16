# TASK-020: import 지정자 재작성 (모듈 그래프 1단계)

- **상태**: 완료
- **시작일**: 2026-08-17
- **완료일**: 2026-08-17
- **커밋**: 0ba7901

## 목적

[TASK-019](./TASK-019-module-graph-proposal.md)에서 제안한 모듈 그래프 3단계
경로의 1단계를 구현한다. `.rl` 소스가 상대 경로로 다른 `.rl`을 import하면
방출된 `.ts`에서 지정자를 소비 측이 해석할 수 있는 형태(`./x.js` 또는
`./x`)로 재작성한다. 이것으로 ".rl 소스가 .rl 소스를 가리킨다"는 성질을
얻는다 — 참조 파일을 열지 않으므로 파일 단위 파이프라인은 그대로다.

## 범위

- 포함: 정적 import 선언(`import ... from`, `import "..."`)과 re-export
  (`export ... from`)의 **상대 경로 `.rl` 지정자** 재작성. CLI 플래그
  `--rewrite-imports <js|bare|off>`와 라이브러리 옵션
  (`Options::rewrite_imports`, `ImportRewrite`). 레퍼런스 문서 갱신.
- 제외: 동적 `import(...)`, `import x = require(...)`, `node_modules`/절대
  경로 지정자 — 통과 영역 유지. 참조 파일 파싱(2단계), 심볼 API(3단계).

## 의사결정

### 결정 1: 기본 동작으로 켠다 (옵트인 플래그가 아니라 옵트아웃)

- **상황**: TASK-019가 열어 둔 결정 — 재작성이 절대 불변 원칙 1(바이트 통과
  계약)의 예외를 만드는데, 기본으로 켤지 플래그 뒤에 둘지.
- **검토한 대안**:
  - 옵트인(`--rewrite-imports` 지정 시에만): 계약 문면을 지키지만, 1단계의
    가치(그냥 `.rl`을 import하면 된다)가 플래그 없이는 없다.
  - 옵트아웃(기본 `js`, `--rewrite-imports off`로 끔): 계약에 좁은 예외가
    생기지만 사용자는 설정 없이 동작을 얻는다.
- **선택과 근거**: 옵트아웃. 방출물에 남은 `.rl` 지정자는 tsc가 **항상**
  `TS2307`로 거부하므로(TASK-019 작업 내역에서 확인) 재작성으로 동작이
  나빠지는 기존 프로젝트가 존재할 수 없다. 예외는 "상대 경로이면서 `.rl`로
  끝나는 정적 지정자 문자열" 하나로 좁고, 문서(`language.md` §1·§7,
  `CLAUDE.md`)에 명시했다. 끄는 길(`off`)도 남겼다.

### 결정 2: 기본 형태는 `.js`, 세 모드 플래그 제공

- **상황**: TASK-019 결정 지점 1 — 방출 지정자의 올바른 형태는 소비 측
  `moduleResolution`에 달려 있어 rlc가 혼자 정할 수 없다.
- **검토한 대안**: `.js` 고정 / 확장자 없음 고정 / `tsconfig.json` 자동 판별 /
  모드 플래그.
- **선택과 근거**: `--rewrite-imports <js|bare|off>` 플래그, 기본 `js`.
  `./x.js`는 `nodenext`(Node ESM)에서 필수이고 `bundler` 해석에서도 tsc의
  `.js`→`.ts` 대응으로 통과하므로 양쪽에서 동작하는 유일한 기본값이다
  (통합 테스트 `cross_file_rl_import_typechecks_and_runs`가 `bundler` 모드
  tsc + node 실행으로 확인). `bare`는 확장자 없는 해석을 선호하는 번들러
  프로젝트용. `tsconfig.json` 자동 판별은 rlc가 TS 설정 해석기를 떠안는
  비용 때문에 TASK-019의 판단대로 배제했다.

### 결정 3: 파서는 "상대 경로 `.rl` 지정자"만 AST로 들어올린다

- **상황**: 지정자 재작성을 파이프라인 어디에서 할지. 모든 import 문을 AST
  노드로 만들 수도, 재작성 대상만 표시할 수도 있다.
- **검토한 대안**:
  - 모든 정적 import를 AST 노드화: 2단계(선언 수집)에 미리 대비하지만, 지금
    쓰지 않는 구조가 생기고 통과 영역이 넓게 재구성된다.
  - 재작성 대상 지정자 문자열의 바이트 범위만 `Segment::RlImport(Span)`으로
    들어올리고 문장의 나머지는 verbatim 유지.
- **선택과 근거**: 후자. 통과 계약이 바이트 단위라서 건드리는 범위가 좁을수록
  안전하고, 재작성에 필요한 정보가 지정자 범위뿐이다. 2단계가 오면 그때
  지정자에서 경로를 읽으면 되므로 미리 구조를 키울 이유가 없다. codegen은
  기존 세그먼트 방출 구조에 arm 하나만 추가된다.

### 결정 4: import 절 스캔은 보수적으로 — 조금이라도 어긋나면 통과

- **상황**: `import`/`export` 뒤 문법은 다양하다(기본/네임스페이스/명명
  import, `type` 한정자, re-export, 동적 import, `import.meta`, TS
  import-assignment). 파서는 무오류(infallible)여야 한다.
- **검토한 대안**: 완전한 import 문법 파서 작성 / 지정자만 찾는 토큰 스캔.
- **선택과 근거**: 토큰 스캔. `import` 뒤 `(`·`.`(동적/`import.meta`)는 즉시
  포기, `export` 뒤 첫 토큰은 `{`·`*`·`type`만 허용(그 외는 re-export가
  아님), 절 안에서는 식별자·`{...}`·`*`·`,`만 허용하고 예약어(`=` 포함 기타
  기호도)를 만나면 포기한다. 기존 rl 구문들과 같은 "완전히 파싱될 때만 변환"
  원칙의 적용이다. 포기 = 원문 통과이므로 오탐의 비용이 없다.

## 작업 내역

- 2026-08-17: TASK-020 등록. 구현 순서: ast → parser/imports → codegen →
  lib(Options) → main(CLI) → 테스트 3계층 → 문서.
- 2026-08-17: `ast.rs`에 `Segment::RlImport(Span)` 추가 — 지정자 문자열
  (따옴표 포함)의 바이트 범위만 담는다.
- 2026-08-17: `parser/imports.rs` 신설. `parse_rl_import`가 `import`/`export`
  키워드 뒤를 토큰 스캔한다: side-effect import는 즉시 문자열, `(`·`.`은
  포기(동적 import/`import.meta`), re-export는 첫 토큰 `{`·`*`·`type`만 허용,
  절 안에서는 식별자·`{...}`·`*`·`,`만 허용하고 예약어·기타 기호(`=` 등)는
  포기. `from` 뒤 문자열이 `./`·`../`로 시작하고 `.rl`로 끝날 때만 Span 반환.
  `parser/mod.rs`의 메인 루프에서 enum 시도 다음에 연결 — verbatim은
  지정자 시작까지 flush하고 지정자만 세그먼트로 들어올린다.
- 2026-08-17: `sema.rs`는 `RlImport`를 무시(arm 추가만). `codegen/mod.rs`에
  `emit_rl_import` 추가 — 마지막 4바이트(`.rl` + 닫는 따옴표)를 모드별로
  치환. `lib.rs`에 공개 `ImportRewrite`(Js/Bare/Off, 기본 Js)와
  `Options::rewrite_imports` 추가, `main.rs`에 `--rewrite-imports` 파싱 추가.
- 2026-08-17: 스모크 테스트 — 정적 import 9형태 재작성, 비상대/동적/
  import-assignment/문자열·주석 내 텍스트 통과, `off` 모드가 원문과 diff
  없음을 `rlc -p`로 확인.
- 2026-08-17: 테스트 3계층 추가. `compile.rs` 9건(기본 js 재작성, 전 형태
  커버, 따옴표 스타일·`../` 유지, bare/off 모드, 비상대·동적·
  import-assignment 통과, rl 구문과의 합성), `passthrough.rs` 4건(비-`.rl`
  지정자, 문자열·주석·템플릿 내 텍스트, 동적 import, `export` 선언 오인 방지),
  `integration.rs` 2건(2파일 프로젝트를 기본 모드로 컴파일 → tsc 타입체크 +
  node 실행 결과 확인, bare 모드 tsc `bundler` 타입체크).
- 2026-08-17: 문서 갱신 — `language.md` §7 신설(§7·§8 → §8·§9 재번호,
  §1에 계약 예외 명시), `cli.md` 플래그 표·예시, `CLAUDE.md` 계약 1 예외와
  아키텍처 맵, `compiler-architecture.md` Segment 목록(누락돼 있던
  Try/LetElse 포함), `module-graph.md` 1단계 구현됨·결정 1/2 기록,
  `CHANGELOG.md` Unreleased, README 특징 목록. `lib.rs` 크레이트 문서의
  "three constructs" 서술도 실제(4개 구문 + import 재작성)로 바로잡았다.

## 이슈 및 해결

### 이슈 1: import-assignment가 verify에서 거부될 것이라는 가정이 틀렸다

- **증상**: `import fs = require("./legacy.rl");` 통과 테스트를 처음에
  `verify: false`로 작성했다 — swc가 이 TS 전용 구문을 모듈 컨텍스트에서
  거부할 것으로 가정했다.
- **원인**: 가정을 실행으로 확인하지 않았다. `rlc -p`로 돌려보니 verify가
  켜진 채로도 정상 통과했다 (swc는 import-assignment를 파싱한다).
- **해결**: 테스트를 기본 옵션(`ok`)으로 단순화하고 잘못된 주석을 제거했다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` — 단위 80 / 통합 17(tsc·node 실행 포함) / 통과 계약 35 /
  stdlib 2 / doctest 4, 전부 통과

## 결과

- 추가: `src/parser/imports.rs`, `docs/tasks/TASK-020-import-specifier-rewrite.md`
- 수정: `src/ast.rs`, `src/parser/mod.rs`, `src/sema.rs`, `src/codegen/mod.rs`,
  `src/lib.rs`(공개 API: `ImportRewrite`, `Options::rewrite_imports`),
  `src/main.rs`(`--rewrite-imports`), `tests/{compile,passthrough,integration}.rs`,
  `docs/reference/{language,cli}.md`, `docs/design/{module-graph,compiler-architecture}.md`,
  `CLAUDE.md`, `README.md`, `CHANGELOG.md`, `docs/tasks/INDEX.md`

후속: 2단계(선언 수집과 프로젝트 단위 소진성)는 `module-graph.md`의 결정
지점 3·4(순환 import, 에러 위치 보고)에 답이 정해지면 별도 태스크로 등록.
