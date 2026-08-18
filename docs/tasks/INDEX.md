# 태스크 인덱스

이 파일은 이 저장소 모든 작업의 **단일 진실 소스**입니다.
모든 작업은 태스크 문서로 관리·기록되어야 합니다 — 규칙은 [`CLAUDE.md`](../../CLAUDE.md)의
"태스크 관리 규칙" 참조. 새 태스크는 [`TEMPLATE.md`](./TEMPLATE.md)로 만듭니다.

## 태스크 목록

| ID | 제목 | 상태 | 시작일 | 완료일 | 문서 |
|----|------|------|--------|--------|------|
| TASK-001 | 태스크 관리 체계 및 CLAUDE.md 구축 | 완료 | 2026-08-16 | 2026-08-16 | [TASK-001](./TASK-001-task-system-and-claude-docs.md) |
| TASK-002 | transform.rs 모듈 분리 | 완료 | 2026-08-16 | 2026-08-16 | [TASK-002](./TASK-002-transform-module-split.md) |
| TASK-003 | 포매팅 표준화 및 린트 게이트 | 완료 | 2026-08-16 | 2026-08-16 | [TASK-003](./TASK-003-formatting-and-lint-gates.md) |
| TASK-004 | 패키지 메타데이터·라이선스·거버넌스 문서 | 완료 | 2026-08-16 | 2026-08-16 | [TASK-004](./TASK-004-governance-and-metadata.md) |
| TASK-005 | CI 파이프라인 구축 | 완료 | 2026-08-16 | 2026-08-16 | [TASK-005](./TASK-005-ci-pipeline.md) |
| TASK-006 | 태스크 기록 상세화 규칙 도입 | 완료 | 2026-08-16 | 2026-08-16 | [TASK-006](./TASK-006-detailed-task-records.md) |
| TASK-007 | 라이브러리 수준 문서화 | 완료 | 2026-08-16 | 2026-08-16 | [TASK-007](./TASK-007-library-level-docs.md) |
| TASK-008 | README 라이브러리 스타일 재작성 | 완료 | 2026-08-16 | 2026-08-16 | [TASK-008](./TASK-008-library-style-readme.md) |
| TASK-009 | 레퍼런스 문서 사용자 관점 단순화 | 완료 | 2026-08-16 | 2026-08-16 | [TASK-009](./TASK-009-user-facing-reference-simplify.md) |
| TASK-010 | swc 스타일 컴파일러 아키텍처 재구성 | 완료 | 2026-08-16 | 2026-08-16 | [TASK-010](./TASK-010-swc-style-compiler-architecture.md) |
| TASK-011 | Option/Result 표준 라이브러리와 내장 enum 소진성 검사 | 완료 | 2026-08-16 | 2026-08-16 | [TASK-011](./TASK-011-option-result-stdlib.md) |
| TASK-012 | try 문 — Rust 스타일 에러 전파 | 완료 | 2026-08-16 | 2026-08-16 | [TASK-012](./TASK-012-try-error-propagation.md) |
| TASK-013 | 파이프라인 연산자 `\|>` 설계 제안 | 완료 | 2026-08-16 | 2026-08-16 | [TASK-013](./TASK-013-pipeline-operator-proposal.md) |
| TASK-014 | match or-패턴 (`A \| B => ...`) | 완료 | 2026-08-16 | 2026-08-16 | [TASK-014](./TASK-014-match-or-patterns.md) |
| TASK-015 | match 가드 (`패턴 if 조건 => ...`) | 완료 | 2026-08-16 | 2026-08-16 | [TASK-015](./TASK-015-match-guards.md) |
| TASK-016 | let-else 문 (`const Tag(x) = 식 else { ... };`) | 완료 | 2026-08-16 | 2026-08-16 | [TASK-016](./TASK-016-let-else.md) |
| TASK-017 | std 콤비네이터 확장 (zip/flatten/transpose/collect/fromPromise) | 완료 | 2026-08-16 | 2026-08-16 | [TASK-017](./TASK-017-std-combinators.md) |
| TASK-018 | VSCode 언어 서비스 (LSP 확장) | 완료 | 2026-08-16 | 2026-08-16 | [TASK-018](./TASK-018-vscode-language-service.md) |
| TASK-019 | 모듈 그래프 설계 제안 | 완료 | 2026-08-17 | 2026-08-17 | [TASK-019](./TASK-019-module-graph-proposal.md) |
| TASK-020 | import 지정자 재작성 (모듈 그래프 1단계) | 완료 | 2026-08-17 | 2026-08-17 | [TASK-020](./TASK-020-import-specifier-rewrite.md) |
| TASK-021 | swc 스타일 렉서 도입 — 토큰 기반 파서 재구성 | 완료 | 2026-08-17 | 2026-08-17 | [TASK-021](./TASK-021-lexer-token-parser.md) |
| TASK-022 | 선언 수집과 프로젝트 단위 소진성 검사 (모듈 그래프 2단계) | 완료 | 2026-08-17 | 2026-08-17 | [TASK-022](./TASK-022-project-exhaustiveness.md) |
| TASK-023 | 심볼 인터페이스와 언어 서버 크로스 파일 기능 (모듈 그래프 3단계) | 완료 | 2026-08-17 | 2026-08-17 | [TASK-023](./TASK-023-symbol-interface.md) |
| TASK-024 | 언어 서버 TS 위임 — rl 파일 전반의 심볼 이동 | 완료 | 2026-08-17 | 2026-08-17 | [TASK-024](./TASK-024-ts-language-service-delegation.md) |
| TASK-025 | TS 위임 확장 — 자동완성·참조 찾기·이름 변경 | 완료 | 2026-08-17 | 2026-08-17 | [TASK-025](./TASK-025-ts-completion-references-rename.md) |
| TASK-026 | 프로젝트 프론트엔드 역할 변경 설계 제안 | 완료 | 2026-08-17 | 2026-08-17 | [TASK-026](./TASK-026-project-front-end.md) |
| TASK-027 | `--rewrite-imports ts` 모드 | 완료 | 2026-08-17 | 2026-08-17 | [TASK-027](./TASK-027-rewrite-imports-ts-mode.md) |
| TASK-028 | TypeScript 사이드카 선언 설계 제안 | 완료 | 2026-08-17 | 2026-08-17 | [TASK-028](./TASK-028-ts-sidecar-declarations.md) |
| TASK-029 | `rlc --sidecar` — 에디터 사이드카 생성 | 완료 | 2026-08-17 | 2026-08-17 | [TASK-029](./TASK-029-sidecar-mode.md) |
| TASK-030 | 저장 시 사이드카 갱신 (언어 서버) | 완료 | 2026-08-17 | 2026-08-17 | [TASK-030](./TASK-030-sidecar-on-save.md) |
| TASK-031 | 사이드카가 소스 트리를 어지럽히지 않게 | 완료 | 2026-08-17 | 2026-08-17 | [TASK-031](./TASK-031-sidecar-visibility.md) |
| TASK-032 | 사이드카를 별도 트리로 — 소스/출력 완전 분리 | 완료 | 2026-08-17 | 2026-08-17 | [TASK-032](./TASK-032-sidecar-out-dir.md) |
| TASK-033 | vite-plugin-rl — 번들러가 `.rl`을 직접 읽는다 | 완료 | 2026-08-17 | 2026-08-17 | [TASK-033](./TASK-033-vite-plugin.md) |
| TASK-034 | `rlc -w` — 감시 모드 | 완료 | 2026-08-17 | 2026-08-17 | [TASK-034](./TASK-034-watch-mode.md) |
| TASK-035 | `@rl/std` — 표준 라이브러리 지정자와 자동 방출 | 완료 | 2026-08-17 | 2026-08-17 | [TASK-035](./TASK-035-std-bare-specifier.md) |
| TASK-036 | 타입·빌드 파이프라인 통일 계획 | 완료 | 2026-08-17 | 2026-08-17 | [TASK-036](./TASK-036-unified-type-build-plan.md) |
| TASK-037 | CLI 통일 — 기본 build 모드와 `--types` 파이프라인 | 완료 | 2026-08-17 | 2026-08-17 | [TASK-037](./TASK-037-cli-unification.md) |
| TASK-038 | unplugin-rl — 번들러 어댑터 통합 | 완료 | 2026-08-17 | 2026-08-17 | [TASK-038](./TASK-038-unplugin.md) |
| TASK-039 | 예제를 표준 라이브러리 정식 참조 방식으로 | 완료 | 2026-08-17 | 2026-08-17 | [TASK-039](./TASK-039-examples-use-std-specifier.md) |
| TASK-040 | `--types`를 메모리 방출로 — 캐시 트리 제거 | 완료 | 2026-08-17 | 2026-08-17 | [TASK-040](./TASK-040-in-memory-types.md) |
| TASK-041 | 레퍼런스 문서를 읽는 문서로 정리 | 완료 | 2026-08-17 | 2026-08-17 | [TASK-041](./TASK-041-reference-docs-slimming.md) |
| TASK-042 | TS↔Rust 타입 추론 격차 분석과 rl 기능 제안 | 완료 | 2026-08-17 | 2026-08-17 | [TASK-042](./TASK-042-type-inference-gaps-proposal.md) |
| TASK-043 | 파이프라인 연산자 `\|>` 구현 | 완료 | 2026-08-17 | 2026-08-17 | [TASK-043](./TASK-043-pipeline-operator-impl.md) |
| TASK-044 | 튜플 match — 다중 스크루티니와 곱집합 소진성 | 완료 | 2026-08-17 | 2026-08-17 | [TASK-044](./TASK-044-tuple-match.md) |
| TASK-045 | 중첩 패턴 — `Ok(value: Some(v))` | 완료 | 2026-08-17 | 2026-08-17 | [TASK-045](./TASK-045-nested-patterns.md) |
| TASK-046 | `if let` 문 — 조건부 값 추출 | 완료 | 2026-08-17 | 2026-08-17 | [TASK-046](./TASK-046-if-let.md) |
| TASK-047 | 에디터 `.rl` 파일 아이콘 | 완료 | 2026-08-17 | 2026-08-17 | [TASK-047](./TASK-047-file-icon.md) |
| TASK-048 | npm 패키징 — `npm install rl-lang`으로 rlc 설치 | 완료 | 2026-08-17 | 2026-08-17 | [TASK-048](./TASK-048-npm-packaging.md) |
| TASK-049 | 버저닝 가이드 수립 (CLAUDE.md) | 완료 | 2026-08-17 | 2026-08-17 | [TASK-049](./TASK-049-versioning-guide.md) |
| TASK-050 | 방출 매핑 기반 TS 위임 — 컴파일 출력을 가상 문서로 | 완료 | 2026-08-17 | 2026-08-17 | [TASK-050](./TASK-050-emit-map-virtual-ts.md) |
| TASK-051 | TypeScript 7 릴리스로 인한 CI 복구 — `--types` 진단과 게이트 고정 | 완료 | 2026-08-18 | 2026-08-18 | [TASK-051](./TASK-051-typescript-7-ci-fix.md) |
| TASK-052 | AI 코딩 도구용 rl 사용 가이드 | 완료 | 2026-08-18 | 2026-08-18 | [TASK-052](./TASK-052-ai-usage-guide.md) |
| TASK-053 | AI 가이드 압축 — 파일 사이즈 최소화 | 완료 | 2026-08-18 | 2026-08-18 | [TASK-053](./TASK-053-ai-guide-compression.md) |
| TASK-054 | AI 가이드 확장(설치·업데이트·워크플로)과 `rlc help` 주제별 헬프 | 완료 | 2026-08-18 | 2026-08-18 | [TASK-054](./TASK-054-ai-guide-full-and-cli-help.md) |
| TASK-055 | 에디터에서 `Option`/`Result`·파이프라인이 `any`로 추론되는 문제 | 완료 | 2026-08-18 | 2026-08-18 | [TASK-055](./TASK-055-editor-any-inference-fix.md) |
| TASK-056 | 대규모 코드베이스 대비 컴파일러 성능 개선 | 완료 | 2026-08-18 | 2026-08-18 | [TASK-056](./TASK-056-large-codebase-performance.md) |
| TASK-057 | 타입 에러를 `.rl` 원본 위치로 — `--types` 위치 매핑과 에디터 TS 진단 | 완료 | 2026-08-18 | 2026-08-18 | [TASK-057](./TASK-057-type-errors-at-source.md) |
| TASK-058 | 에디터 타입 진단 오탐 — lib.d.ts 없는 프로그램이 지어낸 `TS2488` | 완료 | 2026-08-18 | 2026-08-18 | [TASK-058](./TASK-058-editor-type-environment-guard.md) |
| TASK-059 | VSIX 패키징 검증 — TypeScript `lib*.d.ts` 누락 확정과 절차 문서화 | 완료 | 2026-08-18 | 2026-08-18 | [TASK-059](./TASK-059-vsix-lib-packaging.md) |
| TASK-060 | 에디터 타입 진단이 워크스페이스 `tsconfig.json`을 반영할지 검토 | 대기 | — | — | [TASK-060](./TASK-060-editor-tsconfig-adoption.md) |
| TASK-061 | 검증 안 된 lib 폴백 제거와 패키징 CI 게이트 | 완료 | 2026-08-18 | 2026-08-18 | [TASK-061](./TASK-061-lib-fallback-trim-and-ci-guard.md) |
| TASK-062 | 표준 라이브러리 메서드가 자동완성에 안 나오는 문제 | 완료 | 2026-08-18 | 2026-08-18 | [TASK-062](./TASK-062-editor-member-completion.md) |

## 다음 태스크 번호

**TASK-063**
