# TASK-057: 타입 에러를 `.rl` 원본 위치로 — `--types` 위치 매핑과 에디터 TS 진단

- **상태**: 완료
- **시작일**: 2026-08-18
- **완료일**: 2026-08-18
- **커밋**: `c030a44`

## 목적

`.rl` 안의 **타입 에러가 사용자에게 도달하지 않는다.** 사용자가
`const a = (v: number) => v |> Result.mapP((n) => n);`을 쓰면 `n`이 `unknown`이
되는데(head가 `Result`가 아니라 붙지 않으므로 `T`가 추론되지 않는다), 에디터는
아무 표시도 하지 않고 `rlc --types`는 디스크에 존재하지도 않는 파일의 좌표
(`t.ts:2:34`)를 가리켰다. 사용자가 볼 수 있는 것은 호버의 `unknown`뿐이라
원인이 head라는 것을 알 방법이 없었다.

두 경로 모두 타입 에러를 **원본 `.rl`의 행·열**로 보고하게 만든다.

## 범위

- 포함:
  - 공개 API `compile_mapped()` — `compile()`과 같은 파이프라인에 방출 매핑까지.
  - `rlc --types`: tsc 진단을 방출 좌표 → 원본 `.rl` 좌표로 옮겨 보고.
  - 언어 서버: TS 의미 진단 표출 (가상 문서 한정, 매핑되지 않는 스팬은 폐기),
    설정 `rl.typeDiagnostics` (기본 켜짐).
  - 세 계층 테스트 + 서버 테스트, 레퍼런스·AI 문서 갱신.
- 제외:
  - rl 자체 타입 추론 (설계상 금지 — 타입은 tsc 소관).
  - 구문 진단·제안(suggestion) 진단 표출.
  - `.rl` 파일 밖(`.ts`)의 진단 처리 변경 — 원래 실제 좌표를 가지므로 그대로.
  - 파이프라인 추론 자체의 변경. 위 예시는 rlc 버그가 아니라 정상적인 TS
    추론 실패이며, 고칠 대상은 **보고 경로**다.

## 의사결정

### 결정 1: 추론을 바꾸지 않고 보고 경로를 고친다

- **상황**: 신고된 증상은 "`|>` 안에서 `n`이 `unknown`". 먼저 이것이 컴파일러
  버그인지 판정해야 했다.
- **검토한 대안**:
  - (A) `$rl_ap` 방출 형태를 바꿔 추론을 개선한다.
  - (B) 추론은 그대로 두고, 이미 tsc가 내고 있는 에러를 사용자에게 보인다.
- **선택과 근거**: (B). 실측으로 판정했다 — 같은 파일에서 head 타입만 바꿔
  `tsc --strict`로 돌렸을 때 head가 `Result`인 모든 형태(선언된 반환 타입,
  생성자 호출, 2단 체인, `await` head, import한 `.rl` 모듈 경유)는 `n: number`로
  정확히 추론됐고, 실패하는 것은 head가 `number`인 경우(`TS2345`)와 head가
  암시적 `any`인 경우(`TS7006`)뿐이었다. 즉 추론은 설계대로 동작하고 있고
  tsc는 이미 정확한 에러를 내고 있었다 — 문제는 그 에러가 어디에도 보이지
  않는다는 것이다. (A)를 택하면 정상적인 타입 에러를 컴파일러가 숨기는
  방향이 되어 오히려 나쁘다.

### 결정 2: TS 진단 표출 금지 계약을 해제한다

- **상황**: TASK-024와 TASK-050이 "TS 진단은 표출하지 않는다"를 명시적 범위
  제외로 두고 있었다. 이번 작업은 그 결정을 뒤집는다.
- **검토한 대안**: (A) 계약 유지 — 에디터는 계속 rl 에러만. (B) 해제.
- **선택과 근거**: (B). TASK-024의 근거는 "**원문**을 TS 서비스에 먹이므로
  rl 구문에 대한 파스 에러가 새어 나온다"였다. TASK-050이 방출물 + 정확한
  바이트 매핑으로 서빙 방식을 바꾸면서 그 근거가 사라졌다 — 서비스가 보는
  것은 이제 rl 구문이 없는 순수 TypeScript다. CLAUDE.md의 에러 계층 계약은
  "**누가** 어떤 에러를 내는가"에 대한 것이지(rl 에러는 rlc, 타입 에러는
  tsc) "에디터가 tsc 에러를 보여주면 안 된다"가 아니다. 계층은 `source`
  필드로 구분해 유지한다: `rlc` / `ts`.

### 결정 3: 안전장치 두 개 — 가상 문서 한정 + 매핑 실패 시 폐기

