# TASK-047: 에디터 `.rl` 파일 아이콘

- **상태**: 완료
- **시작일**: 2026-08-17
- **완료일**: 2026-08-17
- **커밋**: f67dfc3

## 목적

에디터(VSCode) 탐색기·탭에서 `.rl` 파일이 일반 텍스트 파일과 구분되지 않는다.
다른 언어(TypeScript, Go 등)처럼 파일 아이콘을 제공해 `.rl` 파일을 한눈에
알아볼 수 있게 한다.

## 범위

- 포함: VSCode 확장(`editors/vscode`)에 언어 기본 아이콘
  (`contributes.languages[].icon`) 추가 — 라이트/다크 테마용 SVG 두 벌,
  확장 README 기능 표 갱신.
- 제외: 파일 아이콘 테마(iconTheme) 전체 기여 — 모든 파일 유형의 아이콘을
  다시 정의하는 것은 과잉이고, 언어 기본 아이콘만으로 목적을 달성한다.
  마켓플레이스 확장 대표 아이콘(PNG)도 이번 범위 밖.

## 의사결정

### 결정 1: 아이콘 테마가 아니라 언어 기본 아이콘으로 기여

- **상황**: VSCode에서 파일 아이콘을 보이게 하는 방법은 두 가지다 —
  ① `contributes.iconThemes`(파일 아이콘 테마 전체를 새로 정의),
  ② `contributes.languages[].icon`(VSCode 1.64+, 언어별 기본 아이콘).
- **검토한 대안**:
  - 아이콘 테마 기여: 어떤 테마를 쓰든 무조건 보이지만, 사용자가 테마를
    통째로 갈아타야 하고 `.rl` 외 모든 파일의 아이콘까지 책임져야 한다.
  - 언어 기본 아이콘: 선언 한 줄 + SVG 두 개로 끝나고 사용자의 현재 아이콘
    테마를 존중한다. 언어 아이콘을 지원하는 테마(기본 Seti 포함)에서
    자동으로 보이고, 자체 `.rl` 아이콘을 정의한 테마가 있으면 그쪽이
    우선한다 — 다른 언어 확장들(예: Go, Svelte)이 쓰는 표준 방식.
- **선택과 근거**: 언어 기본 아이콘. 침습이 없고 유지 비용이 0에 가까우며,
  다른 언어 확장들의 관례와 일치한다.

### 결정 2: 디자인 — 둥근 사각 배지 + "RL" 모노그램, 러스트 오렌지

- **상황**: 다른 언어 아이콘을 참고해 `.rl`의 정체성이 드러나는 디자인이
  필요했다.
- **검토한 대안**:
  - TS 스타일(단색 사각 + 약자): TypeScript(#3178C6 파랑 + "TS"),
    JavaScript(노랑 + "JS"), Go 등 대다수 언어 아이콘의 관례. rl이 "TS 위에
    여섯 가지를 얹은 언어"이므로 같은 가족으로 보이는 것이 맞다.
  - Rust 스타일(기어 문양): rl 구문의 유래이긴 하나 언어 자체는 TS 계열이고,
    문양은 16px에서 뭉개진다.
  - 색: TS와 같은 파랑 계열은 TS 아이콘과 혼동된다. rl 구문(enum/match/
    let-else)의 유래인 Rust를 반영해 러스트 오렌지 계열을 선택 —
    라이트 테마 `#CE422B`, 다크 테마는 어두운 배경 대비를 위해 약간 밝힌
    `#E25E3E`.
  - 글자 표기: 소문자 "rl"이 브랜딩과 일치하지만 16px에서 판독이 어렵다
    (r의 훅이 사라짐). 대문자 "RL"이 작게 렌더링돼도 읽힌다.
- **선택과 근거**: 32×32 viewBox, 둥근 사각(#rx=3) 배지에 흰 대문자 "RL".
  글자는 `<text>`가 아니라 스트로크 패스로 그려 시스템 폰트 유무와 무관하게
  항상 같은 모양으로 렌더링되게 했다(아이콘 SVG에서 `<text>`는 폰트 메트릭에
  따라 배치가 흔들린다).

## 작업 내역

- 2026-08-17: `editors/vscode/icons/rl-file-light.svg`,
  `rl-file-dark.svg` 작성 — 동일한 지오메트리(스트로크 기반 R·L 패스),
  배경색만 테마별로 다름.
- 2026-08-17: `editors/vscode/package.json`의 `contributes.languages[0]`에
  `"icon": { "light": "./icons/rl-file-light.svg", "dark":
  "./icons/rl-file-dark.svg" }` 추가. `.vscodeignore`는 `icons/`를 제외하지
  않으므로 vsix 패키징에 포함됨(확인: `.vscodeignore`에 svg/아이콘 관련
  패턴 없음).
- 2026-08-17: 헤드리스 Chromium으로 라이트/다크 배경에서 96px·16px 렌더링을
  스크린샷으로 확인 — 16px에서도 "RL" 판독 가능, 두 테마 모두 대비 충분.
- 2026-08-17: `editors/vscode/README.md` 기능 표에 파일 아이콘 행 추가.

## 이슈 및 해결

없음.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`
- [x] `package.json` JSON 파싱 + SVG XML 파싱 확인 (`node -e`, `python3 -c`)

## 결과

- `editors/vscode/icons/rl-file-light.svg` (신규): 라이트 테마 아이콘.
- `editors/vscode/icons/rl-file-dark.svg` (신규): 다크 테마 아이콘.
- `editors/vscode/package.json`: 언어 기여에 `icon` 추가.
- `editors/vscode/README.md`: 기능 표에 파일 아이콘 행 추가.

언어 아이콘을 지원하는 파일 아이콘 테마(기본 Seti 포함)에서 `.rl` 파일이
탐색기·탭에 러스트 오렌지 "RL" 배지로 표시된다. Rust 코드 변경 없음 —
cargo 게이트는 기존 상태 그대로 통과.
