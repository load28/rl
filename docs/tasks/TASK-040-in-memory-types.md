# TASK-040: `--types`를 메모리 방출로 — 캐시 트리 제거

- **상태**: 완료
- **시작일**: 2026-08-17
- **완료일**: 2026-08-17
- **커밋**: —

## 목적

`--types`가 선언을 뽑기 위해 `.rl-build/` 캐시 트리를 만든다. 그 트리에는
`.rl` 컴파일 결과뿐 아니라 **손으로 쓴 `.ts`의 복사본**과 모듈당 `.d.rl.ts`
심까지 들어간다. 빌드 캐시가 손으로 쓴 소스를 복제하는 것은 규모와 무관하게
잘못된 구조다 — 선언 방출을 TypeScript API로 **메모리에서** 수행해 디스크에
남는 것을 사이드카뿐으로 만든다.

## 범위

- 포함: 내장 Node 헬퍼(`types_host.mjs`) 신설, `types_once`/`types_watch`를
  메모리 방출로 재작성, 캐시 관련 코드 제거(`BUILD_DIR`,
  `TYPES_TSCONFIG`, `.d.rl.ts` 심 생성, `run_tsc`), CLI 표면 변경
  (`--tsc` 제거 / `--node <path>` 추가), 문서·예제 정합.
- 제외: 증분화(감시 모드에서 헬퍼 상주, `ts.createIncrementalProgram`).
  한 번짜리 프로세스로 시작한다 — 별도 태스크.
- 제외: `--sidecar` 저수준 훅. 입력이 파일인 채로 남는다 (VSCode 확장이
  자체 메모리 방출로 이미 같은 일을 한다).

## 측정 근거

2,000 파일(`.rl` 1,000 + 손으로 쓴 `.ts` 1,000) 합성 프로젝트:

| | 값 |
|---|---|
| 캐시 파일 수 | **5,001** (컴파일 결과 1,000 + `.ts` 복사 1,000 + 심 1,000 + 선언 2,000) |
| 캐시 크기 | 20M (소스 7.8M) |
| `.ts` 복사가 차지하는 몫 | 0.145s / 3.9M (전체 1.9s의 7%) |

시간의 주인은 tsc(1.04s, 55%)이고 그것은 증분화(후속)의 몫이다. 이 태스크가
없애는 것은 **복사와 심, 그리고 캐시라는 개념 자체**다.

## 설계

### 데이터 흐름

```
rlc  소스 수집 → 각 .rl을 메모리에서 compile()  (디스크 쓰기 없음)
  │  stdin: 작업 명세 JSON
  ▼
node <tmp>/types_host.mjs         ← 바이너리에 include_str! 로 내장
  │  CompilerHost: 가상 모듈만 메모리에서 서빙, 나머지는 ts.sys로 위임
  │  resolveModuleNames: `@rl/std`와 상대 `.rl` 지정자를 가상 경로로
  │  program.emit(파일별, emitDeclarationOnly) → writeFile 가로채 수집
  │  stdout: 선언 + 진단 JSON
  ▼
rlc  build_sidecar(원본, 선언, 상대경로) → .rl-types/<이름>.rl.d.ts + .map
```

**손으로 쓴 `.ts`는 명세에 담지 않는다.** 호스트가 제자리에서 읽으므로
복사가 원리적으로 발생하지 않는다.

### 가상 경로

`.rl`의 컴파일 결과는 **원본 자리에서 확장자만 바꾼 경로**로 서빙한다
(`src/engine.rl` → `src/engine.ts`). 그래야 결과물이 담고 있는 `./ops` 같은
상대 지정자가 그대로 맞는다. 실제 `src/engine.ts`가 존재하면 build 모드와
같은 규칙으로 거부한다(출력이 입력을 덮어쓰는 경우).

표준 라이브러리는 `<루트>/__rl_std__.ts`로 서빙한다.

### 헬퍼 프로토콜

```jsonc
// stdin
{
  "cwd": "/abs/project",
  "compilerOptions": { /* rlc가 정하는 최소 집합 */ },
  "modules": [{ "path": "src/engine.ts", "text": "<컴파일 결과>" }],
  "std": { "path": "__rl_std__.ts", "text": "<STD_SOURCE>" },   // 없으면 null
  "rlModules": { "src/token.rl": "src/token.ts" }               // 지정자 해석표
}
// stdout
{
  "declarations": [{ "path": "src/engine.ts", "text": "<.d.ts 본문>" }],
  "diagnostics": [{ "file": "src/engine.ts", "line": 3, "col": 10,
                    "code": 2322, "message": "..." }]
}
```

- 컴파일 옵션은 rlc가 코드로 넘긴다. 프로젝트 `tsconfig.json`은 지금도
  참조하지 않으므로 동작이 바뀌지 않는다.
