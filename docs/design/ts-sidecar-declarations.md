# TypeScript 사이드카 선언 — 설계 제안

이 문서는 **제안**이다. 규범 문서가 아니다. TASK-028에서 작성했다.
[`project-front-end.md`](./project-front-end.md)의 역할 변경과 짝을 이루는
에디터 쪽 조각을 다룬다.

## 문제

`.ts` 파일이 `.rl`을 import하면 에디터가 에러를 낸다.

```
main.ts(6,32): error TS2307: Cannot find module './notice.rl' or its
               corresponding type declarations.
```

원인은 관할이 나뉘어 있는 것이다. `.ts` 문서는 tsserver가 소유하는데
tsserver는 `.rl` 확장자를 모르고, rl 언어 서버는 `documentSelector`가
`language: rl`이라 `.ts` 문서에 개입하지 않는다. 확장을 아무리 고쳐도
`.ts` 쪽 진단은 우리 것이 아니다.

## 해법은 TypeScript가 이미 갖고 있다

위 에러 메시지의 뒷부분 — "or its corresponding **type declarations**" —
이 그대로 탈출구다. `.rl` 옆에 사이드카 두 개를 두면 된다.

| 파일 | 역할 |
|------|------|
| `x.rl.d.ts` | tsserver가 `"./x.rl"`을 해결하는 근거. 에러가 사라지고 자동완성·타입·시그니처가 살아난다 |
| `x.rl.d.ts.map` | `sources: ["x.rl"]`. 정의 이동을 `.d.ts`가 아니라 **원본 `.rl`로** 되돌린다 |

`source/rl-interop` 예제에서 프로토타입으로 만들어 tsserver를 직접 구동해
확인했다.

```
진단: 없음
main.ts:23  render  → src/notice.rl:21:17     ← export function render
main.ts:10  Notice  → src/notice.rl:9:13      ← export enum Notice
```

`Notice`에서 정의가 둘 나오는 것은 rl enum 하나가 타입과 값 두 선언으로
컴파일되기 때문이며, 둘 다 원본의 같은 위치를 가리킨다.

세 번째 조각도 필요하다: **`src/`를 담당하는 `tsconfig.json`**. 프로젝트
없이 추론 프로젝트로 열리면 tsserver가 선언 맵을 따라가지 않는다.

## 누가 무엇을 만드나

`.d.ts` **본문**을 만들려면 타입 정보가 필요하다. 통과 영역의 함수·상수는
반환 타입이 추론될 수 있고, 그것은 rlc의 일이 아니다 — 에러 계층 계약상
타입은 tsc의 책임이다.

반대로 **위치 대응**은 rlc만 알 수 있다. codegen이 통과 구간을 바이트 단위로
복사하므로 rl↔ts 구간 대응표를 정확히 낼 수 있고, `--symbols`는 이미 enum
선언의 행·열을 내고 있다.

그래서 역할을 이렇게 나눈다.

```
rlc     x.rl → x.ts                       (+ 구간 대응)
tsc     x.ts → x.d.ts  (--emitDeclarationOnly)
rlc     x.d.ts + 구간 대응 → x.rl.d.ts + x.rl.d.ts.map
```

마지막 단계만 새로 만들면 된다. 제안하는 형태는 `rlc --sidecar` — 방출된
선언 파일과 원본 `.rl`을 받아 사이드카 두 개를 쓴다.

## 프로토타입에서 걸린 것

**세그먼트를 줄의 0열에만 두면 정의 이동이 `.d.ts`에 그대로 선다.** 정의
이동은 심볼 **이름이 시작하는 열**의 대응을 묻기 때문이다. 이름 열에도
세그먼트를 두자 곧바로 원본으로 갔다. 구현 시 반드시 지켜야 하는 조건이다.

## 결정해야 하는 것

1. **본문 생성 주체** — 위 배치대로 tsc에 맡길지, rl enum만이라도 rlc가
   직접 낼지. 후자는 통과 영역 선언을 다루지 못해 반쪽이 된다.
2. **사이드카 배치** — `.rl` 옆에 둘지, 별도 디렉터리에 두고 tsconfig
   `paths`로 연결할지. 상대 경로 지정자는 `paths`를 타지 않으므로 후자는
   추가 장치가 필요하다.
3. **같은 이름의 중복 선언** — 프로토타입은 이름으로 원본 위치를 찾아 첫
   일치를 쓴다. rlc는 구간 대응표를 쓰므로 이 문제가 없지만, 대응표의 입도를
   선언 단위로 할지 행 단위로 할지 정해야 한다.
4. **`src/tsconfig.json` 요구** — 문서로만 안내할지, 없을 때 확장이 알려줄지.
5. **생성 시점** — 빌드 스텝으로 둘지, 언어 서버가 저장 시 갱신할지.

## 범위 밖

- **런타임 소스맵**(`.ts.map`) — 디버거용이며 별개 과제다.
- **타입 추론** — tsc의 책임으로 남긴다.
