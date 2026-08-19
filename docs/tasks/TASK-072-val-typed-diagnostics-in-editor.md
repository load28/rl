# TASK-072: 에디터에 타입 기반 `val` 진단 노출

- **상태**: 완료
- **시작일**: 2026-08-19
- **완료일**: 2026-08-19
- **커밋**: —

## 목적

TASK-071로 `val` 경로의 built-in 변경 메서드 판정이 타입 기반으로 옮겨졌고,
TASK-073~077로 그 판정은 `rlc --check-types`가 TypeScript 백엔드에 물어
답한다. 에디터(LSP)는 rl 진단을 `rlc --check`로만 받으므로
(`editors/vscode/server/src/rlc.ts`), `map.set("a", 1)` 같은 확실한 built-in
변경이 편집 중에는 **전혀 표시되지 않는다**. 확인:

```sh
rlc --check       src/a.rl   # 아무것도 보고하지 않음 (exit 0)
rlc --check-types src/a.rl   # cannot call mutating method `set` … (exit 1)
```

## 범위

- 포함: `rlc --check-types`가 **저장되지 않은 버퍼**를 프로젝트의 일부로 검사할
  수 있게 하고(`--overlay`), rl 수준 진단만 낼 수 있게 하며(`--rl-only`),
  에디터가 그 출력을 그대로 중계한다.
- 제외: 판정 규칙 자체의 변경 (규범은 `language.md` §10.4). 진단 메시지 문안도
  그대로다 — 에디터는 rlc가 쓴 문장을 옮기기만 한다.
- 제외: 에디터가 TypeScript 타입 에러를 이 경로로 받는 것. 그것은 이미 살아
  있는 tsgo 언어 서버가 가상 문서 위에서 답하고 있고, 두 경로가 같은 에러를
  내면 스퀴글이 겹친다.

## 의사결정

### 결정 1: 판정을 에디터로 옮기지 않고, 컴파일러에게 버퍼를 묻는다

- **상황**: 판정에 필요한 두 사실은 ① 접근 경로의 뿌리가 어느 바인딩인지
  (심볼 동일성) ② 그 메서드가 TypeScript 자신이 선언한 것인지다
  (`src/typescript/check.rs`의 `Resolution.id` / `Resolution.builtin`).
  에디터는 tsgo **언어 서버**(LSP)를 들고 있는데, LSP에는 "심볼 id"를 돌려주는
  요청이 없다. 그래서 이 판정을 에디터에서 그대로 재현할 수는 없다.
- **검토한 대안**:
  - **대안 A — LSP `textDocument/definition`으로 대신한다.** 두 식별자가 같은
    정의 위치로 가면 같은 바인딩이고, 메서드의 정의가 `lib.*.d.ts`면 built-in
    이다. 전송만 다를 뿐 같은 두 사실이므로 원리적으로는 가능하다.
    단점: 짝짓기 로직(약 60줄)과 진단 문안이 TypeScript 쪽에 **복제**된다.
    `claude/unpack-file-new-branch-push-4xjnys` 브랜치가 택했던 길이고, 거기서는
    정규식으로 후보를 모으는 형태(`valdiag.ts`)까지 갔다. CLAUDE.md의
    "rl 수준 에러는 전부 rlc가 `파일:행:열`과 함께 **직접** 보고한다"에
    정면으로 어긋난다. 기각.
  - **대안 B — 저장된 파일을 검사한다.** `--overlay` 없이 `--check-types`만
    돌린다. 구현이 가장 싸지만, 편집 중 버퍼와 디스크가 다르면 위치가 어긋난
    진단을 표시하게 된다. 잘못된 위치의 `val` 에러는 진단이 없는 것보다 나쁘다.
    기각.
  - **대안 C — 버퍼를 오버레이로 얹어 `--check-types`를 돌리고, 나온 문장을
    그대로 중계한다.** 판정도 문안도 컴파일러 것 하나뿐이다.
- **선택과 근거**: 대안 C. 에러 계층 계약을 지키는 유일한 대안이고, 에디터
  코드에는 rl 의미가 한 줄도 들어가지 않는다. 확인: `tests/cli.rs`의
  `overlay_keeps_the_buffer_in_its_project`가 다른 모듈에서 온 `Map`은
  보고하고 같은 이름의 사용자 정의 `Query#set`은 보고하지 않는 것을 확인한다 —
  판정이 정말 체커의 것이라는 증거다.

### 결정 2: 오버레이는 임시 파일이 아니라 **원래 경로의 내용 교체**다

- **상황**: `rlc --check`는 이미 버퍼를 temp 파일에 쓰고 그 파일을 검사한다
  (`rlc.ts`의 `runCheck`). 같은 방식을 쓸 수 있는지 검토했다.
