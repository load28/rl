# RL Language Engine — Project/Snapshot 아키텍처 설계

이 문서는 TASK-086의 설계 기록이다. **A. 현재 RL의 흐름 → B. typescript-go
실제 구현 분석 → C. 비교 → D. 채택/변형/기각 결정 → E. 최종 아키텍처** 순으로
서술한다. 분석 대상은 2026-08-19의 `microsoft/typescript-go` HEAD
`c6b013f5`(로컬 클론·빌드로 확인, `tsgo --version` = 7.1.0-dev)와 이
저장소의 main(TASK-085 완료 시점)이다.

한 줄 목표:

> RL은 tsgo와 유사한 지속형 Project/Snapshot 기반 Language Engine이다.
> RL Engine이 `.rl` source, projection, source mapping, RL semantics와
> TypeScript semantic backend를 하나의 project state로 관리한다.
> CLI, editor, LSP, plugin, 외부 tooling은 모두 같은 engine을 사용한다.

절대 조건: **현재 RL이 제공하는 모든 기능의 observable behavior를 동일하게
유지한다.** 기존 테스트 전부(코어 + native + 에디터)가 회귀 게이트다.

---

## A. 현재 RL — 흐름과 문제

### A.1 compiler flow (untyped)

```
rlc [files] → main.rs::compile_jobs
  load_jobs(병렬 read+scan) → ExternCache(1-hop enum 수집)
  → par_map(compile per file, defer_to_checker=false)
  → 진단 입력 순서 취합 → 쓰기
```

파일 단위 순수 함수(`rlc::compile`) 위의 배치 드라이버다. 상태가 없고,
watch(`watch_mode`)는 매 회차 모든 파일을 다시 read+parse해 importer를
찾는다(`with_dependents`).

### A.2 typed flow

```
rlc --check-types → check.rs::run
  collect → find_tsconfig → project_sources(그래프는 프로젝트 전체)
  → NativeBackend::new (toolchain 해석)
  → Pass::once:
      project::lower       ← 매 패스 전 파일 재-lowering (순차)
      project::query       ← 매 패스 probe 재수집 (파일당 4회 추가 파싱)
      backend.ask(Query)   ← 1왕복 batch; host.mjs가 tsgo API server를 몬다
      진단 5계층 보고 (TS → literal → tag → val → pass)
  → watch: stamp 비교 후 Pass::once 반복 (세션은 유지)
```

증분성은 **host.mjs 안에만** 있다(`updateSnapshot({fileChanges})` — 서버는
살아 있고 스냅샷만 전진). Rust 쪽 드라이버는 매 패스 전량 재계산한다.
`Lowered`(source + module_path + MappedEmit)가 스냅샷 파일 상태의 원형이지만
수명이 `Pass::once` 한 번이다.

### A.3 editor flow

```
VSCode ── LSP ──> server.ts (1712줄 단일 모듈)
   ├─ rl 진단:      rlc --check          (300ms 디바운스, 프로세스 스폰)
   ├─ typed 진단:   rlc --check-types --rl-only --overlay
   │                                     (1.2s 디바운스, 매번 프로세스+컴파일러 기동)
   ├─ TS 진단/hover/definition/references/completion/rename/signature:
   │    TsgoProject ── tsgo --lsp -stdio (자체 JSON-RPC 클라이언트, lsp.ts)
   ├─ virtualDocs / diskVirtuals / pendingVirtual: 자체 가상문서 저장소
   │    (rlc --emit-map 스폰, 미열람 import는 execFileSync 동기 블로킹)
   ├─ analysis.ts: rl 구문의 821줄 재구현 (enum/match/masking)
   └─ virtual.ts:  mapper.rs와 드리프트된 두 번째 매핑 구현
```

### A.4 중복 책임 (조사로 확정)

