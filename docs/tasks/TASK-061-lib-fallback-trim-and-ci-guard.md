# TASK-061: 검증 안 된 lib 폴백 제거와 패키징 CI 게이트

- **상태**: 완료
- **시작일**: 2026-08-18
- **완료일**: 2026-08-18
- **커밋**: —

## 목적

TASK-059에서 `TS2488` 오탐의 트리거가 VSIX 패키징으로 확정됐다. 그러자
TASK-058이 넣은 방어 코드 중 일부는 **근거가 사라졌고**, 정작 진짜 원인은
CI가 보지 않는다는 것이 드러났다. 방어의 무게를 실제 위험에 맞게 재배치한다:
검증되지 않은 폴백은 빼고, 원인을 직접 막는 게이트를 넣는다.

## 범위

- 포함:
  - `findDefaultLib()`에서 검증 안 된 후보 두 갈래 제거.
  - CI에 확장 잡 추가 — 빌드·테스트 + 패키징된 vsix의 lib 개수 게이트.
- 제외:
  - `typeEnvironmentError()` 가드와 회귀 테스트 — 유지한다(결정 2).
  - `.vscodeignore`·README (TASK-059에서 끝났다).

## 의사결정

### 결정 1: lib 후보를 하나로 줄인다

- **상황**: TASK-058의 `findDefaultLib()`은 후보를 셋 두었다.
  트리거가 확정된 지금 각각의 근거를 다시 따졌다.
- **측정**: 후보 1(`ts.getDefaultLibFilePath()`)과 후보
  2(`require.resolve("typescript")`의 디렉터리)를 실제 설치에서 비교 —
  **문자열까지 동일**했다:
  ```
  candidate1: .../server/node_modules/typescript/lib/lib.esnext.full.d.ts
  candidate2: .../server/node_modules/typescript/lib/lib.esnext.full.d.ts
  identical: true
  ```
- **검토한 대안**: (A) 셋 다 둔다 — 방어가 두껍다. (B) 후보 1 + 존재 확인만.
- **선택과 근거**: (B).
  - 후보 2는 **죽은 코드**다. 둘이 갈리는 것은 서버가 번들링됐을 때뿐인데,
    이 확장은 `tsc -b`로 빌드하고 번들러를 쓰지 않는다. 번들링을 도입하면
    그때 근거와 함께 넣으면 된다.
  - 후보 3(워크스페이스에서 위로 올라가며 `node_modules/typescript/lib`)은
    **검증되지 않은 데다 위험하다.** 확장이 로드한 checker(5.9)에 프로젝트가
    가진 다른 버전의 lib을 조용히 물릴 수 있고, 그 조합이 어떤 진단을
    만드는지 확인한 적이 없다. 이번 버그가 정확히 "확인 안 한 조합이 만든
    이해할 수 없는 진단"이었다 — 같은 씨앗을 방어라는 이름으로 심지 않는다.
  - 남는 것은 **경로 검증**이며, 그것이 원래 빠져 있던 유일한 검사다.

### 결정 2: 가드와 회귀 테스트는 유지한다

- **상황**: 패키징이 고쳐졌으니 `typeEnvironmentError()` 가드는 정상 설치에서
  절대 걸리지 않는다. 지울 것인가.
- **검토한 대안**: (A) 지운다 — 원인이 사라졌으니 죽은 코드다. (B) 남긴다.
- **선택과 근거**: (B). 후보 2·3과 결정적으로 다른 점은 **동작이 검증됐다는
  것**이다(TASK-058/059에서 lib 없는 트리로 재현). 그리고 막는 대상이
  "원인 하나"가 아니라 **실패 양식**이다 — 구버전 vsix 설치, 부분 설치,
  향후 패키징 회귀 등 lib이 사라지는 경로는 여럿이고, 그때 나오는 것은
  사용자 코드에 붙는 거짓 에러다. 약 30줄로 거짓 양성을 거짓 음성 + 경고로
  바꾼다. 비용 대비 값이 분명하다.

### 결정 3: 진짜 원인을 CI로 막는다