- **검토한 대안**:
  - **대안 A — temp 파일에 쓰고 그것을 검사한다.** `--check`에서는 파일 하나만
    보면 되므로 통한다. 그러나 `--check-types`는 **프로젝트 그래프**를 연다.
    temp 디렉터리의 파일은 프로젝트 밖이라 `./store`도 `@rl/std`도 해석되지
    않고, 그 파일을 import하는 다른 모듈도 여전히 디스크의 옛 텍스트를 본다.
  - **대안 B — 경로는 그대로 두고 내용만 바꾼다.** `project::lower`가 디스크를
    읽는 자리에 오버레이 맵을 끼워 넣는다. 모듈의 이름·위치가 그대로이므로
    양방향 해석이 전부 디스크에서와 같다.
- **선택과 근거**: 대안 B. 언어 서버가 `textDocument/didChange`로 문서를
  대신하는 것과 같은 모델이고, 프로젝트 그래프를 건드리지 않는다. 그 대신
  경로가 실재해야 하므로(저장된 적 없는 버퍼는 프로젝트에 자리가 없다)
  `--overlay`는 존재하지 않는 경로를 에러로 거절하고, 에디터는 그 경우
  프로세스를 띄우지도 않는다.

### 결정 3: 오버레이 텍스트는 stdin으로 받는다

- **상황**: 버퍼 텍스트를 rlc에 전달할 통로가 필요했다.
- **검토한 대안**: ① 인자로 넘긴다 — 큰 파일에서 인자 길이 한계에 걸리고
  셸 이스케이프 문제가 생긴다. ② 별도 파일에 써서 경로를 넘긴다 — 에디터가
  또 temp 파일을 관리해야 하고, 매 키 입력마다 디스크를 친다.
  ③ stdin — rlc의 `--check-types`는 stdin을 쓰지 않으므로 비어 있다.
- **선택과 근거**: ③. `--overlay <path>`가 "이 경로의 내용이 stdin에 온다"는
  뜻이므로 옵션 하나로 끝난다. 파일이 여럿인 오버레이는 지금 필요가 없어
  넣지 않았지만(내부 배관은 맵이라 확장은 열려 있다), CLI가 stdin을 한 번만
  읽으므로 CLI 표면은 한 파일로 제한된다.

### 결정 4: `--rl-only`로 타입 계층을 끈다

- **상황**: `--check-types`는 rl 계층과 타입 계층을 **둘 다** 보고한다.
  에디터는 타입 에러를 이미 살아 있는 tsgo 언어 서버에서 받고 있으므로
  (`rl.typeDiagnostics`, 가상 문서 위), 그대로 쓰면 모든 타입 에러가 두 번
  그려진다.
- **검토한 대안**: ① 에디터가 `ts(` 로 시작하는 메시지를 걸러낸다 —
  메시지 문자열에 의존하는 필터라 문안이 바뀌면 조용히 깨진다.
  ② 컴파일러가 계층을 선택해서 낸다.
- **선택과 근거**: ②. 두 계층이 분리돼 있다는 것은 이 저장소의 설계 계약이고
  (CLAUDE.md 절대 불변 원칙 2), 계약이 있는 축이라면 그 축으로 자르는 옵션이
  옳다. `reported` 집계에서도 빠지므로 종료 코드가 "rl 계층이 깨끗한가"를
  답한다.

### 결정 5: 두 계층을 **위치 기준**으로 합친다

- **상황**: 두 패스가 겹친다. enum 소진성은 `--check`가 선언 표에서, 
  `--check-types`가 타입에서 답하는데 **같은 위치**에 보고한다. 문안은 조금
  다르다(`match on enum Shape is not exhaustive` vs `match is not exhaustive`).
  측정: 같은 소스에 두 모드를 돌려 둘 다 `5:21`에 보고하는 것을 확인했다.
- **검토한 대안**: ① 메시지가 같을 때만 dedupe — 문안이 달라 걸리지 않는다.
  ② 위치가 같으면 하나만 남긴다. ③ 에디터가 `val` 문구만 골라 받는다 —
  에디터에 rl 지식이 다시 들어온다.
- **선택과 근거**: ②. 한 위치에 스퀴글 하나. 먼저 도착한 쪽(빠른 `--check`,
  사용자가 이미 읽고 있는 문장)을 남기므로 진단이 깜빡이며 문안이 바뀌지도
  않는다.

### 결정 6: 별도의 긴 디바운스 + 버전 일치 시에만 표시

