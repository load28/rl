# TASK-026: 프로젝트 프론트엔드 역할 변경 설계 제안

- **상태**: 완료
- **시작일**: 2026-08-17
- **완료일**: 2026-08-17
- **커밋**: 44eeabd

## 목적

rlc를 "파일 하나를 전처리하는 도구"에서 "소스 트리(`.rl` + `.ts`)를 받아
TypeScript 트리를 내는 프로젝트 프론트엔드"로 바꾸는 방향을 정리한다.
사용자가 소스에서 `.rl`/`.ts` 확장자 경계를 신경 쓰지 않게 하는 것이 목표다.

## 범위

- 포함: `docs/design/project-front-end.md` 작성 — 현재 동작 실측, 층 분리
  설계, 필요한 변경 4개, 결정 지점 4개.
- 제외: 구현. 변경 항목은 결정이 내려진 뒤 각각 별도 태스크로 등록한다.
- 제외: 타입 검사 도입, `node_modules` 해석, `.tsx` 지원.

## 의사결정

### 결정 1: 방출 지정자를 `.js`가 아니라 `.ts`로 가는 방향을 제안한다

- **상황**: TASK-020이 `--rewrite-imports js`를 기본으로 정했다. 역할 변경
  후에도 이 값이 맞는지 다시 봐야 했다. rlc의 산출물은 TypeScript인데
  방출하는 지정자가 JavaScript 세계의 것이라는 불일치가 있다.
- **검토한 대안**:
  - `js` 유지: 소비 측 설정이 필요 없다. 대신 rlc가 tsc의 emit 규칙을 알아야
    하고, `.rl` → `.ts` 방향 지정자(`./util.ts`)를 따로 재작성해야 한다.
  - `ts`로 전환: rlc는 `.rl` → `.ts` 하나만 알면 되고, `.ts` 지정자는 이미
    최종 형태라 손댈 필요가 없다. `.ts` → `.js`는 tsc가 한다. 대신 소비 측
    tsconfig에 옵션 두 개와 TypeScript 5.7 이상이 필요하다.
- **선택과 근거**: `ts` 모드 추가를 제안하고 기본값 전환은 결정 지점으로
  남겼다. 층마다 자기 확장자만 알면 되는 배치가 되고, 필요한 변경이 하나
  줄어든다(`.rl` → `.ts` 방향 재작성 불필요). 성립 여부는 실행으로 확인했다
  (tsc 5.9.3, `/tmp/rlprobe2`): `allowImportingTsExtensions` +
  `rewriteRelativeImportExtensions`를 함께 켜면 `./m.ts` 지정자가 통과하고
  방출물에서 `require("./m.js")`로 바뀐다. 두 번째 옵션 없이 첫 번째만 켜면
  `TS5096`으로 emit이 막힌다.

### 결정 2: `js`/`bare` 모드를 유지한다

- **상황**: `ts`가 기본이 되면 기존 두 모드를 없앨 수 있는지 검토했다.
- **검토한 대안**: 정리해 단일 모드로 / 셋 다 유지.
- **선택과 근거**: 유지. `rewriteRelativeImportExtensions`는 TypeScript
  5.7에서 들어왔고, 번들러 해석을 쓰는 프로젝트는 확장자 없는 형태를 쓴다
  (`rl-calc` 예제가 그렇다). 두 경우 모두 `ts` 모드로 대체되지 않는다.

### 결정 3: 구현을 4개 항목으로 쪼갠다

- **상황**: "역할 변경"은 한 태스크로 담기에 크다.
- **검토한 대안**: 단일 변경 / 항목 분리.
- **선택과 근거**: 분리. `--rewrite-imports ts` 추가는 기존 재작성 코드에
  모드 하나를 더하는 수준이라 독립 배포가 가능하고, 입력 수집 확대와 출력
  트리 분리는 서로 맞물려 있어 함께 가야 한다. 그래프의 `.ts` 노드 포함은
  전이 수집이 필요해지는 첫 사례라 별도로 다룬다.

## 작업 내역

- 2026-08-17: 현재 동작을 `/tmp/rlmix`에서 실측했다.
  - `.rl` → `.rl`(`./shape.rl`)은 `./shape.js`로 재작성된다.
  - `.ts` → `.rl`은 그 `.ts`를 입력으로 명시하면 재작성된다
    (`rlc -o out consumer.ts` → `import { Shape } from "./shape.js"`).
  - `.rl` → `.ts`(`./util.ts`)는 그대로 남는다.
  - `rlc inplace.ts`(`-o` 없음)는 `inplace.ts → inplace.ts`로 **원본을
    덮어쓰고** `@generated` 배너를 붙인다.
  - `rlc -o out2 .`은 `.rl` 둘만 수집하고 `.ts` 둘은 건너뛴다.
- 2026-08-17: `.ts` 지정자 성립 여부를 `/tmp/rlprobe2`에서 확인했다
  (tsc 5.9.3). 위 "결정 1" 참조.
- 2026-08-17: `docs/design/project-front-end.md` 작성.
- 2026-08-17: `docs/tasks/INDEX.md`에 TASK-026 등록.

## 이슈 및 해결

### 이슈 1: `.ts` 지정자가 불가능하다고 잘못 결론냈다

- **증상**: TASK-019 제안 문서와 그 이후 논의에서 "`./x.ts`는 어느 쪽에서도
  답이 아니다 — `TS5097`로 거부되고 `allowImportingTsExtensions`를 켜면
  `TS5096`으로 emit이 막힌다"고 적었다.
- **원인**: `allowImportingTsExtensions`만 켜고 검증했다.
  `rewriteRelativeImportExtensions`(TypeScript 5.7+)를 함께 켜는 조합을
  시험하지 않았다. 즉 관측은 맞았지만 조합이 불완전했다.
- **해결**: 두 옵션을 함께 켠 상태로 재검증해 통과를 확인하고, 이 문서의
  설계를 그 결과 위에 세웠다. `module-graph.md`의 해당 서술은 TASK-020
  시점의 결정 근거이므로 그대로 두고, 이 문서에서 조건을 명시했다.

## 검증

이 태스크는 문서만 변경하므로 Rust 게이트는 해당 없다 (`src/`, `tests/`
변경 없음).

- [ ] `cargo fmt --check` — 해당 없음
- [ ] `cargo clippy --all-targets -- -D warnings` — 해당 없음
- [ ] `cargo test` — 해당 없음
- [x] 문서 내 사실 주장을 실행으로 확인 (위 "작업 내역"의 실측 6건)

## 결과

- 추가: `docs/design/project-front-end.md`
- 추가: `docs/tasks/TASK-026-project-front-end.md`
- 수정: `docs/tasks/INDEX.md`

후속: 결정 지점 4개에 답이 정해지면 `--rewrite-imports ts` 모드 추가를
첫 구현 태스크로 등록한다.