| 중복 | 한쪽 | 다른쪽 |
|---|---|---|
| tsgo 클라이언트 | `native.rs`+`host.mjs` (API server, msgpack) | `lsp.ts` (`tsgo --lsp`, JSON-RPC) |
| toolchain 해석 | `native.rs::Toolchain::resolve` | `rlc.ts::findTsgo` (규칙 불일치) |
| project graph | `project.rs::lower`(전체) + overlay | `virtualDocs`+`diskVirtuals`+regex import 추적(재-export 누락) |
| 좌표 매핑 | `mapper.rs`(선형 탐색, glue 폴백 있음) | `virtual.ts::MappedDoc`(이진 탐색, 끝-포함, glue 폴백 없음) |
| rl 구문 인식 | `parser/*`(진짜 파서) | `analysis.ts`(821줄 근사) |
| 소진성 | `sema.rs`(선언 표) | `probe.rs`+`host.mjs`(narrowed type) — 이 이원화는 **규범**(untyped/typed) |
| `val` | `val::check`(어휘 근사, untyped 규범) | `check.rs`(symbol identity) — 역시 규범적 이원화 |
| 디렉터리 walk / TS 확장자 목록 | `main.rs::collect_sources` | `check.rs::project_sources` (`TS_EXTENSIONS` 문자 그대로 중복) |
| watch | `main.rs::watch_mode` | `check.rs::watch_loop` |
| 사이드카 생성 | `main.rs::sidecar_mode` | `check.rs::write_declarations` |
| 죽은 경로 | `Sink::Calls`/`val_method_calls`/`ValMethodCall` (소비자 없음, P4 보류분) | — |

주의: "소진성/`val`의 untyped vs typed" 이원화는 중복이 아니라 **문서화된
규범**이다(untyped 컴파일은 node 없이 동작해야 한다,
`ts7-semantic-unification.md` §2 P1 판정). 제거 대상이 아니다.

---

## B. typescript-go 실제 구현 (HEAD `c6b013f5`)

로컬 클론을 빌드해 확인했다. 요지만 남긴다(상세 근거는 파일 경로).

### B.1 진입점과 프로세스 소유 (`cmd/tsgo`)

- `--lsp`: LSP 서버. `--api`: API 서버(기본 msgpack `SyncConn`, `--async`면
  JSON-RPC `AsyncConn`). 그 외: 배치 `tsc`(`internal/execute`).
- **배치 컴파일은 project 시스템을 거치지 않는다.** `execute.CommandLine`이
  `compiler.Program`을 직접 만든다. 지속 상태가 필요 없는 1회 실행에
  세션/스냅샷 기구를 태우지 않는다는 명시적 분리다.
- LSP 서버 안에서 `custom/initializeAPISession`으로 **같은
  `project.Session`을 공유하는 API 세션**을 파이프로 열 수 있다 — 에디터와
  프로그램적 클라이언트가 하나의 로드된 project graph를 공유하는 구조.

### B.2 Session → Snapshot (`internal/project`)

- `project.Session`(가변, 장수명)이 소유: overlay FS(열린 문서),
  ParseCache(refcount, 세션 간 공유 가능), 현재 snapshot 포인터, pending
  변경 큐, 디바운서, watch 레지스트리, background 큐.
- `Snapshot`(불변, refcount)이 소유: `SnapshotFS`(overlay+디스크 캐시),
  `ProjectCollection`(프로젝트들 — 각각 `Program`+checkerPool),
  `ConfigFileRegistry`, 좌표 변환기. **모든 변경은 `Snapshot.Clone` 한
  깔때기로만** 일어나고, `dirty` 패키지(copy-on-write Box/Map)로 바뀐 것만
  복제된다 — 무변경 갱신은 같은 포인터를 돌려준다.
- 잠금 규율이 핵심: snapshot 포인터를 지키는 `snapshotMu`(RW, 짧게)와 갱신
  연산을 직렬화하는 `snapshotUpdateMu`(길게)를 분리 — **스냅샷 N에 대한
  읽기가 N+1 생성과 동시에 진행**된다.
- 문서 갱신: `didOpen`은 즉시 flush+갱신, `didChange`/`didSave`는 큐잉만,
  요청의 동기 프리픽스가 `getSnapshot`에서 flush — "요청은 자신보다 먼저
  도착한 편집을 반드시 본다"가 큐 구조로 보장된다.
- dirty 추적: 파일 1개 변경이면 `Program.UpdateProgram`(구조 재사용,
  `canReplaceFileInProgram` 판정), 그 외 전체 재구축. `package.json` 변경,
  실패-lookup 위치의 파일 생성 등은 전체 무효화로 승격.

### B.3 Checker와 handle (`internal/api`)

- checkerPool 3분류: diagnostics 1개(결정적 순서), query N개(idle 정리),
  **persistent API 1개(절대 정리 안 함)**. `context`에 실린
  `CheckerLifetime`으로 선택 — API의 Type/Symbol handle 안정성이 persistent
  checker의 존재 이유다.
