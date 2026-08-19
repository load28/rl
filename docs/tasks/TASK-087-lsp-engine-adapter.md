# TASK-087: LSP 재설계 — 엔진 어댑터화와 TsgoProject 제거

- **상태**: 완료
- **시작일**: 2026-08-19
- **완료일**: 2026-08-19
- **커밋**: (기록 커밋에서 기입)

## 목적

RL LSP를 자체 language engine(자체 tsgo LSP 클라이언트·가상문서 저장소·
두 번째 매퍼·probe 오케스트레이션)에서 **RL Engine의 얇은 프로토콜
어댑터**로 재설계한다. tsgo의 실제 LSP/Project Session 구조를 분석해
차용하고, 이중 LSP(Editor→RL LSP→tsgo LSP)를 public architecture에서
제거한다. 설계 기록: `docs/design/lsp-architecture.md`.

## 범위

- 포함: 엔진의 언어 서비스 계층(`engine/language.rs` — hover/definition/
  references/completion+probe/rename/signatureHelp/tsDiagnostics, 전부
  `.rl` 좌표), `typescript/service.rs`(tsgo --lsp 도달 방법, seam 뒤 격리),
  `--server` 프로토콜 확장(문서 lifecycle + semantic 메서드), Node 서버의
  어댑터화와 구 파이프라인 삭제, 테스트 이관.
- 제외: rl 구조 계층(analysis.ts)의 엔진 이관 — 오차 허용 rl 파싱이라는
  언어 표면 결정이 필요, 후속 태스크로 기록. LSP 요청 취소/병렬 처리 —
  기각 사유는 설계 문서 C절.

## 절대 조건

현재 RL LSP가 제공하는 모든 기능의 observable result 유지. e2e
`server.test.ts` **무수정** 통과가 최종 게이트.

## 의사결정

### 결정 1: TypeScript 도달 방법을 기능별로 실측해 정한다 (§33)

- **상황**: 요청은 기능마다 TS7 API / TS7 LSP / RL 자체 중 어느 백엔드를
  쓸지 명시하라고 요구했다.
- **검토**: tsgo HEAD의 `internal/api/proto.go` 282줄 전수 확인 —
  `getCompletionsAtPosition`·references·diagnostics는 API 서버에 있으나
  **hover/quickinfo·rename·prepareRename·signature help·definition은
  없다**. 완전한 language-service 표면은 `internal/lsp`뿐.
- **선택**: LS 표면은 `tsgo --lsp`를 엔진 내부(`typescript/service.rs`)에서
  몬다. typed 검사·선언 방출은 기존 API server 경로(Query/Answers) 유지.
  두 클라이언트 모두 `src/typescript/` 안 — 상위 계층은 어느 쪽인지
  모른다(§34). API 서버가 LS 표면을 얻으면 service.rs만 교체하면 된다.

### 결정 2: LSP Server는 세션 하나만 소유한다 (tsgo `Server → project.Session` 채택)

- **상황**: server.ts가 프로젝트 그래프·매핑·probe·TS 세션을 직접 소유.
- **선택**: Node 서버는 `engine.ts`(EngineSession 클라이언트) 하나를
  소유하고, didOpen/didChange/didClose를 엔진에 전달하며, semantic 요청을
  `.rl` 좌표로 묻고 받는다. 엔진 재시작 시 열린 문서 재동기화는
  `onSessionStart` 콜백 — tsgo의 `project.Client`(서버가 세션에 역방향
  서비스를 제공) 개념의 최소 형태.

### 결정 3: "요청은 앞선 편집을 본다"를 큐가 아니라 파이프 순서로

- **상황**: tsgo는 dispatch 큐의 동기 프리픽스로 이 순서를 보장한다.
- **선택**: 문서 sync를 **동기 write**로 보내고 서버가 순차 처리하므로,
  같은 파이프에 뒤에 쓴 요청은 반드시 앞의 didChange 이후에 처리된다 —
  같은 보장을 더 작은 기계로. stale 결과는 기존 버전 검사(어댑터)가
  버린다. 요청 취소·요청별 병렬은 기각(설계 문서 C절): 순차 서버 +
  타임아웃 + 버전 드롭으로 동등한 UX이고, 스냅샷이 불변이라 필요해지면
  자리를 만들 수 있다.

### 결정 4: 좌표 계약 — 프로토콜은 `.rl`의 LSP 위치만 말한다 (§25·26)

- **상황**: 세 좌표계(rl 바이트 / 방출 TS UTF-16 / LSP line·char)가
  에디터와 컴파일러 양쪽에 흩어져 드리프트했다 (끝-포함 vs 끝-배제).
