# TASK-027: `--rewrite-imports ts` 모드

- **상태**: 완료
- **시작일**: 2026-08-17
- **완료일**: 2026-08-17
- **커밋**: —

## 목적

`.rl` import 지정자를 rlc가 실제로 방출하는 파일(`./x.ts`)로 재작성하는 모드를
추가한다. [TASK-026](./TASK-026-project-front-end.md) 역할 변경의 첫 구현
항목이며, 그 문서의 "필요한 변경 1"에 해당한다.

## 범위

- 포함: `ImportRewrite::Ts` 추가, CLI 인자·도움말·에러 메시지, 단위 테스트 2개,
  `language.md` §7.2와 `cli.md` 옵션 표 갱신.
- 제외: 기본값 전환. 기본은 `js`로 그대로 둔다 (TASK-026 결정 지점 1).
- 제외: 입력 수집 확대, 출력 트리 분리 (TASK-026의 나머지 항목).

## 의사결정

### 결정 1: 기존 모드를 건드리지 않고 변이만 추가한다

- **상황**: `ts`가 층 분리상 더 옳은 형태이므로 기본값으로 바로 올릴지
  검토했다.
- **검토한 대안**:
  - 기본값을 `ts`로 전환: 설계 의도와 일치하지만, 소비 측 tsconfig에
    `allowImportingTsExtensions` + `rewriteRelativeImportExtensions`가 없으면
    빌드가 즉시 깨진다. TypeScript 5.7 미만은 아예 불가능하다.
  - 변이만 추가: 기존 사용자에게 영향이 없고 독립 배포가 가능하다.
- **선택과 근거**: 변이만 추가. 기본값 전환은 호환성 결정이라 TASK-026의
  결정 지점으로 남겨두는 편이 맞다. 이 태스크는 "선택지를 만든다"까지다.

### 결정 2: `.ts` 지정자는 재작성 대상에서 제외한 채로 둔다

- **상황**: `ts` 모드에서 `.rl` → `.ts`를 재작성하니, `.rl` 파일이 쓴
  `./util.ts` 지정자도 다뤄야 하는지 확인이 필요했다.
- **검토한 대안**: `.ts` 지정자도 수집·재작성 / 그대로 둔다.
- **선택과 근거**: 그대로 둔다. `ts` 모드에서는 `./util.ts`가 **이미 최종
  형태**다. 실측으로 확인했다 — `/tmp/rlmix`에서 `use.rl`을 `ts` 모드로
  컴파일하면 두 지정자가 모두 `.ts`로 통일된다:
  ```
  import { Shape } from "./shape.ts";   // .rl → .ts 재작성됨
  import { label } from "./util.ts";    // 원문 그대로, 이미 정답
  ```
  TASK-026 문서가 예상한 "격차 1이 사라진다"가 실제로 성립했다.

## 작업 내역

- 2026-08-17: `src/lib.rs`에 `ImportRewrite::Ts` 변이를 추가하고, 소비 측
  요구사항(두 tsconfig 옵션, TypeScript 5.7+)을 doc 주석에 적었다.
- 2026-08-17: `src/codegen/mod.rs`의 `emit_rl_import`에 `Ts` 암 추가 —
  `Js`와 같은 방식으로 마지막 4바이트(`.rl` + 닫는 따옴표)를 잘라내고
  `.ts`와 따옴표를 붙인다.
- 2026-08-17: `src/main.rs` — 도움말의 `<js|bare|off>`를 `<js|ts|bare|off>`로
  바꾸고, 인자 파싱에 `Some("ts")` 분기와 두 에러 메시지를 갱신했다.
- 2026-08-17: `tests/compile.rs`에 테스트 2개 추가 —
  `ts_mode_points_at_the_emitted_file`(기본 형태),
  `ts_mode_preserves_the_quote_style_and_path`(작은따옴표·상위 경로·
  `export * from` 재수출).
- 2026-08-17: `docs/reference/language.md` §7.2 표에 `ts` 행과 tsconfig
  요구사항·층별 재작성 설명을 추가하고, `docs/reference/cli.md`의 옵션 행을
  갱신했다.
- 2026-08-17: CLI 확인.
  ```
  $ rlc --rewrite-imports ts -o outts use.rl shape.rl consumer.ts
  $ grep -h '^import' outts/*.ts
  import { Shape } from "./shape.ts";
  import { label } from "./util.ts";
  import { Shape } from "./shape.ts";
  $ rlc --rewrite-imports mjs use.rl
  rlc: --rewrite-imports expects js, ts, bare, or off (got mjs)
  ```

## 이슈 및 해결

없음.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` — 90(compile) + 21(integration) + 35(passthrough·stdlib 등)
      전부 통과, 신규 테스트 2개 포함
- [x] CLI 실행 확인 (위 "작업 내역")

## 결과

- 수정: `src/lib.rs`, `src/codegen/mod.rs`, `src/main.rs`,
  `tests/compile.rs`, `docs/reference/language.md`, `docs/reference/cli.md`
- 추가: `docs/tasks/TASK-027-rewrite-imports-ts-mode.md`

후속: TASK-026의 나머지 항목 — 입력 수집에 `.ts` 포함, 소스/출력 트리 분리,
그래프의 `.ts` 노드. 기본값 전환은 그 뒤에 다룬다.
