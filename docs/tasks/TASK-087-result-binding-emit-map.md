# TASK-087: result 바인딩 선언 위치 emit-map

- **상태**: 완료
- **시작일**: 2026-08-19
- **완료일**: 2026-08-19
- **커밋**: `8231619`

## 목적

에디터 가상 문서에서 `result { const id <- ...; }`로 만든 바인딩 선언 위치에 바로
hover/type query가 동작하게 한다. 현재는 바인딩을 사용하는 위치는 매핑되지만, `<-`
선언 위치는 compiler glue로만 방출되어 source→output mapping이 없다.

## 범위

- 포함: `ResultBind`가 바인딩 원본 span을 보존하게 한다.
- 포함: codegen이 emitted `const <binding> = tmp.value`의 `<binding>` 부분을 source
  chunk로 방출해 emit-map에 포함한다.
- 포함: Rust emit-map 테스트와 editor 가상 문서 테스트를 추가한다.
- 제외: `result` 블록의 runtime/type semantics 변경.

## 의사결정

### 결정 1: 바인딩 전체를 verbatim source chunk로 매핑한다

- **상황**: 단순 identifier뿐 아니라 `{ id, name }: Type`, `[count, label]` 같은 binding도
  `<-` 왼쪽에서 TypeScript 선언의 binding 부분으로 그대로 이동한다.
- **검토한 대안**: identifier만 별도로 매핑하면 흔한 hover 문제는 해결되지만
  destructuring/annotation에서 같은 문제가 남는다. 바인딩 전체 span을 보존하면 emitted
  declaration의 binding text와 source text가 동일한 전 범위를 매핑할 수 있다.
- **선택과 근거**: parser가 keyword와 `<-` 사이의 trimmed binding span을 AST에 보존하고,
  codegen이 그 span을 `push_src`로 방출한다. emit-map invariant가 “source와 output
  텍스트가 동일한 chunk만 매핑”이므로 이 방식이 가장 직접적이다.

## 작업 내역

- 2026-08-19: TASK-087을 등록했다.
- 2026-08-19: `ResultBind`에 `binding_span`을 추가하고 기존 중복 `binding: String`은
  제거했다.
- 2026-08-19: `parser/results.rs`가 `<-` 왼쪽 binding의 trimmed source span을 계산해
  AST에 넣게 했다.
- 2026-08-19: `codegen/mod.rs`의 `emit_result_block`이 binding text를 source chunk로
  방출하게 했다.
- 2026-08-19: `tests/emit_map.rs`에 result block binding declaration mapping 회귀 테스트를
  추가했다.
- 2026-08-19: `editors/vscode/server/src/test/emitmap.test.ts`에 result binding 선언 위치
  quick-info 테스트를 추가했다.

## 이슈 및 해결

- **증상**: `ResultBind`를 `binding_span` 기반으로 바꾸는 도중 기존 `binding:
  String` 필드가 남아 있으면 컴파일은 되더라도 사용되지 않는 필드가 되어 clippy
  게이트에서 부채가 된다.
- **원인**: codegen이 더 이상 owned string을 필요로 하지 않고 source span만으로
  binding text를 다시 읽을 수 있다.
- **해결**: AST에서 `binding` 필드를 제거하고 parser가 `binding_start` /
  `binding_end`만 전달하게 정리했다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`
- [x] `PATH=/Users/seominyeong/orca/workspaces/rl/tsgo-frontend-review/target/debug:$PATH npm test` (`editors/vscode`)

## 결과

`result` 블록의 `<-` 왼쪽 binding 원문 범위가 emitted TypeScript 선언의 binding
위치로 정확히 매핑된다. 따라서 에디터가 `const id <- ...`의 `id` 선언 위치에서
바로 quick info/type query를 수행할 수 있고, destructuring 및 type annotation이
있는 binding도 같은 경로로 매핑된다. runtime semantics, 타입 의미, 문법 표면은
변경하지 않았으므로 레퍼런스와 AI 사용 문서 갱신은 필요하지 않다.