- **검토한 대안**: newtype 래퍼 도입 vs 변환의 모듈 격리.
- **선택**: 변환을 `mapper.rs`+`language.rs`에 격리하고 프로토콜 좌표를
  `.rl`의 0-기반 line/character(UTF-16)로 고정. 에디터의 끝-포함 조회
  의미론은 `to_output_inclusive`/`to_source_inclusive`로 승격해 규범화
  (language-service 위치는 포함, 진단 span은 배제). newtype은 기각 —
  변환이 한 모듈에 격리된 상태에서 얻는 것보다 마찰이 크다고 판단,
  문서로 결정을 남긴다.

### 결정 5: probe는 엔진의 completion 내부 단계다 (§13)

- **선택**: `$rl_probe` 삽입 → `emit_mapped`(무오류) → 포함-끝 매핑 →
  임시 서빙 → 질의 → lazy 복원이 `Project::completion` 안에서 끝난다.
  resolve를 위한 `last_probe`도 엔진 상태. 구 구현의 "probe 텍스트로
  진단 금지" 불변식은 구조적으로 성립한다 — 진단 경로는 probe 상태를
  아예 볼 수 없다 (구 구현은 serialize 큐가 지키지 못했던 부분).

### 결정 6: rename 원자성은 엔진 규칙 (§17)

- **선택**: 글루로 역매핑 불가한 edit·파일 아닌 대상·설명 불가한
  newText 형태 → rename 전체 null. placeholder 검증까지 엔진으로 이동,
  어댑터는 placeholder→새 이름 치환만.

### 결정 7: rl 구조 계층(analysis.ts)은 어댑터 프로세스에 남긴다

- **상황**: §55는 semantic의 완전한 엔진 이관을 최종 목표로 한다.
- **검토**: arm 태그 completion·quick fix가 가장 필요한 순간은 버퍼가
  **아직 rl로 파싱되지 않는** 순간이다. rlc 파서는 무오류·전량-파싱
  (계약 1)이라 미완성 match를 인식하지 않는다 — AST 기반 이관은 이
  기능들을 정확히 그 순간에 잃는다(관측 회귀).
- **선택**: 타입 무관·미완성-내성 계층으로 어댑터에 유지(진단 생성 금지
  계약 그대로). §38의 격리 요구와 일치 — 엔진이 죽어도 rl 구조 기능은
  산다. 엔진에 오차 허용 구문 계층이 생기면 이관(후속 태스크 기록).

### 결정 8: 의도된 개선 4건 (§50 — 문서화)

① TS 세션 사망 시 다음 질문이 재시작(구: 영구 침묵, `alive`는 죽은 코드).
② import 추적이 재-export를 본다(구 정규식은 누락 — scan_module 사용).
③ raw-text 서빙 폴백 제거(그 실패 모드는 엔진 경로에 존재하지 않음).
④ `rl.compilerPath` 변경이 세션 재기동+문서 재동기화로 반영(구: 프로세스
수명 동안 고정). 전부 lsp-architecture.md에 기록.

## 작업 내역

- 2026-08-19: 조사 — server.ts/tsgo.ts/lsp.ts/probe.ts/virtual.ts 전문
  정독, tsgo `internal/api/proto.go` 메서드 전수 확인(결정 1의 근거),
  tsgo LSP 구조 분석(TASK-086의 분석 위에 §53 체크리스트 작성 —
  lsp-architecture.md C절).
- 2026-08-19: Rust — `typescript/service.rs`(LSP 프레이밍·핸드셰이크·
  서버발 요청 응답·타임아웃, reader 스레드), `mapper.rs`에 포함-끝 조회
  2종, `engine/language.rs`(RL-owned 결과 타입 + hover/definition/
  references/completion+probe/completionResolve/rename/signatureHelp/
  service_diagnostics + serve(이행적 import)/ensure_std_module + u16
  좌표 변환), `server.rs`에 문서 lifecycle + semantic 메서드 8종
  (Sessions로 재구조화, typedCheck는 열린 문서의 overlay를 보존).
- 2026-08-19: Node — `engine.ts` 신설(rlc.ts의 클라이언트를 이전·확장:
  문서 sync, semantic 래퍼, 재기동 콜백), `rlc.ts` 축소(runEmitMap/
  runEmitMapFileSync/ensureStdModule/stdModulePath 삭제), `server.ts`
  전면 재작성(어댑터 + rl 구조 계층 + 표시만; virtualDocs/diskVirtuals/
  pendingVirtual/toServiceOffset류/probe 설치/getTsProject 삭제),
  `tsgo.ts`/`lsp.ts`/`probe.ts`/`virtual.ts`/`tstypes.ts` 삭제.
