# TASK-050: 방출 매핑 기반 TS 위임 — 컴파일 출력을 가상 문서로

- **상태**: 완료
- **시작일**: 2026-08-17
- **완료일**: 2026-08-17
- **커밋**: c044e1c

## 목적

언어 서버의 TypeScript 위임(TASK-024/025)은 지금까지 `.rl` 원문을 그대로 TS로
서빙하고 TS 파서의 error recovery에 기댔다. 통과 영역에서는 정확하지만 rl 구문
내부(match 암 본문, 스크루티니, try 식 등)에서는 TS가 깨진 구문을 보므로
hover/자동완성/타입 정보가 best-effort에 그친다. rlc는 어차피 rl 구문을 순수
TypeScript로 변환하므로, **방출 결과를 가상 문서로 TS 언어 서비스에 서빙하고
원본↔방출 오프셋 매핑으로 질의/결과를 왕복**시키면 rl 구문 내부에서도 TS의
완전한 타입 추론을 얻는다 (Svelte/Volar의 가상 코드 방식). 이를 이용해 match
자동완성이 스크루티니의 TS 추론 타입으로 enum을 특정하는 기능도 함께 넣는다.

자체 타입 추론 엔진은 만들지 않는다 — rl 구문은 임의의 TS 식을 품으므로
"rl에 한한 추론"의 경계는 타입 차원에서 존재하지 않고, 결국 tsc 재구현으로
수렴한다 (TASK-042 §6과 같은 결론). 추론은 전부 TS가 하고 rl은 결과만 쓴다.

## 범위

- 포함:
  - codegen을 소스 복사 조각을 추적하는 Rope 기반으로 재구성 (출력 바이트
    불변 — 기존 스냅샷 테스트가 계약).
  - 공개 API `emit_mapped(source) -> MappedEmit { code, mappings }` —
    파싱+방출만 수행 (sema/verify 생략: 편집 중 소진성 오류가 있어도 방출),
    import 재작성 off, 바이트 오프셋 매핑.
  - CLI `rlc --emit-map` — 입력별 `{file, code, mappings}` JSON 출력.
  - 언어 서버: 문서 변경 시 `--emit-map` 실행, 열린 문서를 방출 코드로
    TsProject에 서빙, 위임 기능(hover/definition/completion/references/
    rename)의 질의·결과 오프셋 양방향 변환 (UTF-16↔바이트 변환 포함).
  - `TsProject.typeAt()` — 위치의 TS 추론 타입 (이름 + 선언 파일). match
    자동완성에서 structural `inferEnum()` 실패 시 스크루티니 타입으로 enum
    특정 (이름 + 선언 파일 교차 검증, 실패 시 기존 전체 pool 동작 유지).
  - 세 계층 테스트: Rust 매핑 불변식·방출 동일성, LSP 매핑 변환·typeAt.
- 제외:
  - 자체 타입 추론 구현 (설계상 금지).
  - 열려 있지 않은 `.rl` 파일의 가상 문서화 (디스크 파일은 기존처럼 원문
    서빙 — error recovery 폴백, 후속 태스크 여지).
  - TS 진단 표면화 (계약: 진단은 `rlc --check`만).
  - enum 선언 내부(필드 타입 텍스트)의 세밀 매핑 — rl 자체 hover가 담당.

## 의사결정

### 결정 1: 자체 추론 엔진 대신 방출 코드를 TS에 서빙

- **상황**: rl 구문 내부의 타입 정보를 어떻게 얻을지. 후보는 (A) rl 자체
  경량 추론 엔진, (B) 원문 + error recovery (현행), (C) 방출 코드 + 매핑.
- **검토한 대안**: (A)는 rl 구문이 임의 TS 식을 품어 경계를 그을 수 없고
  tsc 재구현으로 수렴 — 프로젝트 철학(타입은 TS 소관) 위반. (B)는 스크루티니
  타입까지는 실측으로 동작 확인(quickInfo로 `Shape` 회수)했으나 rl 구문
  내부는 원리적으로 불가. (C)는 방출이 순수 TS라 완전한 추론을 얻고, 방출이
  span 기반 복사라 매핑을 정확히 만들 수 있다.
- **선택과 근거**: (C), (B)를 폴백으로. 임베디드 언어 도구의 표준 구조
  (Volar/svelte-language-tools)와 같고, rl은 방출물이 이미 순수 TS라는 점에서
  이 구조에 특히 유리하다.

### 결정 2: 매핑은 codegen 내부 Rope로 방출과 동시에 생성

- **상황**: 원본↔방출 매핑을 어떻게 얻을지.
- **검토한 대안**: (A) 방출 후 텍스트 정렬(diff)로 복원 — 글루 텍스트와
  소스 텍스트가 우연히 일치하면 오정렬, 근본적으로 휴리스틱. (B) codegen이
  소스 복사 지점(`Segment::Verbatim`, 템플릿 Raw, 스크루티니/암 본문/가드 등
  하위 프로그램 방출)을 조각(Lit/Src) 단위로 기록하는 Rope로 조립 후 평탄화.
