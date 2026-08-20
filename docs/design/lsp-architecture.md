# RL LSP — 엔진 어댑터 아키텍처

TASK-087의 설계 기록이다. **A. 기존 RL LSP → B. tsgo LSP 실제 구현 →
C. 채택/변형/기각 → D. 최종 구조**. 분석 대상은 `microsoft/typescript-go`
HEAD `c6b013f5`(로컬 클론·빌드)와 TASK-086 완료 시점의 main이다.

한 줄 원칙:

> LSP is not the language engine.
> LSP is a protocol adapter to the language engine.

절대 조건: 현재 RL LSP가 제공하는 모든 기능의 observable result 유지.
에디터 테스트 스위트(및 e2e `server.test.ts` 무수정 통과)가 게이트다.

---

## A. 기존 RL LSP — 무엇이 문제였나

`server.ts`(1712줄)는 어댑터가 아니라 그 자체가 language engine이었다:

- **자체 tsgo 클라이언트.** `lsp.ts`(수제 JSON-RPC) + `tsgo.ts`(TsgoProject)가
  `tsgo --lsp`에 직결 — 컴파일러 백엔드(host.mjs/API server)와 **별개의**
  두 번째 TypeScript 세션. Editor → RL LSP → tsgo LSP의 이중 LSP 구조.
- **자체 프로젝트 그래프.** virtualDocs(열린 버퍼의 방출 TS) +
  diskVirtuals(미열람 import, `rlc --emit-map`을 **동기** 실행) +
  pendingVirtual + 정규식 import 추적(재-export 누락).
- **두 번째 매퍼.** `virtual.ts::MappedDoc` — Rust `mapper.rs`와 드리프트
  (끝-포함 vs 끝-배제, glue 폴백 유무).
- **probe 오케스트레이션**(`probe.ts` + server.ts의 install/serialize),
  **좌표 변환 헬퍼**(toServiceOffset/fromServiceSpan), **표준 라이브러리
  실체화**(ensureStdModule)까지 전부 LSP 프로세스 소유.
- 복구 없음: tsgo 세션이 죽으면 그 세션의 7개 기능이 영구 침묵
  (`alive`는 죽은 코드), 재시작 경로 부재.

## B. tsgo LSP 실제 구현에서 확인한 것

- **`lsp.Server`는 semantic을 소유하지 않는다.** `session *project.Session`
  하나를 들고, 요청의 동기 프리픽스에서 스냅샷을 고르고 본론은 비동기로
  넘긴다. Server는 동시에 `project.Client`(WatchFiles/RefreshDiagnostics/
  PublishDiagnostics/Progress/Configuration)를 구현해 **역방향 의존**을
  인터페이스로 뒤집는다.
- 3개 루프(read/dispatch/write) + 요청별 goroutine, `$/cancelRequest`는
  read 루프에서 즉시 처리(자기 뒤에 줄 서지 않게), 모든 비동기 본론은
  `recover()`로 감싸 요청 실패가 서버를 죽이지 않는다.
- 문서 lifecycle: didOpen은 즉시 flush, didChange/didSave는 큐잉 —
  "요청은 자신보다 먼저 온 편집을 반드시 본다"를 큐 구조로 보장.
- diagnostics: 문서 진단은 **pull**(`textDocument/diagnostic`), 설정 파일
  진단만 push, 갱신 신호는 `RefreshDiagnostics`(클라이언트 re-pull 유도).
- position encoding은 initialize에서 협상해 세션 상태로.
- **API 서버에는 language-service 표면이 없다** (`internal/api/proto.go`
  282줄 전수 확인): completion(`getCompletionsAtPosition`)·references·
  diagnostics는 있으나 hover/quickinfo·rename·signature help·definition은
  LSP(`internal/lsp`)에만 있다.

## C. 채택 / 변형 / 기각 (§53 체크리스트)