- `api.Session`: snapshot id(u64) → `snapshotData`(refcount + symbol
  registry는 snapshot 전역 / type·signature registry는 project별).
  Node handle은 `"index.kind.path"` — 파서와 인코더가 같은 순서로 만든
  index 테이블 기반.
- `updateTemporarySnapshot` / `runWithTemporaryFileUpdate`: base snapshot의
  overlay를 복제해 임시 스냅샷을 만들고 콜백 후 폐기. **`latestSnapshot`을
  전진시키지 않는다** — 메인 상태 무오염.
- batch API(`getTypesAtPositions` 등)는 checker 획득 1회 + 순차 루프 —
  병렬성이 아니라 왕복·획득 상각이 목적이다(checker는 동시 사용 불가).

### B.4 sync/async 클라이언트 (`_packages/native-preview`)

- sync 클라이언트는 async 소스에서 **자동 생성**된다(단일 소스, 두 번
  실체화). sync가 존재하는 이유는 기존 동기 생태계(transformer, 빌드 툴).
- sync 동작 원리: 자식 프로세스 fd를 libuv에서 떼어내 `readSync`/`writeSync`
  로 직접 구동. 요청 id가 없고 method 이름이 상관 키 — 그래서 서버의
  `SyncConn`은 뮤텍스 하나로 읽기·쓰기를 전부 감싼다.
- FS 콜백: 클라이언트가 `--callbacks=`로 구현 목록을 선언, 서버 FS가 그
  훅만 클라이언트로 역호출. `readFile`의 3-상태(내용/부재/실물 폴백)가
  와이어에서 보존된다.

### B.5 오류 격리

- 요청 핸들러는 전부 `recover()` — panic은 스택을 실은 에러 응답이 되고
  서버는 산다. 쓰기 실패만 연결 폐기.
- 부모 프로세스 워치독(에디터 사망 → 서버 자살), JS 쪽 `process.on("exit")`
  자식 정리 — 고아 방지가 양방향.

---

## C. 비교 — RL 현재 vs tsgo

| 관심사 | tsgo | RL 현재 |
|---|---|---|
| 지속 상태의 소유자 | `project.Session`(Rust로 치면 라이브러리 타입) | 없음 — `native::Session`(프로세스 핸들)과 host.mjs 내부 상태뿐 |
| 스냅샷 | 불변 + refcount + COW builder | 없음 — `Lowered`가 매 패스 재생성 |
| 문서(버퍼) 모델 | overlay = 정상 상태, FileChange 큐 + 축약 | CLI `--overlay`(1파일, stdin) + 에디터의 3중 자체 저장소 |
| 증분 | 파일 1개 변경 → Program 구조 재사용; ParseCache 해시 키 | host.mjs의 fileChanges만; Rust 드라이버는 전량 재계산 |
| 배치 컴파일 | project 시스템 밖(직접 Program) | 동일 (main.rs 배치 경로) — **이미 tsgo와 같은 분리** |
| semantic 소비자 | CLI/LSP/API가 같은 Session 소비 | CLI는 API server, 에디터는 별도 `tsgo --lsp` |
| 임시 질의 | temporary snapshot (메인 무오염) | 에디터 probe가 자체 임시 문서로 흉내 |
| 오류 격리 | recover + 워치독 | host 죽음 = ask 에러(요청 실패로 국한)는 되어 있으나 에디터 tsgo 세션은 재시작 없음 |

RL 고유의 차이: **Projection 계층**. tsgo는 `TS source → Program`이지만 RL은
`RL source → lowering → TS source → Program`이 끼어 있고, 이 계층
(MappedEmit + EmitMapping + ScrutineeTemp)이 engine의 일급 상태여야 한다.

---

## D. 채택 / 변형 / 기각

