# Typed match analysis — MatchAnalysis 설계 기록

TASK-096의 설계 기록이다. match를 "필요한 순간마다 부분 추론"에서
**타입이 붙은 공통 분석 결과**로 정규화한 구조와, 그 결과를 에디터
hover/definition이 소비하는 경로를 규범으로 남긴다.

한 줄 원칙:

> 패턴 안의 binding 위치와 arm body의 binding 참조는 **다른 질문**이다.
> or-pattern은 대안별로 먼저 독립 분석하고, 병합은 body 쪽에서만 한다.

## 1. 문제 — or-pattern binding 위치의 침묵

```rl
enum E { A(x: string), B(x: number) }
const v = match (e) {
  A(x) | B(x) => x
};
```

기대 동작:

- `A(x)`의 `x` hover → `string` (A의 payload)
- `B(x)`의 `x` hover → `number` (B의 payload)
- body의 `x` hover → `string | number` (병합)

body의 `x`는 원래부터 동작한다: 방출된 `const { x } = $rl_m;`가
`kind === "A" || kind === "B"`로 좁혀진 위치에 있으므로, verbatim으로
매핑된 body 참조에 tsgo가 union 타입을 답한다.

동작하지 않던 것은 **패턴 내부**의 두 `x`다. or-arm의 구조 분해는 모든
대안을 한꺼번에 대표하므로 codegen이 **의도적으로 매핑 없이** 방출한다
(TASK-080: 어느 한 대안의 span에 귀속시키면 rename이 그 대안 하나만
고쳐 프로그램을 깨뜨린다 — `codegen/matches.rs::binding_list_lit`).
매핑이 없으니 그 span은 TS 좌표로 번역되지 않고, 언어 서비스 질의가
도달하지 못해 hover가 비어 있었다.

즉 이것은 "매핑을 붙이면 되는 버그"가 아니다. 매핑을 붙여도 ① rename
원자성 계약이 깨지고 ② 보이는 타입은 대안별 payload가 아니라 병합
union이 된다. 대안별 타입은 **방출 형태 밖의 정보**다 — match를 typed
분석 결과로 정규화해야 답할 수 있다.

## 2. 구조 — 어느 계층에 무엇이 놓이나

이 저장소는 tsgo의 구조를 따른다: 순수 파이프라인 단계(파서/sema/
codegen), 그 위의 engine(Project/Snapshot/projection), TypeScript 도달은
seam(`typescript/backend.rs`·`service.rs`) 뒤에 격리. MatchAnalysis도 그
결을 따른다.

```
src/analysis.rs            ← 순수 단계 (probe.rs·sema.rs와 같은 층위)
   pattern_analyses(source, externs: &[EnumSymbol]) -> PatternAnalyses
   · 선언 테이블: 로컬 enum > import된 enum > 내장 Option/Result
     (sema의 소진성 해석과 같은 섀도잉·후보 규칙)
   · match마다: subjects(위치별) / arms / coverage
   · arm마다: patternBindings(span-키) / bodyBindings(이름-키, 병합)
   · sites: match 밖의 패턴 사이트(let-else, if let) — 같은 subject 해석과
     같은 patternBindings (TASK-102)
   · unresolved: 이름 해석의 답 (TASK-102, §7)

sema.rs                    ← 소비자 (컴파일 에러): unresolved → RlError,
                             Coverage → 위치 있는 RlError
engine/language.rs         ← 소비자 (에디터 semantic 표면)
   hover:      TS 서비스 → (없으면) 대안 격리 프로브 → 선언 타입 폴백
   definition: TS 서비스 → (비면) body 참조 → 패턴 binding span들
   externs 수집: rl_imports + overlay/디스크 + enum_symbols
                 (CLI가 sema에 extern_enums를 모아 주는 방식의 엔진판)
```

- **core에 두는 이유**: sema가 `extern_enums`를 입력으로 받듯 분석기도
  소스 + 외부 선언만 받는 순수 함수다. 파일 시스템도 TypeScript도
  모른다. 그래서 rlc(sema·CLI)와 엔진(LSP)이 **같은 모델**을 소비할 수
  있고, 툴체인이 없는 환경에서도 항상 계산된다(semantic tokens와 같은
  가용성 — TASK-093).
