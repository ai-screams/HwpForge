# Benchmarks

HwpForge 성능 벤치마크

## Structure

- `foundation.rs` — HwpUnit, Color 연산
- `parsing.rs` — HWPX/HWP5 파싱 속도
- `serialization.rs` — HWPX 쓰기 속도

## Running

```bash
cargo bench
```

## Criterion Output

결과는 `target/criterion/` 디렉토리에 HTML 리포트로 생성됩니다.