- **상황**: 표출을 켜면 "잘못된 진단"이 새어 나올 두 경로가 생긴다.
  ① 컴파일러가 없거나 방출이 실패해 **원문**이 서빙 중일 때 — TS는 `match`를
  파싱하지 못하고 오류 복구가 지어낸 에러를 파일 전체에 뿌린다.
  ② 진단이 컴파일러 글루(switch IIFE, `$rl_ap`)에 걸릴 때.
- **검토한 대안**: 글루 진단은 (a) 폐기 / (b) 인접 원본 위치로 이동 /
  (c) 파일 첫 줄에 표시.
- **선택과 근거**:
  - ①은 `activeVirtual()`이 null이면 한 건도 내지 않는다. 덧붙여 방출물에
    **구문 진단이 하나라도 있으면** 그 파일의 의미 진단을 통째로 버린다 —
    구문이 깨진 방출물은 컴파일되지 않은 버퍼라는 뜻이고, 거기서 나온 타입
    에러는 전부 노이즈다.
  - ②는 (a) 폐기. 계약상 rlc 방출물 때문에 tsc 에러가 나면 그건 rlc 버그이지
    사용자에게 보고할 것이 아니다. (b)는 사용자가 고칠 수 없는 위치에 빨간
    줄을 긋는 셈이고, (c)는 원인 불명의 에러를 파일 머리에 붙인다.
  - CLI(`--types`)는 진단을 **버릴 수 없다**(종료 코드가 걸려 있다). 그래서
    거기서는 (b) — 뒤따르는 첫 원본 조각의 위치로 옮긴다.

### 결정 4: 위치 변환은 이미 있는 방출 매핑을 재사용한다

- **상황**: 방출 좌표 → 원본 좌표 변환이 필요하다.
- **검토한 대안**: (A) 방출물에 소스맵을 붙인다. (B) TASK-050의
  `EmitMapping`(원본↔출력 바이트 청크)을 그대로 쓴다.
- **선택과 근거**: (B). 에디터 쪽은 `MappedDoc`/`fromServiceSpan`이 이미
  그 변환을 하고 있어 새 코드가 거의 필요 없었다. CLI 쪽만 같은 매핑을
  받을 수 있으면 됐고, 그래서 `compile_mapped()`를 공개해 `--types`가
  `compile()` 대신 쓰게 했다. 소스맵은 방출물에 주석을 추가해야 해서
  "생성물은 순수 TypeScript" 성질을 흐린다.

### 결정 5: 좌표 단위 — TS는 UTF-16, rlc는 UTF-8 코드포인트

- **상황**: 호스트가 넘기는 열은 TypeScript의 것(UTF-16 코드 단위)이고,
  rlc의 `line_col`은 UTF-8 코드포인트로 센다. 그대로 쓰면 비ASCII 줄에서
  어긋난다.
- **선택과 근거**: 변환 지점을 하나로 고정했다 — `utf16_offset()`이 (행,
  UTF-16 열) → 방출물 바이트 오프셋을 계산하고, 그 뒤로는 전부 바이트로
  다루다가 마지막에 `rlc::line_col()`이 원본 좌표를 만든다. 서로게이트 쌍
  케이스는 단위 테스트로 고정했다 (`🎉`가 든 줄: UTF-16 열 17 = 바이트 18).

### 결정 6: 기본값은 켜짐, 끄는 스위치를 둔다

- **상황**: `rl.typeDiagnostics`의 기본값.
- **선택과 근거**: 켜짐. 이 태스크의 목적 자체가 "안 보이던 에러를 보이게"
  하는 것이고, 켜지 않으면 사용자가 존재를 모른다. 다만 대형 프로젝트에서
  의미 진단은 비용이 있고(디바운스 300 ms 안에서 돌지만) `.rl`에 타입 검사를
  아직 원하지 않는 단계가 있을 수 있어 끌 수 있게 뒀다.

## 작업 내역

- 2026-08-18: 증상 재현. `rlc`로 컴파일한 뒤 `tsc --strict --noEmit`으로
  head 타입을 바꿔가며 실측 — `(v: number)`는 `TS2345` + `n: unknown`,
  `(v: Result<number, string>)`·`ev()`·`Result.Ok(1)`·2단 체인·`await` head는
  전부 정상 추론. 추론이 아니라 **보고**가 문제임을 확인.
