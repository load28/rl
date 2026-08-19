# TASK-086: Project/Snapshot 기반 Language Engine 아키텍처 재구성

- **상태**: 진행 중
- **시작일**: 2026-08-19
- **완료일**: —
- **커밋**: —

## 목적

RL을 CLI/에디터 중심 구조에서 tsgo(typescript-go)와 유사한 지속형
Project/Snapshot 기반 Language Engine으로 재구성한다. rlc CLI, VSCode
LSP 서버, 향후 plugin/외부 API가 모두 하나의 engine을 소비하게 하고,
현재 이원화된 semantic pipeline(rlc typed pipeline vs VSCode TsgoProject)을
단일 authoritative state로 통합한다.

## 범위

- 포함: engine/project/snapshot/document/projection 계층 신설, CLI를
  engine consumer로 재작성, typed 검사·probe·mapping의 engine 이관,
  에디터 가상문서/프로젝트 그래프 로직의 core 이관, tsgo 실제 구현
  분석 리포트, behavior compatibility suite.
- 제외: 언어 표면 변경(구문/에러/CLI 옵션/방출 코드 의미는 그대로),
  std 변경, 버전 변경.

## 절대 조건

현재 RL이 제공하는 모든 기능의 observable behavior를 동일하게 유지한다.
기존 테스트 전부 통과 + 신규 compatibility 테스트 통과가 게이트다.

## 의사결정

(진행 중 기록)

## 작업 내역

- 2026-08-19: 태스크 시작. 현재 코드베이스 전수 조사 + microsoft/typescript-go
  최신 main 클론 및 아키텍처 분석 착수. 베이스라인 `cargo test` 실행.

## 이슈 및 해결

(진행 중 기록)

## 검증

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo test`

## 결과

(완료 시 기록)
