# TASK-152: TanStack Start 기반 공식 홈페이지와 rl 하이라이팅

- **상태**: 완료
- **시작일**: 2026-08-22
- **완료일**: 2026-08-22
- **커밋**: —

## 목적

기존 공식 홈페이지의 정보와 사용 흐름을 유지하면서 TanStack Start로 재구현한다. 모든 공개 문서를 정적 HTML로 생성하고 rl 코드 예시에 언어 전용 구문 하이라이팅을 제공한다.

## 범위

- 포함: 기존 영문·한글 콘텐츠와 화면 보존, TanStack Start 최신 배포 버전 도입, SSG, SEO 메타데이터, rl 코드 하이라이팅, GitHub Pages 배포 변경
- 제외: 컴파일러 동작 변경, 온라인 컴파일러, 서버 런타임, 기존 TASK-151 변경

## 의사결정

### 결정 1: 정적 프리렌더를 배포 단위로 사용한다

- **상황**: GitHub Pages는 정적 파일만 제공하며 검색 엔진이 초기 HTML에서 문서 내용을 읽을 수 있어야 한다.
- **검토한 대안**: CSR 단일 페이지는 이전 동작과 가깝지만 초기 HTML에 콘텐츠가 없다. SSR 서버는 Pages에서 실행할 수 없다. TanStack Start 프리렌더는 라우트별 완성 HTML과 클라이언트 탐색을 함께 제공한다.
- **선택과 근거**: TanStack Start의 공식 `prerender.enabled` 설정으로 공개 라우트를 빌드 시점에 생성한다. 공식 문서의 정적 호스팅 권장 구성과 GitHub Pages 제약을 함께 만족한다.

### 결정 2: 콘텐츠와 URL 구조를 분리한다

- **상황**: 기능·언어별 검색 결과가 필요하지만 번역 콘텐츠를 라우트 파일마다 복제하면 관리 지점이 늘어난다.
- **검토한 대안**: 쿼리 기반 단일 URL은 검색 엔진별 문서 수집에 불리하다. 페이지별 콘텐츠 파일은 정적 경로는 분명하지만 같은 구조와 예제가 중복된다.
- **선택과 근거**: 콘텐츠는 `content.json` 하나에 두고, `/match`와 `/ko/match` 같은 경로만 TanStack Start가 생성한다. 각 경로는 고유 title·description·canonical·hreflang과 완성된 본문을 가진다.

### 결정 3: 저장소의 rl 문법을 빌드 시점에 재사용한다

- **상황**: 홈페이지의 하이라이팅이 에디터 문법과 달라지면 새 rl 구문을 두 곳에서 유지해야 한다.
- **검토한 대안**: 정규식 하이라이터는 작지만 전체 TypeScript와 rl 문법을 정확히 재현하기 어렵다. 브라우저 런타임 하이라이터는 초기 비용과 CSR 의존성을 만든다.
- **선택과 근거**: 기존 `editors/vscode/syntaxes/rl.tmLanguage.json`을 Shiki가 빌드 때 읽어 정적 토큰 HTML을 생성한다. 브라우저에서는 하이라이터를 실행하지 않는다.

### 결정 4: 웹사이트 패키지 관리는 Bun으로 통일한다

- **상황**: 개발·CI의 패키지 관리자와 lockfile을 하나로 정해야 한다.
- **검토한 대안**: npm은 기존 설치 흔적과 맞지만 사용자가 Bun을 지정했다. Bun은 설치·스크립트 실행·lockfile을 한 도구로 제공한다.
- **선택과 근거**: `bun.lock`을 단일 lockfile로 두고 로컬 스크립트와 Pages workflow 모두 Bun 1.3.13을 사용한다.

## 작업 내역

- 2026-08-22: 기존 `website/dist` 번들과 TASK-149 기록, Pages workflow를 확인했다.
- 2026-08-22: TanStack Start 공식 문서와 npm 배포 정보를 확인해 최신 배포 버전과 정적 프리렌더 설정을 확정했다.
- 2026-08-22: 기존 번들의 영문·한글 콘텐츠를 단일 `content.json`으로 보존하고 공통 React 레퍼런스 화면을 구현했다.
- 2026-08-22: 기능별 영문 경로와 `/ko` 하위 한글 경로 28개를 구성하고, 기존 쿼리 주소 호환 이동을 추가했다.
- 2026-08-22: 저장소의 TextMate rl 문법을 Shiki로 정적 변환하는 Bun 스크립트를 추가했다.
- 2026-08-22: Pages workflow를 Bun 설치, 고정 lockfile 설치, TanStack Start 빌드, `dist/client` 배포 순서로 변경했다.
- 2026-08-22: `/rl/` base path로 29개 프리렌더 경로를 빌드하고 실제 산출 HTML의 본문, 하이라이트 토큰, canonical, hreflang을 확인했다.
- 2026-08-22: 브라우저에서 기능 탐색, 한글 전환, 레거시 쿼리 이동과 콘솔 오류 0건을 확인했다.

## 이슈 및 해결

### 이슈 1: 로컬 미리보기에서 CSS가 적용되지 않음

- **증상**: `vite preview`로 `/rl/ko/match`를 열면 stylesheet 링크는 존재하지만 CSS 규칙 수가 0이고 기본 브라우저 스타일로 표시됐다.
- **원인**: TanStack Start의 정적 파일은 `dist/client`에 있지만 Vite preview는 상위 `dist`를 기준으로 제공했다. `--outDir dist/client`은 Start preview plugin의 서버 번들 상대 경로를 깨뜨렸다.
- **해결**: 외부 의존성 없이 Bun 내장 서버가 `dist/client`를 `/rl`에 마운트하도록 `scripts/preview.ts`를 추가했다. 브라우저에서 CSS 규칙 47개, 배경색, grid 레이아웃, 76px 제목 크기를 확인했다.

## 검증

- [x] 홈페이지 프로덕션 빌드와 정적 HTML 확인
- [x] rl 코드 토큰 하이라이팅 확인
- [x] 데스크톱 렌더와 상호작용 확인
- [x] `bun run typecheck`
- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`
- [x] `git diff --check`

## 결과

기존 홈페이지의 화면·영문·한글 콘텐츠를 유지하면서 TanStack Start 1.168.48로 전환했다. 기능·언어별 정적 HTML, 고유 SEO 메타데이터, sitemap, rl TextMate 문법 기반 빌드 타임 하이라이팅을 제공하며 GitHub Pages와 로컬 미리보기 모두 Bun으로 실행한다.