- **engine이 소비하는 이유**: TypeScript에 묻는 행위는 엔진의 서비스
  세션(`tsgo --lsp`) 소유다. tsgo 개념은 여전히 seam 밖으로 새지 않는다.

### 모델 — 두 map을 분리한다

```
MatchAnalysis
  subjects:  Vec<Option<MatchSubject>>   // 위치별 (튜플 match는 여러 개)
  arms:      Vec<AnalyzedArm>
  coverage:  Option<Coverage>            // §5

MatchSubject   { enum_name, constructors: Vec<MatchConstructor> }
MatchConstructor { tag, fields: Option<Vec<PayloadField>> }

AnalyzedArm
  pattern_bindings: Vec<PatternBinding>  // span-키: 출현마다 하나
  body_bindings:    Vec<BodyBinding>     // 이름-키: 대안 병합

PatternBinding { name, span, tag, ty, enum_name,
                 group span, alternative span, alternatives }
BodyBinding    { name, ty /* 병합; 하나라도 모르면 None */ }
```

`A(x) | B(x)`에서 pattern_bindings는 두 항목(각각 자기 constructor의
payload 타입), body_bindings는 한 항목(`string | number`)이다. 이 둘을
일찍 합치면 정확히 원래 버그가 된다 — 분리가 모델의 존재 이유다.

or-pattern 분석 순서는 고정이다: ① 각 대안을 subject에 대해 **독립**
분석해 출현별 span에 payload 타입을 기록 → ② 대안들의 binding 환경을
**그다음에** 병합해 body 타입을 만든다.

## 3. hover — 답의 우선순위

요구 계약: TypeScript/tsgo가 이미 아는 타입을 중심에 두고, RL 자체
추론은 폴백이다.

```
1. TS 서비스 (기존 경로 그대로)
   — emit-map이 잇는 모든 위치: 단일-대안 binding, body 참조, 일반 TS.
2. 대안 격리 프로브 (or-pattern binding span일 때)
   — completion probe와 같은 "질의 한 번 동안만 다른 텍스트 서빙" 패턴.
     source에서 그 or-group을 hover 대상 대안 하나로 치환해 방출하면
     codegen이 단일-대안 경로를 타서 구조 분해가 **매핑되고** 그 tag로
     narrowing된다. 그 위치에 hover를 물으면 제네릭 인스턴스화까지 된
     정확한 payload 타입이 나온다. 답의 range는 사용자가 보던 원본
     span이다(프로브는 사용자의 텍스트가 아니다). 질의 직후 실제
     projection을 되서빙한다.
3. 선언 테이블 폴백 (`const x: <declared>` + 출처 문서화)
   — 프로브가 실패하거나(서비스 사망, 미완성 버퍼) 툴체인 자체가 없을
     때. 내장/제네릭 enum은 선언 그대로 `T`를 보여준다 — 인스턴스화는
     checker의 몫이라는 정직한 답. subject를 못 찾으면 답하지 않는다
     (모르는 타입을 지어내지 않는다).
```

방출 코드(lowering)와 emit-map은 바꾸지 않았다. rename/references가
or-pattern binding span에서 계속 침묵하는 것도 **의도된 유지**다:
하나의 구조 분해를 공유하는 이상 span별 rename은 원자적으로 성립하지
않고, 절반 rename을 거부하는 것이 기존 계약이다.

definition은 같은 재료의 자연스러운 확장이다: or-arm body의 binding
참조에 TS가 빈 답을 주면(목적지가 글루라 navigation이 버린다),
그 arm의 대안별 패턴 binding span들을 목적지로 준다. 패턴 binding
자신 위에서는 자기 span이 선언이다. 어느 쪽도 TS 답이 있으면 나서지
않는다.

## 4. or-pattern binding 집합 검증

