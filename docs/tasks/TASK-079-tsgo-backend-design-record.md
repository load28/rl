# TASK-079: tsgo native backend 전환 설계 문서 편입

- **상태**: 완료
- **시작일**: 2026-08-19
- **완료일**: 2026-08-19
- **커밋**: `bfa17ca`

## 목적

폐기된 `claude/unpack-file-new-branch-push-4xjnys` 브랜치에 tsgo API 표면을
실제로 빌드해 확인한 조사 기록(`docs/design/tsgo-native-backend.md`, 417줄)이
있다. main은 그 사이 TASK-073~077로 전환을 **완료**했지만, 왜 이 backend를
택했는지·tsgo API에서 무엇이 되고 무엇이 안 됐는지에 대한 기록은 main에
전혀 없다. 그 조사를 잃지 않고 편입한다.

## 범위

- 포함: 브랜치의 설계 문서를 `docs/design/`에 편입하되, main이 실제로 도달한
  결과와 대조해 각 절에 무엇이 채택됐고 무엇이 넘어섰는지 표시한다.
- 제외: 구현 변경. 이 태스크는 문서만 다룬다.
- 제외: 브랜치의 `tools/tsgo-native-smoke.mjs`와 `src/tsgo_host.mjs` 편입.
  전자는 제품 경로 위의 테스트가 같은 사실을 확인하므로 불필요하고, 후자는
  main이 제거한 이중 호스트 구성의 절반이다.

## 의사결정

### 결정 1: 원문을 그대로 넣지 않고 "실제 결과"를 병기한다

- **상황**: 원문의 결론은 "legacy JS host를 기본으로 두고 tsgo를
  `RLC_TS_BACKEND=tsgo` opt-in으로 둔다"이다. main은 그 반대로 갔다 —
  native 단일 경로로 넘어가고 `types_host.mjs`를 지웠다(TASK-075). 원문을
  그대로 넣으면 저장소에 **틀린 현재 상태**를 기술한 문서가 생긴다.
- **검토한 대안**:
  - **대안 A — 편입하지 않는다.** 오해는 없지만 조사도 사라진다. tsgo API를
    실제로 빌드해 확인한 기록은 다시 만들려면 비싸다.
  - **대안 B — 결론만 고쳐 쓴다.** 본문과 결론이 어긋나 읽는 사람이 어디까지
    믿어야 할지 모른다.
  - **대안 C — 원문을 보존하고, 절마다 `> **실제 결과**` 블록으로 대조한다.**
    "이렇게 계획했고 실제로는 이렇게 됐다"가 한 화면에 보인다. 설계 판단이
    왜 바뀌었는지가 기록의 핵심 가치이므로, 바뀐 사실 자체가 콘텐츠다.
- **선택과 근거**: 대안 C. 이 저장소의 설계 문서는 이미 "제안 / 규범 아님 /
  구현은 어디" 를 머리말에 밝히는 관례가 있고(`module-graph.md`,
  `project-front-end.md`, `ts-sidecar-declarations.md`), 대안 C는 그 관례를
  절 단위로 밀고 나간 것이다.

### 결정 2: 대조 내용을 코드로 검증하고 쓴다

- **상황**: "실제 결과" 블록은 사실 주장이다. 기억이나 커밋 메시지가 아니라
  코드에서 확인해야 한다.
- **선택과 근거**: 각 주장을 실제로 확인했다.
  - 모듈 배치 → `src/typescript/` 실제 파일 목록 (`backend`/`check`/`native`/
    `project`/`mapper`/`host.mjs`; 원문이 나눈 `semantic`/`emit`/`protocol`은 없음)
  - trait 형태 → `backend.rs`의 `TypeScriptBackend::ask` 단일 메서드와
    `Query`/`Answers` 한 쌍
  - `.rl` import 해석 → `project.rs`의 `Lowered::module_path_of`
    (`src/token.rl` → `src/token.rl.ts`), shim 없음
  - `@rl/std` → `project.rs`의 `STD_MODULE = "node_modules/@rl/std/index.ts"`
  - symbol query → `backend.rs`의 `SymbolQuery`/`Resolution{id,builtin}`,
    enum 소진성은 `TagQuery`
  - 선언 방출 → `host.mjs`가 메모리에서 방출하고 rlc는 map만 만듦
  - 릴리스 패키지의 선언 방출 제약 → `cli.md` §컴파일러 해석
  - 에디터의 language service → `editors/vscode/server/src/lsp.ts`(`tsgo --lsp`)