- 2026-08-18: `rlc --types`가 `t.ts:2:34`(존재하지 않는 파일)를 가리키는 것,
  언어 서버가 TS 진단을 한 건도 내지 않는 것(`server.ts`의 "TS diagnostics are
  never surfaced") 확인.
- 2026-08-18: `src/lib.rs` — `compile_mapped()` 추가, `compile()`은 그 위의
  얇은 래퍼로. `src/codegen/mod.rs`의 `emit()`은 `emit_with_map()`에 흡수
  (유일한 호출자가 사라짐).
- 2026-08-18: `src/main.rs` — `EmittedTypes.diagnostics`를 포맷된 문자열에서
  구조체 `TypeDiagnostic`으로. `RlOrigin`(가상 모듈 → `.rl` 파일·원문·방출물·
  매핑), `utf16_offset()`, `source_offset()`, `TypeDiagnostic::render()` 추가.
  `types_once()`가 `compile_mapped()`를 쓰고 origins 테이블을 만들어 진단
  출력 시 넘긴다.
- 2026-08-18: `editors/vscode/server/src/tsproject.ts` — `TsDiagnostic`과
  `diagnosticsFor()`(구문 진단 있으면 통째로 폐기 → 의미 진단만).
- 2026-08-18: `editors/vscode/server/src/server.ts` — `RlSettings.typeDiagnostics`,
  `validate()`에서 `refreshVirtual`의 프라미스를 잡아 두었다가 await 후
  `typeDiagnostics()`가 매핑해 rlc 진단에 합침. `editors/vscode/package.json`에
  설정 선언.
- 2026-08-18: 테스트 — `src/main.rs`에 `type_diagnostic_tests`(5개, 좌표 변환·
  렌더링), `tests/integration.rs`에
  `cli_types_reports_rl_type_errors_at_the_source_position`,
  `editors/vscode/server/src/test/emitmap.test.ts`에 4개(파이프라인 안 타입
  에러 / match 암 안 타입 에러 / 정상 코드는 무진단 / 원문 서빙은 무진단).
- 2026-08-18: 문서 — `docs/reference/errors.md`에 "타입 에러 (tsc)" 절,
  `docs/reference/cli.md` `--types` 절에 원본 위치 보고, `docs/reference/language.md`
  §7.3에 "문맥 추론은 head 타입에서 출발한다" + `unknown` 증상 설명,
  `editors/vscode/README.md`에 타입 진단 절과 설정, `docs/ai/rl.md` Errors에
  두 줄(타입 에러 보고 경로 / `|>` `unknown` 진단법).

## 이슈 및 해결

### 이슈 1: 단위 테스트가 UTF-16 열을 잘못 계산

- **증상**: `utf16_offset_counts_lines_and_utf16_columns`가
  `left: 15, right: 18`로 실패.
- **원인**: 테스트가 기댓값을 손으로 센 것이었고, `🎉`를 1 코드 단위로 셌다.
  서로게이트 쌍은 UTF-16에서 2단위다 — `const t`는 열 14가 아니라 17.
- **해결**: 기댓값을 17/바이트 18로 고치고, 바이트 위치도 함께 단언해 다음에
  같은 실수를 하면 어느 쪽이 틀렸는지 드러나게 했다. 구현은 처음부터 맞았다.

### 이슈 2: 통합 테스트의 기대 열이 실제와 2 어긋남

- **증상**: `rlc: src/bad.rl:5:55:`가 나오는데 테스트는 `5:53`을 기대.
- **원인**: 역시 손으로 센 열. tsc는 `n.length`의 **프로퍼티 이름**에
  진단을 붙이므로 `n.`을 건너뛴 위치가 맞다.
- **해결**: 55로 고정. 실제 출력이 더 정확한 위치였다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` (전 스위트 통과, 통합 테스트 포함)
- [x] `npm run compile && node --test "server/out/test/*.test.js"` (57 pass)

## 결과

| 파일 | 변경 |
|------|------|
| `src/lib.rs` | `compile_mapped()` 공개, `compile()`은 래퍼 |
| `src/codegen/mod.rs` | `emit()` 제거 (`emit_with_map()`로 통합) |
| `src/main.rs` | `TypeDiagnostic`·`RlOrigin`·`utf16_offset`·`source_offset`, `--types` 진단 위치 매핑, 단위 테스트 |
| `tests/integration.rs` | `--types`가 `.rl` 위치로 보고하는지 |
| `editors/vscode/server/src/tsproject.ts` | `diagnosticsFor()` |
| `editors/vscode/server/src/server.ts` | 타입 진단 병합, `rl.typeDiagnostics` |
| `editors/vscode/package.json` | 설정 선언 |
| `editors/vscode/server/src/test/emitmap.test.ts` | 진단 테스트 4개 |
| `docs/reference/{errors,cli,language}.md`, `docs/ai/rl.md`, `editors/vscode/README.md` | 문서 |

후속 여지: 진단에 대한 빠른 수정(quick fix)은 TS 언어 서비스의
`getCodeFixesAtPosition`을 매핑해 붙일 수 있다 — 이번 범위에는 넣지 않았다.