- **선택과 근거**: (B). 복사 지점은 codegen만 알고 있으므로 그 자리에서
  기록하는 것만이 정확하다. 출력 문자열은 조각 연결 결과 그대로라 바이트
  불변이 자명하고, 기존 스냅샷 테스트가 이를 강제한다.

### 결정 3: `--emit-map`은 sema/verify를 생략한다

- **상황**: 편집 중인 버퍼는 소진성 오류 등 rl 수준 오류를 자주 가진다.
  `compile()`을 그대로 쓰면 오류 시 방출물이 없어 가상 문서가 원문/방출
  사이를 오간다.
- **검토한 대안**: (A) compile() 재사용 — 오류 버퍼에서 편집기 기능 저하.
  (B) 파싱(무오류)+codegen(무오류)만 수행하는 전용 경로 — 항상 방출 가능,
  진단은 기존 `--check`가 그대로 담당하므로 에러 계층 분리 계약 유지.
- **선택과 근거**: (B). 파이프라인 단계 분리가 이미 있어 조합만 바꾸면 된다.

### 결정 4: import 재작성은 off, 매핑은 바이트 오프셋

- **상황**: 가상 문서의 import가 TsProject의 기존 해석기와 맞아야 하고,
  Rust(바이트)와 LSP(UTF-16) 좌표계가 다르다.
- **선택과 근거**: 방출 시 `.rl` 지정자를 그대로 두면(off) TsProject의
  커스텀 해석기가 기존과 동일하게 동작한다 — 새 해석 경로 없음. 매핑은
  컴파일러의 자연 단위인 바이트로 방출하고, UTF-16 변환은 텍스트를 가진
  LSP 쪽에서 1회 전처리로 해결한다 (`--symbols`가 line/col로 변환해 주는
  것과 달리 매핑은 수가 많아 바이트가 간결).

### 결정 5: match 자동완성은 structural 우선, TS 추론은 폴백

- **상황**: 스크루티니 타입 기반 enum 특정과 기존 `inferEnum()`의 우선순위.
- **선택과 근거**: structural 우선. 기존 동작(명시적 `Enum.` 단서, 암 태그
  소유자)이 성공하는 경우를 한 글자도 바꾸지 않는 순수 추가 변경이 되고,
  TS 질의는 실패 시에만 발생해 비용도 최소다. TS 결과는 이름 문자열만 믿지
  않고 타입 심볼의 선언 파일을 로컬(현재 파일)/임포트(`imported.path`)와
  교차 검증해 동명 TS 타입 오탐을 막는다.

## 작업 내역

- 2026-08-17: 사전 실측 — TsProject와 동일한 호스트 구성으로 TS 6.0.2에 rl
  원문을 서빙해 quickInfo/checker 질의: 함수 반환·체이닝·빈 match 본문·연속
  match 모두에서 스크루티니 타입 이름과 선언 파일이 정확히 회수됨을 확인
  (원문 폴백 경로의 근거). 코드베이스 조사 — codegen의 String 조립 구조,
  server.ts 위임 지점, `--symbols` JSON 선례, 편집기 테스트 실행 방식 확인.
- 2026-08-17: `src/codegen/rope.rs` 신설 — Lit/Src 조각 로프. `trim()`은
  평탄화 텍스트의 `str::trim`과 동일 의미(유니코드 공백 포함, Src 앞
  트리밍 시 src 오프셋 보정), `flatten()`은 양 좌표 연속 조각을 병합.
- 2026-08-17: `codegen/mod.rs`·`codegen/matches.rs`를 로프 조립으로 재구성
  (기존 `format!` 조립을 조각 push 순열로 1:1 치환; if-chain/튜플 match의
  중복 exit 조립은 `arm_exit`로 통합). enum 방출은 통째로 Lit — 선언 내부
  매핑은 범위 제외. `.rl` import 재작성은 확장자 앞까지 Src 유지.
  `cargo test` 전체(스냅샷 포함) 통과로 출력 바이트 불변 확인.
- 2026-08-17: `lib.rs`에 `EmitMapping`/`MappedEmit`/`emit_mapped()` 공개,
  `main.rs`에 `--emit-map` 모드(`--symbols`와 같은 JSON 배열 관례) 추가,
  `tests/emit_map.rs` 신설 — 매핑 불변식(원본=출력 바이트 동일·경계 내·
  무겹침), 전 구문 코퍼스, sema 오류 시 무오류 방출, `compile()`(off)과의
  바이트 동일성. `docs/reference/cli.md`에 `--emit-map` 절 추가.
- 2026-08-17: LSP — `virtual.ts` 신설(바이트↔UTF-16 변환기 + `MappedDoc`
  양방향 조각 탐색, 조각 끝 오프셋 포함 의미), `rlc.ts`에 `runEmitMap`,
  `tsproject.ts`에 `typeAt(fileName, offset, within?)`(가장 넓은 포함 식
  노드의 checker 타입, 제네릭 인자 스트립, 선언 파일 동봉)과 OpenDoc
  version의 문자열 허용(raw/emitted 구분 캐시 키).