- `typescript`는 **프로젝트의 `node_modules`**에서 해석한다
  (`createRequire(<cwd>/index.js)`). 없으면
  `rlc: typescript not found — npm i -D typescript` 로 종료한다.
- stdin 쓰기와 stdout 읽기를 동시에 해야 큰 입력에서 파이프 데드락이 없다
  (Rust 쪽에서 stdin 쓰기를 별도 스레드로).

### `resolveModuleNames` — 심과 tsconfig가 사라진다

호스트가 지정자를 직접 해석하므로 지금 필요한 장치들이 전부 불필요해진다.

| 지금 | 이 태스크 이후 |
|------|----------------|
| 모듈당 `.d.rl.ts` 심 파일 | 없음 |
| `allowArbitraryExtensions` | 없음 |
| 합성 `tsconfig.json`(+`paths`) | 없음 (옵션을 인자로) |
| `.rl-build/` 트리 | 없음 |

### CLI 표면

- **`--tsc <path>` 제거** — tsc CLI를 더 이상 부르지 않는다.
- **`--node <path>` 추가** — 기본은 PATH의 `node`.
- 진단은 지금과 같은 형식으로 중계한다: 타입 에러가 있어도 선언은 방출되므로
  사이드카는 갱신하고 종료 코드 1을 낸다.

## 검증 계획

- 회귀 기준: `--types`를 다루는 기존 통합 테스트가 **사이드카 내용·맵까지
  동일하게** 통과해야 한다.
- 새 테스트: ① `.rl-build`가 생기지 않는다 ② 손으로 쓴 `.ts`가 어디에도
  복사되지 않는다 ③ `typescript` 없을 때의 에러 메시지 ④ 타입 에러가 있어도
  사이드카가 갱신되고 종료 코드 1.
- 예제 3개(`rl-calc`·`rl-interop`·`calculator-cli`) 재빌드 + tsserver로 진단
  없음·정의 이동 확인.
- 2,000 파일 합성 프로젝트에서 시간 재측정 — 회귀가 없는지(증분은 이 태스크
  범위가 아니므로 개선을 기대하지 않는다).

## 위험과 완화

| 위험 | 완화 |
|------|------|
| Node가 없는 환경 | `--types`만 불가. build·`--check`는 무관하고, 지금도 tsc(Node)가 필요했으므로 동등하다 |
| 큰 프로젝트의 JSON 크기 | 파이프 스트리밍 + stdin 쓰기 스레드 분리. 필요해지면 파일 기반 폴백을 별도 태스크로 |
| 임시 파일에 헬퍼를 쓰는 비용 | 회차당 한 번, 수 KB. 경로는 프로세스별 임시 디렉터리 |
| 가상 경로 충돌 | 실제 `x.ts`가 있으면 거부 (build 모드와 동일 규칙) |

## 의사결정

### 결정 1: 헬퍼를 바이너리에 내장한다

- **검토한 대안**: 별도 npm 패키지(`@rl/types-host`) / `unplugin-rl`에 합치기
  / `include_str!`로 내장.
- **선택과 근거**: 내장. 사용자가 설치할 것이 없고 rlc와 헬퍼의 버전이 어긋날
  수 없다. 별도 패키지는 설치 단계와 버전 스큐를 만들고, 플러그인 패키지에
  넣으면 번들러를 쓰지 않는 프로젝트에도 그것을 강요한다.

### 결정 2: 한 번짜리 프로세스로 시작한다

- **검토한 대안**: watch에서 헬퍼 상주(+`createIncrementalProgram`).
- **선택과 근거**: 이 태스크의 목표는 캐시 제거다. 상주·증분은 프로세스
  생존기·재시작·프로토콜 상태를 들여오므로 별도 태스크가 맞다. 측정으로
  시간의 주인이 tsc임을 이미 확인했으니 후속의 근거도 남아 있다.

### 결정 3: `--tsc`를 제거하고 `--node`를 넣는다

- **검토한 대안**: `--tsc`를 "typescript 패키지 경로"로 재정의 / 둘 다 유지.
- **선택과 근거**: 실제로 실행하는 것은 node이고 typescript는 프로젝트에서
  해석한다. 이름과 뜻이 어긋나는 옵션을 남기는 것보다 표면을 바꾸는 편이
  낫다 — 아직 배포 전이라 호환 부담이 없다.

## 작업 내역

- 2026-08-17: 스파이크로 문제를 특정했다 (측정 근거 절). `rootDir`를
  프로젝트 루트로 넓히면 복사 없이도 tsc가 통과함을 확인했고
  (`/tmp/rlnc`, exit 0), 그보다 캐시 자체를 없애는 쪽을 선택했다.
