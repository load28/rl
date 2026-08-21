# TASK-123: 이름 해석의 단일화 — analysis가 resolver를 소비한다 (Phase 3 1/2)

- **상태**: 완료
- **시작일**: 2026-08-21
- **완료일**: 2026-08-21
- **커밋**: —

## 목적

TASK-122가 남긴 부채의 해소: 이름 해석(미지의 태그/필드 진단, 해석된 이름
표, match 단위 억제 플래그)의 구현이 analysis와 resolve 두 벌로 병존하던
것을 **resolve 단일 구현**으로 만든다. subject 식별 규칙(`identify`)도
resolver만 갖는다. analysis에 남는 것은 typed 모델(바인딩 타입,
coverage/usefulness)뿐이다.

## 범위

- 포함: analysis의 `Names`/`resolve_alternatives`/`resolve_bindings`/
  `unique_near_case`/`Table::identify` 삭제, 편집 거리·제안 함수
  (`nearest`/`nearest_within`/`typo_distance`/`edit_distance`)의 resolve
  이동(테스트 포함), `attach_resolution` — resolver의 답(`unresolved`/
  `uses`)을 `PatternAnalyses`의 기존 어휘(`UnresolvedName`/`ResolvedName`/
  `has_unresolved`)로 변환·주입. resolve에 `ExternDecl`(필드 포함 외부
  선언; `ExternEnum`/`EnumSymbol` 변환)과 `UnresolvedUse::site`(복구
  경계 키) 추가.
- 제외: coverage/usefulness의 HIR 이동(Phase 3 2/2), `Table` 자체의 제거
  (구축과 `resolve`/`candidates`는 typed 모델용으로 유지 — 후속 단계).

## 의사결정

### 결정 1: PatternAnalyses의 공개 어휘를 유지하고 데이터 원천만 바꾼다

- **상황**: 소비처(sema의 보고, 에디터의 `names.rs`)가
  `UnresolvedName`/`ResolvedName`을 소비 중.
- **선택과 근거**: 형태 유지 + `attach_resolution` 변환. 소비처 무변경으로
  회귀 위험 최소화. 사이트↔match 대응은 양쪽 다 갖고 있는 구문 키워드
  오프셋(HIR site node span의 start == `MatchAnalysis::keyword_off`)으로
  잇는다 — 문자열이 아니라 위치 키지만, identity가 아니라 조인 키로만
  쓰인다(둘 다 같은 파스에서 나온 같은 구문).

### 결정 2: `Table::identify`를 삭제한다

- **상황**: analysis의 해석 호출부가 사라지면 identify의 유일한 존재
  이유가 사라진다.
- **선택과 근거**: 삭제. subject 식별 규칙("모든 태그 후보 → 유일 최다
  포함 → 동률/무관은 침묵")은 이제 `resolve::Resolver::identify` 하나다.
  `Table::resolve`(바인딩 타입이 읽는 첫 all-tags 후보)와
  `candidates`(usefulness의 열 알파벳)는 다른 질문이므로 유지.

## 작업 내역

- 2026-08-21: `src/resolve/mod.rs` — `ExternDecl`/`ExternVariant`/
  `ExternField` 도입(+`From` 변환), `resolve_file(&mut HirFile,
  &[ExternDecl])`로 시그니처 변경, `UnresolvedUse::site` 추가와 해석
  경로 전체에 사이트 전달, 제안 함수 4종과 그 단위 테스트 이동.
- `src/analysis/mod.rs` — 해석 관련 코드 전부 삭제(위 범위), `analyze_*`
  시그니처에서 `Names` 제거, `pattern_analyses`/`coverage_analyses`가
  `attach_resolution`으로 resolver 실행·주입, `has_unresolved`는 resolver
  의 사이트 단위 답으로 설정.
- `src/hir/mod.rs` — `lower_program`을 crate 내부로 재수출.
- `tests/resolve.rs` — 새 시그니처 반영. 동등성 테스트는 유지(이제
  "resolver 답이 기존 표면으로 손실 없이 변환되는가"의 계약).

## 이슈 및 해결

### 이슈 1: 편집 스크립트가 rustfmt 재포매팅과 어긋나 일부 변경 유실

- **증상**: 중간 빌드에서 `resolve_constructor` 시그니처 불일치 등 5건.
- **원인**: 일괄 치환 스크립트의 앵커 문자열이 도중의 `cargo fmt`로
  재포매팅된 코드와 달라져 assert로 중단, 그 회차의 변경이 통째로 미적용.
- **해결**: 현재 파일 상태를 다시 읽어 앵커를 갱신한 뒤 재적용. 스크립트는
  assert로 중단되게 되어 있어(부분 적용 없음) 어긋난 채 섞이지는 않았다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` — 전 스위트 통과. **native 스위트 31건이 이번부터 실제
  실행**된다: `../typescript-go` 체크아웃(main, 7.1.0-dev)을
  `go build -o built/local/tsgo ./cmd/tsgo`와 `npm ci && npx tsc -b
  _packages/native-preview`로 직접 빌드해 연동했다(해석 순서 3 —
  `src/typescript/native.rs`; npm 배포본에는 rlc가 쓰는 API server가 없어
  직접 빌드가 요구 사항이다).

## 결과

이름 해석·subject 식별·제안 라이선스의 구현이 `src/resolve` 하나가 됐다
(TASK-122의 부채 해소, D5 해소). 후속: Phase 3 2/2 — coverage/usefulness의
resolved identity 소비(TASK-124 예정), Phase 4 — typed facts.