규칙 자체(모든 대안이 같은 (필드, 이름) 집합을 바인딩)는 sema의 기존
에러다. TASK-096은 보고 채널을 새로 만들지 않고 **메시지를 지목형으로**
바꿨다: `match: or-pattern alternatives must bind the same names —
`y` is bound in `B(...)` but not in `A(...)`` 형태로, 이름 결손과
필드-이름 불일치를 구분해 알려준다(`errors.md`). 에러 계층 계약(모든
rl 수준 에러는 rlc가 위치와 함께 직접 보고)은 그대로다.

## 5. coverage — 소진성의 단일 원천

TASK-096은 모델에 `coverage`를 파생 데이터로 노출하는 데까지만 갔고, 보고
주체인 sema는 자기 구현을 따로 갖고 있었다. TASK-097이 그 중복을 없앴다.
지금은 **계산은 `analysis.rs`, 보고는 `sema.rs`** 다.

```
analysis.rs   후보 표(로컬 > 임포트 > 내장) · subject 해석 · 커버 규칙 ·
              튜플 곱집합(odometer)  →  Coverage
sema.rs       Coverage → 위치 있는 RlError (문안·오프셋·보고 순서)
```

`Coverage`는 arity로 단일/튜플을 함께 표현한다:

```
Coverage
  positions:   Vec<Option<CoveredEnum>>  // 위치별 subject, None = 보편 위치(`_`만 쓰인 자리)
  covered:     Vec<String>               // 단일 match에서 arm이 통째로 덮은 태그 (요약)
  missing:     Vec<Vec<String>>          // witness: 빠진 값을 rl 패턴으로 렌더 (행 = 값, 칸 = 위치)
  unreachable: Vec<usize>                // 죽은 arm의 인덱스 (계산만 — 아래)
CoveredEnum { name, origin: Local | Imported { from } | Builtin }
```

**TASK-103 갱신**: 계산은 이제 `analysis/usefulness.rs`의 Maranget usefulness다.
`missing`이 태그가 아니라 **패턴**인 이유가 그것이다 — 재귀가 페이로드 안까지
내려가므로 빠진 것이 `Ok(value: None)`처럼 값의 모양으로 나온다. 중첩 패턴 arm이
"아무것도 커버하지 못한다"는 v1 규칙은 사라졌고(가드 arm만 남는다), 도달 불가
arm은 같은 재귀가 답하지만 **보고하지 않는다**: rl에는 경고 계층이 없어 rustc의
lint를 하드 에러로 바꾸면 지금 컴파일되는 프로그램이 깨진다.

`origin`이 모델에 있는 이유는 에러 문안이 그것을 부르기 때문이다 —
"enum E" / "built-in enum Option" / "enum T (imported from \"./token.rl\")".
sema는 이제 자기 후보 표를 갖지 않는다.

규칙 두 가지가 여기서 규범이 된다:

- **커버 판정**: 가드가 붙은 arm과 중첩 패턴이 있는 arm은 subject를
  식별하되 아무것도 커버하지 않는다(둘 다 런타임에 어긋날 수 있다).
- **후보 선택**: 소진성은 arm 태그를 모두 포함하는 후보들 중 ① 커버 arm이
  **만족시키는** 후보가 있으면 소진, ② 없으면 **결손이 가장 적은** 후보를
  이름으로 부른다. 타입 질문(패턴 binding이 필드 타입을 읽는 자리)은 다른
  질문이라 계속 **첫 후보**를 쓴다 — 두 질문에 두 해석이 있는 것이 아니라,
  같은 표에 두 질의가 있는 것이다.

내장 enum(`Option`/`Result`)의 선언도 이 표 하나뿐이다. sema가 태그만 담긴
사본(`stdlib::BUILTIN_ENUMS`)을 따로 보던 것은 함께 없앴다.

extern 입력의 모양이 둘인 것(컴파일러의 `ExternEnum` — 태그와 지정자,
에디터의 `EnumSymbol` — 필드 타입까지)은 표 빌더가 흡수한다. 컴파일러
경로는 필드 타입이 필요 없으므로 binding 분석을 아예 건너뛴다
(`Depth::CoverageOnly`) — 소진성 답은 그대로 완전하다.

## 6. 한계 (알고 유지하는 것)

