# TASK-054: AI 가이드 확장(설치·업데이트·워크플로)과 `rlc help` 주제별 헬프

- **상태**: 완료
- **시작일**: 2026-08-18
- **완료일**: 2026-08-18
- **커밋**: —

## 목적

사용자 요청 두 가지: ① AI 가이드(`docs/ai/rl.md`)가 언어 내용만 다루므로
설치·업데이트·프로젝트 셋업·개발 워크플로까지 포함해 완결시키고, ② CLI에
헬프 기능을 넣어 (특히 AI가) 문법·워크플로를 더 쉽게 찾을 수 있게 한다.

## 범위

- 포함: `docs/ai/rl.md`에 Install/Setup/Workflow 섹션 추가(기존 Build 섹션
  재구성), `rlc help [topic]` 서브커맨드(가이드 임베드·주제별 출력), 단위·CLI
  테스트, `cli.md`/`errors.md`/README/docs·ai README 갱신.
- 제외: `-h`(옵션 헬프)의 형식 변경, 셸 자동완성, man 페이지.

## 의사결정

### 결정 1: 헬프 콘텐츠 소스 — `docs/ai/rl.md`를 `include_str!`로 임베드

- **상황**: `rlc help`가 보여줄 문서를 어디서 가져올지.
- **검토한 대안**: ① 헬프 텍스트를 main.rs에 하드코딩(가이드와 이중화 —
  어긋남 필연), ② `docs/reference/`를 임베드(사람용 산문이라 크고, 4파일
  구조가 주제 분할과 안 맞음), ③ 압축된 AI 가이드 `docs/ai/rl.md` 한 파일을
  `include_str!`로 임베드하고 `##` 헤딩을 주제 경계로 사용.
- **선택과 근거**: ③. 단일 진실 소스가 유지되고(가이드를 고치면 다음 빌드의
  헬프도 바뀜), 압축 표기라 바이너리 증가가 ~10KB로 무시 가능하며, 주제
  분할이 문서 구조에서 자동으로 나온다. 단위 테스트가 모든 주제 헤딩의
  존재를 검증하므로 문서 구조 변경이 헬프를 조용히 깨뜨릴 수 없다.

### 결정 2: 표면 — `rlc help [topic]` 서브커맨드 (첫 인자일 때만)

- **상황**: 기존 CLI는 서브커맨드 없이 옵션+입력 구조. 헬프를 어떤 형태로
  노출할지.
- **검토한 대안**: ① `--help-topic <t>` 옵션(기존 스타일과 일관되지만 발견성
  낮음), ② `rlc help [topic]` 서브커맨드(관례적·짧음, 단 `help`라는 입력
  파일명과 충돌 가능).
- **선택과 근거**: ②. `git help`/`npm help` 관례라 사람도 AI도 추측으로
  찾아낸다. 충돌은 **첫 번째 인자일 때만** 서브커맨드로 인식해 해소 —
  `help` 파일은 `./help`나 `rlc --check help`(옵션 뒤)로 넘길 수 있고, 이
  동작을 cli.md에 명시하고 CLI 테스트로 고정했다.

### 결정 3: Build 섹션을 Install/Setup/Workflow로 분리

- **상황**: 설치·업데이트·개발 루프를 추가하면 기존 Build 섹션 하나로는
  주제가 섞인다.
- **선택과 근거**: 헬프 주제 경계가 `##` 헤딩이므로, "설치가 궁금할 때
  `rlc help install`"처럼 질문 단위와 섹션 단위를 일치시키는 3분할이 맞다.
  기존 Build 내용은 Setup(프로젝트 구성)과 Workflow(명령·루프)로 재배치.

## 작업 내역

- 2026-08-18: `docs/ai/rl.md` — `## Build`를 `## Install`(npm/cargo 설치,
  업데이트 절차, TS 5/6 요건, VSCode 확장), `## Setup`(신규 프로젝트 스캐폴드,
  scripts/tsconfig, gitignore, unplugin 대안), `## Workflow`(편집 루프,
  check/types/build 명령, CI, `@generated` 금지, `rlc help` 안내)로 재구성.
- 2026-08-18: `src/main.rs` — `GUIDE`(`include_str!`)·`HELP_TOPICS`(14주제 +
  별칭)·`guide_section`(헤딩 경계 슬라이스)·`run_help` 추가, `main()` 최상단에
  첫 인자 `help` 분기, `usage()`에 시놉시스 한 줄 추가. 단위 테스트 3개
  (`help_tests`: 전 주제 해석, 섹션 경계, 이름·별칭 유일성).
- 2026-08-18: `tests/cli.rs` 신설 — `CARGO_BIN_EXE_rlc`로 바이너리 실행:
  주제 목록·전 주제 출력, 섹션 경계, 별칭·대소문자, `help all` = 파일 전체,
  미지 주제 종료 코드 1 + stdout 오염 없음, `--check help`는 입력 경로로
  처리(총 6개).
- 2026-08-18: 레퍼런스 갱신 — `cli.md`(시놉시스, "주제별 헬프" 섹션: 주제·별칭
  표, 첫 인자 규칙, stdout 목록에 `help` 추가), `errors.md`(CLI 표에 미지
  주제·다중 주제 에러 2행). `README.md` 사용 예에 `rlc help` 한 줄,
  `docs/ai/README.md`에 임베드 조회 안내.
- 2026-08-18: 수동 확인 — `rlc help`/`help match`/`help install`/`help nosuch`
  (종료 1) 출력 검증. 검증 게이트 3종 통과.

## 이슈 및 해결

없음.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`
- [x] AI 제공 문서 반영 확인 — `docs/ai/rl.md` Workflow 섹션에 `rlc help`
  사용법 포함 (이 태스크 자체가 그 문서를 확장)

## 결과

- 수정: `docs/ai/rl.md`(Install/Setup/Workflow), `src/main.rs`(+help 구현·
  단위 테스트), `docs/reference/cli.md`, `docs/reference/errors.md`,
  `README.md`, `docs/ai/README.md`, `docs/tasks/INDEX.md`.
- 신규: `tests/cli.rs`.
- `rlc help`(목록) / `rlc help <주제|별칭>`(섹션) / `rlc help all`(전체)이
  오프라인으로 동작하며, 가이드 파일이 단일 진실 소스다.