### 결정 3: 원문에 없던 "지금 상태와 남은 것" 절을 더한다

- **상황**: 원문의 마지막 절은 "바로 다음 작업" 4항목인데, 넷 다 이미 끝났거나
  채택되지 않았다. 그대로 두면 미처리 할 일 목록처럼 읽힌다.
- **선택과 근거**: 그 절을 빼고 현재 남은 것(Content Mapper adapter, 릴리스
  패키지의 선언 방출, 하지 않기로 한 CLI 재편)으로 대체했다. 남은 것에는
  "막힌 것이 아니라 하지 않기로 한 것"임을 명시했다.

## 작업 내역

- 2026-08-19: `git show origin/claude/unpack-file-new-branch-push-4xjnys:docs/design/tsgo-native-backend.md`
  로 원문(417줄)을 꺼냈다.
- 2026-08-19: main의 실제 구조를 읽어 대조표를 만들었다 (결정 2의 목록).
- 2026-08-19: `docs/design/tsgo-native-backend.md`(544줄)를 썼다. 머리말에서
  출처(폐기된 브랜치의 TASK-073)와 상태(조사 기록·제안, 규범 아님)를 밝히고,
  12개 절 각각에 `> **실제 결과**` 블록을 붙였다. 마지막 절은 "지금 상태와
  남은 것"으로 교체했다.
- 2026-08-19: `docs/tasks/INDEX.md`에 TASK-079를 등록했다.

주요 대조 결과 셋:

1. **`.rl` import 해석** — 원문은 VFS overlay + `x.d.rl.ts` shim +
   `allowArbitraryExtensions`를 설계했고 "package boundary 사례는 별도 fixture로
   확장해야 한다"는 부채를 남겼다. main은 `src/token.rl`을 `src/token.rl.ts`로
   낮추는 것만으로 해결했다 — 평범한 모듈 해석이 그대로 찾고, shim도 `paths`도
   필요 없고, package boundary 문제가 애초에 생기지 않는다.
2. **backend 개수** — 원문은 legacy JS host와 tsgo host를 `RLC_TS_BACKEND`로
   고르게 두려 했다. main은 native 하나로 갔다. TypeScript 7이 JS Compiler API를
   내놓지 않으므로 legacy는 유지해도 죽는 길이었다.
3. **batch** — 원문의 Risk register는 "IPC chattiness → query batch 유지"를
   대응으로 적었는데, 실제 `Query`는 batch가 **강제**되는 형태다 (한 프로젝트에
   대한 모든 질문이 한 번의 왕복).

## 이슈 및 해결

없음.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` — 435 통과, 실패 0

(문서만 바꾸는 태스크지만 게이트는 그대로 돌렸다. `docs/ai/rl.md`는 이번에
바뀌지 않았으므로 바이너리에 임베드된 내용과의 일치 테스트도 그대로 통과한다.)

## 결과

`docs/design/tsgo-native-backend.md` 추가(544줄). 조사 기록은 보존되고, 각 절이
main의 실제 결과와 대조돼 있어 "지금 코드가 왜 이렇게 생겼는지"를 문서만 읽고
재구성할 수 있다.

변경 파일:

- `docs/design/tsgo-native-backend.md` (신규)
- `docs/tasks/TASK-079-tsgo-backend-design-record.md` (신규)
- `docs/tasks/INDEX.md`

폐기 브랜치에서 건져낼 것으로 지목했던 세 항목(TASK-087 emit-map → TASK-078,
TASK-072 에디터 val 진단, 이 설계 문서)이 모두 처리됐다. 브랜치의
`examples/typed-val-demo`는 이번 범위에서 제외했다.