- 폴백 타입은 **선언 텍스트**다. narrowing·제네릭 인스턴스화·다른 rl
  enum들의 TS-union scrutinee는 checker 경로(1·2번)만 안다. 이것이 "RL
  타입체커를 만들지 않는다"는 계약의 형태다. TASK-098이 위치별로 확인한
  결과, 매핑이 없어 checker가 도달하지 못하는 자리는 **or-pattern
  binding(단일 match와 튜플 원소) 둘뿐**이고 그 둘은 프로브가 덮는다 —
  중첩 패턴 leaf와 튜플 단일 대안 원소는 매핑된 채 방출되므로 1번이
  인스턴스화까지 정확히 답한다. 즉 폴백의 부정확함은 "체커에 물을 수 없는
  상황"에만 남고, 그 상황에서는 치환할 입력도 없다.
- extern 수집은 CLI와 같은 1-hop(직접 상대 `.rl` import)이다.
- `body_definitions`/`body_binding_at`은 body 안의 섀도잉을 모델링하지
  않는다 — TS가 빈 답을 준 뒤에만 쓰는 폴백이라는 전제가 계약이다.
- 대안 격리 프로브는 tsgo 통합 환경에서의 e2e 테스트가 아직 없다
  (순수 부분 — 합성 소스·매핑 — 은 단위 테스트로 고정). vscode
  `engine.test.ts` 계층에 추가하는 것이 후속이다.
- 튜플 match의 위치별 후보 해석은 **첫 후보**를 쓴다(단일 match의 소진성
  후보 선택과 다르다). 이것은 이관 전 sema의 동작을 그대로 옮긴 것이고,
  위치마다 "만족시키는 후보"를 따지는 규칙으로 바꿀지는 열려 있다.

## 7. 이름 해석 — 모델이 답하는 두 번째 질문 (TASK-102)

TASK-096의 모델은 "이 바인딩의 타입은 무엇인가"만 답했다. 그 반대 방향 —
**이 이름은 무엇을 가리키는가, 가리키는 것이 없으면 어떻게 되는가** — 은
자리가 없었고, 그래서 태그 오타가 rlc를 통과해 글루 위의 tsc 에러가 됐다
(`docs/design/rust-parity-analysis.md` §GAP-1).

TASK-102이 그 질문을 같은 모델에 넣었다. rustc의 단계 구성과 같은 자리다:
**resolve가 먼저, 그것을 전제로 하는 질문(소진성)이 나중.**

```
PatternAnalyses.unresolved: Vec<UnresolvedName>
   { kind: Case | Field, name, span, enum_name, origin, tag, suggestion }
```

규범이 되는 규칙 둘:

- **보고의 자격은 "고칠 이름을 댈 수 있음"이다.** 태그 패턴은 `kind` 필드를 가진
  모든 태그드 유니언에 쓸 수 있으므로(`language.md` §3.2), 선언 표에 없는 태그가
  곧 오류는 아니다. 그래서 분석은 해석 실패 자체를 내보내지 않고, **오타로
  보이는 것만** 내보낸다(대소문자 무시 일치, 또는 편집 거리 — 자리바꿈은 한 번).
  이 판단은 분석의 것이고, sema는 문안만 만든다 — `Coverage`와 같은 분업이다.
- **지목(identify)은 세 번째 질의다.** 표에는 이미 두 질의가 있었다:
  `resolve`(타입을 읽을 선언)와 `resolve_coverage`(소진성을 잴 선언). 해석은
  "이 사이트가 말하는 enum"을 묻는다 — 모든 태그를 포함하는 후보, 없으면 가장
  많이 포함하는 **유일한** 후보. 동점이거나 하나도 없으면 답하지 않는다.
  단일 패턴 사이트(let-else·`if let`)는 다른 태그의 뒷받침이 없으므로 **편집
  한 번** 거리로 면허를 좁힌다.

남은 것은 타입이 있어야 아는 것들이다 — 오타가 아닌 틀린 이름, 스크루티니가
정말 그 enum인지. 그것은 체커에 묻는 질문이고 P4의 몫이다.