- **상황**: 타입 검사는 프로젝트를 열고 TypeScript 컴파일러를 띄운다. 측정:
  작은 프로젝트에서 1회 약 630~690 ms (debug 빌드, 3회 측정). 300 ms짜리
  기존 검증 디바운스에 태울 수 없다.
- **검토한 대안**: ① 기존 `validate()` 안에서 같이 기다린다 — 모든 진단이
  0.7초씩 늦어진다. ② 저장할 때만 돌린다 — 편집 중에 보이지 않는다는 원래
  문제가 절반 남는다. ③ 별도 디바운스로 뒤따라 돌고, 끝나면 다시 게시한다.
- **선택과 근거**: ③, 1.2초. 텍스트 진단은 지금처럼 빠르게 나오고 타입 계층만
  뒤따른다. 대신 **버전이 맞을 때만** 표시한다 — 검사 중에 버퍼가 바뀌었으면
  그 결과의 위치는 이미 없는 텍스트를 가리키므로 버린다. 타이핑 중 타입 계층이
  잠시 사라졌다 돌아오는 것은 위치가 틀린 진단을 남기는 것보다 낫다.

### 결정 7: 답할 수 없으면 "깨끗함"이 아니라 "모름"으로 처리한다

- **상황**: TypeScript가 설치되지 않은 프로젝트, 저장된 적 없는 버퍼, rl 수준
  에러로 낮출 것이 없어 검사가 시작조차 못한 경우(종료 코드 2)가 모두 "진단
  0건"으로 돌아온다.
- **선택과 근거**: `runTypedCheck`가 `{kind:"unavailable"}`을 따로 돌려주고,
  서버는 그때 **기존 진단을 그대로 둔다**. 0건으로 처리하면 검사되지 않은
  파일이 검사되어 깨끗한 파일과 구분되지 않는다. 이유는 출력 채널에 한 번만
  적는다 — TypeScript 없는 프로젝트는 정상 상태이지 사용자에게 들이밀 에러가
  아니다.

## 작업 내역

- 2026-08-19: 현상 확인. TypeScript 7.1(`typescript@next`)을 설치한 작은
  프로젝트에서 `rlc --check`는 `scores.set(...)`에 아무 말도 하지 않고
  (exit 0), `rlc --check-types`는 두 건을 보고했다(exit 1). 사용자 정의
  `Box#set`은 어느 쪽도 보고하지 않는 것까지 확인.
- 2026-08-19: 비용 측정. `rlc --check-types` 1회 630/626/681 ms (debug 빌드).
  디바운스를 분리해야 한다는 결정 6의 근거.
- 2026-08-19: `src/typescript/project.rs` — `lower(files)`에 오버레이 맵
  (`&HashMap<PathBuf, String>`) 인자를 추가했다. 디스크를 읽는 자리에서
  canonical 경로로 조회한다.
- 2026-08-19: `src/typescript/check.rs` — `run()`의 인자를 `CheckOptions`
  구조체로 묶고(`project`/`node`/`out_dir`/`emit`/`watch`/`overlay`/`rl_only`),
  `Pass`에 `overlay`·`rl_only`를 실었다. `rl_only`면 타입 진단 루프에 빈
  슬라이스를 넘겨 보고도 `reported` 집계도 건너뛴다.
- 2026-08-19: `src/main.rs` — `--overlay <path>`(stdin에서 텍스트를 읽고
  경로를 canonicalize)와 `--rl-only`를 파싱하고, 세 가지 조합을 거절한다:
  `--check-types` 없이 쓸 때, `--types`와 함께 쓸 때, `--overlay`와 `--watch`를
  함께 쓸 때.
- 2026-08-19: `editors/vscode/server/src/rlc.ts` — `runTypedCheck()` 추가.
  파일의 디렉터리를 cwd로 잡아 컴파일러가 경로를 파일명만으로 출력하게 하고
  (`shown()`이 cwd 기준 상대 경로를 쓴다), 버퍼를 stdin으로 넘긴 뒤 기존
  `parseStderr`로 파싱한다. 디스크에 없는 경로는 프로세스를 띄우지 않고
  `unavailable`.
- 2026-08-19: `editors/vscode/server/src/server.ts` — `rl.typedChecks` 설정,
  1.2초 디바운스(`scheduleTypedCheck`), 계층별 캐시(`baseDiagnostics` /
  `typedDiagnostics`, 둘 다 문서 버전 기록), 위치 기준 병합(`mergeTyped`),
  게시 함수(`publish`), 닫힘·실패 시 캐시 정리(`forget`).