| tsgo 설계 | 결정 | 이유와 RL에서의 형태 |
|---|---|---|
| Session(가변) / Snapshot(불변) 분리 | **채택** | `engine::Project`(가변: 문서·캐시·백엔드 세션) / `engine::Snapshot`(불변: 파일 집합 + projection + probe). stale 결과 문제를 구조로 해결. |
| 모든 변경은 Clone 한 깔때기 | **채택** | `Project::snapshot()`만 스냅샷을 만든다. 내용 해시로 무변경 파일의 projection을 재사용(COW의 RL 판). |
| 배치 컴파일은 project 시스템 밖 | **채택** | `rlc`의 untyped 빌드 경로(main.rs)는 엔진을 태우지 않는다 — tsgo의 `execute.CommandLine`과 같은 자리. 상태가 필요 없는 1회 실행에 세션 기구를 강제하지 않는다. |
| 문서 갱신을 FileChange 큐로 | **변형** | RL의 규모에서는 큐+축약 대신 `DocumentStore`의 직접 mutate(open/update/close)로 충분하다. 요청 시점에 스냅샷을 만드는 "요청은 앞선 편집을 본다" 보장은 동일하게 성립. |
| ParseCache(내용 해시 키) | **변형** | AST 캐시 대신 **projection 캐시**: `(경로, 내용 해시) → Arc<ProjectedDocument>`(방출 TS + 매핑 + probe 앵커 + module scan). RL의 비싼 단계는 파싱이 아니라 "파일당 5회 재파싱"이므로, 한 번 계산해 스냅샷 간 재사용하는 것이 같은 문제의 더 싼 해답. |
| checkerPool 3분류 / persistent checker | **기각(현재)** | checker는 tsgo 프로세스 안에 있고 RL은 `ask` 1왕복 batch만 쓴다 — Type/Symbol handle을 세션 간 보존하지 않으므로 필요 없음. handle을 노출하게 되면 그때 도입. |
| api.Session의 handle registry (Symbol/Type/Node id) | **기각(공개 API)** | TS7 구현 세부가 RL 공개 API로 새면 안 된다(요청 §15). RL-owned `Diagnostic` 등으로만 반환. host 내부의 symbol id 사용은 지금처럼 `Resolution.id`로 격리 유지. |
| temporary snapshot | **채택(개념)** | 에디터 completion probe가 이미 같은 필요다. 엔진의 `--server`에 "임시 텍스트로 1회 질의" 형태로 수용하되, 메인 스냅샷을 전진시키지 않는 규칙을 계약으로. tsgo의 `runWithTemporaryFileUpdate`를 host에서 직접 쓰는 것은 후속(지금은 layered FS diff로 충분). |
| sync/async 이중 클라이언트 | **기각(현재)** | RL의 외부 소비자는 아직 에디터 서버 하나다. JSON-lines 단일 프로토콜로 시작하고, 필요가 생기면 tsgo처럼 한 소스에서 생성한다. "tsgo와 같다는 이유만으로 만들지 않는다"(요청 §6). |
| msgpack 프레이밍 | **기각(현재)** | rlc↔host는 이미 line-JSON이고 병목은 IPC 횟수였다(TASK-083에서 batch로 해결). 전송 형식 교체는 측정된 필요 없이는 하지 않는다. 전송은 `native.rs`에 격리되어 있어 언제든 교체 가능 — 그것이 이 경계의 존재 이유. |
| recover 기반 오류 격리 | **채택(부분)** | host 죽음 → ask 에러(요청 실패, 세션 폐기 후 재시작 가능)는 이미 있음. 엔진 서버 모드는 요청 단위 격리(한 요청의 실패가 세션을 죽이지 않음)를 계약으로 명시. 에디터의 "진단을 성공으로 지우지 않기"는 유지. |
| 부모 프로세스 워치독 | **채택** | `rlc --server`는 stdin EOF에서 종료 — LSP 서버가 죽으면 자연 종료. (tsgo의 pid 폴링 워치독까지는 불필요: 전송이 stdio뿐이다.) |
| FENNEL checker 파티셔닝, 워크스페이스 다중 프로젝트 서비스, ATA, auto-import 레지스트리 | **기각** | RL의 현재 표면에 대응물이 없다. multi-tsconfig는 `Engine`이 프로젝트를 여럿 소유할 수 있는 형태(키: tsconfig 경로)로 자리만 만든다. |

---

## E. 최종 아키텍처

### E.1 계층

```
                         rlc (CLI)                editors/vscode (LSP adapter)
                            │                            │
              배치 빌드      │ typed 검사/감시              │ --server (JSON-lines)
          (엔진 밖, tsc식)   │                            │
                            ▼                            ▼
                      ┌──────────────────────────────────────┐
                      │            rlc::engine               │
                      │  Engine ── open_project ──► Project  │
                      │                │  update()           │
                      │                ▼                     │
                      │            Snapshot (불변)           │
                      │   files: Arc<ProjectedDocument>[]    │
                      │   (source·generated·mappings·probes) │
                      └──────────────┬───────────────────────┘
                                     │ Query / Answers (backend seam, 불변 계약)
                                     ▼
                      ┌──────────────────────────────────────┐
                      │      rlc::typescript (라이브러리로 이동) │
                      │  TypeScriptSession(native) → host.mjs │
                      │  → tsgo --api (증분 snapshot)          │
                      └──────────────────────────────────────┘
```