- **상황**: `.vscodeignore`가 다시 깨져도 아무 테스트도 실패하지 않는다.
  단위 테스트는 소스를 보지 패키지를 보지 않는다. 가드가 있어도 결과는
  "거짓 에러" 대신 "모든 타입 진단 실종"일 뿐, 여전히 고장이다.
- **검토한 대안**:
  - (A) 문서에만 남긴다(TASK-059의 README 확인 명령).
  - (B) CI에서 `vsce ls`의 lib 개수를 센다.
  - (C) 실제 vsix를 만들어 압축을 풀고 E2E 진단까지 돌린다.
- **선택과 근거**: (B). (A)는 사람이 잊으면 끝이다. (C)는 TASK-059에서 이미
  한 번 했고 재발 방지에는 과하다 — 패키징 목록만 봐도 회귀는 100% 잡힌다.
  임계값은 정확한 개수(100)가 아니라 `>= 90`으로 뒀다: TypeScript 버전이
  올라가며 lib 파일 수는 자연히 변하지만, 회귀는 0으로 떨어지는 형태라
  느슨한 하한으로도 확실히 걸린다. 버전 업마다 CI가 깨지는 것은 게이트가
  아니라 소음이다.
- 확장 잡은 별도 job으로 뒀다. `check` 잡에 얹으면 Rust 게이트와 확장
  게이트가 한 덩어리로 실패해 원인 구분이 흐려진다. 대신 서버 테스트가
  `rlc`를 PATH에서 찾지 못하면 조용히 skip되므로, 잡 안에서 `cargo build`로
  바이너리를 먼저 만든다 — 스킵된 테스트를 통과로 세지 않기 위해서다.

## 작업 내역

- 2026-08-18: 후보 1과 2의 동일성 측정(위 결정 1). 동일 확인.
- 2026-08-18: `editors/vscode/server/src/tsproject.ts` — `findDefaultLib()`을
  인자 없는 함수로 축소(후보 1 + `fs.existsSync`), 주석을 TASK-059가 확정한
  실제 트리거로 갱신. 생성자의 `rootDir` 전달 제거.
- 2026-08-18: `.github/workflows/ci.yml` — `extension` job 추가.
  `cargo build` → `npm ci` → `npx tsc -b` → `node --test` →
  `npx @vscode/vsce@3 ls --no-dependencies | grep -c "typescript/lib/lib"`가
  90 이상인지 확인.
- 2026-08-18: 검증. 확장 의존성을 전부 지우고(`rm -rf node_modules
  client/node_modules server/node_modules`) CI와 같은 순서로 재현 →
  `npm ci` 성공, 빌드 성공, **59/59 통과(skip 0)**.
  게이트는 양방향으로 확인했다:
  - 현재 상태 → `count=100`, 통과.
  - `.vscodeignore`를 TASK-058 이전으로 되돌림 → `count=0`, **실패**.

## 이슈 및 해결

### 이슈 1: 주석 안의 glob이 블록 주석을 조기 종료시킴

- **증상**: `findDefaultLib`의 doc 주석에 `.vscodeignore`의 패턴을 그대로
  적었더니 `tsc -b`가 `TS1005: ',' expected` 등 5건으로 실패.
- **원인**: 그 패턴에 들어 있는 `*` + `/` 조합이 블록 주석의 종료 토큰이라
  주석이 중간에서 끝나고 뒤가 코드로 파싱됐다.
- **해결**: 패턴을 그대로 쓰지 않고 "blanket `.ts` rule"로 풀어 썼다.
  재빌드 후 59/59 통과.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test`
- [x] 클린 상태에서 `npm ci` + `npx tsc -b` + `node --test` → 59/59 (skip 0)
- [x] 패키징 게이트 — 현재 100 통과 / 회귀 상태 0 실패

## 결과

| 파일 | 변경 |
|------|------|
| `editors/vscode/server/src/tsproject.ts` | `findDefaultLib()` 후보 3 → 1, 주석을 확정된 트리거로 갱신 |
| `.github/workflows/ci.yml` | `extension` job — 확장 빌드·테스트 + vsix lib 개수 게이트 |

TASK-058이 넣은 코드 중 남은 것: 경로 존재 확인, `typeEnvironmentError()`
가드, 회귀 테스트 2건. 빠진 것: 검증되지 않은 lib 후보 2갈래.