| tsgo 설계 | 결정 | RL에서의 형태 |
|---|---|---|
| Server ↛ semantic, Session 소유 | **채택** | Node LSP는 `engine.ts`(EngineSession 클라이언트) 하나를 소유. semantic은 전부 `rlc --server` 뒤의 엔진 |
| Project Session / 문서 lifecycle | **채택** | didOpen/didChange/didClose → 엔진의 `openDocument`/`updateDocument`/`closeDocument`. LSP는 문서 상태의 소유자가 아니다 |
| "요청은 앞선 편집을 본다" 보장 | **변형** | 큐 대신 **파이프 순서**: 문서 sync는 동기 write, 서버는 순차 처리 — 같은 보장을 더 작은 기계로 |
| pull diagnostics | **변형(현상 유지)** | 엔진↔tsgo 사이는 pull(`textDocument/diagnostic`), 에디터↔RL LSP는 기존 push 유지 — VSCode UX 불변이 우선 |
| request cancellation / 요청별 병렬 | **기각(현재)** | 서버는 순차 + 클라이언트 타임아웃 + 버전 기반 stale-drop(기존 것)으로 동등한 UX. 취소·병렬은 측정된 필요가 생기면 (스냅샷이 이미 불변이라 자리는 있다) |
| recover 격리 | **채택** | 요청 실패는 그 요청의 error 응답; 세션은 산다. tsgo LSP 사망 → 다음 질문이 **재시작**(구 구현에 없던 복구, §38) |
| position encoding 협상 | **변형** | VSCode 기본(UTF-16)을 프로토콜 좌표로 고정, 엔진 내부에서 rl 바이트↔UTF-16 변환을 독점. 협상은 다중 클라이언트가 생기면 |
| watch 등록 위임(project→client) | **기각(현재)** | RL 에디터의 watch 표면은 컴파일러 바이너리 감시 하나뿐 — 지금 기구를 만들 이유 없음. 경로만 기록 |
| progress / telemetry / ATA | **기각** | 필요 입증 전 복제하지 않는다 (§40·41) |
| Server가 client 역할 겸장(역방향 인터페이스) | **채택(개념)** | 엔진 재시작 시 `onSessionStart` 콜백으로 열린 문서를 재동기화 — 서버가 세션에 제공하는 유일한 역방향 서비스 |

기능별 TypeScript 도달 방법 (§33 — API vs LSP vs 자체):

| 기능 | 백엔드 | 근거 |
|---|---|---|
| hover / definition / rename / signature help / completion resolve | **tsgo LSP** (`typescript/service.rs`) | API 서버에 해당 표면 없음 (B절) |
| completion | tsgo LSP | API에 `getCompletionsAtPosition`이 있으나 resolve가 item-echo 방식(LSP형) — 한 백엔드로 통일 |
| TS 진단 (에디터) | tsgo LSP pull | parse-error 가드(코드<2000) 등 기존 계약이 이 표면 위에 정의됨 |
| typed rl 진단·소진성·val | tsgo **API server** (기존 Query/Answers) | TASK-073~085의 규범 경로 그대로 |
| rl 구조 기능 (arm completion, 문서 심볼, quick fix, rl hover/definition) | **RL 자체** (analysis.ts) | 타입 불필요 + 미완성 버퍼 내성 필요 |
| rl 이름 hover/definition (enum·케이스·필드) | **엔진** (`engine/names.rs`, `rlSymbol`) | 위와 같은 이유지만 구현이 엔진에 있어야 규칙이 하나다 — 정규식 재구현은 컴파일러와 다른 답을 했다 (TASK-105). 나머지 rl 구조 기능의 이관은 후속 |
| semantic tokens (하이라이팅 정밀화) | **RL 자체** (`engine/tokens.rs`, 파스 전용·무상태) | TextMate 문법이 못 하는 판별(파서가 청구한/안 한 `match`·`result`·`flow`)의 단일 원천은 파서; 툴체인 없이도 답해야 하므로 text 기반 요청 (TASK-093) |

핵심: 이 표는 전부 **엔진 내부**의 세부다. Node LSP는 어느 백엔드가
답했는지 모른다 — 이중 LSP는 public architecture에서 제거됐고(§34),
`tsgo --lsp` 클라이언트는 `src/typescript/service.rs`(Rust) 안에만 있다.

## D. 최종 구조

```
Editor ─ LSP ─ editors/vscode/server (어댑터)
                 ├─ server.ts    프로토콜·능력·디바운스·표시(마크다운/스니펫)
                 ├─ engine.ts    EngineSession 클라이언트 (문서 sync + semantic 요청)
                 ├─ analysis.ts  rl 구문 계층 (미완성 버퍼 내성; 아래 참조)
                 └─ rlc.ts       --check/typedCheck (엔진 경유 + one-shot 폴백)
                       │ JSON lines
                       ▼
              rlc --server (src/server.rs)
                 openDocument/updateDocument/closeDocument
                 hover/definition/references/completion/completionResolve
                 rename/signatureHelp/tsDiagnostics/semanticTokens
                 (+check/emitMap/typedCheck)
                       │
                       ▼
              rlc::engine  ── Project(문서·projection·세션) / Snapshot
                 language.rs   semantic API — 질문도 답도 .rl 좌표
                 (probe·매핑·serve·std 실체화 전부 여기)
                       │                      │
                       ▼                      ▼
              typescript/service.rs    typescript/native.rs + host.mjs
                 tsgo --lsp (LS 표면)     tsgo --api (typed 검사·방출)
```