- 2026-08-19: 테스트. `tests/cli.rs`에 7건 — 플래그 조합 거절 4건과 동작 3건
  (버퍼가 검사된다 / `--rl-only`가 타입 계층만 뺀다 / 오버레이가 버퍼를 자기
  프로젝트에 둔다). `editors/vscode/server/src/test/typedcheck.test.ts`에 3건.
- 2026-08-19: 문서. `docs/reference/cli.md`(옵션 표 + "에디터의 타입 검사" 절),
  `docs/reference/errors.md`(CLI 에러 6줄), `editors/vscode/README.md`(기능 표·
  설정 표 + "타입 기반 rl 진단" 절), `editors/vscode/package.json`
  (`rl.typedChecks` 기여), `docs/ai/rl.md`(val 절과 에디터 줄),
  `CHANGELOG.md`.

## 이슈 및 해결

### 이슈 1: clippy `too_many_arguments` — `check::run`이 8개 인자가 됐다

- **증상**: `cargo clippy --all-targets -- -D warnings` 실패.
  `warning: this function has too many arguments (8/7) --> src/typescript/check.rs:28:1`.
- **원인**: `overlay`와 `rl_only`를 기존 6개 인자 뒤에 그냥 붙였다.
- **해결**: `#[allow]`로 덮지 않고 `CheckOptions` 구조체로 묶었다. 같은
  저장소의 `BuildOptions`(`src/main.rs`)와 같은 형태라 관례에도 맞고, 각
  필드에 어느 CLI 옵션에서 왔는지 문서를 붙일 자리도 생겼다.

### 이슈 2: 소진성 진단이 두 번 그려진다

- **증상**: 같은 파일에 `--check`와 `--check-types --rl-only`를 돌리면 둘 다
  `src/main.rl:5:21`에 소진성을 보고한다. 문안만 다르다
  (`match on enum Shape is not exhaustive` vs `match is not exhaustive`).
- **원인**: 설계상 그렇다. `--check`는 자기 선언 표에서, `--check-types`는
  타입에서 답하며 둘 다 정당하다(`cli.md` §타입 검사에 이미 명시돼 있다).
- **해결**: 결정 5 — 위치 기준 병합. 이미 진단이 있는 (행, 열)에는 타입 계층의
  진단을 얹지 않는다.

### 이슈 3: 에디터 테스트 4건이 실패한다 (이번 변경과 무관)

- **증상**: `node --test server/out/test/*.test.js`에서 `server.test.ts`의
  완성(completion) 테스트 4건 실패 — `Result.`가 빈 목록을 돌려주는 등.
- **원인**: 이 환경에는 tsgo **실행 파일**이 없고 API 모듈만 있다. 완성은
  `tsgo --lsp`가 답하므로 조용히 빈 결과가 된다.
- **해결**: 이번 변경 탓이 아님을 확인했다 — `origin/main`(952a438)을 워크트리로
  꺼내 같은 명령을 돌려 같은 4건이 같은 이유로 실패하는 것을 확인했다.
  이번 태스크에서는 손대지 않는다.

## 검증

- [x] `cargo fmt --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` — 435 통과, 실패 0. TypeScript를 실제로 물리는 테스트는
  `RLC_TSGO_API`를 설치된 `typescript@7.1`의 `dist/api/sync/api.js`로 지정해
  돌렸다(지정하지 않으면 스킵된다).
- [x] `editors/vscode`: `npx tsc -b`, `node --test server/out/test/*.test.js`
  — 73건 중 42 통과 / 27 스킵 / 4 실패(이슈 3, main에서도 동일).

## 결과

`val` 변경과 타입 기반 소진성이 편집 중에 표시된다. 판정과 문안은 컴파일러
하나에만 있고, 에디터는 중계만 한다.

변경 파일:

- `src/typescript/project.rs` — `lower`에 오버레이
- `src/typescript/check.rs` — `CheckOptions`, `rl_only` 계층 게이트
- `src/main.rs` — `--overlay` / `--rl-only` 파싱과 조합 거절
- `editors/vscode/server/src/rlc.ts` — `runTypedCheck`
- `editors/vscode/server/src/server.ts` — 설정·디바운스·계층 병합·게시
- `editors/vscode/package.json` — `rl.typedChecks`
- `tests/cli.rs`(+7), `editors/vscode/server/src/test/typedcheck.test.ts`(+3)
- `docs/reference/cli.md`, `docs/reference/errors.md`,
  `editors/vscode/README.md`, `docs/ai/rl.md`, `CHANGELOG.md`

남은 부채: 여러 파일을 동시에 오버레이하는 CLI 표면은 없다(내부 배관은 맵).
여러 개의 더러운 버퍼를 한 번에 묻고 싶어지면 그때 확장한다.
