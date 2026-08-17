# TASK-029: `rlc --sidecar` — 에디터 사이드카 생성

- **상태**: 완료
- **시작일**: 2026-08-17
- **완료일**: 2026-08-17
- **커밋**: —

## 목적

[TASK-028](./TASK-028-ts-sidecar-declarations.md)에서 검증한 사이드카를 rlc가
직접 만들도록 한다. `.ts` 파일이 `.rl`을 import할 때 나던 `TS2307`을 없애고,
정의 이동이 `.d.ts`가 아니라 **원본 `.rl`**로 가게 하는 것이 목표다.

## 범위

- 포함: `src/sidecar.rs`(맵 생성), 공개 API `build_sidecar`/`Sidecar`,
  CLI `--sidecar <dir>`, 단위 테스트 6개, `cli.md` 갱신.
- 제외: 선언 본문 생성. tsc `--emitDeclarationOnly`가 만든 것을 입력으로 받는다.
- 제외: 언어 서버 연동(저장 시 자동 갱신). 지금은 빌드 스텝이다.

## 의사결정

### 결정 1: 선언 본문은 받아쓰고 위치만 채운다

- **상황**: TASK-028의 결정 지점 1 — `.d.ts` 본문을 rlc가 생성할지.
- **검토한 대안**:
  - rlc가 전부 생성: 외부 도구가 필요 없다. 대신 통과 영역 함수·상수의
    반환 타입을 추론해야 하는데 rlc는 타입 검사기가 아니다.
  - tsc가 본문, rlc가 맵: 각자 아는 것만 한다.
- **선택과 근거**: 후자. 에러 계층 분리 계약("타입은 tsc의 책임")과 일치하고,
  rlc가 새로 구현할 범위가 "선언이 원본 어디에서 왔는가" 하나로 줄어든다.
  `build_sidecar(source, declarations, rl_file_name)`이 그 계약을 그대로
  드러낸다.

### 결정 2: 사이드카는 `.rl` 옆에 쓴다

- **상황**: TASK-028의 결정 지점 2 — 배치 위치.
- **검토한 대안**: `.rl` 옆 / 별도 디렉터리 + tsconfig `paths`.
- **선택과 근거**: `.rl` 옆. 상대 경로 지정자는 `paths`를 타지 않으므로
  별도 디렉터리는 추가 장치가 필요하고, TypeScript의 해석 규칙상
  `"./x.rl"` → `x.rl.d.ts`가 이미 성립한다. `-o`와 무관하게 입력 옆에
  쓰는 이유도 같다 — 에디터가 보는 것은 소스 트리다.

### 결정 3: 위치는 enum은 파싱 결과에서, 나머지는 이름으로 찾는다

- **상황**: TASK-028의 결정 지점 3 — 대응표 입도.
- **검토한 대안**:
  - 전체 오프셋 대응표: codegen이 통과 구간을 복사하므로 정확한 rl↔ts
    대응을 낼 수 있다. 대신 codegen에 수집 지점을 심어야 하고, tsc가 만든
    `.d.ts.map`을 디코드해 합성해야 한다.
  - 선언 단위: 각 선언 이름의 원본 위치만 찾는다. rl enum은
    `enum_symbols`가 정확한 오프셋을 주고, 통과 영역 선언은 소스에서 선언
    키워드로 찾는다.
- **선택과 근거**: 선언 단위. 정의 이동에 필요한 정보가 그뿐이라 v1으로
  충분하고, codegen과 tsc 맵 디코더를 건드리지 않아 변경 범위가 작다.
  같은 이름이 여러 번 선언되면 첫 일치를 쓴다는 한계가 있고, 필요해지면
  전체 대응표로 올릴 수 있다.

### 결정 4: 세그먼트를 0열과 이름 열 두 곳에 둔다

- **상황**: TASK-028의 이슈 1에서 드러난 조건.
- **선택과 근거**: 정의 이동은 심볼 **이름이 시작하는 열**의 대응을 묻는다.
  0열에만 두면 매핑을 찾지 못해 `.d.ts`에 그대로 선다. 두 곳에 두는 것이
  안전하다 — 0열은 줄 단위 조회의 보험이다. 회귀를 막으려고
  `each_declaration_gets_a_segment_at_its_name_column` 테스트로 고정했다.

## 작업 내역

- 2026-08-17: `src/sidecar.rs` 작성. `build_sidecar`가 `.d.ts` 각 줄에서
  선언 이름을 뽑고(`declared_name`), 원본에서 위치를 찾고(`locate` — enum은
  `enum_symbols`, 나머지는 `declares`로 줄 스캔), source map v3 `mappings`로
  인코딩한다(`encode_mappings`, `vlq`). 열은 UTF-16 코드 유닛으로 센다
  (`utf16_column`) — 한국어 주석이 있는 줄에서도 어긋나지 않게.
- 2026-08-17: `src/lib.rs`에 `mod sidecar;`와
  `pub use sidecar::{Sidecar, build_sidecar};` 추가.
- 2026-08-17: `src/main.rs`에 `--sidecar <dir>` 플래그와 `sidecar_mode`
  추가. `--symbols`와 같이 컴파일하지 않는 모드다.
- 2026-08-17: `tests/sidecar.rs` 작성 — VLQ 디코더를 테스트 안에 두고
  좌표를 직접 검증한다(불투명한 문자열 비교 대신). 6개.
- 2026-08-17: `docs/reference/cli.md`에 옵션 행과 "에디터 사이드카" 절 추가.
- 2026-08-17: `source/rl-interop` 예제를 Node 프로토타입
  (`scripts/sidecar.mjs`)에서 `rlc --sidecar types src/notice.rl`로 교체하고
  tsserver를 구동해 동일한 결과를 확인했다.
  ```
  진단: 없음
  main.ts:23  render  → src/notice.rl:21:17
  main.ts:10  Notice  → src/notice.rl:9:13   (타입·값 두 선언이 같은 위치로)
  ```

## 이슈 및 해결

없음. TASK-028의 프로토타입에서 두 함정(이름 열 세그먼트, 추론 프로젝트)을
미리 확인해 둔 덕분에 구현에서 새로 만난 문제는 없었다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` — 90 + 21 + 35 + 6(신규) + 2 전부 통과
- [x] tsserver 구동으로 정의 이동 확인 (위 "작업 내역")

## 결과

- 추가: `src/sidecar.rs`, `tests/sidecar.rs`,
  `docs/tasks/TASK-029-sidecar-mode.md`
- 수정: `src/lib.rs`, `src/main.rs`, `docs/reference/cli.md`,
  `docs/tasks/INDEX.md`

후속: 언어 서버가 저장 시 사이드카를 갱신하는 것(TASK-028 결정 지점 5),
그리고 같은 이름의 중복 선언을 다루는 전체 오프셋 대응표.