### E.2 소유권 (source of truth)

| 소유자 | 소유물 |
|---|---|
| `engine::DocumentStore` | 디스크/overlay 문서의 현재 텍스트와 버전 — "TypeScript가 보는 텍스트"의 유일 원천 |
| `engine::ProjectedDocument` | 한 파일의 projection 전부: 원문, module path(`x.rl.ts`), `MappedEmit`, probe 앵커, module scan — **내용 해시로 캐시** |
| `engine::Snapshot` | 특정 순간의 프로젝트: 파일 집합 + projection + 조립된 Query 재료. 불변 |
| `engine::Project` | 문서·projection 캐시·백엔드 세션·스냅샷 생산. 가변, 장수명 |
| `typescript::backend` | Query/Answers seam — rl의 용어로만. **이번 작업에서 계약 불변** |
| `typescript::native`+`host.mjs` | tsgo 도달 방법. 불안정성 격리 지점(불변) |

### E.3 CLI

- `rlc --check-types`/`--types`/`-w`/`--overlay`: `Engine::open_project` →
  `Project::update()` → `Snapshot` → `Project::check(&snapshot)` →
  RL-owned 진단을 CLI가 출력. **문안·순서·종료 코드는 현재와 바이트 동일**
  (native 테스트 23개가 게이트).
- watch: stamp 변경 → `Project::update()`가 바뀐 파일만 재-projection →
  같은 세션에 ask. 지금까지 host에만 있던 증분성이 Rust 쪽에도 생긴다.
- untyped 빌드/`--check`/`--symbols`/`--emit-map`/`--sidecar`: 엔진 밖
  (tsgo의 배치 tsc와 같은 지위). 단 walk/확장자 목록 등 공용 조각은 공유.

### E.4 에디터 (이번 단계와 최종 목표)

이번 단계: `rlc --server`(엔진 세션을 지속시키는 JSON-lines 서버)를 추가하고,
에디터의 rlc 호출(`--check`·`--check-types`·`--emit-map`) 이 서버를
경유하게 한다 — 프로세스/컴파일러를 매번 기동하던 것이 세션 재사용으로
바뀐다(§41 latency). 프로토콜 응답은 기존 one-shot 출력과 동형이고, 서버가
없으면 one-shot으로 폴백하므로 observable behavior는 불변.

최종 목표(후속 태스크로 기록): hover/definition/references/completion/rename/
signatureHelp도 엔진 semantic API가 답하고 `tsgo --lsp` 직결(TsgoProject)을
제거한다. 그 시점에 virtualDocs/diskVirtuals/analysis.ts가 엔진으로 흡수된다.
이 순서를 택한 이유: 그 7개 기능은 에디터 테스트 76개가 잠근 표면이고, 한
번에 옮기면 parity 증명이 불가능하다 — 요청 §37의 "각 단계에서 테스트로
parity를 증명한다"에 따라 컴파일러 쪽 경로부터 옮긴다.

### E.5 삭제 (이번 단계)

- `check.rs`의 절차적 오케스트레이션 → 엔진으로 해체.
- `typescript/project.rs`의 `Lowered`/`query` → `engine::projection`으로 승격.
- 죽은 경로 `Sink::Calls`/`val::method_calls`/`ValMethodCall`/
  `rlc::val_method_calls`(P4 보류분) → 제거.
- `TS_EXTENSIONS` 중복, walk 중복 → 공용화.

### E.6 지키는 계약

- backend seam(Query/Answers)과 host 프로토콜: 불변.
- 진단 문안·위치·순서·종료 코드: 불변 (`errors.md`/`cli.md` 규범).
- `defer_to_checker` 이중 구조(untyped 근사 / typed symbol identity): 규범
  그대로.
- emit-map의 "매핑된 조각은 양 좌표계에서 바이트 동일" 불변식: 그대로
  (에디터·사이드카·진단 매핑이 동시에 걸려 있는 유일한 불변식).
