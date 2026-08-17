# 모듈 그래프 — 설계 제안

이 문서는 **제안**이다. 규범 문서가 아니다
(구현된 구조는 [`compiler-architecture.md`](./compiler-architecture.md)).
TASK-019에서 작성했다. **세 단계 모두 구현되었다** — 1단계는 TASK-020,
2단계는 TASK-022, 3단계는 TASK-023 (규범 서술:
[`language.md` §8](../reference/language.md#8-모듈-rl-import-지정자-재작성),
[`cli.md` "심볼 출력"](../reference/cli.md#심볼-출력---symbols)).

rlc는 지금 **파일 하나를 파일 하나로** 바꾼다. `compile(source, &Options) ->
Result<String, CompileError>`라는 시그니처가 그 사실을 그대로 드러낸다. 이
문서는 그 경계를 넘어 rlc에 모듈 그래프를 들이는 것이 무엇을 열어주고 무엇을
결정해야 하는지 정리한다.

## 왜 — 세 증상, 하나의 원인

사용자 눈에는 서로 다른 세 가지 불편으로 보이지만 원인은 하나다.

| 증상 | 지금 동작 |
|------|-----------|
| `.rl`끼리 import할 수 없다 | 소스에 `from "./error.rl"`이라 쓰면 방출된 `.ts`에 그대로 남는다. tsc는 `TS2307: Cannot find module './error.rl'`로 거부한다. |
| 소진성 검사가 파일 단위다 | 다른 파일에서 import한 enum에 대한 `match`는 검사 없이 런타임 가드만 남는다 ([language.md §3.6](../reference/language.md#36-소진성-검사)). |
| 정의로 이동이 파일 단위다 | 언어 서버의 `analysis.parseEnums(src, masked)`는 문서 하나만 받는다. import 문을 해석하는 코드가 없다. |

원인은 **import 문이 통과 영역**이라는 것이다. rlc는 지정자를 읽지도, 고치지도
않는다. 그래서 어떤 `.rl`이 어떤 `.rl`에 의존하는지 rlc가 알지 못하고,
"프로젝트"라는 단위 자체가 없다.

세 증상은 그래프가 생기면 함께 풀린다. 그래서 하나의 제안으로 묶는다.

## 무엇을 — 세 단계

단계마다 독립적으로 가치가 있고, 앞 단계 없이 뒤 단계를 할 수 없다.

### 1단계 — import 지정자 재작성 (구현됨 — TASK-020)

`.rl`로 끝나는 상대 경로 지정자를 방출 시점에 소비 측이 해석할 수 있는
형태로 바꾼다.

```rl
// 소스 (parser.rl)
import { CalcError } from "./error.rl";
```

```ts
// 방출 (parser.ts)
import { CalcError } from "./error.js";   // 또는 "./error"
```

참조된 파일을 열 필요조차 없다. 지정자 문자열만 바꾸면 되므로 현재의
파일 단위 파이프라인을 그대로 두고 codegen 단계에서 처리할 수 있다. 파서는
import 문의 지정자 구간(바이트 범위)만 추가로 기록하면 된다.

이 단계만으로 "소스가 소스를 가리킨다"는 성질을 얻는다.

### 2단계 — 선언 수집과 프로젝트 단위 소진성 (구현됨 — TASK-022)

재작성 대상이 된 지정자를 실제로 따라가 참조된 `.rl`을 파싱하고, 그 파일의
`enum` 선언(태그 목록)만 뽑아 현재 파일의 sema에 넘긴다.

```
parse(a.rl) ──┐
              ├─→ 선언 테이블 ─→ sema::check(b.rl, 테이블)
parse(b.rl) ──┘
```

전체 타입 정보가 아니라 **enum 태그 집합만** 필요하다는 점이 중요하다.
소진성 검사에 필요한 정보가 그뿐이기 때문이다. 타입 검사는 여전히 tsc의
책임이고, 이 단계는 그 경계를 넘지 않는다.

이 단계에서 `import { Token } from "./token.rl"` 한 줄이면 파서 파일의
`match (token)`이 빠뜨린 케이스를 rlc가 잡는다.

### 3단계 — 심볼 인터페이스와 언어 서버 (구현됨 — TASK-023)

2단계에서 만들어진 선언 테이블을 언어 서버가 쓸 수 있게 내보낸다. 예를 들어
`rlc --symbols <file>`이 JSON으로 `{ 파일, 이름, 태그, 위치 }`를 출력하면,
서버는 지금처럼 rlc를 자식 프로세스로 호출해 그 결과를 쓰면 된다 — 서버가
rl 문법을 다시 구현할 필요가 없다.

이때 크로스 파일 정의 이동, 완성, 호버가 한꺼번에 열린다. 진단이 이미
"에디터의 에러 = 컴파일러의 에러"인 것처럼, 심볼 해석도 컴파일러 하나를
정본으로 두는 구조가 된다.

## 결정해야 하는 것

### 1. 방출 지정자의 형태 (결정됨 — TASK-020)

소비 측 `moduleResolution`에 달린 값이라 rlc가 혼자 알 수 없다.

| 소비 설정 | 필요한 형태 | 확인 |
|-----------|-------------|------|
| `nodenext` | `"./error.js"` | 확장자 없으면 Node ESM이 해석 실패 |
| `bundler` | `"./error"` | 확장자 없이 통과 (tsc 5.9.3에서 확인) |

`"./error.ts"`는 어느 쪽에서도 답이 아니다 — `TS5097`로 거부되고,
`allowImportingTsExtensions`를 켜면 `TS5096`으로 emit이 막힌다.

선택지: (a) `--rewrite-imports=js|bare` 플래그, (b) `nodenext` 기본 + 플래그로
변경, (c) 근처 `tsconfig.json`을 읽어 자동 판별. (c)는 rlc가 TypeScript
설정을 해석하기 시작한다는 뜻이라 비용이 크다.

**결정(TASK-020)**: `--rewrite-imports <js|bare|off>` 플래그, 기본 `js` —
`./x.js`는 `nodenext`에서 필수이고 `bundler`에서도 tsc의 `.js`→`.ts` 대응으로
동작하는 유일한 기본값이다. (c)는 배제.

### 2. 절대 불변 원칙 1과의 관계

"모든 유효한 TypeScript 파일은 그대로 유효한 `.rl` 파일이며 자기 자신으로
컴파일된다"는 계약이 있다. 재작성은 이 계약의 예외를 하나 만든다 — 유효한
TS 파일이 `"./x.rl"`을 import하고 있으면 결과가 원문과 달라진다.

다만 그런 파일은 실질적으로 존재하기 어렵다. tsc는 `.rl` 지정자를 해석하지
못하므로(`TS2307`), 번들러에 `.rl` 로더를 붙인 프로젝트에서만 유효한 TS다.
그리고 그 로더는 rl 자신일 것이다. 계약에 예외를 명시하고 기본 동작으로
넣을지, 옵트인 플래그 뒤에 둘지 정해야 한다.

**결정(TASK-020)**: 기본 동작으로 넣고 `--rewrite-imports off`를 옵트아웃으로
남겼다. 예외는 `CLAUDE.md` 계약 1과 `language.md` §1에 명시했다.

### 3. 순환 import (결정됨 — TASK-022)

2단계에서 `a.rl ↔ b.rl`이 서로를 import하면 선언 수집이 무한히 돌 수 있다.
선언만 필요하므로 방문 집합으로 끊으면 충분하다 — 순환 자체를 에러로 볼
이유는 없다 (TypeScript도 타입 수준 순환을 허용한다).

**결정(TASK-022)**: 수집을 **직접 import 1-홉**으로 한정해 재귀 자체를
없앴다 — 순환은 발생할 수 없고, 방문 집합도 필요 없다. re-export 체인
추적(전이 수집)은 필요가 확인되면 별도 태스크로 다룬다.

### 4. 에러 위치 보고 (결정됨 — TASK-022)

지금 모든 에러는 컴파일 중인 파일 기준 `파일:행:열`이다. 2단계에서는 "다른
파일의 enum" 때문에 에러가 날 수 있으므로, 선언 위치를 함께 보여줄지
(`... declared at token.rl:7:1`) 정해야 한다.

**결정(TASK-022)**: 위치는 컴파일 중인 파일의 `match` 키워드 그대로 두고,
메시지에 출처를 넣는다: `match on enum Token (imported from "./token.rl")
is not exhaustive: ...`. 선언 파일의 행:열까지 담으려면 수집 API가 위치를
운반해야 해서 (지금은 이름+태그뿐) 비용 대비 이득이 작다.

### 5. 공개 API (결정됨 — TASK-022)

`compile(source, &Options)`은 파일 하나를 컴파일하는 API로 그대로 두고,
그래프를 다루는 새 진입점을 추가하는 편이 안전하다. 라이브러리 사용자가
파일 하나만 변환하는 경우가 사라지지 않기 때문이다.

**결정(TASK-022)**: `compile`은 그대로, IO도 여전히 라이브러리 밖이다.
수집 재료를 주는 두 함수(`rl_imports` — import 목록,
`exported_enums` — exported enum 선언 추출)와 주입구
(`Options::extern_enums: &[ExternEnum]`)만 추가했고, 파일을 읽는 그래프
순회는 CLI(`collect_extern_enums`)가 한다.

## 범위 밖

- **타입 검사** — 여전히 tsc의 책임이다. 선언 수집은 enum 태그 집합까지만.
- **증분 컴파일·캐시** — 그래프가 생긴 뒤에 별도로 다룬다.
- **`node_modules` 해석** — 상대 경로 `.rl` 지정자만 대상으로 한다.

## 참고

- 현재 구조: [`compiler-architecture.md`](./compiler-architecture.md)
- 소진성 검사 규칙: [`language.md §3.6`](../reference/language.md#36-소진성-검사)
- 언어 서버: [`editors/vscode/README.md`](../../editors/vscode/README.md)