- 2026-08-19: 테스트 이관 — `engine.test.ts` 신설(구 tsgo.test.ts의 11개
  의도: 미저장 버퍼 hover, .ts로의 definition, 참조, rename 3-span·
  shorthand 확장·거부-null, 시그니처, 편집 반영 + 신규: close 후 디스크
  복귀), `emitmap.test.ts`/`completion.test.ts`를 엔진 API 위로 재작성
  (모든 구 의도 유지 — TASK-050/055/057/058/062/080 시나리오),
  `virtual.test.ts` 삭제(매핑 불변식은 Rust 단위 테스트로 이동),
  `positions.ts` 헬퍼, `tracked.ts` 삭제(after에서
  `shutdownEngineServer`). `server.test.ts`(e2e)·analysis·typedcheck·
  sidecar는 **무수정**.
- 2026-08-19: 문서 — `docs/design/lsp-architecture.md` 신설(A~D +
  채택/변형/기각 표 + 기능별 백엔드 표 + lifecycle), `cli.md` `--server`
  절 확장, `CLAUDE.md` 맵 갱신, 확장 README의 아키텍처 서술 갱신(낡은
  vsix lib 검증 문단과 "TS 추론 타입 특정" 과잉 주장도 실측에 맞게 수정).
- 검증: `cargo fmt --check` / clippy `-D warnings` / `RLC_TSGO_ROOT=…
  cargo test`(450 passed, 0 failed) / 에디터 `npm test` 70/70
  (skip 0, e2e 무수정 통과). 수동 스모크: `--server`로 match 암 바인딩
  hover(`const radius: number`, rl 좌표 span)·완성·진단 확인.

## 이슈 및 해결

### 이슈 1: definition이 rl enum 방출 글루에서 빈 결과

- **증상**: `import { Shape }`의 `Shape` 위 definition이 [] — 대상이
  enum 방출(글루)이라 역매핑 불가.
- **원인 조사**: 구 경로도 동일했다 — server.ts가 rl 심볼을 구조 계층
  (importedEnums)에서 먼저 답하고, TS 위임은 글루 span을 버려 null이었다.
- **해결**: 버그 아님 — 구조 계층이 답하는 기존 구성 유지. 엔진 답이
  구 TS-위임 경로와 동치임을 확인하는 계기가 됐다.

### 이슈 2: 요청 타임아웃과 세션 사망의 의미 분리

- **증상**: 최초 설계에서 timeout을 Err로 취급하면 세션이 재시작되어,
  느린 1회 요청이 서빙 상태 전체를 날림 (구 클라이언트는 timeout에도
  세션 유지).
- **해결**: `Service::request`가 timeout·서버측 에러 → `Ok(Null)`(기능만
  무응답), 전송 파손 → `Err`(다음 질문이 재시작)로 계약을 분리. 구
  동작(=timeout에 세션 유지) 보존 + 개선(사망 시 재시작)만 추가.

### 이슈 3: 테스트 개수 착시 (87 → 70)

- **증상**: 이관 직후 실행에서 87 pass — 삭제한 소스의 **낡은 컴파일
  산출물**(out/*.test.js)이 함께 돌고 있었다.
- **해결**: out/ 정리 후 70/70 (skip 0). 구 76과의 차이는 Rust로 이동한
  단위 테스트(MappedDoc 5 + documentUri 1)이며, observable-behavior
  테스트는 전수 이관됐음을 Subtest 목록 대조로 확인.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` — 450 passed / 0 failed (tsgo 툴체인 포함 전 경로)
- [x] 에디터 `npm test` — 70/70, skip 0, `server.test.ts` e2e 무수정 통과

## 결과

변경 요약: `src/typescript/service.rs`·`src/engine/language.rs` 신설,
`mapper.rs`·`server.rs` 확장 / 에디터 `engine.ts` 신설, `server.ts` 재작성
(1712→어댑터), `tsgo.ts`·`lsp.ts`·`probe.ts`·`virtual.ts`·`tstypes.ts`·
`tracked.ts`·`virtual.test.ts` 삭제, 테스트 3파일 엔진 API로 이관 /
문서 4건.

성능: semantic 요청은 파이프 홉 추가에도 정상 상태 1–2ms(hover 실측,
첫 요청 91ms = tsgo 기동+핸드셰이크 — 구 구현도 첫 사용 시 동일 비용).
typedCheck는 TASK-086의 세션 재사용(3–5ms) 그대로.

**남은 부채**: ① rl 구조 계층(analysis.ts)의 엔진 이관 — 오차 허용 rl
파싱 필요(언어 표면 결정). ② quick fix의 구조화(진단 message 정규식
의존 제거 — 코드/데이터 채널). ③ 요청 취소·병렬(필요 측정 시).
④ multi-root 워크스페이스(엔진은 이미 프로젝트 identity별 — 어댑터의
파일별 라우팅은 자연히 동작하나 fixture가 없다: 테스트 추가 후보).