소유권 이동의 결과:

- **매퍼는 하나다.** `mapper.rs`가 유일한 좌표 변환자가 됐고, 에디터의
  끝-포함 의미론은 `to_output_inclusive`/`to_source_inclusive`로 승격됐다
  (language-service 위치는 포함, 진단 span은 배제 — 두 규칙 다 규범으로
  문서화).
- **probe는 엔진 기능이다.** `$rl_probe` 삽입→emit→위치 매핑→임시 서빙→
  질의→복원이 `engine/language.rs` 안에서 한 요청으로 끝난다. 메인 문서
  상태는 오염되지 않는다(다음 serve가 텍스트 비교로 복원).
- **미열람 `.rl` import는 Project의 기본 동작이다.** 대상 파일 + 이행적
  `.rl` import를 엔진이 projection 캐시에서 서빙한다. import 추적은
  정규식이 아니라 실제 스캐너(`rl_imports`)다 — 재-export도 따라간다
  (의도된 개선, 아래).
- **rename 원자성은 엔진 규칙이다.** 글루로 역매핑 안 되는 edit 하나라도
  있으면 rename 전체가 null — half-rename은 구조적으로 불가능.

### rl 구문 계층(analysis.ts)이 Node에 남는 이유

arm 태그 completion·quick fix가 가장 필요한 순간은 **버퍼가 아직 rl로
파싱되지 않는 순간**이다. rlc의 파서는 무오류·전량-파싱(계약 1)이라
미완성 match를 인식하지 않는다 — 오차 허용 파싱은 언어 표면 결정이
필요한 별도 작업이다. 그래서 이 계층은 "빠질 수 있는 편의(진단 생성
금지)"라는 기존 계약 그대로 어댑터 프로세스에 남는다. §38의 격리
요구와도 일치한다: 엔진/TS 백엔드가 죽어도 rl 구조 기능은 산다.
엔진의 오차 허용 구문 계층이 생기면 이관한다(후속 태스크로 기록).

### 의도된 개선 (§50 — 문서화된 behavior 변경)

1. **TS 세션 복구**: tsgo LSP가 죽으면 다음 요청이 재시작한다 (구현 전:
   영구 침묵). 문서는 재시작 시 재동기화(`onSessionStart`).
2. **import 추적이 재-export를 본다**: `export ... from "./x.rl"`로만
   연결된 모듈도 서빙된다 (구 정규식은 누락). 타입이 없던 곳에 타입이
   생기는 방향의 확장.
3. **raw-text 서빙 폴백 제거**: 구 구현은 `rlc --emit-map` 실패(컴파일러
   부재) 시 raw 텍스트를 서빙했다. 엔진 경로에서 projection은 엔진
   자신이므로 그 실패 모드가 존재하지 않는다 — 항상 방출 TS를 서빙한다.
4. **컴파일러 교체가 세션에 반영된다**: `rl.compilerPath` 변경 시 새
   엔진 세션이 뜨고 문서가 재동기화된다 (구현 전: TsgoProject는 프로세스
   수명 동안 고정).

### 지운 것 (§51)

`tsgo.ts`(TsgoProject) · `lsp.ts`(수제 LSP 클라이언트) · `probe.ts` ·
`virtual.ts`(MappedDoc — 매핑 불변식은 Rust 테스트로 이동) ·
`tstypes.ts` · virtualDocs/diskVirtuals/pendingVirtual와 그 캐시 정책 ·
toServiceOffset/fromServiceOffset/fromServiceSpan · probe 설치/직렬화 ·
`rlc.ts`의 runEmitMap/runEmitMapFileSync/ensureStdModule/stdModulePath.
`server.ts`는 LSP 표면 + rl 구조 계층 + 표시만 남는다.

### lifecycle

- **초기화**: initialize(능력 반환) → initialized. 엔진 세션은 첫 사용
  시 lazy 기동(구 getTsProject와 동일한 타이밍). 기동 시 std 실체화 +
  열린 문서 재동기화.
- **종료**: LSP 클라이언트가 죽으면 서버 프로세스가 죽고, `rlc --server`는
  stdin EOF로, 그 자식 tsgo들은 각 파이프 EOF/kill로 정리된다 — 계층별
  소유자가 자기 자식을 정리하는 사슬. 테스트는 `shutdownEngineServer()`로
  명시 종료.
- **실패 격리**: 요청 실패 = 그 요청의 빈 답 + 로그. 엔진 서버 부재 =
  기능 침묵(기존 "tsgo 없음"과 동일 UX) + rl 구조 기능 유지.