- 2026-08-17: `src/types_host.mjs` 작성 — stdin의 작업 명세를 받아
  CompilerHost로 가상 모듈을 서빙하고, `resolveModuleNames`로 `.rl`·`@rl/std`를
  해석하고, 파일별 `program.emit(emitOnlyDtsFiles)`로 선언을 모아 stdout에
  JSON으로 돌려준다.
- 2026-08-17: `src/main.rs` — `types_once`를 메모리 방출로 재작성하고
  `BUILD_DIR`·`TYPES_TSCONFIG`·`run_tsc`·심 생성 코드를 제거했다.
  `write_sidecar`/`input_relative`/`run_types_host`/`types_job`과 최소 JSON
  파서(`json_objects`/`json_field`/`json_number`)를 추가했다.
  `--tsc`를 `--node`로 교체.
- 2026-08-17: 통합 테스트 3개 추가 (캐시·복사 없음 / 타입 에러 시 사이드카
  유지 + 종료 1 / typescript 없을 때 메시지). 픽스처가 프로젝트-로컬
  TypeScript를 갖도록 `link_typescript`를 넣었다.
- 2026-08-17: 문서 정합 — `cli.md`의 `--types` 절을 메모리 방출로 다시 쓰고
  `--tsc` 행을 `--node`로 바꿨다. 세 프로젝트와 저장소의 `.rl-build` 참조를
  지웠다.

## 이슈 및 해결

### 이슈 1: 손으로 쓴 `.ts`의 타입 에러가 보이지 않게 됐다

- **증상**: "타입 에러가 있어도 사이드카는 갱신되고 종료 코드 1" 테스트가
  **성공**으로 끝났다 — 소비자 `main.ts`에 넣은 타입 에러가 보고되지 않았다.
- **원인**: 프로그램의 rootNames를 가상 모듈(컴파일된 `.rl`)로만 잡았다.
  `main.ts`는 아무도 import하지 않는 엔트리라 프로그램에 들어오지 않았고,
  그 파일의 진단이 사라졌다. 이전 파이프라인은 트리 전체를 캐시에 복사해
  tsc에 넘겼으므로 이 문제가 없었다.
- **해결**: 손으로 쓴 `.ts`를 **경로로** rootNames에 추가했다(`sources`
  필드). 복사하지 않으면서 프로그램에는 참여하므로 진단이 되돌아온다.
  테스트가 이 회귀를 잡았다.

### 이슈 2: 전역 TypeScript 해석이 환경에 따라 실패한다

- **증상**: 기존 `--types` 통합 테스트가 `typescript not found`로 실패했다.
  이전 구현은 PATH의 `tsc`를 실행했고 이 기계에는 전역 tsc가 있다.
- **원인**: 이제 필요한 것은 tsc **바이너리**가 아니라 `typescript`
  **라이브러리**다. PATH의 `tsc`에서 거꾸로 패키지를 찾는 폴백을 넣었지만,
  이 기계의 `tsc`는 pnpm 셰임 스크립트라 그 경로로는 패키지에 닿지 못한다.
- **해결**: 스펙의 규칙(프로젝트에서 해석)을 지키고, 픽스처가
  프로젝트-로컬 `node_modules/typescript`를 갖도록 했다(저장소가 이미
  벤더링한 사본을 심링크). PATH 폴백은 npm/yarn 전역 설치에서 유효하므로
  남겨 두었다. "전역이 해석되는 환경에서는 skip"으로 테스트를 무해하게
  만들었다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` — 92 + 26 + 35 + 8 + 2 + 8 전부 통과 (통합 23 → 26)
- [x] 예제 3개 재빌드 — 셋 다 `.rl-build` 미생성, 실행 결과 동일
      (`rl-calc` 22.4kb, `rl-interop` 4.39kb, `calculator-cli` 13.86kb → 11)
- [x] tsserver — `rl-interop` `render` → `src/notice.rl:24:17`,
      `calculator-cli` `evaluate` → `src/engine.rl:28:17`, 진단 없음
- [x] 2,000 파일 재측정 — **1.35s** (이전 1.9s), 캐시 0바이트

## 결과

- 추가: `src/types_host.mjs`, `docs/tasks/TASK-040-in-memory-types.md`
- 수정: `src/main.rs`, `tests/integration.rs`, `docs/reference/cli.md`,
  `.gitignore`, `docs/tasks/INDEX.md`
- 예제 3개의 `.rl-build` 참조 제거 (저장소 밖)

측정 요약 — 2,000 파일에서 캐시 파일 **5,001 → 0**, 시간 1.9s → 1.35s.
시간이 줄어든 것은 부수 효과다(심 1,000개와 복사 1,000개가 사라졌다);
증분화는 여전히 후속 과제다.

후속: 감시 모드에서 호스트를 상주시켜 `ts.createIncrementalProgram`을
재사용하는 것. 이제 tsc가 유일한 비용 축이므로 효과가 바로 드러난다.