- 2026-08-17: `server.ts` — 가상 문서 저장소(버전 일치 시에만 활성),
  validate 디바운스에 `refreshVirtual` 동승, TsProject 호스트가 활성 가상
  문서를 서빙(`{version}:emitted`/`{version}:raw`), 위임 기능 전부(정의·
  호버·완성·참조·이름 변경)의 질의/결과 오프셋을 `toServiceOffset`/
  `fromServiceSpan`으로 왕복(이름 변경은 한 위치라도 못 되돌리면 전체
  거부). match 암 완성에 `tsScrutineeEnum` — 구조적 `inferEnum` 실패 시
  스크루티니의 TS 추론 타입을 visible enum과 이름+선언 파일로 교차 검증.
- 2026-08-17: 편집기 테스트 — `virtual.test.ts`(변환·글루 null·멀티바이트·
  조각 경계), `tsproject.test.ts`에 typeAt 3건, `emitmap.test.ts`(실제
  rlc `--emit-map` 출력 E2E: 스크루티니 타입, **암 본문 내부 hover**, 암
  본문발 정의 이동 — rlc 없으면 스킵). README·compiler-architecture.md
  갱신, 검증 게이트 전체 실행.

## 이슈 및 해결

### 이슈 1: 사전 실측 하네스에서 `.map()` 체인 타입이 전부 `any`

- **증상**: 실측 스크립트에서 `[getShape()].map((s) => s)[0]`의 타입이
  순수 TS 대조군까지 포함해 `any`로 나옴 — "체이닝은 추론 불가"로 오판할
  뻔함.
- **원인**: 하네스의 `getScriptSnapshot`이 인메모리 파일만 서빙하고
  lib.d.ts를 디스크에서 읽지 못해 `Array.prototype.map`이 `any`가 된 것.
  실제 `TsProject`는 디스크 폴백이 있어 무관한 하네스 버그.
- **해결**: 스냅샷에 `ts.sys.readFile` 폴백 추가 후 재실측 — 전 케이스
  정확한 타입 회수 확인. 실측 하네스도 대상 코드와 같은 폴백 구조를
  가져야 한다는 교훈.

### 이슈 2: `Rope`가 `pub(super)`라 공개 도달 가능 API에서 가시성 경고

- **증상**: `cargo build`에서 `private_interfaces` 경고 —
  `Emitter::emit_program`(pub(crate) 도달)이 `pub(in crate::codegen)`인
  `Rope`를 반환.
- **원인**: mod.rs의 `pub(super)`는 crate 루트, rope.rs의 `pub(super)`는
  codegen — 같은 표기의 다른 스코프.
- **해결**: `Rope`를 `pub(crate)`로. clippy `-D warnings` 게이트 통과.

### 이슈 3: 바이트→UTF-16 역변환 테이블 구축이 최악 O(n²)

- **증상**: 초기 구현이 문자마다 `fill(mark, byte)`로 배열 끝까지 덮어씀 —
  비ASCII 위주 대용량 파일에서 2차 시간.
- **원인**: 마지막 쓰기가 이기는 성질에 기댄 단순화.
- **해결**: 문자 구간 `[시작, 다음 시작)`만 채우도록 수정 — 선형 시간.
  ASCII 파일(대부분)은 테이블 없이 항등 변환.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` (9개 스위트 전부 통과 — 기존 스냅샷 포함 245+)
- [x] `npm test` (editors/vscode) — rlc를 PATH에 두고 50/50 통과
  (E2E: 가상 문서에서 match 암 본문 내부 hover가 `radius: number` 회수)

## 결과

- 신규: `src/codegen/rope.rs`, `tests/emit_map.rs`,
  `editors/vscode/server/src/virtual.ts`,
  `editors/vscode/server/src/test/virtual.test.ts`,
  `editors/vscode/server/src/test/emitmap.test.ts`, 본 태스크 문서.
- 갱신: `src/codegen/mod.rs`·`matches.rs`(로프 조립), `src/lib.rs`
  (`emit_mapped` 공개 API), `src/main.rs`(`--emit-map`),
  `editors/vscode/server/src/{rlc,tsproject,server}.ts`(가상 문서 서빙 +
  오프셋 왕복 + 스크루티니 타입 완성), `docs/reference/cli.md`,
  `docs/design/compiler-architecture.md`, `editors/vscode/README.md`,
  `docs/tasks/INDEX.md`.
- 후속 여지(필요 시 별도 태스크): 열려 있지 않은 `.rl` 파일의 가상
  문서화(현재는 디스크 원문 서빙 폴백), enum 선언 내부 필드 타입의 세밀
  매핑, `if let`/`try` 전용 완성에 typeAt 재사용.
