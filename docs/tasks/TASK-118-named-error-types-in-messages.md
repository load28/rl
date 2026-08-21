# TASK-118: 타입 에러 문안에서 구조적 타입을 선언 이름으로

- **상태**: 대기
- **시작일**: —
- **완료일**: —
- **커밋**: —

## 목적

`try`가 전파하는 `Err`가 반환 타입에 맞지 않을 때 rlc는 이렇게 말한다:

```
the `Err` this `try` propagates does not fit the enclosing function's return
type — ... (ts2322: Type 'Err<{ kind: "OutOfRange"; value: number; }>' is not
assignable to type 'Result<string, { kind: "NotANumber"; text: string; }>'.)
```

위치는 TASK-116이 정확히 잡았지만(그 `try` 위), **문안**은 tsc가 펼친 구조적
타입이라 읽기 어렵다. rl은 그 태그가 어느 enum의 케이스인지 안다 — Rust가
`` `?` couldn't convert the error to `ParseError` ``라고 말하는 수준으로
좁힐 여지가 있다(`Test.OutOfRange` / `ParseError`).

## 범위

- 포함(예정): 진단 문안의 `{ kind: "X"; ... }`를 선언 표가 **유일하게** 지목할
  때만 `Enum.X`로 부른다.
- 제외(예정): 원문을 덮어쓰는 것. 지금 계약은 "옮긴 말 + 괄호 안의 원문"이고,
  원문은 사용자가 번역을 검증할 근거이므로 그대로 실려야 한다 — 이름은 옮긴
  말 쪽에 더한다.

## 의사결정

(작업 시 기록)

## 작업 내역

(작업 시 기록)

## 이슈 및 해결

(작업 시 기록)

## 검증

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo test`

## 결과

(작업 시 기록)
