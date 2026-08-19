# TASK-078: `result` 바인딩 선언 위치 emit-map

- **상태**: 완료
- **시작일**: 2026-08-19
- **완료일**: 2026-08-19
- **커밋**: —

## 목적

`result` 계산 블록의 `<-` 바인딩이 emit-map에 실리지 않아, 에디터가 방출된
선언(`const x = $rl_r0.value;`)에서 `.rl` 원문의 선언 위치로 되짚지 못한다.
바인딩 텍스트를 AST에 문자열로 복사해 두고 codegen이 리터럴로 방출하기
때문이다. 스팬으로 바꾸면 그대로 매핑 구간이 된다.

## 범위

- 포함: `ResultBind`가 바인딩 텍스트 대신 스팬을 들고, codegen이 `push_src`로
  방출한다. 방출 바이트는 그대로 유지한다.
- 제외: `try` 문의 `decl`(같은 형태의 문자열 복사)은 건드리지 않는다 —
  별개 구문이고 별도 태스크로 다룬다. `result` 블록의 의미·에러도 그대로다.

## 의사결정

### 결정 1: 문자열 복사(`String`)를 스팬(`Span`)으로 바꾼다

- **상황**: `ResultBind.binding: String`은 파서가 원문에서 잘라 복사한
  텍스트다. codegen은 이것을 `push_lit`으로 방출하는데, `push_lit`은 원본
  위치를 모르므로 emit-map에 구간이 생기지 않는다. 에디터의
  `fromServiceSpan`이 이 선언을 짚으면 매핑이 없어 진단·정의 이동이 컴파일러
  글루로 취급돼 버려진다.
- **검토한 대안**:
  - **대안 A — codegen에서 `binding` 문자열을 원문에서 다시 찾는다.**
    파서가 이미 알고 있던 위치를 codegen이 문자열 검색으로 복원하는 것이라
    같은 텍스트가 여러 번 나오면 틀린다. 기각.
  - **대안 B — `binding: String`을 유지하고 스팬 필드를 추가한다.** 두
    표현이 공존해 어긋날 수 있고, `String`은 아무도 쓰지 않게 된다. 기각.
  - **대안 C — `binding_span: Span`으로 대체한다.** 파서는 이미
    `scan_bind`에서 바이트 범위를 계산하고 있으므로 잘라내지 않고 범위만
    넘기면 된다. codegen은 `src_slice` + `push_src`로 방출한다 — 다른 모든
    verbatim 구간이 쓰는 것과 같은 경로다.
- **선택과 근거**: 대안 C. AST가 원문 좌표를 들고 다니는 것이 이 저장소의
  기존 규약이고(`Segment::Verbatim(Span)`, `EnumDecl`, `MatchExpr` 모두
  스팬), 방출 경로도 이미 존재한다. 확인: 기존 스냅샷 테스트가 전부
  통과하면 방출 바이트가 변하지 않았다는 뜻이다 — `cargo test`로 확인.

### 결정 2: `scan_bind`의 트림을 스팬 산술로 바꾼다

- **상황**: 기존 코드는 `src[kw_end..lt_start].trim()`으로 텍스트를 얻었다.
  스팬을 만들려면 `trim`이 앞뒤에서 몇 바이트를 떼었는지 알아야 한다.
- **검토한 대안**: 앞뒤를 각각 `trim_start`/`trim_end`로 두 번 재는 방식과,
  `trim_start`로 시작을 정한 뒤 `trim()`의 길이를 그대로 끝으로 쓰는 방식.
- **선택과 근거**: 후자. `trim() == trim_start().trim_end()`이므로
  시작 오프셋 + `trim()` 길이가 곧 끝 오프셋이고, 슬라이싱이 한 번 줄어
  읽기도 낫다.

## 작업 내역

- 2026-08-19: `origin/main`(952a438) 기준으로 시작. `claude/unpack-file-...`
  브랜치의 TASK-087 변경을 이식 대상으로 확인했다 — main에는
  `binding_span`이 없다(`git grep binding_span origin/main -- src/` 빈 결과).
- 2026-08-19: `src/ast.rs`의 `ResultBind.binding: String`을
  `binding_span: Span`으로 바꿨다.
- 2026-08-19: `src/parser/results.rs`의 `BindRun::Bind`가 `binding: &str`
  대신 `binding_start`/`binding_end`를 나르도록 바꾸고, `scan_bind`에서
  트림된 범위를 계산하도록 했다. 판정에 쓰이는 `binding` 지역 변수는
  그대로 유지해 빈 바인딩 검사가 달라지지 않게 했다.
- 2026-08-19: `src/codegen/mod.rs`의 `emit_result_block`이 키워드와 `=` 는
  리터럴로, 바인딩은 `src_slice` + `push_src`로 방출하도록 바꿨다.
- 2026-08-19: `tests/emit_map.rs`에
  `result_block_bindings_are_mapped_to_emitted_declarations`를 추가했다.
  평범한 이름과 타입 주석이 붙은 구조 분해 패턴 둘 다 매핑되는지 확인한다.
- 2026-08-19: `docs/reference/cli.md` §방출 매핑의 "매핑되는 조각" 목록에
  `result` 블록의 `<-` 바인딩을 추가했다 — 이 목록은 규범이므로 구현과
  어긋나면 버그다.
- 2026-08-19: `cargo fmt --check` / `cargo clippy --all-targets -- -D warnings`
  / `cargo test`(428 통과, 실패 0) 전부 통과.

## 이슈 및 해결

없음.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` — 428 통과, 실패 0 (통합 테스트 포함)

## 결과

`ResultBind`가 바인딩 텍스트 대신 스팬을 들고, codegen이 그 구간을 원본에서
복사해 방출한다. 방출 바이트는 변하지 않았고(기존 스냅샷 테스트 전부 통과),
`--emit-map`에 `result` 바인딩 선언 구간이 새로 실린다. 에디터는 이 구간을
통해 `const x = $rl_r0.value;`의 `x`를 `.rl` 원문의 `const x <- ...`로 되짚을
수 있다.

변경 파일:

- `src/ast.rs` — `ResultBind.binding: String` → `binding_span: Span`
- `src/parser/results.rs` — `BindRun::Bind`가 스팬을 나름, `scan_bind`의
  트림을 스팬 산술로
- `src/codegen/mod.rs` — 바인딩을 `src_slice` + `push_src`로 방출
- `tests/emit_map.rs` — 계약 테스트 1건 추가
- `docs/reference/cli.md` — `--emit-map` 매핑 목록 갱신

`try` 문의 `decl` 역시 같은 형태의 문자열 복사라 같은 매핑이 빠져 있다. 이번
범위 밖이며 후속 태스크로 다룬다.

상태를 `완료`로 바꾼다.
