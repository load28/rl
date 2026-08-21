# TASK-145: 타입 진단 범위의 구문 anchor 폴백

- **상태**: 완료
- **시작일**: 2026-08-22
- **완료일**: 2026-08-22
- **커밋**: —

## 목적

checker가 비교한 표현식 범위가 생성 코드와 원문 매핑에 걸쳐 있을 때 타입 진단이
한 글자만 표시되는 문제를 해결한다. 특정 match가 아니라 모든 lowering 구문에서
완전한 원본 범위를 보장한다.

## 범위

- 포함: 진단 span의 source-map 완전성 판정, 구문 anchor 폴백, CLI·서버·에디터
  공통 범위, 회귀 테스트와 문서
- 제외: 타입 차이 계산과 메시지 변경, 새 RL 문법, VSCode 자체 범위 추측

## 의사결정

1. **진단 시작점과 끝점을 독립 투영하지 않는다.** 시작점만 원문에 있고 끝점이
   생성 glue에 있으면 존재하지 않는 원문 범위를 만들게 된다. 한 verbatim mapping이
   checker span 전체를 덮을 때만 정확한 범위로 인정한다.
2. **모든 lowering에 공통인 origin 분류기를 둔다.** match/result별 진단 번호
   분기 대신 `Exact | Anchor | Nearest`를 source-map 계층에 둔다. batch typed
   check와 언어 서비스가 같은 함수를 사용한다.
3. **표시 범위와 원인 소유 범위를 분리한다.** match는 짧은 head를 밑줄로 보여도
   arm에서 생긴 직접 RL 원인이 match 전체의 생성 결과를 소유해야 한다. anchor와
   RL 진단에 완전한 syntax owner를 기록하고 동일한 owner인 결과만 억제한다.

## 작업 내역

- 2026-08-22: rl-tour의 `toPort` 진단이 `(105, 9) → 범위 없음`으로 서버에서
  전달되어 에디터가 `match` 한 단어만 표시하는 현상을 재현했다.
- 2026-08-22: source-map에 checker span 전체를 판정하는 `DiagnosticOrigin`을
  추가하고 projection, batch report, 언어 서비스의 범위 계산을 통합했다.
- 2026-08-22: lowering anchor에 primary span과 syntax owner span을 분리해
  기록했다. mixed-pattern RL 원인의 owner와 일치하는 생성 비교 진단만 억제했다.
- 2026-08-22: native server 회귀 테스트에 match 반환 불일치, result 바인딩
  불일치, mixed-pattern 연쇄 진단을 함께 고정했다.
- 2026-08-22: `errors.md`, compiler-core 설계 문서, AI 컨텍스트에 공통 범위와
  소유권 계약을 반영했다.
- 2026-08-22: release compiler와 VSCode 확장을 재설치하고 rl-tour의 file 패키지를
  갱신했다. 실제 `_errors-demo.rl`의 서버 응답에서 match와 result 바인딩의
  `endLine`/`endCol`, mixed-pattern의 단일 RL 원인을 확인했다.

## 이슈 및 해결

- **증상**: checker 진단 시작은 원문 조각에 매핑되지만 끝은 생성 glue에 있어
  `endLine`/`endCol`이 사라지고 에디터가 한 단어만 표시했다.
  **원인**: 기존 투영이 span 전체의 매핑 완전성을 판정하지 않고 양 끝점을 따로
  되돌렸다. **해결**: 전체 span을 하나의 origin으로 분류하고 glue와 걸친 범위는
  가장 안쪽 lowering anchor의 primary span으로 투영했다.
- **증상**: mixed-pattern의 직접 RL 오류와 생성 코드의 TS2678이 함께 표시됐다.
  **원인**: 직접 오류의 좁은 표시 범위와 match 전체의 생성 결과 사이에 명시적
  관계가 없었다. **해결**: 별도 syntax owner 범위를 도입해 동일 owner의 연쇄
  진단만 억제했다.
- **증상**: VSCode 테스트가 변경 전 진단 결과를 반환했다. **원인**: 테스트가
  저장소 release가 아니라 PATH의 이전 `~/.cargo/bin/rlc`를 실행했다.
  **해결**: 저장소 `target/release`를 PATH 앞에 둔 검증으로 현재 컴파일러를
  명시하고, 실제 확장은 `scripts/setup`으로 현재 release에 연결했다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` — 665개 통과
- [x] VSCode 진단 경로 회귀 테스트 3개 통과
- [x] VSCode 전체 테스트 103개 중 97개 통과, 기존 범위 밖 6개 실패
- [x] `scripts/setup`으로 release 컴파일러·VSCode 확장 재설치
- [x] rl-tour `npm install` 및 실제 서버 진단 범위 확인

## 결과

checker 진단 범위는 source-map의 한 조각만 우연히 닿는 위치가 아니라 전체 span의
출처로 결정된다. 정확한 원문, lowering의 primary span, 최후 근접점의 세 단계가
모든 RL 구문에 공통으로 적용된다. 별도의 syntax owner가 직접 RL 원인과 생성
checker 결과를 연결하므로 독립적인 사용자 오류는 유지하면서 연쇄 오류만 제거한다.
